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

use super::cell::{bits, Ages, Cell};
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
///
/// **A roll and not a count**, which was tried the other way and is wrong for
/// two reasons. The scatter is doing work: a corpse reborn before the charge
/// falls due escapes it, and a chance means *some* of a pattern's corpses
/// escape rather than all of them or none, which is what grades the cost by
/// how much a pattern leaves lying about. And the age field is spoken for —
/// [depleted mines] wants it, and a mine's age is a much better fade than a
/// flag would be.
///
/// [depleted mines]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#depleted-mines
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

// --- payloads ----------------------------------------------------------------

/// How often a payload's fuse advances, while it is alive and not under ice.
///
/// A chance rather than a count, and the second reason is what earns it. A
/// chance **scatters** payloads laid in one gesture, so four do not go off in
/// lockstep. And [`PAYLOAD_WARN`] makes the last step certain, so the warning
/// is reliable — a weapon with a random warning is a weapon with no warning.
pub const PAYLOAD_FUSE: Chance = 16;
/// The age at and above which the fuse always advances.
///
/// One below [`bits::MAX_AGE`], so the last sprite is on screen for exactly
/// one generation, always.
pub const PAYLOAD_WARN: u8 = bits::MAX_AGE - 1;
/// How far **one** stick of dynamite reaches from its centre, in cells.
///
/// **Six, and it was ten.** Area goes as the square, so ten was a disc of about
/// three hundred and seventeen squares and six is a hundred and thirteen —
/// roughly a third of the ground for the same price, on top of now paying
/// [`BLAST_DRAIN`] per square of it. Ten turned over more of somebody's country
/// in one generation than they could rebuild in twenty, which is a weapon that
/// ends a game rather than one that changes it.
///
/// [`PAYLOAD_MOST_REACH`] follows it, being a multiple.
///
/// Ten rather than eight: a payload has to be built around and kept alive to
/// go off at all, and eight was a blast you had to look for.
///
/// A cluster that goes off together reaches further — see [`blast_reach`],
/// where each payload is worth a constant *area* of blast.
pub const PAYLOAD_REACH: i32 = 6;

/// The furthest any blast may reach, however many payloads went into it.
///
/// Ten times one payload's, so a hundred of them is the biggest bomb there is.
/// A bound rather than a balance figure: the pass is one roll per square,
/// which is nothing until somebody works out that a thousand payloads would
/// rewrite a quarter of a large world in one generation.
pub const PAYLOAD_MOST_REACH: i32 = PAYLOAD_REACH * 10;

/// How far a cluster of `n` payloads reaches when they go off together.
///
/// **Each payload is worth a constant area**, so the radius goes as the square
/// root of how many there are: a hundred of them reach ten times as far as
/// one, not a hundred times. Anything else and a blob is either worth less
/// than laying the same payloads apart — which makes clustering pointless —
/// or so much more that nothing else in the game matters.
///
/// It is also the honest reading of what a cluster *is*: one bomb made of n
/// charges, rather than n bombs that happen to be adjacent.
pub fn blast_reach(n: usize) -> i32 {
    let reach = (PAYLOAD_REACH as f64 * (n as f64).sqrt()).round() as i32;
    reach.clamp(PAYLOAD_REACH, PAYLOAD_MOST_REACH)
}
/// How many squares in sixty-four a detonation brings to life.
///
/// Conway's classic soup is a half, which mostly burns down; a third is where
/// a random field goes on happening longest. This wants playing with rather
/// than deriving — see `examples/balance.rs`.
pub const PAYLOAD_DENSITY: u64 = 24;
/// The furthest a blast's centre may be thrown from the payload, in cells.
///
/// **Bounded, or it is a homing weapon with a range of the whole world.** A
/// payload deep inside a large country lobs itself at the nearest frontier;
/// past this it goes off where it stands.
pub const PAYLOAD_THROW: i32 = 12;
/// How much of a blast's disc has to be **held by somebody else** for it to be
/// worth setting off there, in squares out of sixty-four.
///
/// Somebody else's, not merely not-yours: unowned ground passes "not mine"
/// trivially, so that test sent payloads out of their own country to go off
/// over the nearest empty stretch — and over the debris of earlier blasts,
/// which is mostly unowned, so they detonated in each other's craters.
///
/// A quarter, which is lower than it reads: a disc centred on a frontier is
/// half somebody's country at best, and anything further in has to be reached
/// by walking past ground that qualifies less.
pub const PAYLOAD_FOREIGN: u64 = 16;

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
/// What a payload costs. Dearer than a turret: a turret takes one square a
/// generation and this rearranges a disc.
pub const PAYLOAD_COST: i32 = 40;
/// One cell of a pane.
pub const ICE_COST: i32 = 5;
/// Taking back your own, and taking somebody else's.
pub const RECLAIM: i32 = 1;
/// One birth of [`super::Kind::MINE`] that pays.
pub const MINE_YIELD: i32 = 1;

