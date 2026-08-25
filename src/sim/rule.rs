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

/// How much influence is lost crossing one square.
///
/// The whole feel of the map hangs on this. A source is
/// [`super::bits::MAX_LEVEL`], so at one it reaches seven squares and a lone
/// blinker holds a disc of about a hundred and fifty; at two it reaches three
/// and holds about thirty. Two, because a blinker should spread its influence
/// a little and not gain territory everywhere.
///
/// It is also the *only* thing bounding a halo. There is no rule about radius
/// anywhere — the fall is the rule, and the field is a pure function of where
/// the sources are, so it cannot drift or ratchet outward the way the old one
/// did.
pub const LEVEL_FALL: u8 = 2;

/// How often a dead square works out what reaches it, out of
/// [`super::seed::OUT_OF`].
///
/// **The roll decides the rate, not the outcome.** That is the change from the
/// old rule, where a coin flip chose which owner a square took. Recomputed
/// every generation for every square, this field would be an exact distance
/// transform that snaps the instant anything moves, and a glider would drag a
/// geometrically perfect halo behind it. Updating a fraction per generation
/// makes it lag and smear, which is the difference between a country and a
/// Voronoi diagram — and a square that is not updating costs one roll.
pub const LEVEL_ADJUST: Chance = 24;

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

/// How many squares a turret flips a generation.
///
/// The knob that decides whether a turret is a frontier tool or a weapon, and
/// the arithmetic to set it by:
///
/// On **empty ground** each claim a generation holds about thirty squares
/// against [`DECAY`], which eats N/32 of what is held — so a turret settles at
/// roughly `30 × TURRET_POWER` squares, and the 2x2 block a turret is really
/// bought as settles at four times that.
///
/// Against a **living** neighbour it is far weaker, and this is the number
/// that matters. Their life takes a claimed square straight back through
/// [`SPREAD`] at forty in sixty-four, so what a turret holds of contested
/// ground is about `TURRET_POWER × 64 / SPREAD` — one and a half squares at
/// one, six at four. Below about four a turret cannot press on a neighbour at
/// all, which is why one reads as a way of reaching past your own frontier
/// rather than a way of fighting over somebody's colony.
///
/// It is also the cost. A turret reads the `(2R+1)²` box around itself twice
/// per square it flips — once to find the nearest and once to walk to the one
/// the tie-break picked — so this multiplies [`TURRET_REACH`]'s bill directly:
/// 338 reads a turret a generation at one, 1352 at four.
///
/// A dead turret gives back the same number it would have taken, so the mirror
/// holds however this is set.
pub const TURRET_POWER: usize = 1;

/// What influence a turret plants on a square it takes.
///
/// Planted rather than added, because [`super::rule`]'s territory rule
/// **assigns** a square the strongest claim reaching it rather than
/// accumulating — so a turret that nudged a level up by a little would have it
/// wiped the next time that square worked itself out, and would achieve
/// nothing whatever the number was.
///
/// What planting buys is a brake the old boolean turret needed a separate
/// constant for. A flag planted where nothing of its owner's is near enough to
/// feed it falls back on its own, at the rate the rule re-evaluates, so what a
/// turret holds is however much it can plant against however fast the ground
/// argues back — and a turret planting **deep** in somebody else's country
/// loses it again almost at once, where one planting just past its own edge
/// keeps it.
///
/// Full, because anything less is planted inside its own falloff: a flag at
/// three beside ground of its owner's already reaching four is not a push at
/// all.
pub const TURRET_PUSH: u8 = super::cell::bits::MAX_LEVEL;

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
/// What placing where your influence is thin multiplies the cost by, per level
/// short of full.
///
/// Ten times flat was too weak — playtesting said so — and it was flat because
/// territory was a flag and there was nothing to grade it by. There is now: at
/// full influence a placement costs its own price, and every level thinner
/// adds one multiple, so the edge of your reach costs seven times. Refused
/// where nothing of yours reaches at all.
///
/// The wall is back, in other words, but with a slope in front of it — and it
/// is safe this time in a way it was not before, because granted ground is a
/// source, so a player whose life has gone out still has a patch with a live
/// gradient to build on.
pub const THIN_MULTIPLIER: i32 = 1;


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
    if level == 0 {
        return Then::Next(cell.with_player(PlayerId::UNOWNED).with_level(0));
    }
    Then::Next(cell.with_player(player).with_level(level))
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
