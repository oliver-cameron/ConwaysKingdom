//! What this server holds for each person that **only that person is shown**.
//!
//! The patterns they have saved and the games they have played — see
//! [`crate::net::kept`], which is the shape and the argument for it. This is
//! where they live between visits.
//!
//! ## Why a server holds something nobody else sees
//!
//! Every other table here exists so that a *second* player can be told
//! something: a rating is worth having because it is not self-reported. This
//! one is the opposite, and it is here for the plainer reason — a library kept
//! by a browser is a fact about a browser. Somebody who plays on a phone and a
//! laptop had two libraries and two diaries, which is [a bug that was already
//! written down][known-bugs] rather than a design.
//!
//! So the server is a **locker and not a witness**: it holds a pattern, it does
//! not read one, and it will not show one to anybody else. What it does do is
//! bound what it holds, because this is the one message that writes a client's
//! own words to a server's disk — [`crate::net::kept::Kept::clamped`] is that,
//! and it runs on everything that arrives.
//!
//! ## Whole, not merged
//!
//! A locker is replaced rather than added to. The client is a cache and the
//! server is the authority, so a join hands back what the server has and a
//! change hands up the whole of what the client now holds. There is no merge
//! and so no rule about whose edit wins — which is what makes "the server's
//! copy is the copy" a sentence with nothing behind it.
//!
//! The exception is a **person this server has never held anything for**: their
//! client seeds it, once, with whatever it was carrying. That is what makes a
//! library follow somebody to a server they have not played on, without any
//! two servers ever talking to each other.
//!
//! [known-bugs]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/known-bugs.md

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::net::kept::{Game, Kept, Stamp};
use crate::net::{jsonl, PersonId};

/// The format of a stored row.
const VERSION: u8 = 1;

/// One person's patterns, as one row.
///
/// A row per person rather than a row per pattern, because a locker is
/// replaced whole and a person's library is the unit that is written: a row per
/// pattern would mean deleting an unknown number of rows to store a known
/// number, for a file nothing queries.
#[derive(Serialize, Deserialize)]
struct StampRow {
    v: u8,
    who: PersonId,
    stamps: Vec<Stamp>,
}

/// One person's diary, as one row. Newest first, which is how it is read.
#[derive(Serialize, Deserialize)]
struct GameRow {
    v: u8,
    who: PersonId,
    games: Vec<Game>,
}

/// Everybody's locker.
#[derive(Default)]
pub struct Lockers {
    held: HashMap<PersonId, Kept>,
}

