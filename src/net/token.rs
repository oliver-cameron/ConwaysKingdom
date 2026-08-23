//! Where a client keeps the secret it comes back with.
//!
//! Two stores, because the two clients have nothing in common here: a browser
//! has `localStorage` and no filesystem, and a native client has a filesystem
//! and no browser. Both answer the same two questions, so the client above
//! them asks once and does not care which it got.
//!
//! Losing it is not a disaster, only a nuisance: you rejoin as somebody new,
//! and your old ground sits there, yours and out of reach. Which is exactly
//! why it is written down at all.
//!
//! **Filed under the room.** A room is a separate world with its own player
//! numbers, so one secret for the whole server would offer a token minted in
//! one world to a server that keeps its players in another — where it matches
//! nobody, joins you as somebody new, and overwrites the token that would have
//! got you back. A token that returns you to the wrong room is worse than no
//! token, so the room is part of where it is kept rather than something to
//! check after the fact.
//!
//! Not keyed by *server*, though, and that is a gap rather than a decision:
//! two servers both running a room called `main` share one secret, and
//! visiting the second costs you your player on the first. The address a
//! client typed is not remembered anywhere yet — when it is, it belongs here,
//! beside this, rather than in a second place of its own.

/// Where tokens are kept, when somewhere other than the usual place is
/// wanted. A directory, one file per room. Native only, and set before the
/// client starts.
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

/// What we last kept for this room, if anything.
pub fn load(room: &str) -> Option<String> {
    imp::load(room).filter(|t| !t.is_empty())
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
pub fn for_join(room: Option<&str>) -> Option<String> {
    match room {
        Some(room) => load(room),
        None => load(&last_room()?),
    }
}

/// The room this client last joined, if it has joined one.
pub fn last_room() -> Option<String> {
    imp::last_room().filter(|r| !r.is_empty())
}

/// Keep this for next time. Failing to is worth a line in the log and nothing
/// more — a client that could not write a file still plays perfectly well
/// today, and only pays for it on its next visit.
pub fn store(room: &str, token: &str) {
    if let Err(e) = imp::store(room, token) {
        log::warn!("could not keep the rejoin token ({e}); a reconnect will start fresh");
        // And do not remember the room either. A room recorded as the last one
        // visited, with no token kept for it, is a name that will be offered
        // an empty secret for as long as it stands -- which is worse than
        // having recorded nothing, since it also hides the room before it.
        return;
    }
    // Which room this was, so a later join that names none can offer the right
    // secret. Losing this costs the convenience; losing the token costs the
    // player, which is why it is written first and separately.
    if let Err(e) = imp::remember_room(room) {
        log::debug!("could not remember the room ({e})");
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    /// Namespaced, because a page may be serving more than this. The room is
    /// the last segment, so two rooms on one origin keep two secrets.
    fn key(room: &str) -> String {
        format!("conwayskingdom.token.{room}")
    }

    fn storage() -> Option<web_sys::Storage> {
        // `local_storage` is an error, not `None`, when a browser has storage
        // switched off or the page is in a context that forbids it.
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn load(room: &str) -> Option<String> {
        storage()?.get_item(&key(room)).ok().flatten()
    }

    pub fn store(room: &str, token: &str) -> Result<(), String> {
        storage()
            .ok_or_else(|| "no local storage".to_string())?
            .set_item(&key(room), token)
            .map_err(|_| "local storage refused the write".to_string())
    }

    /// A different prefix from the tokens, not a longer one: keyed as
    /// `conwayskingdom.token.last` it would be the token of a room called
    /// `last`, which is a name somebody may well use.
    const LAST_ROOM: &str = "conwayskingdom.room.last";

    pub fn last_room() -> Option<String> {
        storage()?.get_item(LAST_ROOM).ok().flatten()
    }

    pub fn remember_room(room: &str) -> Result<(), String> {
        storage()
            .ok_or_else(|| "no local storage".to_string())?
            .set_item(LAST_ROOM, room)
            .map_err(|_| "local storage refused the write".to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    /// Beside the rest of a user's data rather than in the working directory,
    /// so running the client from somewhere else does not lose the player.
    ///
    /// The room is the file name, and it reaches the filesystem, so it is put
    /// through [`crate::net::room_name`] first — a name that is not one gets
    /// no store rather than a path that escapes the directory.
    fn path(room: &str) -> Option<PathBuf> {
        let room = crate::net::room_name(room).ok()?;
        if let Some(chosen) = super::OVERRIDE.lock().unwrap().clone() {
            return Some(chosen.join(room));
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(base.join("conwayskingdom").join("tokens").join(room))
    }

    pub fn load(room: &str) -> Option<String> {
        let text = std::fs::read_to_string(path(room)?).ok()?;
        Some(text.trim().to_string())
    }

    pub fn store(room: &str, token: &str) -> Result<(), String> {
        let path = path(room).ok_or_else(|| "nowhere to keep it".to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, token).map_err(|e| e.to_string())
    }

    /// Beside the tokens rather than among them, under a name a room cannot
    /// have -- a room called `last` would otherwise overwrite this, and this
    /// would overwrite it.
    fn last_room_path() -> Option<PathBuf> {
        // Any valid room name resolves the directory; the file is then its
        // sibling, so the override and the default are both honoured.
        Some(path("main")?.with_file_name(".last-room"))
    }

    pub fn last_room() -> Option<String> {
        Some(std::fs::read_to_string(last_room_path()?).ok()?.trim().to_string())
    }

    pub fn remember_room(room: &str) -> Result<(), String> {
        let path = last_room_path().ok_or_else(|| "nowhere to keep it".to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, room).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    /// One test rather than several, because the store's location is a process
    /// global and cargo runs tests of one binary in parallel threads: two of
    /// them pointing it at two directories would take turns and neither would
    /// be testing what it thought.
    ///
    /// Two rooms are two secrets. One store for the whole server would offer a
    /// token minted in one world to a server keeping its players in another.
    #[test]
    fn a_token_is_filed_under_its_room() {
        let dir = std::env::temp_dir().join(format!("ck-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::keep_in(dir.clone());

        super::store("main", "aaaa");
        super::store("lobby", "bbbb");
        assert_eq!(super::load("main").as_deref(), Some("aaaa"));
        assert_eq!(super::load("lobby").as_deref(), Some("bbbb"));
        assert_eq!(super::load("nowhere"), None, "a room never visited has no token");

        // A client that names no room offers the last room's token: it cannot
        // know which room the server will put it in until it is already in
        // one, and by then it has joined as somebody new.
        assert_eq!(super::last_room().as_deref(), Some("lobby"), "the latest store wins");
        assert_eq!(super::for_join(None).as_deref(), Some("bbbb"));
        assert_eq!(super::for_join(Some("main")).as_deref(), Some("aaaa"), "a named room's own");
        assert_eq!(super::for_join(Some("elsewhere")), None, "or none at all");

        // A name that could escape the directory gets no store at all, rather
        // than a path outside it -- and does not become the room a later join
        // asks after, since nothing was kept for it.
        super::store("../escape", "cccc");
        assert_eq!(super::load("../escape"), None);
        assert!(!dir.parent().unwrap().join("escape").exists());
        assert_eq!(super::last_room().as_deref(), Some("lobby"), "a failed store remembers nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
