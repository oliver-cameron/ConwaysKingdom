//! What this client has played, and where it is kept.
//!
//! **The server keeps it now**, filed against the person rather than the
//! browser — see [`crate::net::kept`], which holds the shape, and
//! [`crate::net::ClientMessage::Keep`], which sends it. This module is what
//! reads it on a home screen and what keeps a copy for playing alone.
//!
//! That is a change of premise rather than of plumbing. This used to be "the
//! one thing the client knows and the server does not", and the cost of that
//! was in the known-bugs list: a diary was a fact about a machine, so playing
//! on a phone and a laptop was two records of one person. The server is the
//! authority and this is the cache.
//!
//! Deliberately small. Five numbers a player recognises — worlds played,
//! matches won, the most ground ever held, generations lived through — and not
//! a statistics page. A record on a home screen is there to make the last
//! session feel like it happened, not to be studied.

pub use crate::net::kept::{Game, Outcome};

use crate::sim::WorldKind;

/// How many games are kept here.
///
/// The server's own cap, so the cache and the store hold the same number and a
/// game does not appear on one screen and not another.
pub const KEEP: usize = crate::net::kept::GAMES_MOST;

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

/// Every game this client has a copy of, newest first.
pub fn games() -> Vec<Game> {
    crate::net::jsonl::read(&crate::net::keep::games(), "the games this client kept")
}

/// File a finished game, newest first, dropping the oldest past [`KEEP`].
///
/// Written here and sent up separately — see `Session::file_game`. The two are
/// not one call because a client with no link still plays and still remembers.
pub fn remember(game: &Game) {
    let mut kept = games();
    kept.insert(0, game.clone());
    kept.truncate(KEEP);
    write(&kept);
}

/// Replace the copy wholesale, which is what arriving on a server does: the
/// server is the authority and this is the cache — see [`crate::net::kept`].
pub fn replace_with(games: &[Game]) {
    write(games);
}

fn write(games: &[Game]) {
    crate::net::keep::remember_games(&crate::net::jsonl::write(games));
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
                let text = crate::net::jsonl::write([&g]);
                assert_eq!(
                    crate::net::jsonl::read::<Game>(&text, "test"),
                    vec![g.clone()],
                    "{g:?}"
                );
            }
        }
    }

    /// **A store a person can edit is a store that can contain anything**, and
    /// this one is `localStorage`. A row this build cannot read is skipped, so
    /// one unreadable game does not cost the whole history and no keystroke in
    /// a browser's inspector can make the home screen panic.
    #[test]
    fn nothing_in_the_store_can_panic_the_home_screen() {
        let good = crate::net::jsonl::write([game("hall", 10, Outcome::Played)]);
        let text = format!(
            "{good}{}",
            concat!(
                r#"{"room":"future","world":"Infinite","generations":1,"best":1,"outcome":"drew"}"#,
                "\n",
                r#"{"room":"short"}"#,
                "\n\n",
                "garbage\t\t\t\n",
                "\0\0\0\n",
                r#"{"room":"hall","world":{"Toroidal":{"rows":"many","cols":1}},"#,
                r#""generations":1,"best":1,"outcome":"played"}"#,
                "\n",
            )
        );
        let read = crate::net::jsonl::read::<Game>(&text, "test");
        assert_eq!(read.len(), 1, "the readable one, and only it: {read:?}");
        assert_eq!(read[0].room, "hall");
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
