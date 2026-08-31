//! Which screen the browser's address bar is pointing at.
//!
//! A single-page client with one URL is a client where **the back button
//! leaves the game**, a link can only ever mean "the front door", and a
//! reported bug arrives as "it broke" rather than as an address. None of those
//! are rendering problems and all three are fixed by the same thing: the
//! address bar saying where you are.
//!
//! **Paths, with parameters read as well.** A path is what people expect and
//! what reads as an address rather than as machinery; the objection to it was
//! that the client is one file served out of `--serve`, so `/play` would be a
//! request with no route behind it and would 404 on a refresh. That is a fact
//! about the server and the server was changed: `server::ws::serve_client`
//! answers each of these paths with the page — by name, so an unknown path is
//! still a 404 and not a copy of the client.
//!
//! Query parameters are still read, because `?room=` was the link this game
//! had before it had any others and links do not stop existing when a scheme
//! changes.
//!
//! Native has no address bar, so all of this is a no-op there — deliberately
//! not `cfg`-gated at the call sites, because a client that has to remember
//! which platform it is on before saying where it is would forget.

use crate::net::RoomId;

/// The parameter naming a screen. `room` is the other one and predates this;
/// it is read by the same code and means the same thing it always did.
const SCREEN: &str = "screen";
const ROOM: &str = "room";
const WATCH: &str = "watch";
const LOBBY: &str = "lobby";

/// Where the client is, as an address can say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Your name and your record.
    Home,
    /// A server and what is on it.
    Play,
    /// Describing a world to play in on your own.
    Alone,
    /// In a solitary world.
    ///
    /// **A screen with no room in it, which is why it needed a route of its
    /// own.** Playing alone reported `Home`, so the address bar said `/home`
    /// while you were in a world — and since the way in was a button on the
    /// home screen that built one immediately, the whole solitary game lived
    /// at `/home` and nothing about the address ever changed.
    ///
    /// It names no world, deliberately. There is no id to name, and a shape
    /// in the address would be a link that builds a world rather than one that
    /// reaches a world somebody is in — which is what every other route here
    /// is. Following it opens the form.
    Solo,
    /// In a world, playing.
    Room(RoomId),
    /// Waiting in a match's lobby, before the whistle.
    ///
    /// **Read the same as [`Self::Room`] and written differently.** Following
    /// either does one thing — join that room — because they are one request;
    /// what differs is what the address bar *says* while you are there, which
    /// is the whole point of having addresses per screen. A lobby is a screen
    /// of its own, so it gets a word of its own.
    ///
    /// It follows that a link can be out of date: send somebody `?lobby=` and
    /// they open it after the whistle, and they are refused for being late
    /// rather than shown a lobby. That is the honest outcome — the refusal
    /// says why — and it is not made better by pretending the two screens are
    /// one address.
    Lobby(RoomId),
    /// Watching a room without a seat in it. Its own route rather than a flag,
    /// because it is a different thing to be handed: "come and play" and "come
    /// and watch" are two invitations.
    Watch(RoomId),
}

impl Route {
    /// The query string this route is, without the leading `?`.
    ///
    /// **A test fixture, not client API.** The client writes a path; what is
    /// live is [`Self::read`], because a `?room=` link made before paths
    /// existed still has to go where it always did. This is how the tests
    /// produce one of those to read back, and it was `pub` on a public type —
    /// a promise to callers that the query form is something to *write*, which
    /// nothing has meant since paths arrived.
    #[cfg(test)]
    fn query(&self) -> String {
        match self {
            Self::Home => format!("{SCREEN}=home"),
            Self::Play => format!("{SCREEN}=play"),
            Self::Alone => format!("{SCREEN}=alone"),
            Self::Solo => format!("{SCREEN}=solo"),
            Self::Room(id) => format!("{ROOM}={id}"),
            Self::Lobby(id) => format!("{LOBBY}={id}"),
            Self::Watch(id) => format!("{WATCH}={id}"),
        }
    }

    /// The path this route is, for the address bar.
    pub fn path(&self) -> String {
        match self {
            Self::Home => "/home".into(),
            Self::Play => "/play".into(),
            Self::Alone => "/alone".into(),
            Self::Solo => "/solo".into(),
            Self::Room(id) => format!("/room/{id}"),
            Self::Lobby(id) => format!("/lobby/{id}"),
            Self::Watch(id) => format!("/watch/{id}"),
        }
    }

