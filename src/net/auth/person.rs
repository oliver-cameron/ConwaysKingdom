//! Who somebody is, across the rooms of one server.
//!
//! Two halves, and which is which is the whole design.
//!
//! A [`Secret`] is what the client keeps and never shows. It is made at
//! startup, it is the only thing that proves you are you, and carrying it to
//! another machine is what makes you the same person there.
//!
//! A [`PersonId`] is what the **server issues** the first time it sees a
//! secret, and is what everybody else is shown: it appears in a lobby, beside
//! a rating, in a result. It reveals nothing about the secret behind it,
//! because it is not derived from one — the server remembers the pairing.
//!
//! ## Why the server issues it
//!
//! This used to be an ed25519 keypair: the client made both halves, the public
//! one *was* the id, and a join was a signature over a nonce the server sent.
//! That buys one thing this does not — a server cannot impersonate you to a
//! *different* server, because it never learns the secret half.
//!
//! With one server that is worth nothing, and it cost a signature scheme, an
//! OpenSSH key parser, a round trip before every join, and a dependency. What
//! is left is exactly as strong as the rejoin token this replaces — whoever
//! holds the secret is you, and the server it is presented to knows it — and
//! that is the strength [networking.md] already argues is right for a game
//! with no accounts.
//!
//! **Before a second server exists, this has to be revisited.** Not because
//! anything here breaks, but because "the server knows your secret" stops
//! being harmless the moment there is another one to be you on. See
//! [planned.md].
//!
//! [networking.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/networking.md#coming-back
//! [planned.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#player-profiles

use serde::{Deserialize, Serialize};

/// How many bytes of secret. Sixteen is what nobody guesses, and is what
/// `server::new_token` already uses for the same job.
const SECRET_N: usize = 16;

/// **What a server calls you, publicly.** Issued on the first join and shown
/// to everybody: in the lobby, beside a rating, on a result.
///
/// Safe to show, log and store, because it says nothing about the secret it
/// stands for. That is the one thing it must keep being true, and it is true
/// by construction rather than by care: the server picked it at random.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PersonId(pub String);

impl PersonId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The short form, for a name that has to fit beside another name.
    ///
    /// Two players may pick the same name and nothing stops them; this is what
    /// tells them apart without either having to accept being Alice2. Four
    /// characters is enough for a room of fifteen and is not meant to be
    /// enough for the world — the whole id is what identifies anybody.
    pub fn short(&self) -> &str {
        let end = self.0.char_indices().nth(4).map_or(self.0.len(), |(i, _)| i);
        &self.0[..end]
    }
}

impl std::fmt::Display for PersonId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// **What the client keeps, and never shows.** Whoever holds this is you.
///
/// Sent to the server on a join and nowhere else. It is not in a world, not in
/// a lobby, and not in a log — see the `Debug` below, which prints the fact of
/// it and not the thing.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Secret(String);

impl Secret {
    /// A new person, from bytes nobody else can guess.
    ///
    /// Randomness per platform rather than through one crate: `getrandom` on
    /// `wasm32-unknown-unknown` needs a feature *and* a `--cfg` flag before it
    /// will admit the browser has an entropy source, and the browser's own is
    /// one call away and is a real CSPRNG.
    pub fn new() -> Option<Self> {
        Some(Self(hex(&bytes()?)))
    }

    /// Read one back from what [`Self::written`] produced, or say why not.
    pub fn read(written: &str) -> Result<Self, String> {
        let raw = written.trim().to_ascii_lowercase();
        if raw.len() != SECRET_N * 2 || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("that is not a key: {} hex characters are", SECRET_N * 2));
        }
        Ok(Self(raw))
    }

    /// The secret, to keep somewhere and to carry to another machine.
    ///
    /// **This is the whole of it**, and it has to be: there is no weaker thing
    /// to export that would still be you at the far end. Whoever holds it is
    /// you on every server that has met it, for ever. Wherever it is shown,
    /// that sentence goes with it.
    pub fn written(&self) -> String {
        self.0.clone()
    }
}

impl std::fmt::Debug for Secret {
    /// Never the thing itself. A secret that prints into a log is a secret in
    /// the log, and logs are copied around by people not thinking about what
    /// is in them.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(target_arch = "wasm32")]
fn bytes() -> Option<[u8; SECRET_N]> {
    let mut out = [0u8; SECRET_N];
    web_sys::window()?.crypto().ok()?.get_random_values_with_u8_array(&mut out).ok()?;
    Some(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn bytes() -> Option<[u8; SECRET_N]> {
    let mut out = [0u8; SECRET_N];
    getrandom::fill(&mut out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of what a transfer format has to do: survive being written
    /// down and read back, so a secret copied out of one browser is the same
    /// person in another.
    #[test]
    fn a_secret_survives_being_written_down() {
        let ours = Secret::new().expect("no entropy");
        assert_eq!(Secret::read(&ours.written()).expect("would not read back"), ours);
        // And spacing and case are what a paste brings with it.
        let messy = format!("  {}  ", ours.written().to_ascii_uppercase());
        assert_eq!(Secret::read(&messy).expect("a pasted secret was refused"), ours);
    }

    /// Two of them are two people. A generator that repeated itself would make
    /// everybody the same person and nothing would look wrong until two of
    /// them were in one room.
    #[test]
    fn two_secrets_are_two_people() {
        let a = Secret::new().expect("no entropy");
        let b = Secret::new().expect("no entropy");
        assert_ne!(a, b);
    }

    /// Anything that is not one says so rather than becoming a person nobody
    /// can be.
    #[test]
    fn what_is_not_a_secret_is_refused() {
        for bad in ["", "hello", "zz", &"a".repeat(31), &"a".repeat(33), "not hex here!!!!"] {
            assert!(Secret::read(bad).is_err(), "{bad:?} was accepted");
        }
    }

    /// **It must not print itself.** A secret in a log is a secret, and the
    /// one thing that reliably puts it there is a `Debug` that includes it.
    #[test]
    fn a_secret_does_not_print_itself() {
        let ours = Secret::new().expect("no entropy");
        let shown = format!("{ours:?}");
        assert!(!shown.contains(&ours.written()), "a secret printed itself: {shown}");
    }

    /// The short form is what tells two people with one name apart, so it has
    /// to be there and it has to be stable.
    #[test]
    fn a_person_has_a_short_name() {
        let id = PersonId("3f2a91c4".into());
        assert_eq!(id.short(), "3f2a");
        // And something shorter than four is itself rather than a panic.
        assert_eq!(PersonId("ab".into()).short(), "ab");
        assert_eq!(PersonId(String::new()).short(), "");
    }
}
