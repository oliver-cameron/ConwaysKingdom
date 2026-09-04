//! What a client keeps between visits.
//!
//! One question — *who was I, and where* — asked from two sides, so it is one
//! module: the [`Secret`] that says who, the name last typed, the server and
//! room last reached, the record, and the stamp library.
//!
//! Two stores, because the two clients have nothing in common here: a browser
//! has `localStorage` and no filesystem, and a native client has a filesystem
//! and no browser. Both answer the same questions, so the client above them
//! asks once and does not care which it got.
//!
//! **There is no rejoin token any more**, and its going is the whole shape of
//! what changed. A token said *which seat in which room*, so it was filed per
//! room — one secret for the whole server would have offered a token minted in
//! one world to a server keeping its players in another, where it matched
//! nobody and overwrote the one that would have got you back. It was also not
//! keyed by *server*, so two servers running a room called `main` shared one
//! and visiting the second cost you your player on the first.
//!
//! A secret says *who*, and a server finds the seat from that. One secret for
//! everything, no room in the key, and the per-server hole closed by there
//! being nothing per-room to collide. See [`crate::net::auth`].
//!
//! Losing the secret is the one loss that matters: you rejoin as somebody new,
//! and your old ground sits there, yours and out of reach. Which is exactly
//! why it is written down at all.

use crate::net::Secret;

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

/// Taken by every test that touches the store, so they do not race.
///
/// The store is **one per process** — a directory on native, `localStorage` in
/// a browser — which is right for a client and wrong for a test suite that
/// runs its tests in parallel threads of one process. Two tests calling
/// [`keep_in`] interleave, and a third that merely *reads* (building a `Menu`
/// reads the remembered name and address) picks up whichever directory won.
///
/// The symptom is a test that passes on one machine and fails on another,
/// which is the worst kind: it looks like the code differing when it is the
/// scheduling.
#[cfg(test)]
pub fn lock_store() -> std::sync::MutexGuard<'static, ()> {
    static STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that panics while holding it poisons it; the next test still
    // wants the lock, and the poisoning tells it nothing it can act on.
    STORE.lock().unwrap_or_else(|e| e.into_inner())
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

/// What the key is filed under.
///
/// It was `id_ed25519`, on the argument that natively this **is** a file and
/// an OpenSSH name is one somebody recognises. That argument stopped holding
/// when the keypair went: what is in there now is thirty-two hex characters,
/// so anybody who found it and reached for `ssh-keygen` was told it is not a
/// key — by a name this client chose to mislead them with.
const KEY_FIELD: &str = "player-key";

/// The name it used to have, read when the current one is empty.
///
/// **Read, never written.** Losing a key is losing the person, and there is no
/// account behind it to ask — so a client that already had one under the old
/// name keeps it, and writes it back under the new one the first time it is
/// asked for. See [`secret`].
const KEY_FIELD_WAS: &str = "id_ed25519";

/// Where the key file is, for a client that can say so.
///
/// Native only, and not because the browser's answer is different — it is
/// that the browser has no answer. `localStorage` is not a path and nothing
/// can be pointed at it.
#[cfg(not(target_arch = "wasm32"))]
pub fn secret_path() -> Option<std::path::PathBuf> {
    Some(imp::base()?.join(KEY_FIELD))
}

/// This client's key, if it has one.
///
/// **One, not one per server**, which is the whole of what changed when the
/// key stopped being something a server issued. A signature proves who you are
/// to anybody who cares to check, so there is nothing to file under a server's
/// name — and filing it that way would have made you a different person on
/// every machine you visited, which is exactly the bug this replaced.
///
/// Stored as the written key, which is what [`Key::written`] produces, so
/// exporting an identity is reading this field and importing one is writing
/// it. One format rather than a store format and a transfer format that have
/// to agree.
pub fn secret() -> Option<Secret> {
    if let Some(key) = imp::get(KEY_FIELD).and_then(|k| Secret::read(k.trim()).ok()) {
        return Some(key);
    }
    // Under the name it used to have. Moved across on the way past rather than
    // read from there for ever: one write, and the old name stops mattering.
    let key = Secret::read(imp::get(KEY_FIELD_WAS)?.trim()).ok()?;
    remember_secret(&key);
    set(KEY_FIELD_WAS, "");
    Some(key)
}

