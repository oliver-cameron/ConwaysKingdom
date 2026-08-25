//! What a client keeps between visits.
//!
//! Four things, and they are all here rather than in four places because they
//! are one question — *who was I, and where* — asked from two sides. The
//! rejoin token per room, which room was last played, which server was last
//! reached, and what name was typed. The menu writes the last two and reads
//! all four; the join path reads the token.
//!
//! Two stores, because the two clients have nothing in common here: a browser
//! has `localStorage` and no filesystem, and a native client has a filesystem
//! and no browser. Both answer the same questions, so the client above them
//! asks once and does not care which it got.
//!
//! Losing any of it is a nuisance rather than a disaster: you rejoin as
//! somebody new, and your old ground sits there, yours and out of reach. Which
//! is exactly why it is written down at all.
//!
//! **A token is filed under its room.** A room is a separate world with its
//! own player numbers, so one secret for the whole server would offer a token
//! minted in one world to a server that keeps its players in another — where
//! it matches nobody, joins you as somebody new, and overwrites the token that
//! would have got you back. A token that returns you to the wrong room is
//! worse than no token, so the room is part of where it is kept rather than
//! something to check after the fact.
//!
//! Not keyed by *server*, though, and that is a gap rather than a decision:
//! two servers both running a room called `main` share one secret, and
//! visiting the second costs you your player on the first. The address is
//! remembered now, which is half of what that key would need; making it the
//! other half of the token's is the work that is left.

/// Where all of this is kept, when somewhere other than the usual place is
/// wanted. Native only, and set before the client starts.
///
/// Two clients on one machine otherwise share one store and so try to be one
/// player. The server refuses that — the second joins as somebody new — but
/// then neither can come back to the right player afterwards, and testing two
/// players on one machine wants two identities that both persist.
#[cfg(not(target_arch = "wasm32"))]
static OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
pub fn keep_in(dir: std::path::PathBuf) {
    *OVERRIDE.lock().unwrap() = Some(dir);
}

/// The token kept for this room, if any.
pub fn token(room: &str) -> Option<String> {
    imp::token(room).filter(|t| !t.is_empty())
}

/// The token to offer when asking to join `room`.
///
/// Naming a room is easy: it is that room's token. Naming none is the awkward
/// case — the server decides where you go, so the client cannot look up the
/// right secret before it has been told, and by the time it is told it has
/// already joined as somebody new.
///
/// So it offers the last room's. A client that names no room means "wherever
/// you would put me", which is nearly always where it was last time, and a
/// token the server does not recognise is not an error — it joins you as
/// somebody new, exactly as having no token at all would. The wrong guess
/// therefore costs nothing that was not already lost, and the right one is the
/// common case.
pub fn token_for_join(room: Option<&str>) -> Option<String> {
    match room {
        Some(room) => token(room),
        None => token(&last_room()?),
    }
}

/// Keep this room's token for next time, and remember the room.
///
/// Failing to is worth a line in the log and nothing more — a client that
/// could not write a file still plays perfectly well today, and only pays for
/// it on its next visit.
pub fn store_token(room: &str, token: &str) {
    if let Err(e) = imp::store_token(room, token) {
        log::warn!("could not keep the rejoin token ({e}); a reconnect will start fresh");
        // And do not remember the room either. A room recorded as the last one
        // visited, with no token kept for it, is a name that will be offered
        // an empty secret for as long as it stands — which is worse than
        // having recorded nothing, since it also hides the room before it.
        return;
    }
    set("last-room", room);
}

/// The room this client last joined, if it has joined one.
pub fn last_room() -> Option<String> {
    get("last-room")
}

/// The address last connected to, so the menu opens on it rather than on a
/// blank field. Nothing else reads it: it is a convenience, and a wrong one
/// costs a retype.
pub fn server() -> Option<String> {
    get("server")
}

pub fn remember_server(address: &str) {
    set("server", address);
}

/// The name last played under.
pub fn name() -> Option<String> {
    get("name")
}

pub fn remember_name(name: &str) {
    set("name", name);
}

/// Every game this client has finished, as lines — the format is
/// [`crate::client::record`]'s business, not this module's.
///
/// Kept here because this is where a client keeps what it has, and a second
/// store beside it would be a second answer to "where does the browser put
/// things" for no reason. Empty rather than `None` when there is nothing: a
/// history that has not started and one that could not be read are the same
/// thing to a home screen, and both draw nothing.
pub fn games() -> String {
    imp::get(&field("games")).unwrap_or_default()
}

pub fn remember_games(lines: &str) {
    set("games", lines);
}

/// The field name, so the one caller that needs an untrimmed value can ask for
/// it the same way [`get`] does.
fn field(name: &str) -> String {
    name.to_string()
}

