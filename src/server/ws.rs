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

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tower_http::services::{ServeDir, ServeFile};

use crate::net::codec::{decode_client, encode_server};
use crate::net::{ClientMessage, RoomId, ServerMessage};
use crate::server::rooms::{Caller, ConnectionId, Rooms, Seat};
use crate::server::{api, console, unjoined};
use crate::sim::WorldKind;

/// What a connection sends to the simulation task.
enum ToSim {
    Message {
        /// Which socket, and where it is sitting once it has joined. A `Join`
        /// carries its own room and so needs no seat; `Create` needs the
        /// connection, because a room is made before there is a seat to make
        /// it from.
        from: Caller,
        msg: ClientMessage,
        reply: mpsc::UnboundedSender<ServerMessage>,
    },
    Left(Seat),
    /// One request from the HTTP API, answered on the same task everything
    /// else is — see [`api::handle`].
    Api {
        req: api::Request,
        reply: tokio::sync::oneshot::Sender<api::Reply>,
    },
    /// `/healthz` asking how many rooms there are. Asked of this task rather
    /// than read off a counter, so a server whose one task has stalled fails
    /// its health check instead of passing it.
    Health {
        reply: tokio::sync::oneshot::Sender<usize>,
    },
}

/// Hands out one id per socket, never reusing one.
///
/// Never reused so that a room's owner cannot silently become a different
/// person: a counter that wrapped, or that filled gaps left by connections
/// that had gone, would let a new socket inherit what an old one opened. At
/// one connection a nanosecond this wraps in about six hundred years.
static NEXT_CONNECTION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_connection() -> ConnectionId {
    NEXT_CONNECTION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A message meant for everyone in one room. The room travels with it because
/// the rooms are separate worlds: a `Step` from one is not a fact about
/// another, and applying it there would advance a world nobody stepped.
type Broadcast = (RoomId, ServerMessage);

#[derive(Clone)]
struct AppState {
    to_sim: mpsc::UnboundedSender<ToSim>,
    broadcast: broadcast::Sender<Broadcast>,
    connections: Arc<Connections>,
}

/// Every socket that is open, counted for `/healthz`, and the ones in no room
/// by address, for the cap.
#[derive(Default)]
struct Connections {
    open: AtomicUsize,
    unjoined: Mutex<unjoined::PerAddress>,
}

impl Connections {
    /// A place for one more socket from `from`, or `None` because that address
    /// already holds [`MOST_UNJOINED_PER_ADDRESS`] that have joined nothing.
    fn admit(self: &Arc<Self>, from: IpAddr) -> Option<Counted> {
        let mut unjoined = self.unjoined.lock().unwrap_or_else(PoisonError::into_inner);
        if !unjoined.admit(from, MOST_UNJOINED_PER_ADDRESS) {
            return None;
        }
        self.open.fetch_add(1, Ordering::Relaxed);
        Some(Counted { of: self.clone(), from, unjoined: true })
    }
}

/// One socket's place in the counts: against its address's cap until it joins
/// a room, and in the total until its task ends, however it ends.
struct Counted {
    of: Arc<Connections>,
    from: IpAddr,
    unjoined: bool,
}

impl Counted {
    /// Seated or watching, so no longer one of the address's unjoined. A
    /// `Leave` later does not put it back: a socket that has been somebody
    /// is not the stranger the cap is for.
    fn joined(&mut self) {
        if std::mem::take(&mut self.unjoined) {
            self.of.unjoined.lock().unwrap_or_else(PoisonError::into_inner).release(self.from);
        }
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.joined();
        self.of.open.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct Config {
    pub addr: SocketAddr,
    /// Directory served at `/`, so the browser client comes from here too.
    pub static_dir: Option<PathBuf>,
    /// How often every room is written out. Where is the rooms directory,
    /// which [`Rooms`] already knows.
    pub save_every: Duration,
    /// What every room here starts at, in generations a minute — see
    /// [`crate::net::Rules::bpm`]. A room may be set to something else once it
    /// exists; this is the answer for one nobody has said anything about.
    pub bpm: u16,
    /// What shape a room made from the console gets when it is not given one.
    /// The same shape the command line asked for, so `new arena` means what
    /// `--room arena` would have meant.
    pub shape: WorldKind,
    /// The bearer token the HTTP API answers to, and **without one the routes
    /// are not mounted** — see [`api`]. An API with no token would be every
    /// room's lobby open to anybody who could reach the port.
    pub api_token: Option<String>,
}

/// Where log lines go while somebody is typing.
///
/// A log line written straight to the terminal lands **in the middle of the
/// half-typed command**, because the cursor is sitting after a prompt the
/// logger knows nothing about. rustyline's external printer is the fix: it
/// wipes the prompt, writes the line, and draws the prompt and whatever was
/// typed back underneath — so the log scrolls past above and the command being
/// typed stays put.
///
/// A global because the logger is set up before there is a terminal to print
/// to, and may never get one: a server under systemd has no prompt to protect,
/// and its lines go to stderr as they always did. So this is empty until the
/// console thread has an editor, and every write asks.
#[cfg(feature = "server")]
static PRINTER: std::sync::Mutex<Option<Box<dyn rustyline::ExternalPrinter + Send>>> =
    std::sync::Mutex::new(None);

/// Somewhere for `env_logger` to write that respects a prompt.
///
/// A `Write` rather than a `log::Log`, so env_logger goes on doing the
/// formatting — the timestamps, the levels and the colours are its business
/// and there is no reason to reimplement them to change where the bytes land.
///
/// Buffered to the newline, because a printer call is a whole line: env_logger
/// writes a record in several `write` calls, and printing each one separately
/// would redraw the prompt in the middle of a log line.
#[cfg(feature = "server")]
pub struct ConsoleLog {
    line: Vec<u8>,
}

#[cfg(feature = "server")]
impl ConsoleLog {
    /// Hand this to `env_logger::Builder::target`, and log lines will appear
    /// above whatever is being typed for as long as there is a prompt.
    pub fn target() -> env_logger::Target {
        env_logger::Target::Pipe(Box::new(Self { line: Vec::new() }))
    }
}

#[cfg(feature = "server")]
impl std::io::Write for ConsoleLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.line.extend_from_slice(buf);
        while let Some(at) = self.line.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.line.drain(..=at).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            let printed = PRINTER
                .lock()
                .ok()
                .and_then(|mut p| p.as_mut().map(|p| p.print(text.clone()).is_ok()))
                .unwrap_or(false);
            // No prompt to protect, or the printer gave up: stderr, which is
            // where these went before there was a line editor at all.
            if !printed {
                let _ = writeln!(std::io::stderr(), "{text}");
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A prompt, with history on the up arrow.
///
/// **History is the whole reason there is a library here.** Typing
/// `match new arena toroidal 18x18 territory 500` a second time to correct one
/// number is the sort of thing that makes a console not worth using, and none
/// of it is something to hand-roll: the up arrow is a terminal escape, and
/// once you are decoding those you are writing a line editor.
///
/// Kept in a file as well as in memory, so the commands survive a restart —
/// which is the case that matters, since restarting is when you are most
/// likely to want the command you typed before.
///
/// One caveat worth knowing: the server logs while you type, and a log line
/// arriving mid-line scrambles the prompt until the next keystroke redraws it.
/// rustyline's `ExternalPrinter` is the proper fix and wants the logger routed
/// through it, which is more than this is worth until it annoys somebody.
#[cfg(feature = "server")]
fn edited(mut editor: rustyline::DefaultEditor, tx: &mpsc::UnboundedSender<String>) {
    // Installed before the first prompt is drawn, so a line arriving in the
    // same breath as the console starting does not land on top of it.
    match editor.create_external_printer() {
        Ok(printer) => {
            if let Ok(mut slot) = PRINTER.lock() {
                *slot = Some(Box::new(printer));
            }
        }
        Err(e) => log::debug!("no external printer ({e}); logs will interrupt the prompt"),
    }

    let history = history_path();
    if let Some(path) = &history {
        // Absent the first time, which is not a failure.
        let _ = editor.load_history(path);
    }
    loop {
        match editor.readline("> ") {
            Ok(line) => {
                // Blank lines are somebody pressing return, and a history full
                // of them is a history you cannot page through.
                if !line.trim().is_empty() {
                    let _ = editor.add_history_entry(line.as_str());
                    // Written as it is typed, not on the way out. `stop` ends
                    // the process while this thread is parked in `readline`,
                    // so a save at the end of the loop never ran -- which is
                    // every session that ended the way sessions end, and the
                    // file only ever caught the ones that died some other way.
                    if let Some(path) = &history {
                        let _ = editor.append_history(path);
                    }
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
            // Ctrl-C abandons the line rather than the server: the way to
            // stop is `stop`, which saves on the way out, and a stray Ctrl-C
            // taking every world down unsaved would be an expensive twitch.
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            // End of input is **not** `stop`. It arrives when a terminal
            // closes, and it arrives at once for a server started in the
            // background or with its input redirected -- so treating it as a
            // command shuts down a server nobody asked to shut down, which is
            // exactly what it did the first time this was tried. The way out
            // is to type it.
            Err(rustyline::error::ReadlineError::Eof) => {
                log::debug!("console closed; running headless");
                break;
            }
            Err(e) => {
                log::debug!("console closed ({e})");
                break;
            }
        }
    }
    // There is no prompt to write above any more, and a printer for a dead
    // editor is a line that goes nowhere.
    if let Ok(mut slot) = PRINTER.lock() {
        *slot = None;
    }
}

/// Where the typed history is kept, beside the rest of a user's data.
#[cfg(feature = "server")]
fn history_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
    })?;
    let dir = home.join("conwayskingdom");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("console-history"))
}

/// Lines, with no editing and no prompt: a pipe, a file, or a terminal the
/// editor could not take.
#[cfg(feature = "server")]
fn plain(tx: &mpsc::UnboundedSender<String>) {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) => {
                if tx.send(line).is_err() {
                    break;
                }
            }
            // Not UTF-8, or the terminal went away. Either way there is
            // nothing more to read.
            Err(_) => break,
        }
    }
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
            match rustyline::DefaultEditor::new() {
                Ok(editor) => edited(editor, &tx),
                // No terminal to edit: input is a pipe, a file, or systemd's
                // /dev/null. Piped commands still have to work -- a script
                // that says `echo rooms | server` is a reasonable thing to
                // write -- so this falls back to reading lines rather than
                // giving up on a console altogether.
                Err(e) => {
                    log::debug!("no line editor ({e}); reading plainly");
                    plain(&tx);
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
    // **Fine, because each room keeps its own rate now.** This was one
    // interval at the generation span and every room stepped on it; a
    // laboratory's rate is its own to change, so the ticker is a grain and
    // `Rooms::step` hands each room the elapsed time to bank -- see
    // `Server::owe`. Fast enough that the fastest allowed rate is smooth, and
    // slow enough that a quiet server is not a busy loop.
    let grain = std::time::Duration::from_millis(25);
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
        let mut ticker = tokio::time::interval(grain);
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

                // Each room advances on its own clock, at the rate in its
                // `Rules` -- which a `Welcome` has always carried, so there is
                // nothing new for a client to be told. The ticker is the
                // grain; `Server::owe` is what decides.
                _ = ticker.tick() => {
                    for labelled in rooms.step(grain) {
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
                            for out in rooms.handle(&from, msg) {
                                let _ = reply.send(out);
                            }
                            // Anything the rooms want said at once rather than
                            // at the next tick -- an action, so that a cell
                            // appears on everybody's screen in a round trip
                            // instead of half a generation later.
                            for labelled in rooms.take_announcements() {
                                let _ = sim_broadcast.send(labelled);
                            }
                        }
                        // And the same again for a request off the API: an
                        // engine's action is an action, and the room hears
                        // about it now.
                        Some(ToSim::Api { req, reply }) => {
                            let _ = reply.send(api::handle(&mut rooms, req));
                            for labelled in rooms.take_announcements() {
                                let _ = sim_broadcast.send(labelled);
                            }
                        }
                        Some(ToSim::Health { reply }) => {
                            let _ = reply.send(rooms.len());
                        }
                    }
                }
            }
        }

        if rooms.save().is_ok() {
            log::info!("saved {} room(s) on shutdown", rooms.len());
        }
    });

    let state = AppState { to_sim, broadcast: broadcast_tx, connections: Default::default() };
    let app = router(state, &config);

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
        None => log::warn!("no --serve DIR, so http://{host}/ will 404; only the socket is up"),
    }
    log::info!("ws://{host}/ws  websocket");
    match &config.api_token {
        Some(_) => log::info!("http://{host}/api  the API, to whoever has the token"),
        None => log::info!("no --api-token and no CK_API_TOKEN, so /api is not mounted"),
    }
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
    // With the peer's address, which the cap on unjoined connections is per.
    let served = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
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