/// The key this client will use, making one if it has none.
///
/// **Made at startup, not on the first join.** It used to be made on first
/// use, on the reasoning that a client which never reaches a server never
/// needs one — and that stopped being true when the key became the thing
/// everything is filed against. A record of games played and a library of
/// stamps both exist without a server ever being reached, and both want an
/// owner; see [profiles]. Until this ran at startup there was also nothing for
/// the settings screen to show, so the one control that lets somebody carry
/// their identity to another machine was blank until they had already played
/// somewhere.
///
/// Returns `None` where there is no entropy to make one from, which on the web
/// means a page with no `crypto`. A client with no key still plays; it is just
/// nobody a server will remember.
///
/// [profiles]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#player-profiles
pub fn secret_or_new() -> Option<Secret> {
    if let Some(key) = secret() {
        return Some(key);
    }
    let key = Secret::new()?;
    remember_secret(&key);
    // Read back rather than returned directly: if the store refused the write,
    // this client is about to be somebody new on its next visit and the log
    // should say so once rather than never.
    if self::secret().is_none() {
        log::warn!("could not keep this client's key; it will be somebody new next time");
    }
    Some(key)
}

pub fn remember_secret(key: &Secret) {
    set(KEY_FIELD, &key.written());
}

/// Forget who we are. The next join is somebody new, and there is no way back
/// to who we were — see [`Secret::written`].
pub fn forget_key() {
    set(KEY_FIELD, "");
}

