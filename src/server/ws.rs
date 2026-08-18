//! WebSocket transport, over axum.
//!
//! axum rather than a bare websocket crate because it serves the wasm client
//! and the socket from one origin and one port: no second static-file server,
//! and no cross-origin question to answer.
//!
//! One task owns the [`Server`] and the tick; connections talk to it through
//! channels. That keeps the simulation single-threaded and its ordering fixed,
//! which is what determinism requires — a mutex shared between connection tasks
//! would make the order actions land in depend on scheduling.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tower_http::services::ServeDir;

use crate::net::codec::{decode_client, encode_server};
use crate::net::{ClientMessage, ServerMessage};
use crate::server::Server;
use crate::sim::PlayerId;

/// What a connection sends to the simulation task.
enum ToSim {
    Message {
        from: Option<PlayerId>,
        msg: ClientMessage,
        reply: mpsc::UnboundedSender<ServerMessage>,
    },
    Left(PlayerId),
}

#[derive(Clone)]
struct AppState {
    to_sim: mpsc::UnboundedSender<ToSim>,
    broadcast: broadcast::Sender<ServerMessage>,
}

pub struct Config {
    pub addr: SocketAddr,
    /// Directory served at `/`, so the browser client comes from here too.
    pub static_dir: Option<PathBuf>,
    /// Where the world is saved, and how often.
    pub save_path: Option<PathBuf>,
    pub save_every: Duration,
    pub generation_span: Duration,
}

/// Run until the process is asked to stop, saving on the way out.
pub async fn serve(mut server: Server, config: Config) -> std::io::Result<()> {
    let (to_sim, mut from_conns) = mpsc::unbounded_channel::<ToSim>();
    let (broadcast_tx, _) = broadcast::channel::<ServerMessage>(1024);

    let sim_broadcast = broadcast_tx.clone();
    let save_path = config.save_path.clone();
    let save_every = config.save_every;
    let span = config.generation_span;

    // The one task that touches the world.
    let sim = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(span);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut save_timer = tokio::time::interval(save_every);
        save_timer.tick().await; // the first tick is immediate; skip it

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    for msg in server.step() {
                        let _ = sim_broadcast.send(msg);
                    }
                }
                _ = save_timer.tick() => {
                    if let Some(path) = &save_path {
                        if let Err(e) = server.save(path) {
                            log::error!("saving world: {e}");
                        } else {
                            log::info!("saved at tick {}", server.tick());
                        }
                    }
                }
                incoming = from_conns.recv() => {
                    match incoming {
                        None => break,
                        Some(ToSim::Left(id)) => server.leave(id),
                        Some(ToSim::Message { from, msg, reply }) => {
                            for out in server.handle(from, msg) {
                                let _ = reply.send(out);
                            }
                        }
                    }
                }
            }
        }

        if let Some(path) = &save_path {
            if let Err(e) = server.save(path) {
                log::error!("saving world on shutdown: {e}");
            }
        }
    });

    let state = AppState { to_sim, broadcast: broadcast_tx };
    let mut app = Router::new().route("/ws", get(upgrade));
    if let Some(dir) = &config.static_dir {
        app = app.fallback_service(ServeDir::new(dir));
    }
    let app = app.with_state(state);

    let listener = tokio::net::TcpListener::bind(config.addr).await?;

    // Say what is actually reachable, and where. One line that only mentioned
    // the socket left the HTTP server looking like it was not running.
    let host = if config.addr.ip().is_unspecified() {
        format!("localhost:{}", config.addr.port())
    } else {
        config.addr.to_string()
    };
    match &config.static_dir {
        Some(dir) => log::info!("http://{host}/  serving {}", dir.display()),
        None => log::warn!(
            "no --serve DIR, so http://{host}/ will 404; only the socket is up"
        ),
    }
    log::info!("ws://{host}/ws  websocket");
    if config.addr.ip().is_unspecified() {
        log::info!("bound to {} — reachable from other machines", config.addr);
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("shutting down");
        })
        .await?;

    sim.abort();
    Ok(())
}

async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| connection(socket, state))
}

async fn connection(socket: WebSocket, state: AppState) {
    log::info!("connection opened");
    let (mut sink, mut stream) = socket.split();
    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let mut subscribed = state.broadcast.subscribe();
    let mut me: Option<PlayerId> = None;

    loop {
        tokio::select! {
            // Replies addressed to this connection.
            Some(msg) = reply_rx.recv() => {
                if let ServerMessage::Welcome { you, .. } = &msg {
                    me = Some(*you);
                }
                if !send(&mut sink, &msg).await { break; }
            }
            // Everything every client needs.
            broadcast = subscribed.recv() => {
                match broadcast {
                    Ok(msg) => if !send(&mut sink, &msg).await { break },
                    // Dropped frames mean this client is behind; a resync is
                    // the honest answer, but it needs the tick, so just log
                    // until the client asks with a Checkpoint.
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("connection lagged {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = stream.next() => {
                let Some(Ok(frame)) = incoming else { break };
                match frame {
                    Message::Binary(bytes) => match decode_client(&bytes) {
                        Ok(msg) => {
                            let _ = state.to_sim.send(ToSim::Message {
                                from: me,
                                msg,
                                reply: reply_tx.clone(),
                            });
                        }
                        Err(e) => log::warn!("undecodable frame: {e}"),
                    },
                    Message::Close(_) => break,
                    // Text frames would mangle raw cell bytes; the protocol is
                    // binary only.
                    Message::Text(_) => log::warn!("text frame ignored"),
                    _ => {}
                }
            }
        }
    }

    match me {
        Some(id) => {
            let _ = state.to_sim.send(ToSim::Left(id));
        }
        None => log::info!("connection closed before joining"),
    }
}

async fn send(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
) -> bool {
    match encode_server(msg) {
        Ok(bytes) => sink.send(Message::Binary(bytes.into())).await.is_ok(),
        Err(e) => {
            log::error!("encoding outbound message: {e}");
            true
        }
    }
}
