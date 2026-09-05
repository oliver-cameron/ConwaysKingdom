//! **What this server can vouch for about each person it has met.**
//!
//! The table [`crate::server::rating`] was waiting for, and then some. The
//! arithmetic had nothing to be keyed by, because a `PlayerId` is a seat that
//! gets handed on and a rejoin token was filed per room — and a match *is* a
//! room, so a number kept against either was earned in a match and thrown away
//! with it. A person outlives both, so there is finally something to file
//! against.
//!
//! It holds more than a rating now, and that is the line [player profiles]
//! draws: **anything another player is shown has to be the server's.** Client
//! state is self-asserted, so a rating you keep is a rating you can type — and
//! the same goes for how many matches you have played and how much ground you
//! have held. `client::record` stays as a client's own diary; this is what a
//! server will say about you to somebody else.
//!
//! **Per server, deliberately.** Numbers that travelled between servers would
//! need servers to trust each other's results, which is a much larger thing
//! than a key. These are facts about how somebody has done *here*.
//!
//! One JSON object per person per line, beside `people.jsonl` and in the same
//! shape — see [`crate::net::jsonl`], which says why the separator is not a
//! character a value can contain.
//!
//! [player profiles]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#player-profiles

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::net::{jsonl, PersonId};
use crate::server::rating::{self, Entrant};

/// The format of a stored row.
///
/// Back to 1 with the move to [`crate::net::jsonl`]. The three versions before
/// it were tab separated and none of them are read: a rating is a fact about
/// matches this server ran, and a server whose table it cannot read is one
/// where everybody is provisional again. That is a line in a release note
/// rather than a migration, which is the same answer this file already gives
/// for records filed under a room's display name.
const VERSION: u8 = 1;

/// One person as this table stores them.
///
/// Flat rather than a [`Record`] with the id beside it, because a row is what
/// is read forward and a nested object hides which fields a version added.
/// `rating` is `null` for somebody met and not yet settled — see
/// [`Record::rating`], where nought and unearned are different things.
#[derive(Serialize, Deserialize)]
struct Row {
    v: u8,
    who: PersonId,
    name: String,
    rating: Option<i32>,
    games: u32,
    best: usize,
    history: Vec<i32>,
}

/// **What one person has done here.**
///
/// Everything on it was counted by this server, which is the whole of what it
/// is allowed to say about anybody.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    /// The name they last joined under. Self-chosen, so it is shown as a name
    /// and never as a fact — the fingerprint beside it is the part that is.
    pub name: String,
    /// Matches settled for them here. What makes a rating provisional, and
    /// what a profile shows beside it.
    pub games: u32,
    /// The most ground they have held at once, in squares, over every match.
    pub best: usize,
    /// Their number, once a match has moved it. `None` is somebody who has
    /// been met and has not finished a match: [`rating::START`] is where they
    /// would start, and showing it as though it had been earned is the thing
    /// the provisional mark exists to stop.
    rating: Option<i32>,
    /// **Where that number has been**, oldest first, at most
    /// [`rating::HISTORY`] of them.
    ///
    /// One entry per settled match including the ones that moved nothing, so
    /// the line's length is the number of matches and a flat stretch reads as
    /// a run of draws rather than as missing data.
    pub history: Vec<i32>,
}

impl Record {
    /// What they are rated. Everybody starts on the same number, so somebody
    /// with no result yet is not a special case in the arithmetic — only in
    /// what is said about it.
    pub fn rating(&self) -> i32 {
        self.rating.unwrap_or(rating::START)
    }

    /// Whether that number has been earned yet.
    pub fn provisional(&self) -> bool {
        self.games < rating::PROVISIONAL_AFTER
    }
}

/// Everybody this server can say something about.
#[derive(Default)]
pub struct Profiles {
    known: HashMap<PersonId, Record>,
}

/// One person's result out of a finished match, before their rating is known.
///
/// The server holds the seat, the team and the ground; the rating is this
/// table's business, which is why the two are put together here rather than by
/// whatever noticed the match was over.
pub struct Finisher {
    pub who: PersonId,
    pub name: String,
    pub team: u8,
    pub score: usize,
}

