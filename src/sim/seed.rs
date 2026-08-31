//! Rolling dice that every peer rolls the same way.
//!
//! Client-side prediction needs the step to be a pure function of state and
//! tick, so nothing here may use `rand`: a client that rolled its own dice
//! would diverge from the server on the first contested birth. Every number
//! comes from a seed derived from the generation and the cell's position, so
//! two peers get the same answer without exchanging anything.
//!
//! Split out of the rule because it is machinery. What a cell does from one
//! generation to the next is worth reading on its own, and bit-twiddling a
//! hash is not part of it.
//!
//! ## Streams
//!
//! One cell wants several independent answers in the same generation — is this
//! ground claimed, does it decay, which parent does a birth take — and they
//! must not correlate, or a cell that decays is also always a cell that would
//! have been claimed. Each question asks on its own **stream**, which mixes a
//! small number into the seed to get an unrelated one.
//!
//! Streams rather than slices of the seed's bits, which is what this replaced.
//! Slices work, and they work only for as long as nobody picks two that
//! overlap — a silent correlation that no test would think to look for.

/// What every chance in [`crate::sim::rule`] is out of.
///
/// One number, so a constant is a chance and nothing has to say which way
/// round it reads. Sixty-four because it is a power of two — the modulo is a
/// mask — and fine enough to say a sixty-fourth without saying more than
/// anybody could tune.
pub const OUT_OF: u64 = 64;

/// Mix a value into a seed. SplitMix64's finaliser: cheap, and it decorrelates
/// the near-identical inputs — adjacent cells, consecutive ticks — that a plain
/// hash would leave visibly patterned.
#[inline]
pub const fn mix(seed: u64, value: u64) -> u64 {
    let mut z = seed ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The half of a cell's seed that is the same for every cell this generation.
///
/// **Split from [`cell_seed`] for the GPU.** A compute shader dispatches one
/// thread per cell, and the only thing a thread knows cheaply is its own
/// coordinates — so a seed that had to be derived through a chunk, or through
/// anything a thread would have to look up, is a seed a thread cannot compute.
/// Splitting it here makes that split explicit rather than accidental: this
/// half is one value per dispatch, passed in a uniform, and [`cell_seed`] is
/// the **one** mix a thread does for itself.
///
/// `world` is the game's own id, so two rooms holding the same cells do not
/// roll the same dice — see [`crate::sim::World::seed`]. It is not a secret
/// and it is not meant to be: every peer in a room has to know it or they
/// disagree about the first contested birth.
#[inline]
pub const fn generation_seed(world: u64, generation: u64) -> u64 {
    mix(world, generation)
}

/// One cell's seed, from where it is and nothing else.
///
/// **Absolute cell coordinates**, not a chunk and an offset inside it. The
/// chunk was never part of the question — it is how the CPU stores the world,
/// and a cell's dice should not change because the storage did. Keying on the
/// cell's own position means the answer survives a change to `CHUNK_N`, is the
/// same on a torus and a plane for the same square, and is computable by
/// anything that knows where it is.
///
/// Packed into one `u64` rather than mixed twice, so this is a single [`mix`]:
/// two multiplies and three shifts, which is what a fragment or a compute
/// thread can afford per cell.
#[inline]
pub const fn cell_seed(generation: u64, row: i32, col: i32) -> u64 {
    mix(generation, ((row as u32 as u64) << 32) | (col as u32 as u64))
}

/// One cell's dice for one generation.
///
/// Copy, and every method takes `self` by value, because there is no state to
/// advance: the same stream always gives the same answer. That is the point —
/// a rule may take its rolls in any order, or skip some, and still agree with
/// a peer that took them differently.
#[derive(Clone, Copy, Debug)]
pub struct Roll(u64);

impl Roll {
    #[inline]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// An independent number for this stream.
    #[inline]
    const fn stream(self, stream: u64) -> u64 {
        mix(self.0, stream)
    }

    /// Whether a chance of `n` out of [`OUT_OF`] came up.
    #[inline]
    pub const fn chance(self, stream: u64, n: u64) -> bool {
        self.stream(stream) % OUT_OF < n
    }

    /// One of `count`, evenly. Zero for an empty choice, so a caller that has
    /// nothing to pick from gets an index it can still use.
    #[inline]
    pub const fn pick(self, stream: u64, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        (self.stream(stream) % count as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole contract: the same seed gives the same answer, every time and
    /// in any order. Two peers rolling for the same cell must agree.
    #[test]
    fn a_roll_is_a_pure_function_of_its_seed_and_stream() {
        for seed in 0..64 {
            let roll = Roll::new(seed);
            let (a, b, c) = (roll.chance(0, 16), roll.chance(1, 40), roll.pick(2, 5));
            for _ in 0..8 {
                // Taken in a different order, which must not matter.
                assert_eq!(Roll::new(seed).pick(2, 5), c);
                assert_eq!(Roll::new(seed).chance(1, 40), b);
                assert_eq!(Roll::new(seed).chance(0, 16), a);
            }
        }
    }

    /// Streams are independent. A cell that decays must not be the same cell
    /// that would have been claimed, or two rules that were meant to be
    /// unrelated move together and nothing says so.
    #[test]
    fn streams_do_not_agree_with_each_other() {
        let both = (0..2000u64)
            .filter(|&s| Roll::new(s).chance(0, 16) == Roll::new(s).chance(1, 16))
            .count();
        // Independent streams agree by luck about 5/8 of the time: both true
        // 1/16, both false 9/16. Anything near 2000 is one stream twice.
        assert!(
            (1100..1400).contains(&both),
            "streams look correlated: agreed {both} times in 2000"
        );
    }

    /// The odds are the odds. A rule's constants only mean anything if the
    /// dice honour them.
    #[test]
    fn the_odds_are_what_they_say() {
        let hits = |f: &dyn Fn(u64) -> bool| (0..20_000u64).filter(|&s| f(s)).count();

        let quarter = hits(&|s| Roll::new(s).chance(0, 16));
        assert!((4600..5400).contains(&quarter), "16 in 64 gave {quarter} in 20000");

        let most = hits(&|s| Roll::new(s).chance(0, 40));
        assert!((12000..13000).contains(&most), "40 in 64 gave {most} in 20000");

        assert!((0..20_000).all(|s| !Roll::new(s).chance(0, 0)), "0 in 64 is never");
        assert!((0..20_000).all(|s| Roll::new(s).chance(0, OUT_OF)), "64 in 64 is always");
    }

    /// Every choice must be reachable, or "at random" is a lie and one parent
    /// can never own a birth.
    #[test]
    fn every_choice_can_come_up() {
        for count in 1..=8usize {
            let seen: std::collections::HashSet<usize> =
                (0..400u64).map(|s| Roll::new(s).pick(0, count)).collect();
            assert_eq!(seen.len(), count, "picking one of {count} only ever gave {seen:?}");
        }
        assert_eq!(Roll::new(7).pick(0, 0), 0, "nothing to pick from is still an index");
    }
}
