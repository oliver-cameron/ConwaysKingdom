//! Life running behind the menu.
//!
//! **A world of its own, and that is the whole reason this is a module.** The
//! obvious thing is to show the world the menu is already sitting on — but
//! that is the world "play alone" drops you into, and a room opens *empty*:
//! there is no seeded pattern anywhere in this game, and a solitary world that
//! started full of somebody else's soup would be a different game from the one
//! a server gives you. So this is a second world that nothing can be played
//! in, thrown away the moment a game starts.
//!
//! ## Answering the objection that was already written down
//!
//! Drawing the board behind the menu was tried and taken out, and the reason
//! is in `game::draw_calls`: *a world sliding about behind a full-height panel
//! is motion nobody asked for beside the thing they are reading.* That is a
//! good argument and it is about a **game** world — one with a camera you were
//! panning, at full contrast, doing whatever it was doing when you left.
//!
//! Three things make this a different proposition:
//!
//! - **The camera never moves.** It is set once, framed on the soup, and
//!   nothing here can pan or zoom it. What was distracting was the sliding.
//! - **It steps slowly** — [`SPAN`], against the game's own quarter second —
//!   so it reads as something alive rather than something happening.
//! - **It is small.** [`ZOOM`] is three pixels a cell, so what shows past the
//!   panel reads as texture rather than as a board somebody could play on.
//!
//! And it **wraps**, which is what makes it affordable: a torus repeats for as
//! far as anyone can look, so four chunks of soup fill any window at any zoom
//! and stepping the backdrop is stepping sixteen chunks rather than however
//! many the window happens to cover.
//!
//! The reference is Google's easter egg for "conway's game of life", which
//! runs cells over the results page: it works because it is quiet, and stops
//! working the moment it asks to be looked at.

use crate::sim::{Cell, PlayerId, World, WorldKind, OUT_OF};

/// How long a generation takes here, in seconds.
///
/// Four times the game's own, which is the difference between ambient and
/// busy. A soup settles into still lifes and blinkers within a few dozen
/// generations either way; slower just means the settling is something you
/// notice rather than something that has already happened.
pub const SPAN: f32 = 1.0;

/// How many chunks a side the soup wraps over.
///
/// **A torus, and that is what makes this cheap enough to be a backdrop.** A
/// wrapping world repeats for as far as anyone can look — see
/// [rendering.md][r] — so four chunks of soup fill a wall of screen at any
/// zoom, and stepping it is stepping sixteen chunks. A bounded plane would
/// have to be sized to the widest window anybody might open, seeded to match,
/// and stepped in full; an unbounded one is unbounded.
///
/// The repeat is findable if you look for it. At [`ZOOM`] a period is 256
/// cells, which is most of a screen, and what is on it is soup — the least
/// patterned thing there is.
///
/// [r]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/rendering.md
const CHUNKS: i32 = 4;

/// How much of the soup is alive at the start.
///
/// **A third, which is the interesting density.** Much below and it dies to a
/// few still lifes in twenty generations; much above and it is a solid block
/// that erodes from the edges. A third is where gliders come out of it.
const DENSITY: u64 = 22;

/// Screen pixels per cell.
///
/// Small, so what is behind the panel reads as texture rather than as a board
/// somebody could play on — and so the soup covers a window without needing to
/// be enormous.
pub const ZOOM: f32 = 3.0;

/// A soup, and the clock that steps it.
pub struct Attract {
    pub world: World,
    /// Seconds owed to the next generation. A leftover rather than a reset, so
    /// a slow frame does not lose a step and a fast one does not take two.
    owed: f32,
}

impl Default for Attract {
    fn default() -> Self {
        Self::new()
    }
}

impl Attract {
    /// A fresh soup.
    ///
    /// Seeded from the same generator the rules use — see [`crate::sim::seed`]
    /// — rather than from a random number crate, because this crate already has
    /// one and a backdrop is not worth a dependency. The seed is fixed, so the
    /// menu looks the same every time this build opens it: what is behind a
    /// menu should not be a thing people compare.
    pub fn new() -> Self {
        let mut world = WorldKind::Toroidal { rows: CHUNKS, cols: CHUNKS }.build();
        let roll = crate::sim::Roll::new(0x51F0_A7ED_5EED);
        let side = CHUNKS * crate::sim::CHUNK_N as i32;
        // Two owners, so the backdrop has the same two-colour reading a real
        // board does. Nobody plays them and nothing scores them.
        for row in 0..side {
            for col in 0..side {
                let at = crate::sim::mix(row as u64, col as u64);
                if !roll.chance(at, DENSITY) {
                    continue;
                }
                let who = if roll.chance(at ^ 0x9E37, OUT_OF / 2) { 1 } else { 6 };
                world.set_cell_at(row, col, Cell::alive(PlayerId(who)));
            }
        }
        Self { world, owed: 0.0 }
    }

    /// Advance it, if enough time has passed.
    ///
    /// Returns whether anything moved, so the caller only re-uploads when
    /// there is something new to upload — a backdrop that resynced every frame
    /// would cost more than the game in front of it.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.owed += dt;
        if self.owed < SPAN {
            return false;
        }
        // One at a time however far behind it is. A tab that was hidden for a
        // minute owes sixty generations, and running them all in one frame is
        // a stall — this is a backdrop, and it may simply arrive late.
        self.owed = 0.0;
        self.world.step();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A soup that is alive, and neither empty nor solid.** Both extremes
    /// look like a bug — one is a blank backdrop and the other a grey slab —
    /// and both are one constant away.
    #[test]
    fn the_soup_is_somewhere_between_empty_and_solid() {
        let it = Attract::new();
        let alive = it.world.live_cells().len();
        let side = CHUNKS as usize * crate::sim::CHUNK_N as usize;
        let squares = side * side;
        assert!(alive > squares / 8, "the backdrop is nearly empty: {alive} of {squares}");
        assert!(alive < squares / 2, "the backdrop is nearly solid: {alive} of {squares}");
    }

    /// Two owners, because a backdrop of one colour does not read as this
    /// game — territory is what the board is *about*, and one colour is a
    /// screensaver.
    #[test]
    fn the_soup_has_two_players_in_it() {
        let it = Attract::new();
        let mut seen: Vec<u8> = it
            .world
            .live_cells()
            .iter()
            .filter_map(|&(r, c)| it.world.cell_at(r, c).map(|cell| cell.player().0))
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 2, "a backdrop of one colour: {seen:?}");
    }

    /// **The clock owes rather than resets.** A backdrop stepping on every
    /// frame is the game's cost again for something nobody is looking at.
    #[test]
    fn it_steps_on_its_own_clock_and_not_on_frames() {
        let mut it = Attract::new();
        let before = it.world.generation;
        assert!(!it.advance(SPAN / 4.0), "a quarter of a span stepped it");
        assert!(!it.advance(SPAN / 4.0));
        assert_eq!(it.world.generation, before, "it stepped early");
        assert!(it.advance(SPAN), "a whole span did not step it");
        assert_eq!(it.world.generation, before + 1);
    }

    /// However far behind it is, one at a time: a tab hidden for a minute owes
    /// sixty generations and running them in one frame is a stall.
    #[test]
    fn a_long_gap_does_not_run_a_minute_of_generations_at_once() {
        let mut it = Attract::new();
        let before = it.world.generation;
        assert!(it.advance(SPAN * 60.0));
        assert_eq!(it.world.generation, before + 1, "it caught up all at once");
    }
}
