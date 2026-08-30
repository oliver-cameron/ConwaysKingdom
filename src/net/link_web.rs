//! The browser's end of the connection.
//!
//! A browser has no sockets to hand tokio, so this is a second implementation
//! of the same [`Link`](super::link::Link) surface over `web_sys::WebSocket`.
//! No threads either: the socket's callbacks push into a queue the app drains
//! each frame, which is the same shape the native side gets from a channel.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::codec::{decode_server, encode_client};
use super::{ClientMessage, ServerMessage};

#[derive(Default)]
struct Shared {
    inbox: Vec<ServerMessage>,
    /// Queued until the socket reports open; `send` before then would throw.
    outbox: Vec<Vec<u8>>,
    open: bool,
    closed: bool,
}

pub struct Link {
    socket: web_sys::WebSocket,
    shared: Rc<RefCell<Shared>>,
    // Kept alive for as long as the socket is: dropping a Closure detaches it.
    _on_open: Closure<dyn FnMut()>,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

impl Link {
    /// The websocket URL for the server that served this page.
    ///
    /// `https` pages must use `wss`, or the browser blocks the connection as
    /// mixed content — so the scheme is derived rather than assumed, and the
    /// host is taken verbatim so a port or a reverse proxy needs no config.
    pub fn origin_url(path: &str) -> Option<String> {
        let location = web_sys::window()?.location();
        let secure = location.protocol().ok()? == "https:";
        let host = location.host().ok()?;
        let scheme = if secure { "wss" } else { "ws" };
        Some(format!("{scheme}://{host}{path}"))
    }

    pub fn connect(url: &str) -> Option<Self> {
        let socket = web_sys::WebSocket::new(url).ok()?;
        // Frames are binary; without this they arrive as Blobs, which can only
        // be read asynchronously.
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
        let shared = Rc::new(RefCell::new(Shared::default()));

        let on_open = {
            let shared = shared.clone();
            let socket = socket.clone();
            Closure::<dyn FnMut()>::new(move || {
                let mut s = shared.borrow_mut();
                s.open = true;
                for bytes in s.outbox.drain(..) {
                    let _ = socket.send_with_u8_array(&bytes);
                }
                log::info!("websocket open");
            })
        };
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let on_message = {
            let shared = shared.clone();
            Closure::<dyn FnMut(_)>::new(move |e: web_sys::MessageEvent| {
                let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() else {
                    log::warn!("non-binary frame ignored");
                    return;
                };
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                match decode_server(&bytes) {
                    Ok(msg) => shared.borrow_mut().inbox.push(msg),
                    Err(e) => log::warn!("undecodable frame: {e}"),
                }
            })
        };
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let on_close = {
            let shared = shared.clone();
            Closure::<dyn FnMut(_)>::new(move |_: web_sys::CloseEvent| {
                shared.borrow_mut().closed = true;
                log::info!("websocket closed");
            })
        };
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let on_error = {
            let shared = shared.clone();
            Closure::<dyn FnMut(_)>::new(move |_: web_sys::Event| {
                shared.borrow_mut().closed = true;
                log::warn!("websocket error");
            })
        };
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Some(Self {
            socket,
            shared,
            _on_open: on_open,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
        })
    }

    pub fn send(&self, msg: ClientMessage) {
        let bytes = match encode_client(&msg) {
            Ok(b) => b,
            Err(e) => {
                log::error!("encoding: {e}");
                return;
            }
        };
        let mut shared = self.shared.borrow_mut();
        if shared.open {
            let _ = self.socket.send_with_u8_array(&bytes);
        } else {
            shared.outbox.push(bytes);
        }
    }

    /// Everything that has arrived since the last call. Never blocks.
    pub fn drain(&mut self) -> Vec<ServerMessage> {
        std::mem::take(&mut self.shared.borrow_mut().inbox)
    }

    pub fn is_closed(&self) -> bool {
        self.shared.borrow().closed
    }
}

/// Closing the socket, which dropping it does not do.
///
/// Dropping a `Closure` detaches the handler it was attached with, so a
/// dropped `Link` stops *listening* straight away — and the connection itself
/// stays open until the browser gets round to collecting the `WebSocket`.
/// Until it does, the server is holding a connection that will never say
/// another word, and the client that walked away from it has no way to say so.
///
/// Native needs none of this: its `Link` owns the sending half of a channel,
/// and the socket thread returns the moment that half is dropped.
impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.socket.close();
    }
}
