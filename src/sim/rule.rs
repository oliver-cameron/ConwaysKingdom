//! The numbers, and the rules they feed, in the order they are applied.
//!
//! Meant to read like a config, because that is what it is. Every tunable
//! number in the game is a constant here and every rule is one entry in
//! [`RULES`]. The dice are [`super::seed`], the tests are next door, and the
//! macro that turns the list into a chain is `rule::order`.
//!
//! A rule gets a cell and its eight neighbours — whole cells, not a count, so
//! it can branch on what a cell *is* — and knows nothing about chunks, worlds
//! or topology.

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

/// Live neighbours a dead cell needs to be born. `[2, 3]` and `[3]` is Conway;
/// `[2, 3]` and `[3, 6]` is HighLife.
pub const BORN_ON: [usize; 1] = [3];

// --- territory ---------------------------------------------------------------

/// A dead cell takes the owner of a living neighbour.
pub const SPREAD: Chance = 40;

/// A dead cell takes the owner of a neighbouring cell, whoever that is —
/// **including nobody**, which is what makes one rule both spread and erode.
///
/// Inside a region every neighbour agrees and nothing moves. At an edge a cell
/// with five owned neighbours and three empty ones has five-in-eight odds of
/// staying and three-in-eight of going, and the square outside has the same
/// odds the other way — so a border is an unbiased walk that neither runs away
/// nor rots, and a thin trail, being nearly all empty neighbours, goes quickly.
pub const CREEP: Chance = 8;

/// A slow bleed on top of [`CREEP`], so ground nothing lives on goes rather
/// than settling into a shape and staying. Without it a patch nobody has
/// touched for four hundred generations is still two hundred squares; with it,
/// none. Granted ground is exempt — see [`super::bits::HOME`].
pub const DECAY: Chance = 2;

// --- mines -------------------------------------------------------------------

/// A dead mine costs its owner [`MINE_DRAIN`].
///
/// Sixteen is the line where a blinker pays and a glider does not: +432 against
/// −1049 over three hundred generations. `cargo run --no-default-features
/// --example balance` prints the table.
pub const MINE_UPKEEP: Chance = 16;

// --- what things cost ---------------------------------------------------------

/// What a player joins with.
pub const STARTING_VALUE: i32 = 100;
/// Drawn by the stroke, so it has to be cheap enough to draw with.
pub const LIFE_COST: i32 = 1;
/// What you are buying is a lineage, not a cell. Against [`MINE_YIELD`] this
/// is the payback period.
pub const MINE_COST: i32 = 10;
/// A wall that costs what a cell costs is not a decision.
pub const ICE_COST: i32 = 5;
/// Reclaiming your own, and what destroying somebody else's costs. Equal to
/// [`LIFE_COST`], so rearranging your own board is free.
pub const RECLAIM: i32 = 1;
/// What one birth of [`super::Kind::MINE`] pays.
pub const MINE_YIELD: i32 = 1;
/// What one upkeep charge on a dead mine costs.
pub const MINE_DRAIN: i32 = 1;

// --- the rules, in order -----------------------------------------------------

/// What a rule left, and whether the rules after it still run.
pub enum Then {
    /// Carry on, with the cell as this rule left it.
    Next(Cell),
    /// Stop here, and let nothing else touch this cell.
    Stop(Cell),
}

// Ice first, so a pane freezes anything without every rule after it having to
// honour it. Territory before life, so ground changes hands on what was alive
// at the start of the generation and not on what its own births left behind.
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

/// Under ice is time-stopped, alive or not.
fn ice(cell: Cell, _: &Neighbours, _: Roll) -> Then {
    if cell.is_ice() {
        Then::Stop(cell)
    } else {
        Then::Next(cell)
    }
}

/// Won from life, traded with the ground around it, and lost where nothing
/// lives. Dead cells only, and it sets the owner and nothing else.
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

    // Granted ground answers to life alone.
    if cell.is_home() {
        return Then::Next(cell);
    }

    // Nothing to trade with if every neighbour already agrees, and most of an
    // empty world is exactly that -- so this guard, not the roll, is what the
    // common case costs.
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

/// The owners of the living neighbours, and how many. A fixed array rather than
/// a `Vec`: this runs for every dead cell of every active chunk, every
/// generation.
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

/// Live, die, or be born, by [`SURVIVES_ON`] and [`BORN_ON`].
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
        // A copy of one of its parents: owner, kind and all. The dead cell's
        // own metadata is discarded, which is the whole of how a kind spreads
        // — a mine's children are mines.
        parent(neighbours, roll)
            // Ice is cleared because a parent may be under a pane and count as
            // a live neighbour while frozen, and a birth outside the pane must
            // not inherit it.
            .with_ice(false)
            // HOME marks the square, so it stays with the square. It is the one
            // thing about a newborn that does not come from its parent.
            .with_home(cell.is_home())
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

/// Which stream each roll asks on, so that they are independent of each other:
/// a square that decays must not also be the square that would have been
/// claimed. Any distinct numbers will do — see [`super::seed`].
mod stream {
    pub const SPREAD: u64 = 1;
    pub const DECAY: u64 = 2;
    pub const CREEP: u64 = 5;
    pub const PARENT: u64 = 3;
    /// Read by [`crate::sim::Halo::step_into`], which charges the upkeep
    /// because it is the only place holding a cell before and after.
    pub const UPKEEP: u64 = 4;
}

pub use stream::UPKEEP as UPKEEP_STREAM;

/// Which parent a birth copies. Indexed by the roll rather than by iteration
/// order, so every peer picks the same one.
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
    chosen
}

#[cfg(test)]
mod tests;