/// The age a mine pays most often at, and how often it pays there and when it
/// is spent — out of [`super::OUT_OF`].
///
/// **Mine income used to scale faster than territory.** A mine pays when one of
/// its kind is born, and births scale with the perimeter of a growing pattern,
/// so a player with four times the ground earned more than four times as much
/// and could spend it on more ground. Nothing pushed back. See
/// [docs/planned.md#depleted-mines].
///
/// What pushes back is the square. A mine born where a mine has been born
/// before inherits that square's depletion and adds to it, so a pattern that
/// keeps re-birthing over the same cells wears them out — and the wear stays on
/// the corpse, so it is not escaped by dying. It is cleared only when the
/// corpse is finally swept to ordinary ground, which is [`MINE_UPKEEP`].
///
/// **A parabola rather than a fade, so there is an age worth holding.** A curve
/// that only fell would make every mine worth most on the generation it was
/// laid, and the only decision would be to lay more. With a peak at
/// [`MINE_PRIME`] the shape rewards letting a field mature and then retiring
/// it, and most of a mine's life is on the falling side, so "older pays less"
/// is still what a player sees:
///
/// ```text
///   age    0   1   2   3   4   5   6   7
///   pays  55  62  64  62  55  43  26   4     out of 64
/// ```
pub const MINE_PRIME: u8 = 2;
/// What a mine at its prime pays, out of [`super::OUT_OF`].
pub const MINE_BEST: Chance = 64;
/// And what one worn all the way out still pays. Not nought: a mine that could
/// never pay again is a cell to be told about, and it is told by the sprite
/// rather than by a surprise.
pub const MINE_SPENT: Chance = 4;

/// How likely a mine this depleted is to pay for a birth.
///
/// Integer arithmetic throughout, because two peers must agree exactly and a
/// float is a way for them not to — see [docs/simulation.md] on determinism.
pub fn mine_chance(age: u8) -> Chance {
    let prime = MINE_PRIME as i64;
    let far = (age as i64 - prime).abs();
    // The longer arm of the parabola, so the far end lands exactly on spent.
    let widest = (bits::MAX_AGE as i64 - prime).max(prime).max(1);
    let fall = (MINE_BEST as i64 - MINE_SPENT as i64) * far * far / (widest * widest);
    (MINE_BEST as i64 - fall).max(MINE_SPENT as i64) as Chance
}
/// One upkeep charge on a dead mine.
pub const MINE_DRAIN: i32 = 2;

