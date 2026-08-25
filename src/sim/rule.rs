//! The numbers, and the rules they feed, in the order they are applied.
//!
//! A config, and read like one. **Why each number is what it is, and what
//! happens if it moves, is in [docs/simulation.md] and [docs/game.md]** — every
//! constant and function here is named there.
//!
//! So the constants are **labelled, not explained**, and the rules stay here
//! beside them. A reason belongs in the docs where it can be read against the
//! reasons next to it; written here it buries the list somebody opened this
//! file to see, and there is no telling from a wall of prose which line is the
//! one they came for. A line each, a section heading, and the argument
//! wherever the argument lives.
//!
//! What that is protecting is that this file is **editable**: every number the
//! game turns on is in one screen, and changing one is finding a line rather
//! than reading an essay to make sure it is the right line.
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

// --- Conway ----------------------------------------------------------------

/// Live neighbours a living cell needs to survive.
pub const SURVIVES_ON: [usize; 2] = [2, 3];
/// Live neighbours a dead cell needs to be born.
pub const BORN_ON: [usize; 1] = [3];

// --- territory: a level, not a flag ------------------------------------------
//
// [docs/simulation.md#territory] explains the whole of it -- why a flag could
// not work, why the claim is a strongest-of rather than a sum, and what the
// roll is deciding.

/// Influence lost crossing one square. The only thing bounding a halo.
pub const LEVEL_FALL: u8 = 2;
/// Levels given up per update where less reaches a square than it holds.
/// Claims rise at once and ebb a step at a time; this is the wake.
pub const LEVEL_EBB: u8 = 2;
/// How often a square works out what reaches it. The rate, not the outcome.
pub const LEVEL_ADJUST: Chance = 16;

// --- mines -------------------------------------------------------------------

/// A dead mine costs its owner [`MINE_DRAIN`] and becomes ordinary ground.
pub const MINE_UPKEEP: Chance = 16;

// --- turrets -----------------------------------------------------------------

/// How far a turret acts, in cells. At or under [`super::CHUNK_N`], or one
/// could write two chunks away, past what `compute_active` knows about.
pub const TURRET_REACH: i32 = 6;
/// How many squares it flips a generation.
pub const TURRET_POWER: usize = 1;
/// What it plants on a square it takes. Planted, not added: the territory rule
/// assigns rather than accumulates, so a nudge would be wiped.
pub const TURRET_PUSH: u8 = super::cell::bits::MAX_LEVEL;
/// A dead turret becomes ordinary ground.
pub const TURRET_DECAY: Chance = 4;

// --- what things cost ---------------------------------------------------------

/// What a player joins with. Zero in a match; see `server::matches`.
pub const STARTING_VALUE: i32 = 100;
/// One cell of life.
pub const LIFE_COST: i32 = 1;
/// One mine.
pub const MINE_COST: i32 = 10;
/// One turret. Read per **emplacement**: the smallest one that works is four.
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

/// What reaches this square, and from whom.
///
/// One rule where there were three. `SPREAD`, `CREEP` and `DECAY` were a
/// constant and a branch each, and the names lied — spread did not spread,
/// creep did. A square takes the **strongest claim** reaching it, and a claim
/// that has fallen to nothing leaves it to nobody, which is the whole of
/// winning, losing and forgetting ground.
///
/// Sources are left alone: a living cell's square is fed by the cell, and
/// granted ground is a spring. Neither is worked out from its neighbours, so
/// neither can be argued away by them.
fn territory(cell: Cell, neighbours: &Neighbours, roll: Roll) -> Then {
    if cell.is_alive() || cell.is_home() {
        return Then::Next(cell);
    }
    // The rate, not the outcome. See `LEVEL_ADJUST`.
    if !roll.chance(stream::LEVEL, LEVEL_ADJUST) {
        return Then::Next(cell);
    }

    let (player, level) = strongest(neighbours, cell.player());

    // Claims **rise at once and ebb a step at a time**. Assigning outright in
    // both directions is the tidier rule and gives a glider no trail at all:
    // the square behind it goes from held to nobody's the moment it looks.
    // Ground that drains rather than switching off leaves a short, thinning
    // wake, which is what something passing through ought to leave.
    //
    // Only downwards. A claim that has arrived is felt immediately, or a
    // frontier would lag behind the life pushing it.
    let holder = cell.player();
    if player != holder || level >= cell.level() {
        if level == 0 {
            return Then::Next(cell.with_player(PlayerId::UNOWNED).with_level(0));
        }
        return Then::Next(cell.with_player(player).with_level(level));
    }
    let ebbed = cell.level().saturating_sub(LEVEL_EBB).max(level);
    if ebbed == 0 {
        return Then::Next(cell.with_player(PlayerId::UNOWNED).with_level(0));
    }
    Then::Next(cell.with_level(ebbed))
}

/// The best claim reaching a square, and whose it is.
///
/// **Strongest claim rather than a sum of all eight**, and the difference
/// matters. A sum makes a diagonal neighbour count as much as an orthogonal
/// one, so the field grows as a square rather than a disc and the number stops
/// being a distance — and a number that is not a distance is one nobody can
/// read off the screen. The best-of is Minecraft's water, and it is what makes
/// a front between two players settle at the line equidistant between them
/// without anything having to work out where that is.
///
/// Mass still gets a say, at the one place it can without bending the
/// geometry: **ties go to the player pushing hardest**, by the total of
/// everything they have reaching this square. Where that ties too, the square
/// keeps whoever holds it, and failing that the lower number — so two peers
/// always agree and a border does not flicker between two owners who are
/// exactly matched.
#[inline]
fn strongest(neighbours: &Neighbours, holder: PlayerId) -> (PlayerId, u8) {
    let mut best = [0u8; PlayerId::COUNT];
    let mut total = [0u16; PlayerId::COUNT];
    for n in neighbours {
        let who = n.player();
        if !who.is_owned() {
            continue;
        }
        let claim = n.influence().saturating_sub(LEVEL_FALL);
        if claim == 0 {
            continue;
        }
        let i = who.0 as usize;
        best[i] = best[i].max(claim);
        total[i] += claim as u16;
    }

    let mut won = (PlayerId::UNOWNED, 0u8, 0u16);
    for (i, &claim) in best.iter().enumerate().skip(1) {
        if claim == 0 {
            continue;
        }
        let who = PlayerId(i as u8);
        let mass = total[i];
        let better = claim > won.1
            || (claim == won.1
                && (mass > won.2 || (mass == won.2 && who == holder)));
        if better {
            won = (who, claim, mass);
        }
    }
    (won.0, won.1)
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