impl Profiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// What this server has to say about somebody, which for anybody it has
    /// not met is nothing — an empty record rather than an absence, so the
    /// caller has one shape to read.
    pub fn of(&self, who: &PersonId) -> Record {
        self.known.get(who).cloned().unwrap_or_default()
    }

    /// What somebody is rated. Everybody starts at the same number, so
    /// somebody this server has never rated is not a special case.
    pub fn rating_of(&self, who: &PersonId) -> i32 {
        self.of(who).rating()
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
            .map(|f| {
                let was = self.of(&f.who);
                Entrant { rating: was.rating(), team: f.team, score: f.score, games: was.games }
            })
            .collect();
        let mut moved = Vec::new();
        for (finisher, delta) in finishers.iter().zip(rating::deltas(&entrants)) {
            let was = self.of(&finisher.who);
            // **The match counts whether or not the number moved.** A draw
            // between equals says nothing about who is better and everything
            // about how much this table knows, so it is one of the results a
            // rating stops being provisional after.
            let rating = (delta != 0).then(|| (was.rating() + delta).max(0)).or(was.rating);
            // Every settled match, whether or not it moved the number: a flat
            // stretch is a run of draws and is worth being able to see as one.
            let mut history = was.history.clone();
            history.push(rating.unwrap_or(rating::START));
            if history.len() > rating::HISTORY {
                history.drain(..history.len() - rating::HISTORY);
            }
            let now = Record {
                // The other door a name comes in by, and clamped for the
                // reason [`Self::met`] gives: a row is one line.
                name: crate::net::player_name(&finisher.name),
                games: was.games + 1,
                best: was.best.max(finisher.score),
                rating,
                history,
            };
            self.known.insert(finisher.who.clone(), now);
            if delta != 0 {
                moved.push((finisher.who.clone(), delta));
            }
        }
        moved
    }

    /// **Who else plays here**: the people whose name matches, or the best
    /// rated when nothing is asked.
    ///
    /// One function for both because they are one question asked two ways, and
    /// two would drift — the leaderboard is this list with an empty query.
    ///
    /// **A name is self-chosen**, so a search that matched only names would
    /// let anybody put themselves in a list under somebody else's name. The
    /// fingerprint travels on every row — `net::Profile::who`, which
    /// `net::Seat::label` already prints beside a name — and it is the row's
    /// identity; the name is a label on it.
    ///
    /// **Provisional players are left off the leaderboard and are still
    /// findable.** A rating from one game is mostly the starting rating, so a
    /// table of them is a table of luck; but somebody looking for a person by
    /// name wants that person whether or not the server is sure about them
    /// yet. See [`Record::provisional`].
    ///
    /// Sorted by rating, then by name, then by fingerprint — all three,
    /// because a `HashMap` has no order of its own and a list that reshuffled
    /// between two identical questions would look broken.
    pub fn search(&self, like: &str, most: usize) -> Vec<PersonId> {
        let needle = like.trim().to_lowercase();
        let mut found: Vec<(&PersonId, &Record)> = self
            .known
            .iter()
            .filter(|(_, row)| {
                if needle.is_empty() {
                    !row.provisional()
                } else {
                    row.name.to_lowercase().contains(&needle)
                }
            })
            .collect();
        found.sort_by(|(a_who, a), (b_who, b)| {
            b.rating()
                .cmp(&a.rating())
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a_who.cmp(b_who))
        });
        found.into_iter().take(most).map(|(who, _)| who.clone()).collect()
    }

    /// Take a name from somebody who has joined, so a profile can be looked at
    /// before they have finished anything.
    ///
    /// A name and nothing else: everything else on a [`Record`] is counted
    /// from results, and taking any of it from a client would be taking a
    /// player's word for what a server is supposed to vouch for.
    ///
    /// **Clamped here rather than by the caller**, because a line of this file
    /// is tab separated and the name is the last field on it. A name with a
    /// newline in it wrote a *second* line, and a second line naming somebody
    /// else's id came back on the next start as that person with whatever
    /// rating it claimed. The store is what the format belongs to, so the
    /// store is what defends it -- see [`crate::net::player_name`].
    pub fn met(&mut self, who: &PersonId, name: &str) {
        let name = crate::net::player_name(name);
        let row = self.known.entry(who.clone()).or_default();
        if row.name != name {
            row.name = name;
        }
    }

    pub fn to_lines(&self) -> String {
        // Sorted, so a save is the same bytes for the same table and a diff
        // between two of them says what changed rather than what moved.
        let mut all: Vec<_> = self.known.iter().collect();
        all.sort_by(|a, b| a.0.cmp(b.0));
        jsonl::write(all.into_iter().map(|(who, r)| Row {
            v: VERSION,
            who: who.clone(),
            name: r.name.clone(),
            // A rating nobody has earned is written as `null` rather than as
            // the starting number, so reading the table back cannot invent a
            // result -- see `Record::rating`.
            rating: r.rating,
            games: r.games,
            best: r.best,
            history: r.history.clone(),
        }))
    }

    /// Read a table back, skipping any row this build cannot make sense of.
    pub fn from_lines(text: &str) -> Self {
        let known = jsonl::read::<Row>(text, "the profiles file")
            .into_iter()
            .filter(|row| row.v == VERSION && !row.who.as_str().is_empty())
            .map(|row| {
                let record = Record {
                    name: crate::net::player_name(&row.name),
                    games: row.games,
                    best: row.best,
                    rating: row.rating,
                    history: row.history,
                };
                (row.who, record)
            })
            .collect();
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
        crate::server::persist::replace(path, self.to_lines().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(n: &str) -> PersonId {
        PersonId(format!("{n}{}", "0".repeat(64 - n.len())))
    }

    /// A person the server has rated and settled enough to be sure about, so
    /// they are off the provisional list and on the leaderboard.
    fn rated(store: &mut Profiles, name: &str, rating: i32) {
        store.met(&who(name), name);
        let row = store.known.get_mut(&who(name)).expect("just met");
        row.rating = Some(rating);
        row.games = rating::PROVISIONAL_AFTER;
    }

    /// **A search matches the name and the row carries the fingerprint**, and
    /// the second half is what stops the first being an impersonation: a name
    /// is self-chosen, so two people may both be `alice` and the list has to
    /// let you tell them apart.
    #[test]
    fn a_search_finds_both_alices_and_keeps_them_apart() {
        let mut store = Profiles::new();
        rated(&mut store, "alice", 1300);
        rated(&mut store, "alicia", 1100);
        rated(&mut store, "bob", 1400);

        let found = store.search("ali", 25);
        assert_eq!(found.len(), 2, "bob is not an alice");
        assert_eq!(found[0], who("alice"), "the higher rated comes first");
        assert_eq!(found[1], who("alicia"));
        assert_ne!(found[0], found[1], "two rows, two fingerprints");
    }

    /// Case folded, because a name is typed by a person looking for somebody
    /// and not by one reciting a key.
    #[test]
    fn a_search_does_not_care_about_case() {
        let mut store = Profiles::new();
        rated(&mut store, "Alice", 1300);
        assert_eq!(store.search("ALI", 25), vec![who("Alice")]);
        assert_eq!(store.search("  alice  ", 25), vec![who("Alice")]);
    }

    /// **Nothing asked is the leaderboard**, which is the same list ordered by
    /// rating — one question, so the two cannot come to disagree.
    #[test]
    fn an_empty_search_is_the_leaderboard() {
        let mut store = Profiles::new();
        rated(&mut store, "cheap", 900);
        rated(&mut store, "best", 1600);
        rated(&mut store, "middling", 1200);
        let board = store.search("", 25);
        assert_eq!(board, vec![who("best"), who("middling"), who("cheap")]);
    }

    /// **A rating from one game is mostly the starting rating**, so a table of
    /// provisional players is a table of luck. They stay findable by name,
    /// because somebody looking for a person wants that person whether or not
    /// the server is sure about them yet.
    #[test]
    fn the_leaderboard_leaves_off_the_provisional_and_the_search_does_not() {
        let mut store = Profiles::new();
        rated(&mut store, "settled", 1300);
        store.met(&who("newcomer"), "newcomer");
        store.known.get_mut(&who("newcomer")).expect("just met").rating = Some(9999);

        assert_eq!(store.search("", 25), vec![who("settled")], "luck topped the board");
        assert_eq!(store.search("newcomer", 25), vec![who("newcomer")], "and is unfindable");
    }

    /// **Capped, so this is not a way to read out everybody a server has met.**
    #[test]
    fn a_search_answers_no_more_than_it_is_asked_for() {
        let mut store = Profiles::new();
        for i in 0..40 {
            rated(&mut store, &format!("p{i:02}"), 1000 + i);
        }
        assert_eq!(store.search("", crate::net::PEOPLE_MOST).len(), crate::net::PEOPLE_MOST);
        assert_eq!(store.search("p", crate::net::PEOPLE_MOST).len(), crate::net::PEOPLE_MOST);
    }

    /// A `HashMap` has no order of its own, so two identical questions have to
    /// be broken to the same answer or the list reshuffles under the reader.
    #[test]
    fn two_identical_searches_answer_identically() {
        let mut store = Profiles::new();
        for i in 0..12 {
            rated(&mut store, &format!("same{i:02}"), 1200);
        }
        assert_eq!(store.search("same", 25), store.search("same", 25));
    }

    fn solo(name: &str, team: u8, score: usize) -> Finisher {
        Finisher { who: who(name), name: name.to_string(), team, score }
    }

    /// Somebody this server has never rated is on the starting number rather
    /// than nothing, so a first match is scored against a real expectation.
    #[test]
    fn everybody_starts_on_the_same_number() {
        let table = Profiles::new();
        assert_eq!(table.rating_of(&who("a")), rating::START);
        assert!(table.of(&who("a")).provisional(), "and nobody has earned it yet");
        assert!(table.is_empty(), "asking about somebody invented them");
    }

    /// A finished match moves both numbers and says by how much, because a
    /// rating that changes silently is one people learn not to look at.
    #[test]
    fn a_match_moves_the_numbers_and_reports_it() {
        let mut table = Profiles::new();
        let moved = table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        assert_eq!(moved.len(), 2);
        let (up, down) = (table.rating_of(&who("a")), table.rating_of(&who("b")));
        assert!(up > rating::START && down < rating::START, "{up} and {down}");
        assert_eq!(up - rating::START, rating::START - down, "a match was not zero-sum");
        assert_eq!(moved.iter().map(|(_, d)| d).sum::<i32>(), 0);
    }

    /// Playing on: the second result is scored against what the first left
    /// behind, which is the whole of a rating being a running number rather
    /// than a per-match score.
    #[test]
    fn a_second_match_is_scored_against_the_first() {
        let mut table = Profiles::new();
        table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        let after_one = table.rating_of(&who("a"));
        // Beating the same person again is worth less, because they are worth
        // less now and a is expected to.
        let first_gain = after_one - rating::START;
        table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        let second_gain = table.rating_of(&who("a")) - after_one;
        assert!(second_gain < first_gain, "{second_gain} was not less than {first_gain}");
    }

    /// **A result that says nothing moves nothing, and still counts.**
    ///
    /// Two equals drawing tells the table nothing about who is better and
    /// everything about how much it now knows, so it is one of the matches a
    /// rating stops being provisional after — and there is nothing to report,
    /// because nothing moved.
    #[test]
    fn a_draw_between_equals_moves_nothing_and_still_counts() {
        let mut table = Profiles::new();
        assert!(table.settle(&[solo("a", 1, 5), solo("b", 2, 5)]).is_empty());
        let drawn = table.of(&who("a"));
        assert_eq!(drawn.games, 1, "the match did not happen");
        assert_eq!(drawn.rating(), rating::START, "and nothing moved");
        assert_eq!(drawn.best, 5, "and the ground held was counted");
    }

    /// **A rating is provisional until enough matches have settled it**, and
    /// what a profile shows is the mark rather than a different number: an Elo
    /// from a fixed start means nothing until it has moved, and a leaderboard
    /// topped by somebody who won once is the thing that stops.
    #[test]
    fn a_rating_stops_being_provisional_after_enough_matches() {
        let mut table = Profiles::new();
        for _ in 0..rating::PROVISIONAL_AFTER - 1 {
            table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
            assert!(table.of(&who("a")).provisional());
        }
        table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        let settled = table.of(&who("a"));
        assert_eq!(settled.games, rating::PROVISIONAL_AFTER);
        assert!(!settled.provisional(), "ten results and still unearned");
    }

    /// **The most ground ever held**, which is a high-water mark and not the
    /// last figure: a profile says what somebody has managed, so a bad match
    /// after a good one does not erase the good one.
    #[test]
    fn the_best_held_is_a_high_water_mark() {
        let mut table = Profiles::new();
        table.settle(&[solo("a", 1, 400), solo("b", 2, 10)]);
        table.settle(&[solo("a", 1, 7), solo("b", 2, 10)]);
        assert_eq!(table.of(&who("a")).best, 400, "a bad match erased a good one");
        assert_eq!(table.of(&who("a")).name, "a", "and the name is the one last used");
    }

    /// A name is the one thing a client is taken at its word for, and it is
    /// taken on joining so a profile can be looked at before anybody has
    /// finished anything.
    #[test]
    fn meeting_somebody_records_a_name_and_nothing_else() {
        let mut table = Profiles::new();
        table.met(&who("a"), "alice");
        let met = table.of(&who("a"));
        assert_eq!(met.name, "alice");
        assert_eq!((met.games, met.best), (0, 0), "a join is not a result");
        assert_eq!(met.rating(), rating::START);
    }

    /// Somebody with no key was in the room and cannot be rated -- but the
    /// people who *can* be are still rated against each other, or an unkeyed
    /// client would be a way to avoid a loss.
    #[test]
    fn a_match_is_still_rated_when_somebody_is_nobody() {
        let mut table = Profiles::new();
        // The unkeyed player simply is not in the list handed over.
        let moved = table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        assert_eq!(moved.len(), 2);
    }

    /// Written down and read back, and the same bytes each time so one save is
    /// comparable with the one before it.
    #[test]
    fn a_table_survives_being_written_down() {
        let mut table = Profiles::new();
        table.settle(&[solo("a", 1, 40), solo("b", 2, 10), solo("c", 3, 30)]);
        let lines = table.to_lines();
        assert_eq!(lines, table.to_lines(), "two saves of one table differ");

        let back = Profiles::from_lines(&lines);
        for name in ["a", "b", "c"] {
            assert_eq!(back.of(&who(name)), table.of(&who(name)), "{name} was lost");
        }
    }

    /// **The tables this replaced are not read**, and that is a decision rather
    /// than an oversight: they were tab separated, nothing is deployed on one,
    /// and carrying three dead formats to save ratings that exist on nobody's
    /// disk is machinery with no job. A server that met people under the old
    /// format meets them again and everybody is provisional.
    #[test]
    fn a_table_from_before_this_format_is_not_read() {
        let old = "1\tabc\t1300\n3\tdef\t1400\t7\t90\t1300,1400\tdee\n";
        assert!(Profiles::from_lines(old).is_empty(), "an old line was read as a new one");
    }

    /// **A point per settled match**, including the ones that moved nothing:
    /// a flat stretch is a run of draws and is worth being able to see as one.
    #[test]
    fn a_rating_remembers_where_it_has_been() {
        let mut table = Profiles::new();
        for _ in 0..3 {
            table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        }
        table.settle(&[solo("a", 1, 5), solo("b", 2, 5)]);
        let row = table.of(&who("a"));
        assert_eq!(row.history.len(), 4, "a drawn match is still a match");
        assert_eq!(row.history.last().copied(), Some(row.rating()), "the last point is now");
        assert!(row.history[0] < row.history[2], "three wins should climb: {:?}", row.history);

        // Bounded, or one line of a text file grows without end.
        for _ in 0..rating::HISTORY * 2 {
            table.settle(&[solo("a", 1, 40), solo("b", 2, 10)]);
        }
        assert_eq!(table.of(&who("a")).history.len(), rating::HISTORY);
    }

    /// **A name cannot write a second line.**
    ///
    /// A row is tab separated and the name is the last field on it, so a name
    /// carrying a newline wrote two rows and the second one said whatever it
    /// liked -- including somebody else's id, which came back on the next
    /// start as that person with a rating they had not earned. The name a
    /// client joins under is the only field here it chooses.
    #[test]
    fn a_name_cannot_forge_somebody_elses_row() {
        let (victim, attacker) = (who("victim"), who("zattacker"));
        let mut table = Profiles::new();
        table.met(&victim, "victim");
        table.met(&attacker, "x\n3\tvictim\t9999\t50\t500\t\towned");
        // And the other door a name comes in by.
        table.settle(&[Finisher {
            who: attacker.clone(),
            name: "y\n3\tvictim\t9999\t50\t500\t\towned".into(),
            team: 1,
            score: 1,
        }]);

        let lines = table.to_lines();
        assert_eq!(lines.lines().count(), 2, "a name wrote its own row:\n{lines}");

        let back = Profiles::from_lines(&lines);
        assert_eq!(back.rating_of(&victim), rating::START, "a name handed out a rating");
        assert_eq!(back.of(&victim).name, "victim", "and took a name with it");
    }

    /// A row this build cannot read is skipped rather than fatal. Losing one
    /// person's number is a nuisance; refusing to start is not better.
    #[test]
    fn a_row_this_build_cannot_read_is_skipped() {
        let table = Profiles::from_lines(concat!(
            r#"{"v":1,"who":"abc","name":"abby","rating":1300,"games":4,"best":80,"history":[1200,1300]}"#,
            "\n",
            r#"{"v":9,"who":"future","name":"f","rating":1,"games":0,"best":0,"history":[]}"#,
            "\n\n",
            r#"{"v":1,"who":"def","name":"dee"}"#,
            "\n",
            "rubbish\n",
        ));
        assert_eq!(table.len(), 1, "a bad row took a good one with it");
        let abby = table.of(&PersonId("abc".into()));
        assert_eq!((abby.rating(), abby.best, abby.games), (1300, 80, 4));
        assert_eq!(abby.history, vec![1200, 1300]);
        assert_eq!(table.rating_of(&PersonId("def".into())), rating::START, "a short row was read");
    }
}
