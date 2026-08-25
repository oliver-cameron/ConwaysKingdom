//! What this client has played, kept between visits.
//!
//! A home screen that says only "Conway's Kingdom" and a name field is a home
//! screen with nothing on it. What belongs there is the one thing the client
//! knows and the server does not: **what you have done before.** A server
//! knows who is in it now; only the client remembers the world you left last
//! week.
//!
//! Deliberately small. Five numbers a player recognises — worlds played,
//! matches won, the most ground ever held, generations lived through — and not
//! a statistics page. A record on a home screen is there to make the last
//! session feel like it happened, not to be studied.
//!
//! ## Why it is text, and why a bad line is skipped
//!
//! [`crate::net::keep`] is a store of strings: `localStorage` in a browser and
//! a file natively. So a record has to become a string, and rather than
//! postcard behind a hex encoding — which would be opaque in both places and
//! unreadable when something goes wrong — this is **one line per game, tab
//! separated, with a version on the front.**
//!
//! The version is not ceremony. A record written by a build that knew about
//! teams, read by one that does not, is exactly the case this has to survive,
//! and the same worry is already written down for stamps in [roadmap.md]. A
//! line this build cannot read is **skipped**, not fatal: losing one game out
//! of a history is a nuisance, and refusing to show any of them because one is
//! from the future is not.
//!
//! Tab separated because a room name is letters, digits, `-` and `_` — see
//! [`crate::net::room_name`] — so no field can contain the separator and there
//! is nothing to escape.
//!
//! [roadmap.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/roadmap.md#stamps-that-outlive-the-tab

use crate::sim::WorldKind;

/// The format of a stored line. Bumped when a field is added, so that a build
/// which does not understand a line can tell that rather than mis-splitting
/// one.
const VERSION: u8 = 1;

/// How many games are kept.
///
/// A cap rather than everything, because this lives in `localStorage` beside
/// a rejoin token that matters more, and a history nobody reads should not be
/// what fills a quota. Fifty is more than a home screen ever shows and enough
/// that "most ground ever held" means something.
pub const KEEP: usize = 50;

/// How a game ended for this player.
///
/// Three answers, not two: most rooms have no way to end at all, so "played"
/// is the ordinary outcome and winning is the special case. A world that never
/// ends is not a game you lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A world with no way to win, or a match left before it decided.
    Played,
    Won,
    Lost,
}

impl Outcome {
    fn tag(self) -> &'static str {
        match self {
            Self::Played => "played",
            Self::Won => "won",
            Self::Lost => "lost",
        }
    }

    fn read(tag: &str) -> Option<Self> {
        match tag {
            "played" => Some(Self::Played),
            "won" => Some(Self::Won),
            "lost" => Some(Self::Lost),
            _ => None,
        }
    }
}

/// One game, as it looked when this client left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub room: String,
    pub world: WorldKind,
    /// Generations this client was present for, not the world's age. A world
    /// that had been running for a week before you arrived is not a week you
    /// played.
    pub generations: u64,
    /// The most ground held at once, which is a better memory of a game than
    /// the ground held at the end — a player who built an empire and lost it
    /// played a more interesting game than one who never held anything, and
    /// the final figure is the same for both.
    pub best: u32,
    pub outcome: Outcome,
}

impl Game {
    fn write(&self) -> String {
        let world = match self.world {
            WorldKind::Infinite => "inf".to_string(),
            WorldKind::Toroidal { rows, cols } => format!("{rows}x{cols}"),
        };
        format!(
            "{VERSION}\t{}\t{world}\t{}\t{}\t{}",
            self.room,
            self.generations,
            self.best,
            self.outcome.tag()
        )
    }

    /// `None` for a line this build does not understand, which the caller
    /// skips. Every field is checked rather than trusted: this is read from a
    /// store a person can edit, and a panic on the home screen would make a
    /// stray keystroke in `localStorage` look like the game being broken.
    fn read(line: &str) -> Option<Self> {
        let mut parts = line.split('\t');
        if parts.next()?.parse::<u8>().ok()? != VERSION {
            return None;
        }
        let room = parts.next()?.to_string();
        let world = match parts.next()? {
            "inf" => WorldKind::Infinite,
            size => {
                let (rows, cols) = size.split_once('x')?;
                WorldKind::Toroidal { rows: rows.parse().ok()?, cols: cols.parse().ok()? }
            }
        };
        let generations = parts.next()?.parse().ok()?;
        let best = parts.next()?.parse().ok()?;
        let outcome = Outcome::read(parts.next()?)?;
        Some(Self { room, world, generations, best, outcome })
    }

    fn is_match(&self) -> bool {
        self.outcome != Outcome::Played
    }
}

/// What the home screen shows: the history, folded into numbers.
///
/// Computed rather than stored, so there is one place a game is recorded and
/// no running totals to fall out of step with the list they came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub games: usize,
    /// Games that had a way to be won, whether or not this player won one.
    pub matches: usize,
    pub won: usize,
    /// The most ground ever held at once, in any game.
    pub best: u32,
    pub generations: u64,
}

impl Summary {
    pub fn of(games: &[Game]) -> Self {
        Self {
            games: games.len(),
            matches: games.iter().filter(|g| g.is_match()).count(),
            won: games.iter().filter(|g| g.outcome == Outcome::Won).count(),
            best: games.iter().map(|g| g.best).max().unwrap_or(0),
            generations: games.iter().map(|g| g.generations).sum(),
        }
    }

    /// Whether there is anything worth drawing. A home screen showing five
    /// zeroes tells a new player only that the game keeps score, which is not
    /// what a first visit should be about.
    pub fn any(&self) -> bool {
        self.games > 0
    }
}

