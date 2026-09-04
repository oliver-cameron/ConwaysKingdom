//! What the client does before the first frame, and what it is told to do it
//! with.
//!
//! Two questions with one answer between them — *where am I going* and *what
//! world do I look at until I get there* — and neither is about drawing, which
//! is why both are here rather than in the view. The two clients ask them
//! differently: a browser reads its own address bar, and a native client is
//! handed a URL on a command line, so [`startup`] has a shape per platform and
//! the rest of the game has one.

use super::*;
use crate::net::link::Link;

/// Set before the event loop starts, like the connection and for the same
/// reason: `App::init` takes no arguments of its own.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) static WORLD: std::sync::Mutex<WorldKind> = std::sync::Mutex::new(WorldKind::Infinite);

/// Choose the world before launching. Native only — a browser has no command
/// line, and its world comes from the server anyway.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_world(mode: WorldKind) {
    *WORLD.lock().unwrap() = mode;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn chosen_world() -> WorldKind {
    *WORLD.lock().unwrap()
}

/// A browser gets the infinite world, and then the server's if it connects.
#[cfg(target_arch = "wasm32")]
pub(crate) fn chosen_world() -> WorldKind {
    WorldKind::Infinite
}

/// Where to connect, as whom, and to which room. Set before the event loop
/// starts, because `App::init` takes no arguments of its own. A one-shot
/// rather than a config store: it is read once.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) static CONNECTION: std::sync::Mutex<Option<Connection>> = std::sync::Mutex::new(None);

/// What a client needs to reach a server: an address, a name, and a room.
#[cfg(not(target_arch = "wasm32"))]
pub struct Connection {
    /// `None` runs offline.
    pub url: Option<String>,
    pub name: String,
    /// Which world on that server. `None` takes whatever the server calls its
    /// default, so a player with nothing to say about rooms still lands
    /// somewhere.
    pub room: Option<String>,
}

/// Point the client at a server before launching it.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_connection(connection: Connection) {
    *CONNECTION.lock().unwrap() = Some(connection);
}

/// What the client does before the first frame: go somewhere, or ask.
pub(crate) enum Start {
    /// Straight into a game, because something said where to go — `--ws` on a
    /// command line, or `?room=` on a page. A stated destination is a choice
    /// already made, and asking again would be the menu getting in the way.
    Join {
        url: String,
        name: String,
        room: Option<String>,
        /// Watch it rather than play in it. A link that says watch is a
        /// different invitation from one that says come and play, and the two
        /// are answered by different messages.
        watch: bool,
    },
    /// Show the menu, with this address filled in and on this page.
    Menu {
        address: String,
        page: menu::Page,
        /// What the make-a-world form should already be describing, for a
        /// link that named a **kind of room** rather than a screen.
        /// `/experiments` is this: a laboratory is a room now, so asking for
        /// one is asking for the form with an answer already given.
        describing: Option<menu::Kind>,
        /// And who should already be able to reach it, for a link that named
        /// **playing alone** — which is the last answer on that same form.
        access: Option<menu::Access>,
    },
}