    /// What a path says, if it says anything.
    pub fn from_path(path: &str) -> Option<Self> {
        let mut parts = path.trim_matches('/').split('/');
        let head = parts.next()?;
        let tail = parts.next().map(decode).filter(|t| !t.is_empty());
        match (head, tail) {
            ("home", _) => Some(Self::Home),
            ("play", _) => Some(Self::Play),
            ("alone", _) => Some(Self::Alone),
            ("solo", _) => Some(Self::Solo),
            ("room", Some(id)) => Some(Self::Room(RoomId(id))),
            ("lobby", Some(id)) => Some(Self::Lobby(RoomId(id))),
            ("watch", Some(id)) => Some(Self::Watch(RoomId(id))),
            _ => None,
        }
    }

    /// Where a whole address points: the path if it says something, and the
    /// query if it does not.
    ///
    /// The path wins, because it is the newer scheme and the one the client
    /// writes. A query is read for the sake of links made before there were
    /// paths — `?room=` in particular, which is the one this game has had
    /// longest and the one most likely to be sitting in somebody's messages.
    pub fn of(path: &str, query: &str) -> Option<Self> {
        Self::from_path(path).or_else(|| Self::read(query))
    }

    /// What a query string says, if it says anything.
    ///
    /// A room wins over a screen, because a link somebody was *sent* names a
    /// room and a screen is only ever what this client last wrote down for
    /// itself. Somebody following a link into a match should not land on the
    /// menu because the tab that made the link happened to be on it.
    pub fn read(query: &str) -> Option<Self> {
        let pairs: Vec<(&str, &str)> =
            query.trim_start_matches('?').split('&').filter_map(|p| p.split_once('=')).collect();
        let find = |key: &str| {
            pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v).filter(|v| !v.is_empty())
        };
        if let Some(room) = find(WATCH) {
            return Some(Self::Watch(RoomId(decode(room))));
        }
        if let Some(room) = find(LOBBY) {
            return Some(Self::Lobby(RoomId(decode(room))));
        }
        if let Some(room) = find(ROOM) {
            return Some(Self::Room(RoomId(decode(room))));
        }
        match find(SCREEN) {
            Some("play") => Some(Self::Play),
            Some("home") => Some(Self::Home),
            Some("alone") => Some(Self::Alone),
            Some("solo") => Some(Self::Solo),
            _ => None,
        }
    }

    /// Which room to join, if this route names one.
    ///
    /// A lobby and a room are the same request: join that room, and what you
    /// get is whichever screen the match's phase calls for. Watching is not
    /// here, because it is a different message.
    pub fn to_join(&self) -> Option<&RoomId> {
        match self {
            Self::Room(id) | Self::Lobby(id) => Some(id),
            Self::Home | Self::Play | Self::Alone | Self::Solo | Self::Watch(_) => None,
        }
    }
}

