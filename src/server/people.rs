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
//! An id and its proof, and nothing else. Not the name — a name is per seat
//! and people change them — and not a rating, which will sit in its own table
//! keyed by the same id, because a rating is a thing this table *enables*
//! rather than a thing it is.
//!
//! The proof is written in the clear, which is consistent rather than careless:
//! [`crate::server::persist`] already writes every rejoin token into a world
//! file the same way. Hashing this one alone would protect the newer secret
//! and not the older one, and both are claim tickets to a game with no
//! accounts. If that ever changes it should change for both at once.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::net::{Person, PersonId};

/// The format of a stored line, so a build that cannot read one can tell that
/// rather than mis-splitting it.
const VERSION: u8 = 1;

/// Everybody this server has minted a key for.
#[derive(Default)]
pub struct People {
    known: HashMap<PersonId, String>,
}

/// Why a join was not admitted. A sentence for the person, not a code.
pub type Refused = String;

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

    /// Who this join is from, and the pair to hand back if one was just minted.
    ///
    /// Four cases, and the two refusals are the ones worth stating.
    ///
    /// **A proof that does not match is refused, not reissued.** Quietly
    /// minting a fresh identity there would turn a mistyped key, a stale
    /// store, and somebody else's key into the same silent outcome — a player
    /// who is now a stranger and finds out when their record is empty.
    ///
    /// **An id this server has never seen is refused too**, and the temptation
    /// is to adopt it, which would be worse than it looks: adopting means
    /// anybody can claim an id nobody has used here yet, with a proof of their
    /// own, and the person who actually owns it elsewhere is then locked out
    /// of this server for good. A refusal costs somebody a message and a fresh
    /// start; adoption costs somebody their name permanently.
    ///
    /// A client keeps its person **per server**, so it never offers one server
    /// a key minted by another. Reaching this refusal means a hand-pasted key
    /// or a table that has been lost, and both want to be told.
    pub fn admit(
        &mut self,
        offered: Option<&Person>,
        mint: impl FnOnce() -> (String, String),
    ) -> Result<(PersonId, Option<Person>), Refused> {
        let Some(offered) = offered else {
            let (id, proof) = mint();
            let person = Person { id: PersonId(id), proof };
            self.known.insert(person.id.clone(), person.proof.clone());
            log::info!("a new player key: {}", person.id);
            return Ok((person.id.clone(), Some(person)));
        };
        match self.known.get(&offered.id) {
            Some(proof) if *proof == offered.proof => Ok((offered.id.clone(), None)),
            Some(_) => {
                log::warn!("a player key with the wrong proof: {}", offered.id);
                Err("that player key is not right for this server".into())
            }
            None => {
                log::warn!("a player key this server never issued: {}", offered.id);
                Err("this server has never issued that player key".into())
            }
        }
    }

    /// Whether this server knows this person, without admitting anybody.
    pub fn knows(&self, id: &PersonId) -> bool {
        self.known.contains_key(id)
    }

    /// The table as it is written down.
    pub fn to_lines(&self) -> String {
        // Sorted, so a save is the same bytes for the same table and a diff
        // between two of them says what changed rather than what moved.
        let mut ids: Vec<_> = self.known.iter().collect();
        ids.sort();
        ids.iter().map(|(id, proof)| format!("{VERSION}\t{id}\t{proof}\n")).collect()
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
                (Some(v), Some(id), Some(proof), None)
                    if v.parse::<u8>() == Ok(VERSION) && !id.is_empty() && !proof.is_empty() =>
                {
                    known.insert(PersonId(id.to_string()), proof.to_string());
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

    fn minter(n: &str) -> impl FnOnce() -> (String, String) + '_ {
        move || (format!("{n}{}", "0".repeat(32 - n.len())), format!("p{n}"))
    }

    /// The ordinary life of a key: minted on a first join, believed on every
    /// join after, and the same person each time.
    #[test]
    fn a_key_is_minted_once_and_believed_after() {
        let mut people = People::new();
        let (who, minted) = people.admit(None, minter("a")).unwrap();
        let person = minted.expect("a first join is handed a key");
        assert_eq!(person.id, who);
        assert!(people.knows(&who));

        let (again, minted) = people.admit(Some(&person), minter("b")).unwrap();
        assert_eq!(again, who, "the same key came back as somebody else");
        assert!(minted.is_none(), "a key was reissued to somebody who already had one");
        assert_eq!(people.len(), 1, "believing a key made a second person");
    }

    /// A proof that does not match is a mistyped key, a stale store, or
    /// somebody else's — and reissuing would make all three look identical to
    /// the person they happened to.
    #[test]
    fn a_wrong_proof_is_refused_rather_than_reissued() {
        let mut people = People::new();
        let (_, minted) = people.admit(None, minter("a")).unwrap();
        let mut wrong = minted.unwrap();
        wrong.proof = "not it".into();

        assert!(people.admit(Some(&wrong), minter("b")).is_err());
        assert_eq!(people.len(), 1, "a refusal minted somebody anyway");
    }

    /// Adopting an unknown id would let anybody claim one nobody has used here
    /// yet and lock its real owner out for good. A refusal costs a message.
    #[test]
    fn an_unknown_key_is_refused_and_not_adopted() {
        let mut people = People::new();
        let squatter = Person { id: PersonId("f".repeat(32)), proof: "mine".into() };
        assert!(people.admit(Some(&squatter), minter("a")).is_err());
        assert!(!people.knows(&squatter.id), "an unknown key was adopted");
        assert!(people.is_empty());
    }

    /// A person is transferable, which means two devices can hold one at once.
    /// The table has nothing to say about that -- it says who you are, and a
    /// seat is what says where you may sit.
    #[test]
    fn one_person_may_arrive_twice() {
        let mut people = People::new();
        let (_, minted) = people.admit(None, minter("a")).unwrap();
        let person = minted.unwrap();
        let first = people.admit(Some(&person), minter("b")).unwrap();
        let second = people.admit(Some(&person), minter("c")).unwrap();
        assert_eq!(first.0, second.0);
        assert!(first.1.is_none() && second.1.is_none());
    }

    /// Written down and read back, and the same bytes each time so a save is
    /// comparable with the one before it.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut people = People::new();
        let mut keys = Vec::new();
        for n in ["a", "b", "c"] {
            keys.push(people.admit(None, minter(n)).unwrap().1.unwrap());
        }
        let lines = people.to_lines();
        assert_eq!(lines, people.to_lines(), "two saves of one table differ");

        let back = People::from_lines(&lines);
        assert_eq!(back.len(), 3);
        let mut back = back;
        for key in &keys {
            assert!(back.admit(Some(key), minter("z")).is_ok(), "{} was lost", key.id);
        }
    }

    /// A line from a build that knew more is skipped, not fatal. Losing one
    /// person is a nuisance; refusing to start is not better.
    #[test]
    fn a_line_this_build_cannot_read_is_skipped() {
        let good = format!("{VERSION}\tabc\txyz");
        let table = People::from_lines(&format!(
            "{good}\n9\tfrom\tthe\tfuture\n\n{VERSION}\tonly-two-fields\nrubbish\n"
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