/// Connect, and ask nothing yet.
///
/// Two shapes because the two links differ: a browser's socket may fail to be
/// constructed at all, and a native one is a thread that starts and may then
/// find nothing there.
#[cfg(target_arch = "wasm32")]
pub(crate) fn dial(url: &str) -> Option<Link> {
    Link::connect(url)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn dial(url: &str) -> Option<Link> {
    Some(Link::connect(url.to_string()))
}

/// On the web nothing needs configuring: the page came from the server, so the
/// server is wherever the page came from. `wss` when the page is `https`, or
/// the browser blocks it as mixed content.
///
/// The room comes from the query string — `?room=lobby` — because that is the
/// one part a page cannot derive from where it was served, and naming it is
/// how a link takes somebody straight to a world. With none, the menu asks.
#[cfg(target_arch = "wasm32")]
pub(crate) fn startup() -> Start {
    use crate::client::route::Route;
    let url = Link::origin_url("/ws").unwrap_or_else(|| "ws://localhost:8080/ws".into());
    let name = crate::net::keep::name().unwrap_or_else(|| "web".into());
    // The address bar is where a browser client is told to go: a link into a
    // match, a link to watch one, or the page on its own.
    match Route::of(&path_name(), &query_string()) {
        Some(Route::Watch(room)) => Start::Join { url, name, room: Some(room.0), watch: true },
        // A lobby and a room are one request: join it, and what comes back is
        // whichever screen the match's phase calls for.
        Some(route) if route.to_join().is_some() => {
            Start::Join { url, name, room: route.to_join().map(|r| r.0.clone()), watch: false }
        }
        Some(Route::Play) => {
            Start::Menu { address: url, page: menu::Page::Play, describing: None, access: None }
        }
        // A solitary world names none, so `/solo` opens the form rather than
        // building something nobody described — see `Route::Solo`. Playing
        // alone is the last answer on that form rather than a page, so this is
        // the same shape as `/experiments` below: the form, one answer given.
        Some(Route::Alone | Route::Solo) => Start::Menu {
            address: url,
            page: menu::Page::Play,
            describing: None,
            access: Some(menu::Access::Solo),
        },
        Some(Route::Lab) => Start::Menu {
            address: url,
            page: menu::Page::Play,
            describing: Some(menu::Kind::Experiment),
            access: None,
        },
        _ => Start::Menu { address: url, page: menu::Page::Home, describing: None, access: None },
    }
}

#[cfg(target_arch = "wasm32")]
/// The path the page was opened at, which is where the client is told to go.
#[cfg(target_arch = "wasm32")]
pub(crate) fn path_name() -> String {
    web_sys::window().and_then(|w| w.location().pathname().ok()).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn query_string() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// On native there is no page to have come from, so the URL is an argument —
/// and without one, the menu asks for it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn startup() -> Start {
    let taken = CONNECTION.lock().unwrap().take();
    let Some(Connection { url, name, room }) = taken else {
        return Start::Menu {
            address: DEFAULT_ADDRESS.into(),
            page: menu::Page::Home,
            describing: None,
            access: None,
        };
    };
    crate::net::keep::remember_name(&name);
    match url {
        // `--ws` is a command line, which has no way to say "watch" yet and
        // does not need one: somebody at a terminal can pass `--room`.
        Some(url) => Start::Join { url, name, room, watch: false },
        None => Start::Menu {
            address: DEFAULT_ADDRESS.into(),
            page: menu::Page::Home,
            describing: None,
            access: None,
        },
    }
}

/// What the native menu offers when nothing has been typed before. The server
/// this repository tells you to run, on the port it tells you to run it on.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const DEFAULT_ADDRESS: &str = "ws://127.0.0.1:8080/ws";

/// An address that works, for a field that would otherwise be blank.
///
/// A hint is a shape; this is a thing you can press enter on. Somebody who has
/// never seen the game should be editing a number rather than inventing a URL.
///
/// Native only, because a browser has no field to fill: its socket comes from
/// the page's own origin, and an address typed there would be a promise the
/// client cannot keep.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_address() -> &'static str {
    DEFAULT_ADDRESS
}

/// A world of one: granted, and where the camera should be pointing at it.
///
/// Shared by [`App::init`] and by pressing play alone, because the two have to
/// produce the same thing and did not. See [`GameApp::play_alone`].
pub(crate) fn solo_world() -> (World, (f32, f32)) {
    solo_world_of(chosen_world())
}

/// The same, on a world somebody described rather than one the command line
/// asked for — see the make-a-world form, which points here when there is no
/// server to point at.
pub(crate) fn solo_world_of(kind: WorldKind) -> (World, (f32, f32)) {
    let mut world = kind.build();
    if crate::net::too_cramped_for_grants(&world) {
        log::warn!("this world is too small for every player to get a square of their own");
    }
    // Placing is confined to a player's own territory, so an offline game
    // needs the grant a server would have made. Without it there is no
    // opening move: nothing is owned, so nothing may be placed, so nothing
    // ever comes to own anything.
    // Offline is a game of one, and a game of one has no teams to seat by.
    crate::net::grant(&mut world, PlayerId(1));
    // And look at it. Where a grant lands depends on the shape of the world,
    // so this is read back rather than assumed -- the same reason `Welcome`
    // carries the spawn for a connected client.
    let home = middle_of(crate::net::spawn_for(PlayerId(1), &world));
    (world, home)
}

/// The middle of a granted patch, as the camera wants it: (x, y), which is
/// (col, row) the other way round.
pub(crate) fn middle_of((row, col): (i32, i32)) -> (f32, f32) {
    let half = crate::net::SPAWN_N as f32 / 2.0;
    (col as f32 + half, row as f32 + half)
}
