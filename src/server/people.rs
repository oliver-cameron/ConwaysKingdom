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
//! ## What is stored, and what is not
//!
//! Ids, and nothing else. **No secrets at all**, which is not carefulness on
//! this file's part but a consequence of the client issuing its own key: a
//! join is a signature over this server's challenge, so it is checked by
//! arithmetic rather than by looking anything up, and there is nothing here
//! worth stealing. It is a record of who has been seen, which is what a rating
//! table will be keyed by.
//!
//! It follows that this table gates nothing. An id that is not in it is simply
//! new — a person the client made and this server has not met — and the right
//! answer to that is to write it down, not to refuse it. That was the opposite
//! of the answer while the server minted keys, and the reversal is the whole
//! difference between an identity a server hands out and one it merely
//! recognises.

use std::collections::HashSet;
use std::io;
use std::path::Path;

use crate::net::PersonId;

/// The format of a stored line, so a build that cannot read one can tell that
/// rather than mis-splitting it.
/// Bumped to 2 when the proof column went: a key is no longer something this
/// server issues, so there is nothing to keep beside the id.
const VERSION: u8 = 2;

/// Everybody this server has seen.
#[derive(Default)]
pub struct People {
    known: HashSet<PersonId>,
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

    /// Note that this person has been here. Returns whether they are new.
    ///
    /// Called after the signature has checked out and never before: this
    /// records a fact rather than deciding one.
    pub fn seen(&mut self, id: &PersonId) -> bool {
        let fresh = self.known.insert(id.clone());
        if fresh {
            log::info!("a player this server has not met: {id}");
        }
        fresh
    }

    /// Whether this server has met this person before.
    pub fn knows(&self, id: &PersonId) -> bool {
        self.known.contains(id)
    }

    /// The table as it is written down.
    pub fn to_lines(&self) -> String {
        // Sorted, so a save is the same bytes for the same table and a diff
        // between two of them says what changed rather than what moved.
        let mut ids: Vec<_> = self.known.iter().collect();
        ids.sort();
        ids.iter().map(|id| format!("{VERSION}\t{id}\n")).collect()
    }

    /// Read a table back, skipping any line this build cannot make sense of.
    pub fn from_lines(text: &str) -> Self {
        let mut known = HashSet::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next()) {
                (Some(v), Some(id), None) if v.parse::<u8>() == Ok(VERSION) && !id.is_empty() => {
                    known.insert(PersonId(id.to_string()));
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

    fn id(n: &str) -> PersonId {
        PersonId(format!("{n}{}", "0".repeat(64 - n.len())))
    }

    /// This table records and never decides. An id it has not seen is a person
    /// the client made and this server has not met, and the answer to that is
    /// to write it down -- which is the reversal from when the server minted
    /// keys and an unknown one had to be refused.
    #[test]
    fn an_unknown_person_is_new_rather_than_wrong() {
        let mut people = People::new();
        assert!(!people.knows(&id("a")));
        assert!(people.seen(&id("a")), "a first visit was not new");
        assert!(people.knows(&id("a")));
        assert!(!people.seen(&id("a")), "a second visit was called new");
        assert_eq!(people.len(), 1);
    }

    /// Written down and read back, and the same bytes each time so one save is
    /// comparable with the one before it.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut people = People::new();
        for n in ["a", "b", "c"] {
            people.seen(&id(n));
        }
        let lines = people.to_lines();
        assert_eq!(lines, people.to_lines(), "two saves of one table differ");

        let back = People::from_lines(&lines);
        assert_eq!(back.len(), 3);
        for n in ["a", "b", "c"] {
            assert!(back.knows(&id(n)), "{n} was lost");
        }
    }

    /// Nothing in here is a secret, which is worth a test rather than a
    /// comment: the moment a proof column comes back, so does a file worth
    /// stealing.
    #[test]
    fn a_line_is_a_version_and_an_id_and_nothing_else() {
        let mut people = People::new();
        people.seen(&id("a"));
        let line = people.to_lines();
        assert_eq!(line.trim().split('\t').count(), 2, "{line:?}");
    }

    /// A line from a build that knew more is skipped, not fatal. Losing one
    /// person is a nuisance; refusing to start is not better.
    #[test]
    fn a_line_this_build_cannot_read_is_skipped() {
        let table = People::from_lines(&format!(
            "{VERSION}\tabc\n9\tfrom\tthe\tfuture\n\n1\tolder\tproof\nrubbish\n"
        ));
        assert_eq!(table.len(), 1, "a bad line took a good one with it");
        assert!(table.knows(&PersonId("abc".into())));
    }

    /// A server nobody has played on is where every server starts, so a table
    /// that is not there is empty rather than an error.
    #[test]
    fn a_table_that_is_not_there_is_empty() {
        let missing = std::env::temp_dir().join("ck-people-that-do-not-exist/people.tsv");
        let _ = std::fs::remove_dir_all(missing.parent().unwrap());
        assert!(People::load(&missing).unwrap().is_empty());
    }
}
