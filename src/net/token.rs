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

/// Where the token is kept, when somewhere other than the usual place is
/// wanted. Native only, and set before the client starts.
///
/// Two clients on one machine otherwise share one file and so try to be one
/// player. The server refuses that — the second joins as somebody new — but
/// then neither can come back to the right player afterwards, and testing two
/// players on one machine wants two identities that both persist.
#[cfg(not(target_arch = "wasm32"))]
static OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

#[cfg(not(target_arch = "wasm32"))]
pub fn keep_at(path: std::path::PathBuf) {
    *OVERRIDE.lock().unwrap() = Some(path);
}

/// What we last kept, if anything.
pub fn load() -> Option<String> {
    imp::load().filter(|t| !t.is_empty())
}

/// Keep this for next time. Failing to is worth a line in the log and nothing
/// more — a client that could not write a file still plays perfectly well
/// today, and only pays for it on its next visit.
pub fn store(token: &str) {
    if let Err(e) = imp::store(token) {
        log::warn!("could not keep the rejoin token ({e}); a reconnect will start fresh");
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    /// Namespaced, because a page may be serving more than this.
    const KEY: &str = "conwayskingdom.token";

    fn storage() -> Option<web_sys::Storage> {
        // `local_storage` is an error, not `None`, when a browser has storage
        // switched off or the page is in a context that forbids it.
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn load() -> Option<String> {
        storage()?.get_item(KEY).ok().flatten()
    }

    pub fn store(token: &str) -> Result<(), String> {
        storage()
            .ok_or_else(|| "no local storage".to_string())?
            .set_item(KEY, token)
            .map_err(|_| "local storage refused the write".to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    /// Beside the rest of a user's data rather than in the working directory,
    /// so running the client from somewhere else does not lose the player.
    fn path() -> Option<PathBuf> {
        if let Some(chosen) = super::OVERRIDE.lock().unwrap().clone() {
            return Some(chosen);
        }
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(base.join("conwayskingdom").join("token"))
    }

    pub fn load() -> Option<String> {
        let text = std::fs::read_to_string(path()?).ok()?;
        Some(text.trim().to_string())
    }

    pub fn store(token: &str) -> Result<(), String> {
        let path = path().ok_or_else(|| "nowhere to keep it".to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, token).map_err(|e| e.to_string())
    }
}