fn get(field: &str) -> Option<String> {
    imp::get(field).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn set(field: &str, value: &str) {
    if let Err(e) = imp::set(field, value) {
        log::debug!("could not keep {field} ({e})");
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    /// Namespaced, because a page may be serving more than this.
    ///
    /// Tokens sit under their own prefix with the room as the last segment.
    /// Everything else is a plain field, and the two prefixes are **separate**
    /// rather than nested: keyed as `conwayskingdom.token.last-room` a field
    /// would be the token of a room called `last-room`, which is a name
    /// somebody may well use.
    fn token_key(room: &str) -> String {
        format!("conwayskingdom.token.{room}")
    }

    fn field_key(field: &str) -> String {
        format!("conwayskingdom.{field}")
    }

    fn storage() -> Option<web_sys::Storage> {
        // `local_storage` is an error, not `None`, when a browser has storage
        // switched off or the page is in a context that forbids it.
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn token(room: &str) -> Option<String> {
        storage()?.get_item(&token_key(room)).ok().flatten()
    }

    pub fn store_token(room: &str, token: &str) -> Result<(), String> {
        write(&token_key(room), token)
    }

    pub fn get(field: &str) -> Option<String> {
        storage()?.get_item(&field_key(field)).ok().flatten()
    }

    pub fn set(field: &str, value: &str) -> Result<(), String> {
        write(&field_key(field), value)
    }

    fn write(key: &str, value: &str) -> Result<(), String> {
        storage()
            .ok_or_else(|| "no local storage".to_string())?
            .set_item(key, value)
            .map_err(|_| "local storage refused the write".to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    /// Beside the rest of a user's data rather than in the working directory,
    /// so running the client from somewhere else does not lose the player.
    fn base() -> Option<PathBuf> {
        if let Some(chosen) = super::OVERRIDE.lock().unwrap().clone() {
            return Some(chosen);
        }
        let home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(home.join("conwayskingdom"))
    }

    /// Tokens go in their own directory, so a room can never be named the same
    /// thing as a field and take its file.
    ///
    /// The room is the file name and it reaches the filesystem, so it is put
    /// through [`crate::net::room_name`] first — a name that is not one gets
    /// no store rather than a path that escapes the directory.
    fn token_path(room: &str) -> Option<PathBuf> {
        let room = crate::net::room_name(room).ok()?;
        Some(base()?.join("tokens").join(room))
    }

    pub fn token(room: &str) -> Option<String> {
        Some(std::fs::read_to_string(token_path(room)?).ok()?.trim().to_string())
    }

    pub fn store_token(room: &str, token: &str) -> Result<(), String> {
        write(token_path(room).ok_or("nowhere to keep it")?, token)
    }

    pub fn get(field: &str) -> Option<String> {
        Some(std::fs::read_to_string(base()?.join(field)).ok()?.trim().to_string())
    }

    pub fn set(field: &str, value: &str) -> Result<(), String> {
        write(base().ok_or("nowhere to keep it")?.join(field), value)
    }

    fn write(path: PathBuf, value: &str) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, value).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    /// One test rather than several, because the store's location is a process
    /// global and cargo runs the tests of one binary in parallel threads: two
    /// of them pointing it at two directories would take turns and neither
    /// would be testing what it thought.
    #[test]
    fn what_a_client_keeps_between_visits() {
        let dir = std::env::temp_dir().join(format!("ck-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::keep_in(dir.clone());

        // Two rooms are two secrets. One store for the whole server would
        // offer a token minted in one world to a server keeping its players in
        // another.
        super::store_token("main", "aaaa");
        super::store_token("lobby", "bbbb");
        assert_eq!(super::token("main").as_deref(), Some("aaaa"));
        assert_eq!(super::token("lobby").as_deref(), Some("bbbb"));
        assert_eq!(super::token("nowhere"), None, "a room never visited has no token");

        // A client that names no room offers the last room's: it cannot know
        // which room the server will put it in until it is already in one, and
        // by then it has joined as somebody new.
        assert_eq!(super::last_room().as_deref(), Some("lobby"), "the latest store wins");
        assert_eq!(super::token_for_join(None).as_deref(), Some("bbbb"));
        assert_eq!(super::token_for_join(Some("main")).as_deref(), Some("aaaa"));
        assert_eq!(super::token_for_join(Some("elsewhere")), None, "or none at all");

        // The menu's two fields, so it opens on what was last used.
        assert_eq!(super::server(), None);
        assert_eq!(super::name(), None);
        super::remember_server("ws://example:8080/ws");
        super::remember_name("hugh");
        assert_eq!(super::server().as_deref(), Some("ws://example:8080/ws"));
        assert_eq!(super::name().as_deref(), Some("hugh"));

        // A room and a field may share a name without sharing a file, which is
        // what the separate directory is for.
        super::store_token("name", "cccc");
        assert_eq!(super::token("name").as_deref(), Some("cccc"));
        assert_eq!(super::name().as_deref(), Some("hugh"), "the field is untouched");

        // A name that could escape the directory gets no store at all, rather
        // than a path outside it — and does not become the room a later join
        // asks after, since nothing was kept for it.
        super::store_token("../escape", "dddd");
        assert_eq!(super::token("../escape"), None);
        assert!(!dir.parent().unwrap().join("escape").exists());
        assert_eq!(super::last_room().as_deref(), Some("name"), "a failed store remembers nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
