//! Who this server has met.
//!
//! **The first thing a server keeps that is not a world.** Everything else it
//! remembers is a room: the ground, the seats in it, the claim ticket for each
//! seat. This is a table of people, and a person outlives the room they were
//! last seen in — which is the whole reason it exists, since a rating kept
//! against a seat belongs to whoever sits there next.
//!
//! One line per person, tab separated, with a version on the front, for the
//! reason [`crate::client::record`] gives for the same choice: a store you can
//! read with `cat` is one you can debug when something has gone wrong with it,
//! and a hex-encoded blob is not. A line this build cannot read is skipped
//! rather than fatal — losing one person out of a table is a nuisance, and
//! refusing to start because one line is from the future is not.
//!
//! ## What is stored
//!
//! A secret and the id this server issued for it. **Both**, and that is worth
//! stating plainly rather than being discovered: this file is the thing an
//! attacker who reached the disk would want, because a secret in it is a
//! player they can be.
//!
//! It is not a new exposure. A room file already holds a rejoin token per
//! seat, which is the same bargain with a smaller blast radius, and this
//! replaces those. What it is, is a **single-server** design: a server that
//! knows your secret can be you on any other server that has met it, and there
//! is one. Before there are two, this has to change — see the note on
//! [`crate::net::auth`].
//!
//! The id is issued here and is random, so it says nothing about the secret it
//! stands for; that is what makes it safe to show in a lobby.
//!
//! This table **gates nothing**. A secret it has not seen is a person it has
//! not met, and the answer to that is to write one down rather than refuse
//! it.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::net::{PersonId, Secret};

/// The format of a stored line, so a build that cannot read one can tell that
/// rather than mis-splitting it.
///
/// Bumped to 3 when the id stopped being a public key and became something
/// this server issues — so a secret sits beside it, and a version 2 line names
/// a person whose secret nobody has.
const VERSION: u8 = 3;

/// Everybody this server has seen, and what it calls them.
#[derive(Default)]
pub struct People {
    /// The secret somebody presents, and the id this server issued for it.
    known: HashMap<Secret, PersonId>,
}

impl People {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Who this secret is, issuing an id if this server has not seen it.
    ///
    /// The second half of the return says whether they are **new**, which the
    /// caller wants so it can put a first-time player on disk before they rely
    /// on having been here: a rating earned by somebody the server forgets on
    /// a restart is worse than none.
    ///
    /// Records rather than decides. A secret it has not met is a person it has
    /// not met, and the answer is to write one down.
    pub fn meet(&mut self, secret: &Secret) -> (PersonId, bool) {
        if let Some(id) = self.known.get(secret) {
            return (id.clone(), false);
        }
        // Random rather than counted, so an id says nothing about the secret
        // behind it or about how many people came before. Retried against the
        // table, because two people sharing an id would be one person.
        let id = (0..10)
            .map(|_| PersonId(crate::server::new_token()))
            .find(|id| !self.known.values().any(|seen| seen == id))
            .unwrap_or_else(|| PersonId(crate::server::new_token()));
        log::info!("a player this server has not met: {id}");
        self.known.insert(secret.clone(), id.clone());
        (id, true)
    }

    /// Whether this server has issued this id.
    pub fn knows(&self, id: &PersonId) -> bool {
        self.known.values().any(|seen| seen == id)
    }

    /// The table as it is written down.
    pub fn to_lines(&self) -> String {
        // Sorted by id, so a save is the same bytes for the same table and a
        // diff between two of them says what changed rather than what moved.
        let mut rows: Vec<_> = self.known.iter().collect();
        rows.sort_by(|a, b| a.1.cmp(b.1));
        rows.iter().map(|(s, id)| format!("{VERSION}\t{id}\t{}\n", s.written())).collect()
    }