/// Every route: the socket and the health line always, the page and its files
/// given a directory, and the API only with a token.
fn router(state: AppState, config: &Config) -> Router {
    let for_api = state.to_sim.clone();
    let mut app = Router::new().route("/ws", get(upgrade)).route("/healthz", get(healthz));
    if let Some(dir) = &config.static_dir {
        app = serve_client(app, dir);
    }
    let mut app = app.with_state(state);
    // Only with a token, and the routes are absent rather than refusing:
    // there is nothing to guess at on a server that was not asked for one.
    if let Some(token) = &config.api_token {
        app = app.merge(api::http::router(token.clone(), move |req| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            let _ = for_api.send(ToSim::Api { req, reply });
            answer
        }));
    }
    app
}

/// How long `/healthz` gives the simulation task to say how many rooms there
/// are before calling the server unwell.
const HEALTH_WAIT: Duration = Duration::from_secs(1);

/// `200 ok` and one line -- how many rooms, how many connections -- for an
/// uptime check. Unauthenticated, and served with or without a page, because
/// it says nothing the room list does not. See [server.md].
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#deploying
async fn healthz(State(state): State<AppState>) -> Response {
    let no_store = [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))];
    let (reply, answer) = tokio::sync::oneshot::channel();
    let _ = state.to_sim.send(ToSim::Health { reply });
    let rooms = match tokio::time::timeout(HEALTH_WAIT, answer).await {
        Ok(Ok(rooms)) => rooms,
        _ => {
            let why = "the simulation is not answering\n";
            return (StatusCode::SERVICE_UNAVAILABLE, no_store, why).into_response();
        }
    };
    let open = state.connections.open.load(Ordering::Relaxed);
    (StatusCode::OK, no_store, format!("ok: {rooms} rooms, {open} connections\n")).into_response()
}

