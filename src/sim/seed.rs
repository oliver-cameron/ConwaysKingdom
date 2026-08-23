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

    /// One chance in `odds`. `odds` of 1 is always, and 0 is never.
    #[inline]
    pub const fn one_in(self, stream: u64, odds: u64) -> bool {
        odds != 0 && self.stream(stream).is_multiple_of(odds)
    }

    /// `n` chances in `outof`.
    #[inline]
    pub const fn chance(self, stream: u64, (n, outof): (u64, u64)) -> bool {
        outof != 0 && self.stream(stream) % outof < n
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
            let (a, b, c) = (roll.one_in(0, 4), roll.chance(1, (10, 16)), roll.pick(2, 5));
            for _ in 0..8 {
                // Taken in a different order, which must not matter.
                assert_eq!(Roll::new(seed).pick(2, 5), c);
                assert_eq!(Roll::new(seed).chance(1, (10, 16)), b);
                assert_eq!(Roll::new(seed).one_in(0, 4), a);
            }
        }
    }

    /// Streams are independent. A cell that decays must not be the same cell
    /// that would have been claimed, or two rules that were meant to be
    /// unrelated move together and nothing says so.
    #[test]
    fn streams_do_not_agree_with_each_other() {
        let both = (0..2000u64)
            .filter(|&s| Roll::new(s).one_in(0, 4) == Roll::new(s).one_in(1, 4))
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

        let quarter = hits(&|s| Roll::new(s).one_in(0, 4));
        assert!((4600..5400).contains(&quarter), "one in four gave {quarter} in 20000");

        let ten_sixteenths = hits(&|s| Roll::new(s).chance(0, (10, 16)));
        assert!(
            (12000..13000).contains(&ten_sixteenths),
            "ten in sixteen gave {ten_sixteenths} in 20000"
        );

        assert!((0..20_000).all(|s| !Roll::new(s).one_in(0, 0)), "one in nothing is never");
        assert!((0..20_000).all(|s| Roll::new(s).one_in(0, 1)), "one in one is always");
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
