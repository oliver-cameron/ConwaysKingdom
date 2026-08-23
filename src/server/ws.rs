//! WebSocket transport, over axum.
//!
//! axum rather than a bare websocket crate because it serves the wasm client
//! and the socket from one origin and one port: no second static-file server,
//! and no cross-origin question to answer.
//!
//! One task owns the [`Rooms`] and their ticks; connections talk to it through
//! channels. That keeps every simulation single-threaded and its ordering
//! fixed, which is what determinism requires — a mutex shared between
//! connection tasks would make the order actions land in depend on scheduling.
//!
//! Rooms are separate worlds, so a `Step` from one of them means nothing in
//! another. There is still **one** broadcast channel: every message carries
//! the room it came from and each connection drops what is not its own. One
//! channel per room would save that comparison and cost a shared map of
//! senders that connections and the simulation task would both have to lock —
//! a lock, to avoid a string compare, on the one path that must not have one.

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
use crate::net::{ClientMessage, RoomName, ServerMessage};
use crate::server::console;
use crate::server::rooms::{Rooms, Seat};
use crate::sim::WorldKind;

/// What a connection sends to the simulation task.
enum ToSim {
    Message {
        /// Where the sender is sitting, once they have joined. A `Join`
        /// carries its own room and so needs none.
        from: Option<Seat>,
        msg: ClientMessage,
        reply: mpsc::UnboundedSender<ServerMessage>,
    },
    Left(Seat),
}

/// A message meant for everyone in one room. The room travels with it because
/// the rooms are separate worlds: a `Step` from one is not a fact about
/// another, and applying it there would advance a world nobody stepped.
type Broadcast = (RoomName, ServerMessage);

#[derive(Clone)]
struct AppState {
    to_sim: mpsc::UnboundedSender<ToSim>,
    broadcast: broadcast::Sender<Broadcast>,
}

pub struct Config {
    pub addr: SocketAddr,
    /// Directory served at `/`, so the browser client comes from here too.
    pub static_dir: Option<PathBuf>,
    /// How often every room is written out. Where is the rooms directory,
    /// which [`Rooms`] already knows.
    pub save_every: Duration,
    pub generation_span: Duration,
    /// What shape a room made from the console gets when it is not given one.
    /// The same shape the command line asked for, so `new arena` means what
    /// `--room arena` would have meant.
    pub shape: WorldKind,
}

/// Read the server's own terminal, a line at a time, on a thread of its own.
///
/// A thread rather than `tokio::io::stdin`, whose reads are documented as not
/// cancellation-safe: a pending read dropped inside a `select!` swallows the
/// line it was in the middle of, so a command would go missing whenever a
/// generation ticked at the wrong moment. A blocking read on its own thread
/// has no such problem, and a channel receiver is cancel-safe.
///
/// Returns `None` when there is no terminal to read — a server under systemd,
/// or one started with `< /dev/null`. That is the ordinary case for a server
/// nobody is sitting at, not a failure, and it must not turn into a loop that
/// spins on end-of-file.
fn read_console() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("console".into())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    // Not UTF-8, or the terminal went away. Either way there
                    // is nothing more to read.
                    Err(_) => break,
                }
            }
        })
        .expect("spawning the console thread");
    rx
}

