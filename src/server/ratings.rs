//! What each person here is rated.
//!
//! The table [`crate::server::rating`] was waiting for. The arithmetic has
//! been in for a while and had nothing to be keyed by, because a `PlayerId` is
//! a seat that gets handed on and a rejoin token is filed per room — and a
//! match *is* a room, so a number kept against either was earned in a match
//! and thrown away with it. A person is a keypair now, so there is finally
//! something a rating can belong to.
//!
//! **Per server, deliberately.** A rating that travelled between servers would
//! need servers to trust each other's results, which is a much larger thing
//! than a keypair — see [planned.md]. This one is a fact about how somebody
//! has done *here*, and the honest reading of it is a ladder on one machine.
//!
//! One line per person, tab separated with a version on the front, beside
//! `people.tsv` and for the same reasons: a store you can read with `cat` is
//! one you can debug, and a line this build cannot read is skipped rather than
//! fatal.
//!
//! [planned.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#rating

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::net::PersonId;
use crate::server::rating::{self, Entrant};

const VERSION: u8 = 1;

/// Everybody's number.
#[derive(Default)]
pub struct Ratings {
    known: HashMap<PersonId, i32>,
}

/// One person's result out of a finished match, before their rating is known.
///
/// The server holds the seat, the side and the ground; the rating is this
/// table's business, which is why the two are put together here rather than by
/// whatever noticed the match was over.
pub struct Finisher {
    pub who: PersonId,
    pub side: u8,
    pub score: usize,
}

