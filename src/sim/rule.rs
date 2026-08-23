//! The numbers, and the rules they feed — in the order the rules are applied.
//!
//! This file is meant to read like a config, because that is what it is. Every
//! tunable number in the game is a constant here: the survival counts, how fast
//! ground changes hands, what everything costs and what mining pays. Every rule
//! is one entry in [`RULES`], applied in order, each taking the cell as the one
//! before it left it. Nothing else is in here — the dice are [`super::seed`]'s
//! problem and the tests are next door — so the rules of the game can be read
//! on one screen and changed by editing a number.
//!
//! What a rule gets is a cell and its eight neighbours, whole cells rather than
//! a count, so it can branch on what a cell *is*. It knows nothing about
//! chunks, worlds or topology.
//!
//! Two invariants hold throughout. **A live cell always has a non-zero
//! player**, because unowned life would have nobody to attribute a birth to.
//! And survival and death touch **only the alive bit**, so a cell that dies
//! keeps its owner and its kind — "recently died, and whose it was" exists
//! without a field for it.

use super::cell::Cell;
use super::player::PlayerId;
use super::seed::Roll;

mod order;
use order::rules;

/// Neighbours in [`super::Dir::ALL`] order: N, NE, E, SE, S, SW, W, NW.
pub type Neighbours = [Cell; 8];

/// A rule. Swap in a different one by changing what the world calls; the
/// signature is a plain function pointer, so there is no dispatch cost.
pub type RuleFn = fn(Cell, &Neighbours, u64) -> Cell;

// --- Conway ------------------------------------------------------------------

/// Live neighbours a **living** cell needs to see the next generation.
pub const SURVIVES_ON: [usize; 2] = [2, 3];

/// Live neighbours a **dead** cell needs to be born. More than one entry and a
/// birth may have more or fewer than three parents, which `parent` handles.
///
/// These two arrays are the whole of the rule everything else is built on.
/// `[2, 3]` and `[3]` is Conway; `[2, 3]` and `[3, 6]` is HighLife, where
/// replicators exist. Changing them changes the game and nothing else has to
/// know.
pub const BORN_ON: [usize; 1] = [3];

// --- territory ---------------------------------------------------------------

/// How often a dead cell beside living ones is claimed by one of them: ten
/// generations in sixteen.
///
/// Not every generation, so a cell between two players goes to whoever's life
/// was beside it more often rather than to whoever's life was beside it last.
pub const SPREAD_CHANCE: (u64, u64) = (10, 16);

/// One chance in this many, per generation, that a dead cell with nothing alive
/// beside it loses its owner.
///
/// Sixteen is about four seconds at the default rate: long enough that a
/// pattern flickering off and on keeps its ground, short enough that a glider's
/// trail fades behind it rather than staking a claim across the world.
///
/// Granted ground is exempt — see [`super::bits::HOME`]. Without that floor a
/// player whose life died out would lose every square they had, and placing is
/// confined to your own territory, so they could never place again.
pub const DECAY_ODDS: u64 = 16;

// --- mines -------------------------------------------------------------------

/// One chance in this many, per generation, that a **dead** mine costs its
/// owner.
///
/// A mine pays when its line is born and costs for as long as its corpse lies
/// there, so income is growth minus the upkeep of everything you have let die.
/// That is what stops a field of mines being something you lay once and forget.
///
/// **Four**, chosen by measuring — `cargo run --no-default-features --example
/// balance` prints this table, in value per generation at steady state:
///
/// ```text
///    odds     block   blinker    glider   r-pentomino
///       1       0.0       0.0     -20.0        -695.0
///       4       0.0       1.5      -3.5        -116.8
///       8       0.0       1.8      -0.8         -20.4
///      16       0.0       1.9       0.6          27.8
///      32       0.0       1.9       1.3          51.9
/// ```
///
/// A blinker — three cells, two corpses — pays; a glider, dragging a trail of
/// twenty behind it, bleeds; and sprawl bleeds badly. **A machine that stays
/// where you put it earns, and anything that leaves a mess does not.** Above
/// sixteen everything pays and sprawl pays best, which is where this started.
///
/// The drain is bounded by [`DECAY_ODDS`] rather than by a timer: a corpse with
/// nothing alive beside it loses its owner and stops costing anybody, so
/// abandoned ground bleeds briefly and goes quiet, while corpses inside a living
/// colony are re-claimed every generation and go on costing.
///
/// This says how *often* and [`MINE_DRAIN`] says how *much*. This is the half
/// that has to be identical on every peer.
pub const MINE_UPKEEP_ODDS: u64 = 4;