/// Everything that means "stop now".
///
/// Ctrl-C is SIGINT and is what a person at a terminal sends. **SIGTERM is
/// what everything else sends** — `kill`, `systemctl stop`, `docker stop`,
/// a `timeout` in a script — and listening only for the first meant every one
/// of those killed the process outright, taking up to `save_every` of every
/// room with it. The one that a person is least likely to use is the one that
/// matters most, because it is how a server is stopped when nobody is
/// watching.
async fn signalled() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(e) => {
                log::error!("cannot listen for SIGTERM ({e}); ctrl-c only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("interrupted"),
            _ = term.recv() => log::info!("terminated"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("interrupted");
    }
}

/// Run until the process is asked to stop, saving on the way out.
pub async fn serve(mut rooms: Rooms, config: Config) -> std::io::Result<()> {
    let (to_sim, mut from_conns) = mpsc::unbounded_channel::<ToSim>();
    let (broadcast_tx, _) = broadcast::channel::<Broadcast>(1024);

    let sim_broadcast = broadcast_tx.clone();
    let save_every = config.save_every;
    let span = config.generation_span;
    let shape = config.shape;

    // Three things say stop -- a signal, a `stop` typed at the console, and
    // the connections all going away -- and they must all mean the same
    // thing, or one of them is a shutdown that does not save. A `watch` rather
    // than a `Notify`: it remembers, so a waiter that arrives after the signal
    // still sees it, and it has as many receivers as there are things to stop.
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let stop_rx_for_http = stop_tx.subscribe();
    let signal_stop = stop_tx.clone();
    tokio::spawn(async move {
        signalled().await;
        let _ = signal_stop.send(true);
    });

    // The one task that touches the world -- and now the console too, because
    // making a room is touching one and there is exactly one place that is
    // allowed to.
    let sim = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(span);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut save_timer = tokio::time::interval(save_every);
        save_timer.tick().await; // the first tick is immediate; skip it
        let mut console = Some(read_console());

        loop {
            tokio::select! {
                // Asked to stop, by a signal or by somebody typing it.
                _ = stop_rx.changed() => break,

                // Typed at the server's own terminal. Guarded, because once
                // there is no terminal the receiver is closed and would
                // otherwise answer instantly and forever.
                typed = async {
                    match console.as_mut() {
                        Some(rx) => rx.recv().await,
                        // Unreachable under the guard below, which select!
                        // evaluates before it polls this. Never-ready rather
                        // than a panic anyway: a branch that cannot fire is
                        // worth expressing as one, not as a crash if the order
                        // ever changed.
                        None => std::future::pending().await,
                    }
                }, if console.is_some() => {
                    match typed {
                        Some(line) => {
                            let reply = console::run(&line, &mut rooms, shape);
                            for line in reply.lines {
                                println!("{line}");
                            }
                            if reply.stop {
                                let _ = stop_tx.send(true);
                                break;
                            }
                        }
                        // No terminal: a server under systemd, or one started
                        // with `< /dev/null`. Ordinary, not a failure.
                        None => {
                            console = None;
                            log::debug!("no console; running headless");
                        }
                    }
                }

                // Every room advances on the same clock. Separate worlds, but
                // one generation span: a room with its own rate would be a
                // second thing for a client to be told and a second way for
                // the two to disagree about what a tick is.
                _ = ticker.tick() => {
                    for labelled in rooms.step() {
                        let _ = sim_broadcast.send(labelled);
                    }
                }
                // A failure is already logged per room, with the name of the
                // one that could not be written; this only says how it went
                // overall, so a quiet log means every world is on disk.
                _ = save_timer.tick() => {
                    if rooms.save().is_ok() {
                        log::info!("saved {} room(s)", rooms.len());
                    }
                }
                incoming = from_conns.recv() => {
                    match incoming {
                        // Every sender is gone, so the router has been
                        // dropped and nothing more can arrive.
                        None => break,
                        Some(ToSim::Left(seat)) => rooms.leave(&seat),
                        Some(ToSim::Message { from, msg, reply }) => {
                            for out in rooms.handle(from.as_ref(), msg) {
                                let _ = reply.send(out);
                            }
                        }
                    }
                }
            }
        }

        if rooms.save().is_ok() {
            log::info!("saved {} room(s) on shutdown", rooms.len());
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
    // The one bind that cannot be reached from another machine, called out
    // because the symptom is indistinguishable from a network problem and the
    // cause is a flag: `--addr 127.0.0.1:8080` serves this machine only.
    if config.addr.ip().is_loopback() {
        log::warn!(
            "bound to {} -- loopback only, so no other machine can reach this. \
             Use --addr [::]:{} to listen on every interface.",
            config.addr,
            config.addr.port()
        );
    }
    match &config.static_dir {
        Some(dir) => log::info!("http://{host}/  serving {}", dir.display()),
        None => log::warn!(
            "no --serve DIR, so http://{host}/ will 404; only the socket is up"
        ),
    }
    log::info!("ws://{host}/ws  websocket");
    if config.addr.ip().is_unspecified() {
        // Not "reachable from other machines". Binding to an unspecified
        // address means the socket accepts on every interface; whether a
        // packet ever arrives is the firewall's business and the network's,
        // and neither is visible from here. Print the address to try instead
        // of a claim that cannot be checked.
        match outward_address(config.addr.port()) {
            Some(addr) => log::info!("http://{addr}/  from another machine, if the network allows"),
            None => log::info!("listening on every interface; no outward address found"),
        }
        // Worth saying out loud, because the failure it prevents is invisible:
        // a client that resolves this host by name gets its AAAA record and
        // arrives over IPv6, and an IPv4-only socket refuses it.
        if config.addr.is_ipv6() {
            log::info!("accepting IPv4 and IPv6");
        } else {
            log::warn!("IPv4 only; a client arriving over IPv6 will be refused");
        }
    }
    // The console is only worth mentioning where there is one to type at.
    log::info!("console: type `help` for commands, `stop` to shut down");

    let mut http_stop = stop_rx_for_http;
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // Any of the three: a signal, `stop` typed at the console, or the
            // simulation task ending of its own accord. One place to wait
            // rather than three, so a shutdown started from anywhere drains
            // connections the same way.
            let _ = http_stop.changed().await;
            // Not "shutting down": that has already been said, by whichever
            // of the three asked for it. This is the part that is only about
            // the socket.
            log::info!("closing connections");
        })
        .await;

    // Waited for rather than aborted, and this is the difference between a
    // clean shutdown saving and not.
    //
    // The simulation task saves *after* its loop, and the loop only ends when
    // every sender is gone -- which happens here, because `serve` consumed the
    // router that held the last one. `abort` cancelled the task at its next
    // await point, which is inside the loop's `select!`, so it never reached
    // the save at all and a clean exit quietly lost up to `save_every` of
    // every room. The symptom was a world that only ever remembered what a
    // periodic save had caught.
    //
    // Bounded, because a sender that somehow outlives the router would
    // otherwise hang the process on the way out -- and a shutdown that does
    // not shut down is worse than one that loses a save it warned about.
    if tokio::time::timeout(Duration::from_secs(10), sim).await.is_err() {
        log::error!("the simulation task did not finish in ten seconds; exiting without its save");
    }
    served?;
    Ok(())
}