/// Percent-decoding, for the one thing that can appear here.
///
/// A room id is letters, digits, `-` and `_` — see [`crate::net::room_name`] —
/// so nothing in a well-formed link needs decoding at all. This exists for the
/// malformed one: a `%` sequence left as-is would become part of an id, which
/// then names no room and refuses with a message about a character the person
/// never typed.
///
/// **Bytes throughout, and that is the fix for a panic.** It used to slice the
/// `&str` — `&raw[i + 1..i + 3]` — after seeing a `%`, and byte `i + 3` can
/// land in the middle of a character: `?room=%€` was three bytes into a
/// four-byte string and Rust refuses that slice by panicking. This runs on
/// `location.pathname` during startup, so a link with a `%` in front of any
/// non-ASCII character killed the wasm module before the first frame — and
/// the page's watchdog then reported it as "the game did not load", which is
/// where a client that cannot reach its server ends up too.
///
/// Decoded as UTF-8 rather than a byte at a time, so `%E2%82%AC` comes back
/// as `€` and not as three of the characters it is spelled with. What cannot
/// be decoded is kept verbatim, so a refusal names what was actually typed.
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match pair {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Where the browser last took us with back or forward, waiting to be acted
/// on.
///
/// A global for the reason the keyboard layout is one: the event arrives on a
/// listener and the thing that can act on it is behind a `RefCell` on the app.
#[cfg(target_arch = "wasm32")]
static WENT_BACK: std::sync::Mutex<Option<Route>> = std::sync::Mutex::new(None);

/// The route the address bar is showing, so a repeat is not a history entry.
#[cfg(target_arch = "wasm32")]
static SHOWING: std::sync::Mutex<Option<Route>> = std::sync::Mutex::new(None);

/// Say where we are, in the address bar.
///
/// **Pushes when the screen changes and replaces when it does not**, which is
/// what makes back a way through the client rather than a way out of it.
///
/// It replaced unconditionally once, on the reasoning that pushing every
/// change would fill the history with the six presses it took to get into a
/// game. That fear was about the wrong thing: a `Route` is already coarse —
/// there is one for each *screen* and none for typing an address, refreshing a
/// list or picking a room — so the entries this actually makes are the three
/// somebody walked through. What it cost was that back could never move
/// between screens, which is the whole reason a client has addresses at all.
///
/// The same route twice is still a replace, because a screen that reports
/// itself every frame would otherwise bury the history in copies of itself.
#[cfg(target_arch = "wasm32")]
pub fn show(route: &Route) {
    let Some(window) = web_sys::window() else { return };
    let Ok(history) = window.history() else { return };
    let url = route.path();

    let moved = {
        let mut showing = match SHOWING.lock() {
            Ok(showing) => showing,
            Err(_) => return,
        };
        let moved = showing.as_ref() != Some(route);
        *showing = Some(route.clone());
        moved
    };

    // Failing is a nuisance and nothing more: some embeddings forbid touching
    // history, and the game plays perfectly well with a stale address.
    let null = wasm_bindgen::JsValue::NULL;
    let wrote = if moved {
        history.push_state_with_url(&null, "", Some(&url))
    } else {
        history.replace_state_with_url(&null, "", Some(&url))
    };
    if wrote.is_err() {
        log::debug!("could not set the address to {url}");
    }
}

/// Listen for the back and forward buttons.
///
/// **The other half of pushing**, and useless without it: the browser changes
/// the address and expects the page to follow, and a single-page client that
/// does not listen shows the old screen under the new address — which is worse
/// than not having addresses, because now they lie.
///
/// Called once at startup. What arrives is drained by [`went_back`].
#[cfg(target_arch = "wasm32")]
pub fn follow_the_back_button() {
    use wasm_bindgen::JsCast;
    let Some(window) = web_sys::window() else { return };
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        let Some(window) = web_sys::window() else { return };
        let location = window.location();
        let path = location.pathname().unwrap_or_default();
        let query = location.search().unwrap_or_default();
        let Some(route) = Route::of(&path, &query) else { return };
        // What the address bar is showing is now this, whoever moved it —
        // otherwise the next `show` would push a duplicate of the screen the
        // browser has just gone back to.
        if let Ok(mut showing) = SHOWING.lock() {
            *showing = Some(route.clone());
        }
        if let Ok(mut went) = WENT_BACK.lock() {
            *went = Some(route);
        }
    });
    if window
        .add_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref())
        .is_err()
    {
        log::debug!("could not listen for the back button");
        return;
    }
    // Handed to the browser for the life of the page. There is nothing to
    // detach it from: the listener outlives every frame that would drop it.
    listener.forget();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn follow_the_back_button() {}