/// What one square of blast costs the player who set it off.
///
/// **Charged on detonation and by area, which is the nerf.** Dynamite used to
/// cost [`PAYLOAD_COST`] once, when it was laid, and nothing afterwards — so
/// the cheapest thing in the game was to lay one and let a glider carry it,
/// and a blob of them was the cheapest of all, because
/// [`blast_reach`] gives a hundred of them ten times the reach and therefore a
/// hundred times the ground for a hundred times the purchase price and no more.
///
/// Paying per square turned over makes the *effect* the thing bought rather
/// than the fuse, so a blob costs exactly what a blob does. It also puts a real
/// decision on a chain: each link is another disc and another bill, and a chain
/// that runs away is one that empties a purse.
///
/// Small, because the areas are not: one payload's disc is a little over three
/// hundred squares, so at this it is about the price of laying it again.
pub const BLAST_DRAIN: i32 = 1;

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
    "whatever a kind counts"     => fuse,
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

/// **Whatever this kind counts, counts** — see [`Ages`], which is the one
/// place a kind's rules live.
///
/// One kind counts today and its own row says so, which is the point of the
/// table: adding the second is a row rather than a branch here.
///
/// After `ice`, so a frozen fuse does not burn: a pane stops time over what it
/// covers and that is every rule. Before `conway`, so this reads the cell as
/// it is now — a payload that dies this generation stops at the age it had
/// reached rather than gaining one on the way out.
fn fuse(cell: Cell, _: &Neighbours, roll: Roll) -> Then {
    if cell.age() >= bits::MAX_AGE {
        return Then::Next(cell);
    }
    match cell.kind().ages() {
        Ages::Never => Then::Next(cell),
        // **While it lives.** Certain at the last step and a chance before it:
        // the chance scatters payloads laid in one gesture, and the certainty
        // is what makes the warning a tell somebody can act on.
        Ages::Fuse(rate) if cell.is_alive() => {
            if cell.age() >= PAYLOAD_WARN || roll.chance(stream::FUSE, rate) {
                Then::Next(cell.with_age(cell.age() + 1))
            } else {
                Then::Next(cell)
            }
        }
        _ => Then::Next(cell),
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
        let born = parent(neighbours, roll).with_ice(false).with_home(cell.is_home());
        // **A mine inherits the square's depletion, not its parent's.** What
        // wears out is the ground: a pattern that keeps re-birthing over the
        // same cells is what income has to be bounded by, and a lineage that
        // travels is not. `parent` clears the age for every other kind, which
        // is right — a payload carried by a glider arms itself from nought —
        // and this is the one kind whose age is a fact about where it is.
        if born.kind() == super::Kind::MINE {
            born.with_age((cell.age() + 1).min(bits::MAX_AGE))
        } else {
            born
        }
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
    /// Whether a dead mine's charge fell due this generation.
    pub const UPKEEP: u64 = 4;
    /// Which of the squares that tie for nearest a turret acts on.
    pub const TURRET: u64 = 6;
    /// Whether a dead turret has become ordinary ground.
    pub const TURRET_ROT: u64 = 7;
    /// Whether a payload's fuse advanced this generation.
    pub const FUSE: u64 = 8;
    /// Whether a square inside a blast comes up alive.
    pub const BLAST: u64 = 9;
    /// Which of the centres that tie for nearest a blast is thrown to.
    pub const THROW: u64 = 10;
    /// Whether a mine's birth paid, which falls with the square's depletion.
    ///
    /// **Its own stream**, like every other roll here. Two questions asked of
    /// one stream on one square in one generation get the same answer, so a
    /// mine's payout would have been decided by whatever the upkeep roll said.
    pub const YIELD: u64 = 11;
}

pub use stream::UPKEEP as UPKEEP_STREAM;
pub use stream::YIELD as YIELD_STREAM;
pub use stream::{BLAST as BLAST_STREAM, THROW as THROW_STREAM};
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
        // **The kind travels and the age does not.** A birth is a new cell, so
        // whatever its parent was part way through, this one is at the start:
        // a payload carried by a glider arms itself from nought rather than
        // arriving already about to go off, and a mine's depletion is a fact
        // about a mine rather than about its line.
        chosen.with_age(0)
    } else {
        // Ownership alone: the ground changes hands, the machine does not copy.
        Cell::alive(chosen.player())
    }
}

#[cfg(test)]
mod tests;