impl Lockers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// What this server holds for somebody, which for anybody it holds nothing
    /// for is an empty locker rather than an absence — the caller has one shape
    /// to read, the same way [`crate::server::profiles::Profiles::of`] does.
    pub fn of(&self, who: &PersonId) -> Kept {
        self.held.get(who).cloned().unwrap_or_default()
    }

    /// Whether there is anything here for somebody.
    ///
    /// What decides between handing a client its locker and taking one from it:
    /// a person this server holds nothing for is a person whose client seeds
    /// it, which is how a library reaches a server nobody has played on.
    pub fn holds_anything_for(&self, who: &PersonId) -> bool {
        self.held.get(who).is_some_and(|kept| !kept.is_empty())
    }

    /// Replace what is held for somebody with what they have offered.
    ///
    /// **Clamped, never believed.** Everything in a `Kept` was chosen by a
    /// client and is going onto this server's disk.
    pub fn keep(&mut self, who: &PersonId, offered: Kept) {
        let kept = offered.clamped();
        if kept.is_empty() {
            // An empty locker is stored as nothing rather than as an empty row:
            // a person who threw their last pattern away should not then be
            // seeded from the next client that carries one.
            self.held.remove(who);
        } else {
            self.held.insert(who.clone(), kept);
        }
    }

    /// The patterns as they are written down, one row per person.
    pub fn stamps_to_lines(&self) -> String {
        jsonl::write(self.rows().filter(|(_, kept)| !kept.stamps.is_empty()).map(|(who, kept)| {
            StampRow { v: VERSION, who: who.clone(), stamps: kept.stamps.clone() }
        }))
    }

    /// The diaries, the same way.
    pub fn games_to_lines(&self) -> String {
        jsonl::write(
            self.rows().filter(|(_, kept)| !kept.games.is_empty()).map(|(who, kept)| GameRow {
                v: VERSION,
                who: who.clone(),
                games: kept.games.clone(),
            }),
        )
    }

    /// Sorted by person, so a save is the same bytes for the same table and a
    /// diff between two of them says what changed rather than what moved.
    fn rows(&self) -> impl Iterator<Item = (&PersonId, &Kept)> {
        let mut all: Vec<_> = self.held.iter().collect();
        all.sort_by(|a, b| a.0.cmp(b.0));
        all.into_iter()
    }

    /// Read both files back. A row this build cannot read is skipped, and the
    /// two are independent: a library that will not read does not cost the
    /// diary beside it.
    pub fn from_lines(stamps: &str, games: &str) -> Self {
        let mut held: HashMap<PersonId, Kept> = HashMap::new();
        for row in jsonl::read::<StampRow>(stamps, "the stamps file") {
            if row.v == VERSION {
                held.entry(row.who).or_default().stamps = row.stamps;
            }
        }
        for row in jsonl::read::<GameRow>(games, "the games file") {
            if row.v == VERSION {
                held.entry(row.who).or_default().games = row.games;
            }
        }
        // Clamped on the way in as well as on the way out. A file is a thing a
        // person can edit, and the bound is a property of the store rather than
        // of the message that happened to fill it.
        Self { held: held.into_iter().map(|(who, kept)| (who, kept.clamped())).collect() }
    }

    pub fn load(stamps: &Path, games: &Path) -> io::Result<Self> {
        Ok(Self::from_lines(&read_or_empty(stamps)?, &read_or_empty(games)?))
    }

    pub fn save(&self, stamps: &Path, games: &Path) -> io::Result<()> {
        crate::server::persist::replace(stamps, self.stamps_to_lines().as_bytes())?;
        crate::server::persist::replace(games, self.games_to_lines().as_bytes())
    }
}