// --- what things cost ---------------------------------------------------------
//
// Here rather than beside the wire types that spend them, because a price is a
// rule: "life costs one" is the same kind of statement as "a cell survives on
// two or three", and somebody balancing the game should not have to find them
// in two files. `net` names the actions and reads these numbers.

/// What a player joins with. Ten mines, or a hundred cells of life.
pub const STARTING_VALUE: i32 = 100;

/// Life is cheap because it is drawn by the stroke rather than placed cell by
/// cell: a pencil lays tens of cells in a gesture, and at five a cell that is a
/// gesture nobody can afford.
pub const LIFE_COST: i32 = 1;

/// A mine is dear because what you are buying is a lineage, not a cell. Against
/// [`MINE_YIELD`] this is the payback period, and it is the number that decides
/// whether mining is worth doing at all.
pub const MINE_COST: i32 = 10;

/// A pane is a wall, and a wall that costs what a cell costs is not a decision.
pub const ICE_COST: i32 = 5;

/// What reclaiming your own living cell pays, and what destroying somebody
/// else's costs — taking ground should not be free.
///
/// Equal to [`LIFE_COST`], deliberately: putting a cell down and taking it back
/// is free, so you may rearrange your own board as much as you like. What
/// drains value is the rule, not the act of placing.
pub const RECLAIM: i32 = 1;

/// What one birth of [`super::Kind::MINE`] pays its owner.
pub const MINE_YIELD: i32 = 1;

/// What one upkeep charge on a dead mine costs its owner. How *often* that
/// falls due is [`MINE_UPKEEP_ODDS`].
pub const MINE_DRAIN: i32 = 1;

// --- the rules, in order -----------------------------------------------------

/// What a rule left, and whether the rules after it still run.
pub enum Then {
    /// Carry on, with the cell as this rule left it.
    Next(Cell),
    /// Stop here, and let nothing else touch this cell.
    Stop(Cell),
}

// **The rules, in the order they are applied.** Read top to bottom and that is
// the game.
//
// An ordered list rather than one function with the order buried in its
// branches, because the order is a decision and decisions should be visible.
// Ice comes first, so a pane freezes anything without every rule after it
// having to remember to honour it. Territory comes before life, so ground
// changes hands on what was alive at the start of the generation rather than
// on what that same generation's births left behind.
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

/// Under ice is time-stopped, whatever the cell is and whether or not it is
/// alive.
fn ice(cell: Cell, _: &Neighbours, _: Roll) -> Then {
    if cell.is_ice() {
        Then::Stop(cell)
    } else {
        Then::Next(cell)
    }
}

/// Ground is won by life growing over it and lost when life goes away.
///
/// Only dead ground: a living cell's square belongs to whoever is standing on
/// it. Sets the owner and nothing else — the cell stays as dead as it was.
fn territory(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Then {
    if cell.is_alive() {
        return Then::Next(cell);
    }
    let (claimants, found) = living_owners(neighbours);
    if found > 0 {
        if roll.chance(stream::SPREAD, SPREAD_CHANCE) {
            return Then::Next(cell.with_player(claimants[roll.pick(stream::SPREAD, found)]));
        }
    } else if cell.player().is_owned()
        && !cell.is_home()
        && roll.one_in(stream::DECAY, DECAY_ODDS)
    {
        return Then::Next(cell.with_player(PlayerId::UNOWNED));
    }
    Then::Next(cell)
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
    pub const PARENT: u64 = 3;
    /// Read by [`crate::sim::Halo::step_into`], which charges the upkeep
    /// because it is the only place holding a cell before and after.
    pub const UPKEEP: u64 = 4;
}

pub use stream::UPKEEP as UPKEEP_STREAM;

/// The owners of the living neighbours, and how many there were.
///
/// A fixed array and a count rather than a `Vec`, because this runs for every
/// dead cell of every active chunk of every generation and was the one
/// allocation in the hot loop.
#[inline]
fn living_owners(neighbours: &Neighbours) -> ([PlayerId; 8], usize) {
    let mut owners = [PlayerId::UNOWNED; 8];
    let mut found = 0;
    for n in neighbours {
        if n.is_alive() {
            owners[found] = n.player();
            found += 1;
        }
    }
    (owners, found)
}

/// Which parent a birth is a copy of.
///
/// Scanned in a fixed order and indexed by the roll, so the choice depends only
/// on the seed and never on iteration order — the same on every peer, and
/// reproducible when replaying a tick.
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
