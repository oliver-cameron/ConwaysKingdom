//! The numbers, and the rules they feed, in the order they are applied.
//!
//! A config, and read like one. **Why each number is what it is, and what
//! happens if it moves, is in [docs/simulation.md] and [docs/game.md]** — every
//! constant and function here is named there.
//!
//! [docs/simulation.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/simulation.md
//! [docs/game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::cell::Cell;
use super::player::PlayerId;
use super::seed::Roll;

mod order;
use order::rules;

/// Neighbours in [`super::Dir::ALL`] order: N, NE, E, SE, S, SW, W, NW.
pub type Neighbours = [Cell; 8];

/// The whole rule, as a function pointer, for swapping it wholesale.
pub type RuleFn = fn(Cell, &Neighbours, u64) -> Cell;

/// A chance out of [`super::seed::OUT_OF`], per cell, per generation.
pub type Chance = u64;

// --- Conway ------------------------------------------------------------------

/// Live neighbours a living cell needs to survive.
pub const SURVIVES_ON: [usize; 2] = [2, 3];
/// Live neighbours a dead cell needs to be born.
pub const BORN_ON: [usize; 1] = [3];

// --- territory ---------------------------------------------------------------

/// A dead cell takes the owner of a living neighbour.
pub const SPREAD: Chance = 40;
/// A dead cell takes the owner of any neighbour, including nobody.
pub const CREEP: Chance = 8;
/// A dead cell nothing lives beside loses its owner. [`super::bits::HOME`] is
/// exempt.
pub const DECAY: Chance = 2;

// --- mines -------------------------------------------------------------------

/// A dead mine costs its owner [`MINE_DRAIN`] and becomes ordinary ground.
pub const MINE_UPKEEP: Chance = 16;

// --- turrets -----------------------------------------------------------------

/// How far a turret acts, in cells.
///
/// The whole cost model: a turret reads the `(2R+1)²` box around itself twice
/// every generation — once to find the nearest square it will act on and once
/// to pick between the ones that tie — so at six that is 338 reads a turret a
/// generation and does not matter, and at twenty-four it is 4802 and a hundred
/// turrets cost more than stepping the world does.
///
/// It also bounds how far a turret can wake the world. A turret writes into a
/// chunk it reaches, and one reaching further than [`super::CHUNK_N`] could
/// write two chunks away, past what `compute_active` has any way to know
/// about. Keep this at or under a chunk.
pub const TURRET_REACH: i32 = 6;

/// A dead turret becomes ordinary ground.
///
/// Slower than [`MINE_UPKEEP`], because the two are punishing different
/// things. A dead mine is a bill to be paid off and wants a bottom to it; a
/// dead turret is a machine firing backwards over the ground behind it, and
/// four in sixty-four leaves it doing that for about sixteen generations —
/// long enough that losing an emplacement is a thing you feel and short enough
/// that it is not the end of the game.
pub const TURRET_DECAY: Chance = 4;

// --- what things cost ---------------------------------------------------------

/// What a player joins with.
pub const STARTING_VALUE: i32 = 100;
/// One cell of life.
pub const LIFE_COST: i32 = 1;
/// One mine.
pub const MINE_COST: i32 = 10;
/// One cell of a pane.
pub const ICE_COST: i32 = 5;
/// Taking back your own, and taking somebody else's.
pub const RECLAIM: i32 = 1;
/// What placing outside your own territory multiplies the cost by.
///
/// Ground somebody else holds and ground nobody has ever reached read the same
/// here, because what is being paid for is the same thing: putting something
/// where your own life has not got to. It used to be refused outright, which
/// made territory a wall rather than a price and left a player whose life died
/// out with nothing they could do about it.
///
/// Ten, so a cell of life outside costs what a mine costs inside — dear enough
/// that reaching is a decision and cheap enough that a hundred is ten cells of
/// somewhere new.
pub const OUTSIDE_MULTIPLIER: i32 = 10;
/// One turret.
///
/// Dearer than a mine, and the number to read per **emplacement** rather than
/// per cell: one turret is one live cell and dies of loneliness in a
/// generation, so the smallest turret that works is the 2x2 block, and what a
/// working turret costs is four of these. Sixty against a starting hundred —
/// an opening a player can afford exactly one of, and not while doing anything
/// else.
///
/// A turret does not inherit, so this is bought once per cell forever, where a
/// mine is bought once per *lineage*. That is the whole of why it costs more
/// than a mine while earning nothing.
pub const TURRET_COST: i32 = 15;
/// One birth of [`super::Kind::MINE`].
pub const MINE_YIELD: i32 = 1;
/// One upkeep charge on a dead mine. Dearer than a birth pays, which is
/// what makes an abandoned corpse cost more than it ever earned.
pub const MINE_DRAIN: i32 = 2;