/// Every game kept, newest first.
pub fn games() -> Vec<Game> {
    crate::net::keep::games().lines().filter_map(Game::read).collect()
}

/// File a finished game, newest first, dropping the oldest past [`KEEP`].
pub fn remember(game: &Game) {
    let mut kept = games();
    kept.insert(0, game.clone());
    kept.truncate(KEEP);
    let text: Vec<String> = kept.iter().map(Game::write).collect();
    crate::net::keep::remember_games(&text.join("\n"));
}

/// A game as it is being played, before there is anything to file.
///
/// Held by the client and committed when the room ends for this player: a
/// different `Welcome`, a link that closed, or the way back to the menu. A tab
/// closed mid-game loses its record, which is the honest cost of not writing
/// on every change — a browser gives no reliable moment to write at, and a
/// store rewritten four times a second to catch it would be worse than the
/// gap it closed.
#[derive(Clone, Debug)]
pub struct InPlay {
    pub room: String,
    pub world: WorldKind,
    /// The world's generation when this client joined, so the record counts
    /// what this player was present for rather than the world's whole age.
    from: u64,
    at: u64,
    best: u32,
    outcome: Outcome,
}

impl InPlay {
    pub fn joined(room: String, world: WorldKind, tick: u64) -> Self {
        Self { room, world, from: tick, at: tick, best: 0, outcome: Outcome::Played }
    }

    /// The world moved on.
    pub fn at(&mut self, tick: u64) {
        // Clamped rather than trusted: a resync can land a client on a tick
        // behind the one it thought it was on, and a subtraction that went
        // negative would wrap into an enormous number of generations played.
        self.at = tick.max(self.from);
    }

    /// The standing arrived. Only ever climbs, because the record keeps the
    /// most ground held at once and not the ground held at the end.
    pub fn holding(&mut self, squares: u32) {
        self.best = self.best.max(squares);
    }

    /// The match decided.
    pub fn decided(&mut self, won: bool) {
        self.outcome = if won { Outcome::Won } else { Outcome::Lost };
    }

    pub fn finish(&self) -> Game {
        Game {
            room: self.room.clone(),
            world: self.world,
            generations: self.at - self.from,
            best: self.best,
            outcome: self.outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(room: &str, best: u32, outcome: Outcome) -> Game {
        Game { room: room.into(), world: WorldKind::Infinite, generations: 100, best, outcome }
    }

    #[test]
    fn a_game_survives_being_written_and_read() {
        for world in [WorldKind::Infinite, WorldKind::Toroidal { rows: 6, cols: 8 }] {
            for outcome in [Outcome::Played, Outcome::Won, Outcome::Lost] {
                let g =
                    Game { room: "arena-2".into(), world, generations: 4096, best: 812, outcome };
                assert_eq!(Game::read(&g.write()), Some(g.clone()), "{g:?}");
            }
        }
    }

    /// The case this exists for: a line from a build that knew more than this
    /// one. Skipped, so one unreadable game does not cost the whole history.
    #[test]
    fn a_line_this_build_cannot_read_is_skipped_and_not_fatal() {
        let good = game("hall", 10, Outcome::Played).write();
        let text = format!("2\tfuture\tinf\t1\t1\tteam-won\n{good}\n\ngarbage\t\t\t");
        let read: Vec<Game> = text.lines().filter_map(Game::read).collect();
        assert_eq!(read.len(), 1, "the readable one, and only it");
        assert_eq!(read[0].room, "hall");
    }

    /// A store a person can edit is a store that can contain anything.
    #[test]
    fn nothing_in_the_store_can_panic_the_home_screen() {
        for line in [
            "",
            "1",
            "1\t",
            "1\thall",
            "1\thall\t6x\t1\t1\tplayed",
            "1\thall\tinf\tmany\t1\tplayed",
            "1\thall\tinf\t1\t1\tabandoned",
            "\0\t\0\t\0",
        ] {
            assert!(Game::read(line).is_none(), "{line:?}");
        }
    }

    #[test]
    fn a_summary_counts_matches_apart_from_worlds() {
        let games = vec![
            game("hall", 40, Outcome::Played),
            game("cup", 900, Outcome::Won),
            game("cup-2", 120, Outcome::Lost),
            game("cup-3", 300, Outcome::Won),
        ];
        let s = Summary::of(&games);
        assert_eq!(s.games, 4);
        assert_eq!(s.matches, 3, "a world with no way to end is not a match");
        assert_eq!(s.won, 2);
        assert_eq!(s.best, 900, "the most ever held, in any game");
        assert_eq!(s.generations, 400);

        assert!(!Summary::of(&[]).any(), "a first visit has nothing to show");
    }

    /// The record is what you were present for, and the most you ever held —
    /// not the world's age, and not what was left at the end.
    #[test]
    fn a_game_in_play_counts_what_this_player_saw() {
        let mut live = InPlay::joined("arena".into(), WorldKind::Infinite, 5_000);
        live.at(5_100);
        live.holding(400);
        live.holding(120); // lost most of it again
        assert_eq!(live.finish().generations, 100, "not the world's five thousand");
        assert_eq!(live.finish().best, 400, "the empire, not the ruins");
        assert_eq!(live.finish().outcome, Outcome::Played);

        live.decided(true);
        assert_eq!(live.finish().outcome, Outcome::Won);
    }

    /// A resync can put a client on a tick behind the one it thought it was
    /// on. Subtracting that unclamped would wrap into billions of generations.
    #[test]
    fn a_tick_that_goes_backwards_does_not_wrap() {
        let mut live = InPlay::joined("arena".into(), WorldKind::Infinite, 900);
        live.at(400);
        assert_eq!(live.finish().generations, 0);
    }
}
