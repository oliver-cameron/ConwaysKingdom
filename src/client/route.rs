//! Which screen the browser's address bar is pointing at.
//!
//! A single-page client with one URL is a client where **the back button
//! leaves the game**, a link can only ever mean "the front door", and a
//! reported bug arrives as "it broke" rather than as an address. None of those
//! are rendering problems and all three are fixed by the same thing: the
//! address bar saying where you are.
//!
//! Query parameters rather than paths, and that is forced rather than
//! preferred. The client is served as one file from whatever directory
//! `--serve` was pointed at; a path like `/play` would be a request the server
//! has no route for and would 404 on a refresh. A parameter is the same
//! document either way, which is what a client with no router can actually
//! honour.
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
    pub fn query(&self) -> String {
        match self {
            Self::Home => format!("{SCREEN}=home"),
            Self::Play => format!("{SCREEN}=play"),
            Self::Room(id) => format!("{ROOM}={id}"),
            Self::Lobby(id) => format!("{LOBBY}={id}"),
            Self::Watch(id) => format!("{WATCH}={id}"),
        }
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
            Self::Home | Self::Play | Self::Watch(_) => None,
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
fn decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { ' ' } else { bytes[i] as char });
        i += 1;
    }
    out
}

/// Say where we are, in the address bar.
///
/// **Replaces rather than pushes.** A client that pushed every screen change
/// would fill the history with the six presses it took to get into a game, and
/// the back button would walk them in reverse rather than leaving — which is
/// not what a back button means to somebody who wants out. What this buys is
/// the address being copyable and refreshable, which is what was actually
/// missing.
#[cfg(target_arch = "wasm32")]
pub fn show(route: &Route) {
    let Some(window) = web_sys::window() else { return };
    let Ok(history) = window.history() else { return };
    let url = format!("?{}", route.query());
    // Failing is a nuisance and nothing more: some embeddings forbid touching
    // history, and the game plays perfectly well with a stale address.
    if history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)).is_err() {
        log::debug!("could not set the address to {url}");
    }
}

/// Native has no address bar.
#[cfg(not(target_arch = "wasm32"))]
pub fn show(_route: &Route) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every route survives being written down and read back, which is the
    /// whole of what an address has to do.
    #[test]
    fn a_route_survives_the_address_bar() {
        for route in [
            Route::Home,
            Route::Play,
            Route::Room(RoomId::from("arena")),
            Route::Room(RoomId::from("r-t6n98x")),
            Route::Lobby(RoomId::from("cup")),
            Route::Watch(RoomId::from("lobby")),
        ] {
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

    /// A room id needs no encoding, so this is for the malformed link: left
    /// as-is, a `%` sequence becomes part of an id and refuses with a message
    /// about a character nobody typed.
    #[test]
    fn a_percent_encoded_room_comes_back_readable() {
        assert_eq!(Route::read("room=my%2Droom"), Some(Route::Room(RoomId::from("my-room"))));
        assert_eq!(Route::read("room=a+b"), Some(Route::Room(RoomId::from("a b"))));
        // A stray percent is kept rather than swallowed, so what comes back is
        // what was typed and the refusal names it.
        assert_eq!(Route::read("room=100%"), Some(Route::Room(RoomId::from("100%"))));
    }
}