/// The largest frame this server will read.
///
/// The default is tens of megabytes, which is the transport being generous
/// about something it cannot judge. Nothing a client sends is large: the
/// longest is a `Subscribe` or a `Checkpoint`, and both are capped in messages
/// -- [`MOST_CHUNKS_AT_ONCE`] chunks at twelve bytes a row is under fifty
/// kilobytes. This is the same argument made where the bytes arrive, so a
/// frame that could only be an attack is dropped before it is decoded into
/// something with a length to check.
///
/// [`MOST_CHUNKS_AT_ONCE`]: crate::server::MOST_CHUNKS_AT_ONCE
const MOST_BYTES_AT_ONCE: usize = 1 << 20;

/// How long a connection in no room may say nothing before it is closed.
/// Chosen, not measured -- see [known-bugs.md].
///
/// [known-bugs.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/known-bugs.md#fixed
const MOST_UNJOINED_SILENCE: Duration = Duration::from_secs(120);

/// How many connections in no room one address may hold at once. Chosen, not
/// measured -- see [known-bugs.md].
///
/// [known-bugs.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/known-bugs.md#fixed
const MOST_UNJOINED_PER_ADDRESS: usize = 8;

async fn upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let named = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok());
    let from = unjoined::remote(named, peer.ip());
    // Refused before the upgrade, so it costs a handshake and not a task.
    let Some(counted) = state.connections.admit(from) else {
        log::warn!(
            "{from} holds {MOST_UNJOINED_PER_ADDRESS} connections that have joined nothing; \
             refusing another"
        );
        let why = "too many connections from this address have joined nothing\n";
        return (StatusCode::TOO_MANY_REQUESTS, why).into_response();
    };
    ws.max_message_size(MOST_BYTES_AT_ONCE)
        .max_frame_size(MOST_BYTES_AT_ONCE)
        .on_upgrade(move |socket| connection(socket, state, from, counted))
        .into_response()
}