    /// Read a table back, skipping any line this build cannot make sense of.
    pub fn from_lines(text: &str) -> Self {
        let mut known = HashMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some(v), Some(id), Some(secret), None)
                    if v.parse::<u8>() == Ok(VERSION) && !id.is_empty() =>
                {
                    match Secret::read(secret) {
                        Ok(secret) => {
                            known.insert(secret, PersonId(id.to_string()));
                        }
                        Err(why) => {
                            log::warn!("line {} of the people file: {why}", n + 1)
                        }
                    }
                }
                _ => log::warn!("skipped line {} of the people file", n + 1),
            }
        }
        Self { known }
    }

    /// Read the table beside a rooms directory. A table that is not there yet
    /// is an empty one: a server with no people is a server nobody has played
    /// on, which is where every server starts.
    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Self::from_lines(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_lines())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(n: &str) -> Secret {
        Secret::read(&format!("{n}{}", "0".repeat(32 - n.len()))).expect("not a secret")
    }

    /// This table records and never decides. A secret it has not seen is a
    /// person it has not met, and the answer is to issue an id and write the
    /// pairing down -- the reversal from when the server minted keys and an
    /// unknown one had to be refused.
    #[test]
    fn an_unknown_person_is_new_rather_than_wrong() {
        let mut people = People::new();
        let (id, fresh) = people.meet(&secret("a"));
        assert!(fresh, "a first visit was not new");
        assert!(people.knows(&id));
        let (again, fresh) = people.meet(&secret("a"));
        assert!(!fresh, "a second visit was called new");
        assert_eq!(again, id, "one secret got two names");
        assert_eq!(people.len(), 1);
    }

    /// **Two people are two ids.** An issuer that repeated itself would make
    /// two players one person, and nothing would look wrong until both of them
    /// were rated.
    #[test]
    fn two_secrets_get_two_names() {
        let mut people = People::new();
        let a = people.meet(&secret("a")).0;
        let b = people.meet(&secret("b")).0;
        assert_ne!(a, b);
    }

    /// **An id says nothing about the secret behind it**, which is what makes
    /// it safe to show in a lobby. It is issued at random rather than derived,
    /// so this is true by construction; the test is here because the day
    /// somebody makes it a hash of the secret is the day that stops holding.
    #[test]
    fn an_id_does_not_contain_its_secret() {
        let mut people = People::new();
        let factory = secret("dead");
        let id = people.meet(&factory).0;
        assert!(!id.as_str().contains(&factory.written()));
        assert!(!factory.written().contains(id.as_str()));
    }

    /// Written down and read back, and the same bytes each time so one save is
    /// comparable with the one before it.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut people = People::new();
        let ids: Vec<PersonId> =
            ["a", "b", "c"].iter().map(|n| people.meet(&secret(n)).0).collect();
        let lines = people.to_lines();
        assert_eq!(lines, people.to_lines(), "two saves of one table differ");

        let mut back = People::from_lines(&lines);
        assert_eq!(back.len(), 3);
        for (n, id) in ["a", "b", "c"].iter().zip(&ids) {
            assert!(back.knows(id), "{n} was lost");
            // And the same secret still gets the same name after a restart,
            // which is the whole job: a rating filed against an id is worth
            // nothing if the id moves.
            assert_eq!(back.meet(&secret(n)).0, *id, "{n} was renamed by a restart");
        }
    }

    /// A table that is not there yet is an empty one: a server with no people
    /// is a server nobody has played on, which is where every server starts.
    #[test]
    fn a_table_that_is_not_there_is_empty() {
        let missing = std::env::temp_dir()
            .join(format!("ck-people-{}", std::process::id()))
            .join("people.tsv");
        let _ = std::fs::remove_dir_all(missing.parent().unwrap());
        assert!(People::load(&missing).unwrap().is_empty());
    }

    /// A line this build cannot read is skipped rather than fatal: losing one
    /// person out of a table is a nuisance, and refusing to start because one
    /// line is from the future is not.
    #[test]
    fn an_unreadable_line_is_skipped_and_the_rest_kept() {
        let mut people = People::new();
        let id = people.meet(&secret("a")).0;
        let mixed = format!("{}\n99\tfrom-the-future\tzz\nrubbish\n", people.to_lines().trim());
        let back = People::from_lines(&mixed);
        assert_eq!(back.len(), 1);
        assert!(back.knows(&id));
    }
}