impl Ratings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// What somebody is rated. Everybody starts at the same number, so
    /// somebody this server has never rated is not a special case.
    pub fn of(&self, who: &PersonId) -> i32 {
        self.known.get(who).copied().unwrap_or(rating::START)
    }

    /// Apply a finished match, and say what each person's rating moved by.
    ///
    /// Returns the changes rather than only writing them, because the reason
    /// to compute them is to tell somebody: a rating that moves silently is a
    /// number people learn not to look at.
    ///
    /// Anybody who was in the match without a key is skipped — they are not a
    /// person this server can remember, so there is nowhere to put a result.
    /// The rest are still rated against each other, which is the right answer:
    /// a stranger in the room is somebody you played, and refusing to rate the
    /// match because of them would make an unkeyed client a way to avoid a
    /// loss.
    pub fn settle(&mut self, finishers: &[Finisher]) -> Vec<(PersonId, i32)> {
        let entrants: Vec<Entrant> = finishers
            .iter()
            .map(|f| Entrant { rating: self.of(&f.who), side: f.side, score: f.score })
            .collect();
        let mut moved = Vec::new();
        for (finisher, delta) in finishers.iter().zip(rating::deltas(&entrants)) {
            if delta == 0 {
                continue;
            }
            let now = (self.of(&finisher.who) + delta).max(0);
            self.known.insert(finisher.who.clone(), now);
            moved.push((finisher.who.clone(), delta));
        }
        moved
    }

    pub fn to_lines(&self) -> String {
        // Sorted, so a save is the same bytes for the same table and a diff
        // between two of them says what changed rather than what moved.
        let mut all: Vec<_> = self.known.iter().collect();
        all.sort();
        all.iter().map(|(who, rating)| format!("{VERSION}\t{who}\t{rating}\n")).collect()
    }

    pub fn from_lines(text: &str) -> Self {
        let mut known = HashMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some(v), Some(who), Some(rating), None) if v.parse::<u8>() == Ok(VERSION) => {
                    match rating.parse::<i32>() {
                        Ok(rating) if !who.is_empty() => {
                            known.insert(PersonId(who.to_string()), rating);
                        }
                        _ => log::warn!("skipped line {} of the ratings file", n + 1),
                    }
                }
                _ => log::warn!("skipped line {} of the ratings file", n + 1),
            }
        }
        Self { known }
    }

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

    fn who(n: &str) -> PersonId {
        PersonId(format!("{n}{}", "0".repeat(64 - n.len())))
    }

    fn solo(name: &str, side: u8, score: usize) -> Finisher {
        Finisher { who: who(name), side, score }
    }

    /// Somebody this server has never rated is on the starting number rather
    /// than nothing, so a first match is scored against a real expectation.
    #[test]
    fn everybody_starts_on_the_same_number() {
        let ratings = Ratings::new();
        assert_eq!(ratings.of(&who("a")), rating::START);
        assert!(ratings.is_empty(), "asking about somebody invented them");
    }

    /// A finished match moves both numbers and says by how much, because a
    /// rating that changes silently is one people learn not to look at.
    #[test]
    fn a_match_moves_the_numbers_and_reports_it() {
        let mut ratings = Ratings::new();
        let moved = ratings.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        assert_eq!(moved.len(), 2);
        let (up, down) = (ratings.of(&who("a")), ratings.of(&who("b")));
        assert!(up > rating::START && down < rating::START, "{up} and {down}");
        assert_eq!(up - rating::START, rating::START - down, "a match was not zero-sum");
        assert_eq!(moved.iter().map(|(_, d)| d).sum::<i32>(), 0);
    }

    /// Playing on: the second result is scored against what the first left
    /// behind, which is the whole of a rating being a running number rather
    /// than a per-match score.
    #[test]
    fn a_second_match_is_scored_against_the_first() {
        let mut ratings = Ratings::new();
        ratings.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        let after_one = ratings.of(&who("a"));
        // Beating the same person again is worth less, because they are worth
        // less now and a is expected to.
        let first_gain = after_one - rating::START;
        ratings.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        let second_gain = ratings.of(&who("a")) - after_one;
        assert!(second_gain < first_gain, "{second_gain} was not less than {first_gain}");
    }

    /// A result that says nothing moves nothing and writes nothing: two equals
    /// drawing leaves the table as empty as it found it.
    #[test]
    fn a_result_with_no_information_is_not_recorded() {
        let mut ratings = Ratings::new();
        assert!(ratings.settle(&[solo("a", 1, 5), solo("b", 2, 5)]).is_empty());
        assert!(ratings.is_empty(), "a draw between strangers wrote a row");
    }

    /// Somebody with no key was in the room and cannot be rated -- but the
    /// people who *can* be are still rated against each other, or an unkeyed
    /// client would be a way to avoid a loss.
    #[test]
    fn a_match_is_still_rated_when_somebody_is_nobody() {
        let mut ratings = Ratings::new();
        // The unkeyed player simply is not in the list handed over.
        let moved = ratings.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        assert_eq!(moved.len(), 2);
    }

    /// Written down and read back, and the same bytes each time so one save is
    /// comparable with the one before it.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut ratings = Ratings::new();
        ratings.settle(&[solo("a", 1, 40), solo("b", 2, 10), solo("c", 3, 30)]);
        let lines = ratings.to_lines();
        assert_eq!(lines, ratings.to_lines(), "two saves of one table differ");

        let back = Ratings::from_lines(&lines);
        for name in ["a", "b", "c"] {
            assert_eq!(back.of(&who(name)), ratings.of(&who(name)), "{name} was lost");
        }
    }

    /// A line this build cannot read is skipped rather than fatal. Losing one
    /// person's number is a nuisance; refusing to start is not better.
    #[test]
    fn a_line_this_build_cannot_read_is_skipped() {
        let table = Ratings::from_lines(&format!(
            "{VERSION}\tabc\t1300\n9\tfrom\tthe\tfuture\n\n{VERSION}\tdef\tnot-a-number\nrubbish\n"
        ));
        assert_eq!(table.len(), 1, "a bad line took a good one with it");
        assert_eq!(table.of(&PersonId("abc".into())), 1300);
        assert_eq!(table.of(&PersonId("def".into())), rating::START);
    }
}
