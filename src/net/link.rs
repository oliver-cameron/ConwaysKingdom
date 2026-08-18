//! The client's end of the connection.
//!
//! Native only for now. A winit event loop is not async, and making it so to
//! accommodate a socket would be the tail wagging the dog — so the socket lives
//! on its own thread with its own runtime, and the two sides pass messages
//! through channels. The app polls, never blocks, and a dead connection is a
//! `None` rather than a stall.
//!
//! The wasm client will need a second implementation over `web_sys::WebSocket`,
//! since a browser has no sockets to give tokio. The [`Link`] surface is the
//! part worth keeping identical.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use super::codec::{decode_server, encode_client};
use super::{ClientMessage, ServerMessage};

/// A connection to a server, or what is left of one.
pub struct Link {
    outbound: Sender<ClientMessage>,
    inbound: Receiver<ServerMessage>,
    /// Set once the socket thread has stopped; the app can then reconnect or
    /// carry on offline, since the simulation runs locally either way.
    closed: bool,
}

impl Link {
    /// Connect in the background. Returns immediately: the socket may not be
    /// open yet, and messages queued before it is are sent once it is.
    pub fn connect(url: impl Into<String>) -> Self {
        let url = url.into();
        let (out_tx, out_rx) = mpsc::channel::<ClientMessage>();
        let (in_tx, in_rx) = mpsc::channel::<ServerMessage>();

        thread::Builder::new()
            .name("ws".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("websocket runtime: {e}");
                        return;
                    }
                };
                rt.block_on(pump(url, out_rx, in_tx));
            })
            .expect("spawning the websocket thread");

        Self { outbound: out_tx, inbound: in_rx, closed: false }
    }

    pub fn send(&self, msg: ClientMessage) {
        if self.outbound.send(msg).is_err() {
            log::warn!("send on a closed link");
        }
    }

    /// Everything that has arrived since the last call. Never blocks.
    pub fn drain(&mut self) -> Vec<ServerMessage> {
        let mut out = Vec::new();
        loop {
            match self.inbound.try_recv() {
                Ok(msg) => out.push(msg),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    break;
                }
            }
        }
        out
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

async fn pump(url: String, outbound: Receiver<ClientMessage>, inbound: Sender<ServerMessage>) {
    let (stream, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(ok) => ok,
        Err(e) => {
            log::error!("connecting to {url}: {e}");
            return;
        }
    };
    log::info!("connected to {url}");
    let (mut sink, mut source) = stream.split();

    loop {
        // The outbound channel is synchronous, so drain it without blocking the
        // runtime, then wait on the socket.
        loop {
            match outbound.try_recv() {
                Ok(msg) => match encode_client(&msg) {
                    Ok(bytes) => {
                        if sink.send(Message::Binary(bytes.into())).await.is_err() {
                            log::warn!("socket closed while sending");
                            return;
                        }
                    }
                    Err(e) => log::error!("encoding: {e}"),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        tokio::select! {
            frame = source.next() => match frame {
                Some(Ok(Message::Binary(bytes))) => match decode_server(&bytes) {
                    Ok(msg) => {
                        if inbound.send(msg).is_err() { return; }
                    }
                    Err(e) => log::warn!("undecodable frame: {e}"),
                },
                Some(Ok(Message::Close(_))) | None => {
                    log::info!("server closed the connection");
                    return;
                }
                Some(Err(e)) => {
                    log::warn!("socket error: {e}");
                    return;
                }
                _ => {}
            },
            // Wake often enough to notice queued outbound messages.
            _ = tokio::time::sleep(std::time::Duration::from_millis(8)) => {}
        }
    }
}
