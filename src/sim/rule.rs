//! The numbers, and the rules they feed, in the order they are applied.
//!
//! A config, and read like one. **Why each number is what it is, and what
//! happens if it moves, is in [docs/simulation.md] and [docs/game.md]** — every
//! constant and function here is named there.
//!
//! So: **labelled, not explained.** A line per constant, a heading per group,
//! and the argument wherever the argument lives. What that protects is that
//! this file is editable — every number the game turns on in one screen, and
//! changing one is finding a line rather than reading an essay to be sure it
//! is the right line.
//!
//! The rules stay here beside the numbers they use, in the order they run:
//!
//! 1. the types the rules are written in
//! 2. the numbers, by what they govern
//! 3. the rule list, and the rules
//! 4. the dice, and what a birth copies
//!
//! [docs/simulation.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/simulation.md
//! [docs/game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::cell::{bits, Cell};
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

// --- Conway ----------------------------------------------------------------

/// Live neighbours a living cell needs to survive.
pub const SURVIVES_ON: [usize; 2] = [2, 3];
/// Live neighbours a dead cell needs to be born.
pub const BORN_ON: [usize; 1] = [3];

// --- territory: a level, not a flag ------------------------------------------
//
// A dead square goes to whoever pushes hardest, at what that push buys.
// [docs/simulation.md#territory] is why, and why a sum needs a cap.

/// Summed influence one level of claim costs. Reach comes from mass.
pub const LEVEL_SPREAD: u8 = 6;
/// The least a step from what feeds you costs. What bounds a halo.
pub const LEVEL_FALL: u8 = 2;
/// Levels shed per update when less reaches a square than it holds. The wake.
pub const LEVEL_EBB: u8 = 2;
/// How often a square works out what reaches it. The rate, not the outcome.
pub const LEVEL_ADJUST: Chance = 16;

// --- mines -------------------------------------------------------------------

/// A dead mine costs its owner [`MINE_DRAIN`] and becomes ordinary ground.
pub const MINE_UPKEEP: Chance = 16;

// --- turrets -----------------------------------------------------------------

/// How far a turret acts, in cells. At or under [`super::CHUNK_N`].
pub const TURRET_REACH: i32 = 6;
/// How many squares it flips a generation.
pub const TURRET_POWER: usize = 1;
/// What it plants on a square it takes. Planted, not added.
pub const TURRET_PUSH: u8 = bits::MAX_LEVEL;
/// A dead turret becomes ordinary ground.
pub const TURRET_DECAY: Chance = 4;

// --- what a new world defaults to ---------------------------------------------

/// A wrapping world's size in chunks, when whoever made it named none.
pub const DEFAULT_TORUS: (i32, i32) = (12, 12);

// --- what things cost ---------------------------------------------------------

/// What a player joins with. Zero in a match; see `server::matches`.
pub const STARTING_VALUE: i32 = 100;
/// The most anybody may hold: six figures.
///
/// **A ceiling on hoarding, not on earning.** Mining pays on birth and births
/// scale with a growing pattern, so income runs away from a big player and
/// there is nothing in the rules pushing back — see [depleted mines], which is
/// the shape of a proper answer. This is the blunt half of it, and it does two
/// things at once: it stops a purse nobody could ever spend, and it makes the
/// figure a fixed six columns wide, which is what lets the bar draw it without
/// the number changing size under the reader's eye.
///
/// [depleted mines]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#depleted-mines
pub const MAX_VALUE: i32 = 999_999;
/// One cell of life.
pub const LIFE_COST: i32 = 1;
/// One mine.
pub const MINE_COST: i32 = 10;
/// One turret. Read per emplacement: the smallest that works is four.
pub const TURRET_COST: i32 = 15;
/// One cell of a pane.
pub const ICE_COST: i32 = 5;
/// Taking back your own, and taking somebody else's.
pub const RECLAIM: i32 = 1;
/// One birth of [`super::Kind::MINE`].
pub const MINE_YIELD: i32 = 1;
/// One upkeep charge on a dead mine.
pub const MINE_DRAIN: i32 = 2;