/// Where back or forward has just taken us, if anywhere.
///
/// Drained, so acting on it happens once. Called each frame and answers `None`
/// on all but the ones somebody pressed a button on.
pub fn went_back() -> Option<Route> {
    #[cfg(target_arch = "wasm32")]
    {
        WENT_BACK.lock().ok().and_then(|mut went| went.take())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Native has no address bar.
#[cfg(not(target_arch = "wasm32"))]
pub fn show(_route: &Route) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every route survives being written down and read back — as a path,
    /// which is what the client writes, and as a query, which is what older
    /// links say. Both are the whole of what an address has to do.
    #[test]
    fn a_route_survives_the_address_bar() {
        for route in [
            Route::Home,
            Route::Play,
            Route::Alone,
            Route::Solo,
            Route::Room(RoomId::from("arena")),
            Route::Room(RoomId::from("r-t6n98x")),
            Route::Lobby(RoomId::from("cup")),
            Route::Watch(RoomId::from("lobby")),
        ] {
            let path = route.path();
            assert_eq!(Route::from_path(&path), Some(route.clone()), "{path}");
            assert_eq!(Route::of(&path, ""), Some(route.clone()), "{path}");

            let query = route.query();
            assert_eq!(Route::read(&query), Some(route.clone()), "{query}");
            // And with the `?` a browser hands over, which `location.search`
            // includes and a hand-written link does not.
            assert_eq!(Route::read(&format!("?{query}")), Some(route));
        }
    }

    /// A link somebody was *sent* names a room, and a screen is only ever what
    /// this client last wrote down for itself. Following a link into a match
    /// must not land on the menu because the tab that made it was there.
    #[test]
    fn a_room_in_a_link_beats_a_screen() {
        assert_eq!(Route::read("screen=play&room=arena"), Some(Route::Room(RoomId::from("arena"))));
        assert_eq!(
            Route::read("screen=home&watch=arena"),
            Some(Route::Watch(RoomId::from("arena")))
        );
        // And watching beats joining, since it is the more specific ask.
        assert_eq!(
            Route::read("room=arena&watch=arena"),
            Some(Route::Watch(RoomId::from("arena")))
        );
    }

    /// Nothing to say is `None`, so the client opens where it always did
    /// rather than somewhere a stray parameter suggested.
    #[test]
    fn a_query_that_says_nothing_routes_nowhere() {
        for query in ["", "?", "&&", "utm_source=somewhere", "screen=", "room=", "screen=nonsense"]
        {
            assert_eq!(Route::read(query), None, "{query:?}");
        }
    }

    /// A lobby and a room are one request read and two screens written, which
    /// is what having an address per screen means.
    #[test]
    fn a_lobby_and_a_room_are_the_same_thing_to_follow() {
        let cup = RoomId::from("cup");
        assert_eq!(Route::Lobby(cup.clone()).to_join(), Some(&cup));
        assert_eq!(Route::Room(cup.clone()).to_join(), Some(&cup));
        // And they are two different things to write down.
        assert_ne!(Route::Lobby(cup.clone()).query(), Route::Room(cup.clone()).query());
        // Watching is a different message, so it is not something to join.
        assert_eq!(Route::Watch(cup).to_join(), None);
        assert_eq!(Route::Home.to_join(), None);
    }

    /// A path that names no screen is nothing, so a mistyped address opens
    /// where the client always did rather than somewhere a stray word
    /// suggested. The server 404s these before the client ever sees them; this
    /// is the second half of the same rule.
    #[test]
    fn a_path_that_names_no_screen_routes_nowhere() {
        for path in ["/", "", "/src/main.rs", "/.git/config", "/room", "/room/", "/nonsense"] {
            assert_eq!(Route::from_path(path), None, "{path:?}");
        }
    }

    /// A path wins over a query, because it is what the client writes — but a
    /// query is still read, so a `?room=` link made before paths existed
    /// still goes where it always did.
    #[test]
    fn a_path_wins_and_an_old_link_still_works() {
        assert_eq!(
            Route::of("/play", "room=arena"),
            Some(Route::Play),
            "the path is what this client wrote down"
        );
        assert_eq!(
            Route::of("/", "room=arena"),
            Some(Route::Room(RoomId::from("arena"))),
            "and a link from before paths still means what it meant"
        );
    }

    /// A room id needs no encoding, so this is for the malformed link: left
    /// as-is, a `%` sequence becomes part of an id and refuses with a message
    /// about a character nobody typed.
    /// **A malformed link is not a crash.** Slicing the string by byte offset
    /// after a `%` panics whenever the next character is multi-byte, and this
    /// runs on the address bar before the first frame — so `?room=%€` was a
    /// blank page reported as "the game did not load".
    #[test]
    fn a_link_nobody_could_have_meant_does_not_panic() {
        for query in ["room=%€", "room=%日x", "room=%🙂", "room=%", "room=%A", "room=%ZZ"] {
            let _ = Route::read(query);
        }
        for path in ["/room/%€", "/watch/%🙂", "/lobby/%"] {
            let _ = Route::from_path(path);
        }
    }

    #[test]
    fn a_percent_encoded_room_comes_back_readable() {
        assert_eq!(Route::read("room=my%2Droom"), Some(Route::Room(RoomId::from("my-room"))));
        assert_eq!(Route::read("room=a+b"), Some(Route::Room(RoomId::from("a b"))));
        // A stray percent is kept rather than swallowed, so what comes back is
        // what was typed and the refusal names it.
        assert_eq!(Route::read("room=100%"), Some(Route::Room(RoomId::from("100%"))));
        // And a multi-byte character comes back as itself rather than as the
        // bytes it is spelled with.
        assert_eq!(Route::read("room=%E2%82%AC"), Some(Route::Room(RoomId::from("€"))));
    }
}