/// Forget everything this client has kept: the secret, the record, the name,
/// the server, and the stamps.
///
/// **Not recoverable, and the key is why.** A name and a record are a
/// nuisance to lose; a key is who you are on every server you have ever
/// played on, nobody else holds a copy, and there is no account behind it to
/// ask. Anything calling this should have asked twice.
///
/// Fields are cleared rather than the store emptied, because the store is
/// shared: `localStorage` belongs to the origin and a native store is a
/// directory somebody may have pointed elsewhere with [`keep_in`]. Removing
/// what is ours is the only thing that is ours to do.
pub fn forget_everything() {
    for field in
        [KEY_FIELD, KEY_FIELD_WAS, "name", "server", "games", "stamps", "person", "last-room"]
    {
        set(field, "");
    }
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

/// What the server last called this client, so a screen can say who you are
/// without waiting for the next join.
///
/// **Every screen that draws a person reads this**, and for a while only the
/// settings one did — the home screen and your own profile took the id off the
/// live session, which on the menu is nothing at all. So a client that had
/// been playing for a week opened on a placeholder face and "no server has met
/// you yet", which is the store's answer being ignored rather than absent.
///
/// The **server** issues it — see [`crate::net::auth`] — so a client that has
/// never reached one has none, and one that has visited two servers is showing
/// the last. That second case is the shape of the single-server design and not
/// a bug in this function.
pub fn person() -> Option<crate::net::PersonId> {
    get("person").map(crate::net::PersonId)
}

pub fn remember_person(id: &crate::net::PersonId) {
    set("person", id.as_str());
}

/// The stamp library, as `client::views::stamp::Library` writes it.
///
/// Same untrimmed read as [`games`] and for the same reason: the format is the
/// library's business, and a blank one and an unreadable one both mean an
/// empty library.
pub fn stamps() -> String {
    imp::get(&field("stamps")).unwrap_or_default()
}

pub fn remember_stamps(text: &str) {
    set("stamps", text);
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

    fn field_key(field: &str) -> String {
        format!("conwayskingdom.{field}")
    }

    fn storage() -> Option<web_sys::Storage> {
        // `local_storage` is an error, not `None`, when a browser has storage
        // switched off or the page is in a context that forbids it.
        web_sys::window()?.local_storage().ok().flatten()
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
    pub(super) fn base() -> Option<PathBuf> {
        if let Some(chosen) = super::OVERRIDE.lock().unwrap().clone() {
            return Some(chosen);
        }
        let home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
        Some(home.join("conwayskingdom"))
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
    ///
    /// That was said and not enforced, and a second test in `views::menu` did
    /// exactly it — so both took turns and which one lost depended on the
    /// machine. [`super::lock_store`] is the enforcement.
    #[test]
    fn what_a_client_keeps_between_visits() {
        let _store = super::lock_store();
        let dir = std::env::temp_dir().join(format!("ck-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::keep_in(dir.clone());

        // The secret, which is the whole of who this client is. One of them
        // for everything: there is no room in the key any more, because a
        // server finds the seat from the person rather than the other way
        // round.
        let me = super::secret_or_new().expect("no entropy");
        assert_eq!(super::secret(), Some(me.clone()), "a secret was not kept");
        assert_eq!(super::secret_or_new(), Some(me), "a second call made a second person");

        // What a server called us, which only a server can say.
        assert_eq!(super::person(), None, "named before any server had met us");
        super::remember_person(&crate::net::PersonId("3f2a".into()));
        assert_eq!(super::person().map(|p| p.0), Some("3f2a".into()));

        // The menu's two fields, so it opens on what was last used.
        assert_eq!(super::server(), None);
        assert_eq!(super::name(), None);
        super::remember_server("ws://example:8080/ws");
        super::remember_name("hugh");
        assert_eq!(super::server().as_deref(), Some("ws://example:8080/ws"));
        assert_eq!(super::name().as_deref(), Some("hugh"));

        // And forgetting is all of it, which is the one press in the client
        // that cannot be undone.
        super::forget_everything();
        assert_eq!(super::secret(), None);
        assert_eq!(super::person(), None);
        assert_eq!(super::name(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A key kept under the old name is moved, not lost.**
    ///
    /// It was filed as `id_ed25519`, on the argument that an OpenSSH name is
    /// one somebody recognises — which stopped being true when the keypair
    /// went. Losing a key is losing the person and there is no account behind
    /// it to ask, so the old name is read once and written back under the new
    /// one.
    #[test]
    fn a_key_under_the_old_name_is_moved_rather_than_lost() {
        let _lock = super::lock_store();
        let mine = "0123456789abcdef0123456789abcdef";
        super::set(super::KEY_FIELD, "");
        super::set(super::KEY_FIELD_WAS, mine);

        let found = super::secret().expect("a key under the old name was not found");
        assert_eq!(found.written(), mine);
        assert_eq!(super::get(super::KEY_FIELD).as_deref(), Some(mine), "it was not moved");
        assert!(super::get(super::KEY_FIELD_WAS).is_none(), "the old name was left behind");

        // And asking again is the same person, off the new name alone.
        assert_eq!(super::secret().map(|k| k.written()).as_deref(), Some(mine));
    }

    /// **A key this build cannot read is not a key**, so the client is
    /// somebody new — once. An OpenSSH key left over from the scheme that was
    /// removed is exactly this: it is read, refused, and replaced with one
    /// that will keep working.
    #[test]
    fn a_key_that_is_not_one_is_replaced_once() {
        let _lock = super::lock_store();
        super::set(super::KEY_FIELD, "-----BEGIN OPENSSH PRIVATE KEY-----");
        super::set(super::KEY_FIELD_WAS, "");
        assert!(super::secret().is_none(), "nonsense read as a key");

        let made = super::secret_or_new().expect("no entropy");
        assert_eq!(super::secret(), Some(made.clone()), "the new key was not kept");
        assert_eq!(super::secret_or_new(), Some(made), "and it is somebody new every launch");
    }
}