// --- the rules, in order -----------------------------------------------------
//
// Each takes the cell as the one before left it and says whether the rest run.
// The order is a decision: ice first, so a pane freezes anything without every
// later rule having to honour it; territory before life, so ground changes
// hands on what was alive at the start of the generation.

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

/// What reaches this square, and from whom. See [docs/simulation.md#territory].
fn territory(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Then {
    // Sources are fed by what stands on them, not by their neighbours.
    if cell.is_alive() || cell.is_home() {
        return Then::Next(cell);
    }
    if !roll.chance(stream::LEVEL, LEVEL_ADJUST) {
        return Then::Next(cell);
    }

    let (player, level) = contested(neighbours, cell.player());
    let taken = |c: Cell, who, n| {
        if n == 0 {
            c.with_player(PlayerId::UNOWNED).with_level(0)
        } else {
            c.with_player(who).with_level(n)
        }
    };

    // Rises at once, ebbs a step at a time. The ebb is the wake.
    if player != cell.player() || level >= cell.level() {
        return Then::Next(taken(cell, player, level));
    }
    Then::Next(taken(cell, player, cell.level().saturating_sub(LEVEL_EBB).max(level)))
}

/// Who is pushing hardest, and how hard: every neighbour's influence summed
/// per player, each player's total less everybody else's, and the best net
/// capped [`LEVEL_FALL`] under the strongest thing feeding it.
///
/// The cap is not decoration. A sum alone feeds itself and saturates the map;
/// [docs/simulation.md#a-sum-and-why-it-needs-a-cap] has the measurement.
#[inline]
fn contested(neighbours: &Neighbours, holder: PlayerId) -> (PlayerId, u8) {
    let mut total = [0i32; PlayerId::COUNT];
    let mut best = [0u8; PlayerId::COUNT];
    let mut all = 0i32;
    for n in neighbours {
        let who = n.player();
        if !who.is_owned() {
            continue;
        }
        let push = n.influence();
        total[who.0 as usize] += push as i32;
        best[who.0 as usize] = best[who.0 as usize].max(push);
        all += push as i32;
    }

    // Ties to the holder, then to the lower number, so two peers agree and a
    // matched border does not flicker.
    let mut won = (PlayerId::UNOWNED, 0i32);
    for (i, &mine) in total.iter().enumerate().skip(1) {
        let net = mine - (all - mine);
        let who = PlayerId(i as u8);
        if mine > 0 && (net > won.1 || (net == won.1 && net > 0 && who == holder)) {
            won = (who, net);
        }
    }
    if won.1 <= 0 {
        return (PlayerId::UNOWNED, 0);
    }

    let level = (won.1 / LEVEL_SPREAD as i32)
        .min(bits::MAX_LEVEL as i32)
        .min(best[won.0 .0 as usize].saturating_sub(LEVEL_FALL) as i32) as u8;
    if level == 0 {
        (PlayerId::UNOWNED, 0)
    } else {
        (won.0, level)
    }
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
    /// Whether a square works out what reaches it this generation.
    pub const LEVEL: u64 = 1;
    pub const PARENT: u64 = 3;
    pub const UPKEEP: u64 = 4;
    /// Which of the squares that tie for nearest a turret acts on.
    pub const TURRET: u64 = 6;
    /// Whether a dead turret has become ordinary ground.
    pub const TURRET_ROT: u64 = 7;
}

pub use stream::UPKEEP as UPKEEP_STREAM;
pub use stream::{TURRET as TURRET_STREAM, TURRET_ROT as TURRET_ROT_STREAM};

/// Which parent a birth copies, and whether its kind travels.
///
/// The carve-out is after the roll, so which parent is chosen never depends on
/// what kind it turned out to be. See [`super::Kind::inherits`].
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
    if chosen.kind().inherits() {
        chosen
    } else {
        // Ownership alone: the ground changes hands, the machine does not copy.
        Cell::alive(chosen.player())
    }
}

#[cfg(test)]
mod tests;
