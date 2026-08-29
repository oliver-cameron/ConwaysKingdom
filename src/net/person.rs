//! Who somebody is, as against which seat they are sitting in.
//!
//! [`PlayerId`] is a seat: four bits in a cell, one to fifteen, per world, and
//! handed to whoever fills the room next. A rejoin token is a seat's claim
//! ticket, filed per room, and a match *is* a room — so a number kept against
//! a token is earned in a match and thrown away with it. Neither of them is a
//! person, and a rating is a fact about a person.
//!
//! So: a **person** is an id and a proof. The id is public — it is what a
//! rating table is keyed by and what a leaderboard would show — and the proof
//! is the secret that says the id is yours. Both are minted by the server, on
//! a first join, and handed back once.
//!
//! ## A bearer secret, and why not a keypair
//!
//! The proof crosses the wire on every join, which means the server sees it
//! and so does anything between. That is fine here: the socket is `wss`
//! wherever the page is `https` — see [`link::Link::origin_url`] — so a
//! deployed server is already TLS, and putting a cipher of our own inside a
//! WebSocket would be strictly worse than the one underneath it.
//!
//! A keypair — sign a server nonce, never send the secret — was the
//! alternative and buys less than it looks. The private key would live in the
//! same `localStorage` the proof does, so against anybody reading their own
//! browser storage the two are equally weak, and the thing being protected is
//! a game rating. Where a keypair genuinely wins is being the same person on
//! every server without any of them vouching for you, which is [many
//! servers](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#many-servers-and-what-must-not-be-decentralised)
//! and not this. **Identity is per server here, deliberately**, and going
//! global later is a new join message rather than a migration of this one.
//!
//! ## Transferable, because a bearer secret already is
//!
//! Two strings you present to be believed are two strings anybody can carry
//! anywhere, so the only question was whether to admit it. Admitting it is
//! better: `localStorage` is not durable — clearing site data loses it, a
//! managed browser may clear it for you — and playing on a phone and a laptop
//! should not be two people. So [`Person::key`] writes the pair as one line to
//! copy and [`Person::parse`] reads it back, and that *is* the export format
//! rather than a second one bolted beside it.
//!
//! What it costs is that two devices can hold one person at once, and the
//! answer to that is the answer the seat already gives: a person says who you
//! are, a seat is per room and is governed by who is online, and
//! `Server::join_with` already refuses to let two connections be one player.
//! The second device gets a seat of its own or takes the first's, exactly as a
//! second join always did.
//!
//! [`PlayerId`]: crate::sim::PlayerId
//! [`link::Link::origin_url`]: crate::net::link::Link::origin_url

use serde::{Deserialize, Serialize};

/// How many hex characters each half is. Two 64-bit halves, which is what
/// `server::new_token` produces.
const HALF: usize = 32;

/// What joins the two halves in a written-down key. Not a character that
/// appears in hex, so a split can never be ambiguous.
const JOIN: char = '-';

/// The public half: who somebody is.
///
/// Shown, stored against results, and safe to hand out. Knowing an id lets you
/// name a person and does not let you be them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PersonId(pub String);

impl PersonId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PersonId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An id and the secret that says it is yours.
///
/// Held by the client, presented on every join, and never stored in a world —
/// a world persists seats, and a seat records only the [`PersonId`] sitting in
/// it. The proof lives in the server's own table and in the client's store,
/// and nowhere else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: PersonId,
    pub proof: String,
}

impl Person {
    /// The pair as one line, to copy to another browser or another machine.
    ///
    /// Deliberately the whole credential and not a reference to it: there is
    /// no account to recover through and no address to send anything to, so a
    /// key that was only a pointer would point at nothing. Whoever holds this
    /// is this person, which is the same bargain the rejoin token has always
    /// made and is worth saying out loud wherever it is shown.
    pub fn key(&self) -> String {
        format!("{}{JOIN}{}", self.id, self.proof)
    }

    /// Read a key back, or say why it is not one.
    ///
    /// Whitespace either side is trimmed and case is normalised, because this
    /// arrives by copy and paste and a trailing newline is not a different
    /// key. Nothing else is forgiven: a key that is nearly right is not a key,
    /// and quietly accepting one would hand somebody an identity that is not
    /// the one they meant to bring.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim().to_ascii_lowercase();
        let Some((id, proof)) = raw.split_once(JOIN) else {
            return Err(format!("a player key is two halves joined by {JOIN:?}"));
        };
        for (half, what) in [(id, "first"), (proof, "second")] {
            if half.len() != HALF {
                return Err(format!(
                    "the {what} half of a player key is {HALF} characters; this one is {}",
                    half.len()
                ));
            }
            if let Some(bad) = half.chars().find(|c| !c.is_ascii_hexdigit()) {
                return Err(format!("a player key is hexadecimal; {bad:?} is not"));
            }
        }
        Ok(Self { id: PersonId(id.to_string()), proof: proof.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(id: &str, proof: &str) -> Person {
        Person { id: PersonId(id.into()), proof: proof.into() }
    }

    /// The whole of what a transfer format has to do: survive being written
    /// down and read back, so a key copied out of one browser is the same
    /// person in another.
    #[test]
    fn a_key_survives_being_written_down() {
        let person = made(&"a1".repeat(16), &"9f".repeat(16));
        let key = person.key();
        assert_eq!(key.len(), HALF * 2 + 1);
        assert_eq!(Person::parse(&key).unwrap(), person);
    }

    /// It arrives by copy and paste, so it arrives with whatever the clipboard
    /// picked up around it.
    #[test]
    fn a_pasted_key_is_forgiven_its_edges() {
        let person = made(&"ab".repeat(16), &"cd".repeat(16));
        let key = person.key();
        for messy in [format!("  {key}"), format!("{key}\n"), format!("\t{key}  \r\n")] {
            assert_eq!(Person::parse(&messy).unwrap(), person, "{messy:?}");
        }
        // And upper case, which is what a key read aloud or retyped comes back
        // as. Hex is hex either way.
        assert_eq!(Person::parse(&key.to_ascii_uppercase()).unwrap(), person);
    }

    /// A key that is nearly right is not a key. Accepting one would hand
    /// somebody an identity that is not the one they meant to bring, and they
    /// would find out when their record was empty.
    #[test]
    fn a_key_that_is_not_one_says_so() {
        let good = made(&"ab".repeat(16), &"cd".repeat(16)).key();
        for (bad, why) in [
            (String::new(), "empty"),
            ("ab".repeat(16), "no separator"),
            (format!("{}-", "ab".repeat(16)), "no second half"),
            (format!("{good}-{good}"), "too long"),
            (format!("{}-{}", "ab".repeat(15), "cd".repeat(16)), "short first half"),
            (format!("{}-{}", "ab".repeat(16), "cd".repeat(15)), "short second half"),
            (format!("{}-{}", "zz".repeat(16), "cd".repeat(16)), "not hexadecimal"),
        ] {
            assert!(Person::parse(&bad).is_err(), "{why}: {bad:?} was accepted");
        }
    }

    /// The separator is not a hex digit, so the split can never fall in the
    /// wrong place however the halves are made.
    #[test]
    fn the_separator_cannot_appear_in_a_half() {
        assert!(!JOIN.is_ascii_hexdigit());
    }
}