/// What the browser client is served from, and nothing else.
///
/// **An allowlist, not a denylist.** `--serve .` is what the documentation
/// tells people to run, and `.` is the repository — so serving the directory
/// wholesale published `src/`, `Cargo.toml` and, worse, `.git/`, which carries
/// every version of everything ever committed. Blocking `/src` and a list of
/// other names would be whack-a-mole against a directory nobody controls; this
/// says what the client needs and there is nothing else to reach.
///
/// Three things: the page, the wasm module `wasm-pack` writes, and the art.
///
/// Every other path a *client route* can be is answered with the page, because
/// the address bar carries the screen — see [`crate::client::route`] — and a
/// refresh on `/play` has to come back with something. That is an allowlist
/// too: an unknown path is a 404 rather than the page, so a mistyped URL says
/// so instead of silently opening the game.
fn serve_client(app: Router<AppState>, dir: &std::path::Path) -> Router<AppState> {
    let page = dir.join("index.html");
    let index = || ServeFile::new(page.clone());

    let page = Router::<AppState>::new()
        .route_service("/", index())
        // The client's own screens. Listed rather than matched by a catch-all,
        // so that `/src/main.rs` is a 404 and not a copy of the page.
        .route_service("/home", index())
        .route_service("/play", index())
        .route_service("/alone", index())
        .route_service("/experiments", index())
        .route_service("/solo", index())
        .route_service("/room/{id}", index())
        .route_service("/lobby/{id}", index())
        .route_service("/watch/{id}", index())
        .layer(axum::middleware::map_response(revalidate));
    let files = Router::<AppState>::new()
        .nest_service("/pkg", ServeDir::new(dir.join("pkg")))
        .nest_service("/assets", ServeDir::new(dir.join("assets")))
        .layer(axum::middleware::from_fn(cached));

    app.merge(page).merge(files)
}