/// A file that is not there yet is an empty one: a server nobody has kept
/// anything on is where every server starts.
fn read_or_empty(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::kept::{Outcome, GAMES_MOST, STAMPS_MOST};
    use crate::sim::WorldKind;

    fn who(n: &str) -> PersonId {
        PersonId(format!("{n}{}", "0".repeat(32 - n.len())))
    }

    fn stamp(name: &str, cells: &[(i32, i32)]) -> Stamp {
        let mut s = Stamp::trimmed(cells.to_vec());
        s.name = name.into();
        s
    }

    fn game(room: &str) -> Game {
        Game {
            room: room.into(),
            world: WorldKind::Infinite,
            generations: 40,
            best: 9,
            outcome: Outcome::Won,
        }
    }

    fn kept() -> Kept {
        Kept {
            stamps: vec![stamp("glider", &[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)])],
            games: vec![game("main"), game("arena")],
        }
    }

    /// Somebody this server holds nothing for gets an empty locker rather than
    /// an absence, so a caller has one shape to read.
    #[test]
    fn a_person_nobody_has_kept_anything_for_has_an_empty_locker() {
        let lockers = Lockers::new();
        assert_eq!(lockers.of(&who("a")), Kept::default());
        assert!(!lockers.holds_anything_for(&who("a")));
        assert!(lockers.is_empty(), "asking about somebody invented them");
    }

    /// Written down and read back, and the same bytes each time, so one save is
    /// comparable with the one before it.
    #[test]
    fn a_locker_survives_being_written_down() {
        let mut lockers = Lockers::new();
        lockers.keep(&who("a"), kept());
        lockers.keep(&who("b"), Kept { stamps: vec![stamp("dot", &[(0, 0)])], games: Vec::new() });

        let (s, g) = (lockers.stamps_to_lines(), lockers.games_to_lines());
        assert_eq!((s.clone(), g.clone()), (lockers.stamps_to_lines(), lockers.games_to_lines()));
        assert_eq!(s.lines().count(), 2, "a person is a row");
        assert_eq!(g.lines().count(), 1, "and somebody with no diary is no row at all");

        let back = Lockers::from_lines(&s, &g);
        assert_eq!(back.of(&who("a")), kept(), "a was lost");
        assert_eq!(back.of(&who("b")).stamps.len(), 1);
        assert!(back.of(&who("b")).games.is_empty());
    }

    /// **A locker is replaced, not added to.** The client is the cache and the
    /// server is the authority, so there is no merge and no rule about whose
    /// edit wins.
    #[test]
    fn keeping_replaces_rather_than_merges() {
        let mut lockers = Lockers::new();
        lockers.keep(&who("a"), kept());
        lockers.keep(&who("a"), Kept { stamps: vec![stamp("dot", &[(0, 0)])], games: Vec::new() });

        let now = lockers.of(&who("a"));
        assert_eq!(now.stamps.len(), 1, "the old library was merged in");
        assert_eq!(now.stamps[0].name, "dot");
        assert!(now.games.is_empty(), "a diary outlived the locker it was in");
    }

    /// **Throwing the last pattern away is not the same as never having one.**
    /// An empty locker is stored as nothing, or the next client to arrive
    /// carrying a library would seed it back.
    #[test]
    fn an_emptied_locker_is_not_held_at_all() {
        let mut lockers = Lockers::new();
        lockers.keep(&who("a"), kept());
        assert!(lockers.holds_anything_for(&who("a")));

        lockers.keep(&who("a"), Kept::default());
        assert!(!lockers.holds_anything_for(&who("a")));
        assert!(lockers.is_empty(), "an empty locker was still a row");
    }

    /// **What a client offers is bounded**, because this is the one message
    /// that writes a client's own words to a server's disk.
    #[test]
    fn what_arrives_is_clamped_before_it_is_stored() {
        let mut lockers = Lockers::new();
        lockers.keep(
            &who("a"),
            Kept {
                stamps: (0..STAMPS_MOST * 2)
                    .map(|n| stamp("s", &[(0, 0), (0, n as i32 % 8)]))
                    .collect(),
                games: (0..GAMES_MOST * 2).map(|_| game("main")).collect(),
            },
        );
        let held = lockers.of(&who("a"));
        assert_eq!(held.stamps.len(), STAMPS_MOST);
        assert_eq!(held.games.len(), GAMES_MOST);
    }

    /// A row this build cannot read is skipped, and the two files are
    /// independent: a library that will not read does not cost the diary.
    #[test]
    fn a_bad_row_costs_one_person_and_not_the_file() {
        let mut lockers = Lockers::new();
        lockers.keep(&who("a"), kept());
        let good = lockers.stamps_to_lines();

        let stamps = format!(
            "{good}{}\nrubbish\n",
            format_args!(r#"{{"v":99,"who":"{}","stamps":[]}}"#, who("b")),
        );
        let back = Lockers::from_lines(&stamps, &lockers.games_to_lines());
        assert_eq!(back.of(&who("a")).stamps.len(), 1, "a bad row took a good one");
        assert!(back.of(&who("b")).stamps.is_empty(), "a row from the future was read");
        assert_eq!(back.of(&who("a")).games.len(), 2, "the diary went with the library");
    }

    /// A file somebody edited is still a file this reads, and the bound belongs
    /// to the store rather than to the message that filled it.
    #[test]
    fn a_hand_edited_file_is_clamped_on_the_way_in() {
        let huge = format_args!(
            r#"{{"v":1,"who":"{}","stamps":[{{"name":"huge","cells":[[0,0],[0,99]]}}]}}"#,
            who("a")
        )
        .to_string();
        let back = Lockers::from_lines(&huge, "");
        assert!(back.of(&who("a")).stamps.is_empty(), "a pattern the pad cannot draw was kept");
    }

    /// A store that is not there yet is an empty one.
    #[test]
    fn files_that_are_not_there_are_an_empty_store() {
        let dir = std::env::temp_dir().join(format!("ck-lockers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Lockers::load(&dir.join("stamps.jsonl"), &dir.join("games.jsonl"));
        assert!(store.expect("a missing file is not an error").is_empty());
    }
}