// --- the rules, in order -----------------------------------------------------

/// What a rule left, and whether the rules after it still run.
pub enum Then {
    /// Carry on, with the cell as this rule left it.
    Next(Cell),
    /// Stop here, and let nothing else touch this cell.
    Stop(Cell),
}

rules! {
    "ice freezes what it covers" => ice,
    "territory is won and lost"  => territory,
    "life and death"             => conway,
}

impl Cell {
    /// Advance this cell one generation, by every rule in turn.
    #[inline]
    pub fn update(self, neighbours: &Neighbours, seed: u64) -> Cell {
        apply(self, neighbours, Roll::new(seed))
    }
}

fn ice(cell: Cell, _: &Neighbours, _: Roll) -> Then {
    if cell.is_ice() {
        Then::Stop(cell)
    } else {
        Then::Next(cell)
    }
}

fn territory(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Then {
    if cell.is_alive() {
        return Then::Next(cell);
    }

    let (living, alive) = living_owners(neighbours);
    if alive > 0 {
        if roll.chance(stream::SPREAD, SPREAD) {
            return Then::Next(cell.with_player(living[roll.pick(stream::SPREAD, alive)]));
        }
        return Then::Next(cell);
    }

    if cell.is_home() {
        return Then::Next(cell);
    }

    if neighbours.iter().any(|n| n.player() != cell.player())
        && roll.chance(stream::CREEP, CREEP)
    {
        return Then::Next(cell.with_player(neighbours[roll.pick(stream::CREEP, 8)].player()));
    }
    if cell.player().is_owned() && roll.chance(stream::DECAY, DECAY) {
        return Then::Next(cell.with_player(PlayerId::UNOWNED));
    }
    Then::Next(cell)
}

/// The owners of the living neighbours, and how many.
#[inline]
fn living_owners(neighbours: &Neighbours) -> ([PlayerId; 8], usize) {
    let mut out = [PlayerId::UNOWNED; 8];
    let mut found = 0;
    for n in neighbours {
        if n.is_alive() {
            out[found] = n.player();
            found += 1;
        }
    }
    (out, found)
}

fn conway(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Then {
    debug_assert!(
        !cell.is_alive() || cell.player().is_owned(),
        "a live cell must have a non-zero player"
    );
    let live = neighbours.iter().filter(|n| n.is_alive()).count();

    Then::Next(if cell.is_alive() {
        if SURVIVES_ON.contains(&live) {
            cell
        } else {
            cell.with_alive(false)
        }
    } else if BORN_ON.contains(&live) {
        parent(neighbours, roll).with_ice(false).with_home(cell.is_home())
    } else {
        cell
    })
}

/// Free-function form, so [`RuleFn`] can point at the default rule.
#[inline]
pub fn next_cell(cell: Cell, neighbours: &Neighbours, seed: u64) -> Cell {
    cell.update(neighbours, seed)
}

// --- dice --------------------------------------------------------------------

/// One stream each, so no two rolls agree by accident. See [`super::seed`].
mod stream {
    pub const SPREAD: u64 = 1;
    pub const DECAY: u64 = 2;
    pub const PARENT: u64 = 3;
    pub const UPKEEP: u64 = 4;
    pub const CREEP: u64 = 5;
    /// Which of the squares that tie for nearest a turret acts on.
    pub const TURRET: u64 = 6;
    /// Whether a dead turret has become ordinary ground.
    pub const TURRET_ROT: u64 = 7;
}

pub use stream::UPKEEP as UPKEEP_STREAM;
pub use stream::{TURRET as TURRET_STREAM, TURRET_ROT as TURRET_ROT_STREAM};

/// Which parent a birth copies.
#[inline]
fn parent(neighbours: &Neighbours, roll: Roll) -> Cell {
    let mut parents = [Cell::DEAD; 8];
    let mut found = 0;
    for n in neighbours {
        if n.is_alive() {
            parents[found] = *n;
            found += 1;
        }
    }
    let chosen = parents[roll.pick(stream::PARENT, found)];
    debug_assert!(chosen.player().is_owned(), "every parent is a live cell, so owned");

    // A birth otherwise takes everything from its parent, which is how a kind
    // travels. A kind that does not inherit passes over ownership alone: the
    // ground still changes hands, and the machine does not copy itself. See
    // `Kind::inherits` for why the two want different answers.
    //
    // After the roll rather than before it, so which parent was chosen does
    // not depend on what kind it turned out to be -- every peer must roll the
    // same number and reach the same parent whatever is standing there.
    if chosen.kind().inherits() {
        chosen
    } else {
        Cell::alive(chosen.player())
    }
}

#[cfg(test)]
mod tests;