/// **Ask before reusing.** `ServeDir` and `ServeFile` send `Last-Modified` and
/// no `Cache-Control` at all, and a response with a validator and no caching
/// directive is one a browser is *entitled* to reuse without asking: the rule
/// is a heuristic freshness lifetime, commonly a tenth of the file's age. A
/// module built a day ago is therefore fresh for a couple of hours, and a
/// rebuild inside that window changes nothing anybody can see.
///
/// Which is the worst version of the failure this repository already knows
/// about. [gotchas.md] says `pkg/` is generated, gitignored and never updated
/// by a pull, so the page and the module diverge with nothing to detect it;
/// this is the same divergence arriving *after* a successful build, which
/// removes the one check that catches the other — reading the build output.
///
/// `no-cache` does not mean do not store. It means revalidate before use, so
/// the conditional request `ServeDir` already answers still comes back 304 and
/// costs a round trip rather than a download. The page is small and is what
/// a deploy changes; the megabytes behind it are [`cached`].
///
/// [gotchas.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/gotchas.md
async fn revalidate(mut response: Response) -> Response {
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// What `pkg/` and `assets/` are served with: an hour, then ask. See
/// [server.md].
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#deploying
const STATIC_CACHE: &str = "public, max-age=3600, must-revalidate";

/// `Content-Length` again, under a name an edge that compresses leaves alone.
const X_CONTENT_LENGTH: header::HeaderName = header::HeaderName::from_static("x-content-length");

/// **An hour at the edge and in the browser, then a conditional request**,
/// for the module and the art. They are the megabytes behind a page that is
/// `no-cache`, and an edge that could not hold them would fetch the module
/// from the origin for every visitor; an hour is how long a rebuild can go
/// unseen without a purge. Nothing here carries a content hash in its name --
/// wasm-pack names the module after the crate -- so nothing is `immutable`.
///
/// `ServeDir` sends `Last-Modified` and no `ETag`, so one is made from what it
/// does send -- the length and the modification time -- rather than from the
/// bytes, which for the module would be a pass over 7.5 MB per request. It is
/// as strong as a filesystem timestamp, which is what nginx sends and for the
/// same reason, and `If-None-Match` is answered here because `ServeDir` has
/// never heard of the tag. `DefaultHasher::new` has fixed keys, so the tag is
/// the same on both sides of a restart -- one that changed would cost every
/// visitor a download for a file nobody had touched.
///
/// The length goes out twice. The loading bar wants `Content-Length`, and an
/// edge that compresses the module on the way through takes it; the same
/// number under a name the edge leaves alone is what the page falls back to.
async fn cached(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let asked_with = request.headers().get(header::IF_NONE_MATCH).cloned();
    let mut response = next.run(request).await;
    let cache = match response.status() {
        StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED => STATIC_CACHE,
        // A missing file is a build half done or a wrong path, and neither is
        // worth an edge holding for an hour.
        _ => "no-store",
    };
    // A whole file only: a range's length is the range's, and a 304 or a 404
    // has nothing to validate.
    let whole = response.status() == StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    let (Some(length), Some(modified)) =
        (headers.get(header::CONTENT_LENGTH).cloned(), headers.get(header::LAST_MODIFIED))
    else {
        return response;
    };
    if !whole {
        return response;
    }
    let tag = {
        use std::hash::{Hash, Hasher};
        let mut stamp = std::hash::DefaultHasher::new();
        modified.as_bytes().hash(&mut stamp);
        format!("\"{}-{:016x}\"", length.to_str().unwrap_or("?"), stamp.finish())
    };
    headers.insert(X_CONTENT_LENGTH, length);
    if let Ok(tag) = HeaderValue::from_str(&tag) {
        headers.insert(header::ETAG, tag);
    }
    let matched = asked_with
        .as_ref()
        .and_then(|w| w.to_str().ok())
        .is_some_and(|w| w.split(',').any(|t| t.trim() == tag));
    if matched {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        *response.body_mut() = axum::body::Body::empty();
    }
    response
}

async fn connection(socket: WebSocket, state: AppState, from: IpAddr, mut counted: Counted) {
    let id = next_connection();
    log::info!("connection {id} opened from {from}");
    let (mut sink, mut stream) = socket.split();

    let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let mut subscribed = state.broadcast.subscribe();
    // Which world this connection is in, and who they are in it. Both come
    // from the `Welcome`: player numbers are per room, so the number alone
    // does not say where its owner is sitting.
    let mut me: Option<Seat> = None;
    // Which world this connection is watching without a seat in it. Set by a
    // `Watching` and cleared by a `Welcome`, because joining a room you were
    // watching makes you a player in it rather than both at once.
    let mut watching: Option<RoomId> = None;
    // **Who this connection is**, learned from a `You` or from the `Welcome`'s
    // profile and never cleared: a person outlives a seat, so going back to
    // the menu and editing a library is still that person doing it. A client
    // that joined without a key has no profile and so is nobody here, which is
    // right -- there is nowhere to file anything against.
    let mut who: Option<crate::net::PersonId> = None;
    // When this connection last said anything. It is under a deadline for as
    // long as it is in no room, because nothing else would ever notice it had
    // gone -- see `unjoined::deadline`.
    let mut last_heard = Instant::now();

    loop {
        tokio::select! {
            _ = async {
                let in_room = me.is_some() || watching.is_some();
                match unjoined::deadline(last_heard, in_room, MOST_UNJOINED_SILENCE) {
                    Some(at) => tokio::time::sleep_until(at.into()).await,
                    None => std::future::pending().await,
                }
            } => {
                log::info!(
                    "connection {id} from {from} said nothing for {}s in no room; closing",
                    MOST_UNJOINED_SILENCE.as_secs()
                );
                break;
            }
            // Replies addressed to this connection.
            Some(msg) = reply_rx.recv() => {
                match &msg {
                    ServerMessage::Welcome { you, room, profile, .. } => {
                        me = Some((room.clone(), *you));
                        watching = None;
                        if let Some(profile) = profile {
                            who = Some(profile.who.clone());
                        }
                        counted.joined();
                    }
                    // A person with no seat yet: the answer to a `Hello`.
                    ServerMessage::You(profile) => who = Some(profile.who.clone()),
                    // A watcher hears the room's broadcast and holds no seat,
                    // so leaving is a socket closing and nothing more -- there
                    // is no player to mark offline and no ground to keep.
                    ServerMessage::Watching { room, .. } => {
                        me = None;
                        watching = Some(room.clone());
                        counted.joined();
                    }
                    _ => {}
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
                        // Seated or watching: both are in the room, and a
                        // spectator that heard no `Step` would be watching a
                        // world that never moved.
                        let ours = me.as_ref().map(|(r, _)| r).or(watching.as_ref());
                        if ours.is_some_and(|ours| *ours == room)
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
                last_heard = Instant::now();
                match frame {
                    Message::Binary(bytes) => match decode_client(&bytes) {
                        Ok(msg) => {
                            // **Addressed before the seat is given up, not
                            // after.** This cleared `me` first, so the one
                            // message whose whole job is to free a seat
                            // arrived saying its sender was sitting in none —
                            // `Rooms::handle` reads `caller.seat` to know
                            // which to free, found nothing, and freed nothing.
                            // The room went on counting the player online, so
                            // the listing still showed them in it and coming
                            // back found them already there.
                            let from = Caller {
                                connection: id,
                                seat: me.clone(),
                                watching: watching.clone(),
                                person: who.clone(),
                            };
                            // And *then* forgotten here, or this task would go
                            // on routing a room's broadcasts to a seat the
                            // simulation has let go.
                            if matches!(msg, ClientMessage::Leave) {
                                me = None;
                                watching = None;
                            }
                            let _ =
                                state.to_sim.send(ToSim::Message { from, msg, reply: reply_tx.clone() });
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
        None => log::info!("connection {id} from {from} closed before joining"),
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

/// Over a real socket, because there is no `tower` here to drive a router
/// without one. Reached by `cargo test --features server` and by nothing
/// else -- see docs/README.md.
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A router with a stand-in for the simulation task that answers every
    /// `Health` with `rooms`, listening on a port of the system's choosing.
    async fn listening(static_dir: Option<PathBuf>, rooms: usize) -> SocketAddr {
        let (to_sim, mut from_conns) = mpsc::unbounded_channel::<ToSim>();
        tokio::spawn(async move {
            while let Some(asked) = from_conns.recv().await {
                if let ToSim::Health { reply } = asked {
                    let _ = reply.send(rooms);
                }
            }
        });
        let (broadcast, _) = broadcast::channel(8);
        let state = AppState { to_sim, broadcast, connections: Default::default() };
        let config = Config {
            addr: "127.0.0.1:0".parse().unwrap(),
            static_dir,
            save_every: Duration::from_secs(30),
            bpm: crate::net::DEFAULT_BPM,
            shape: WorldKind::Infinite,
            api_token: None,
        };
        let app = router(state, &config);
        let listener = tokio::net::TcpListener::bind(config.addr).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = app.into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    async fn get(addr: SocketAddr, path: &str) -> String {
        get_with(addr, path, "").await
    }

    async fn get_with(addr: SocketAddr, path: &str, extra: &str) -> String {
        let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n{extra}\r\n");
        tcp.write_all(request.as_bytes()).await.unwrap();
        let mut text = String::new();
        tcp.read_to_string(&mut text).await.unwrap();
        text
    }

    fn header<'a>(text: &'a str, name: &str) -> Option<&'a str> {
        text.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim_start_matches(": ").trim())
    }

    /// `/healthz` is a 200 and one line whether or not there is a page to
    /// serve, and the line counts what the simulation task said.
    #[tokio::test]
    async fn healthz_answers_with_and_without_a_page() {
        for static_dir in [None, Some(PathBuf::from("."))] {
            let addr = listening(static_dir, 3).await;
            let text = get(addr, "/healthz").await;
            assert!(text.starts_with("HTTP/1.1 200"), "{text}");
            assert!(text.ends_with("\r\n\r\nok: 3 rooms, 0 connections\n"), "{text}");
            assert!(text.contains("cache-control: no-store"), "{text}");
        }
    }

    /// The page is `no-cache`; a file under `assets/` is held for an hour with
    /// a tag, the same tag back is a 304, and the length goes out twice.
    #[tokio::test]
    async fn the_page_asks_every_time_and_the_files_are_held_for_an_hour() {
        let addr = listening(Some(PathBuf::from(".")), 1).await;

        let page = get(addr, "/").await;
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert_eq!(header(&page, "cache-control"), Some("no-cache"));

        let file = get(addr, "/assets/fonts/LICENSE.txt").await;
        assert!(file.starts_with("HTTP/1.1 200"), "{file}");
        assert_eq!(header(&file, "cache-control"), Some(STATIC_CACHE));
        assert_eq!(header(&file, "x-content-length"), header(&file, "content-length"));
        let tag = header(&file, "etag").expect("a tag");
        assert!(tag.starts_with('"') && tag.ends_with('"'), "{tag}");

        let again =
            get_with(addr, "/assets/fonts/LICENSE.txt", &format!("If-None-Match: {tag}\r\n")).await;
        assert!(again.starts_with("HTTP/1.1 304"), "{again}");
        assert_eq!(header(&again, "etag"), Some(tag));

        let missing = get(addr, "/assets/fonts/nope.txt").await;
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
        assert_eq!(header(&missing, "cache-control"), Some("no-store"));
    }
}