/// The address another machine would reach this one on, if it can.
///
/// Asks the routing table rather than enumerating interfaces: connecting a UDP
/// socket sends no packet, it only resolves which local address would be used
/// to reach that destination. The destination is a documentation address, so
/// there is nothing to reach even if something did go out.
fn outward_address(port: u16) -> Option<SocketAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let mut addr = socket.local_addr().ok()?;
    addr.set_port(port);
    (!addr.ip().is_loopback()).then_some(addr)
}

async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| connection(socket, state))
}

async fn connection(socket: WebSocket, state: AppState) {
    log::info!("connection opened");
    let (mut sink, mut stream) = socket.split();
    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let mut subscribed = state.broadcast.subscribe();
    // Which world this connection is in, and who they are in it. Both come
    // from the `Welcome`: player numbers are per room, so the number alone
    // does not say where its owner is sitting.
    let mut me: Option<Seat> = None;

    loop {
        tokio::select! {
            // Replies addressed to this connection.
            Some(msg) = reply_rx.recv() => {
                if let ServerMessage::Welcome { you, room, .. } = &msg {
                    me = Some((room.clone(), *you));
                }
                if !send(&mut sink, &msg).await { break; }
            }
            // Everything every client in this room needs. A connection that
            // has not joined is in no room and so hears nothing -- otherwise
            // it would be handed one world's generations before it had asked
            // for any world at all.
            broadcast = subscribed.recv() => {
                match broadcast {
                    Ok((room, msg)) => {
                        if me.as_ref().is_some_and(|(mine, _)| *mine == room)
                            && !send(&mut sink, &msg).await
                        {
                            break;
                        }
                    }
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
                                from: me.clone(),
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
        Some(seat) => {
            let _ = state.to_sim.send(ToSim::Left(seat));
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
