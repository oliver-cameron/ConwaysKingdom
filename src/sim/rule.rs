//! The rule, evaluated one cell at a time.
//!
//! [`next_cell`] takes a cell and its eight neighbours — whole cells, not just
//! whether they are alive — so a rule can branch on what a cell *is*, not only
//! on how many live things surround it. That is the seam for behaviour that
//! varies by cell type: everything a rule needs is in its arguments, and it
//! knows nothing about chunks, worlds or topology.
//!
//! Survival and death touch **only the alive bit**. A cell that dies keeps its
//! owner and its kind, so "recently died, and whose it was" survives without a
//! field to store it. That is inert as far as the rules go: nothing counts a
//! dead cell, and a birth takes everything it is from a living parent rather
//! than from the corpse it lands on.
//!
//! One invariant holds throughout: **a live cell always has a non-zero player**.
//! Player zero means unowned, and unowned life would have nobody to attribute a
//! birth to.
//!
//! A birth is a **copy of one of its three parents**, kind included, so what
//! a cell is passes down a line rather than sitting on the ground. That is
//! what makes [`super::Kind::MINE`] work: a mine's children are mines, and
//! because the parent is chosen at random the kind spreads through a mixed
//! population rather than being handed down whole.
//!
//! A birth picks that parent at random — but *seeded* random, not `rand`. Client-side prediction needs the step to be a pure
//! function of state and tick, and a client that rolled its own dice would
//! diverge from the server on the first contested birth. The seed is derived
//! from the tick and the cell's absolute position, so every peer rolls the same
//! number without exchanging one.

use super::cell::Cell;
use super::player::PlayerId;

/// Neighbours in [`super::Dir::ALL`] order: N, NE, E, SE, S, SW, W, NW.
pub type Neighbours = [Cell; 8];

/// A rule. Swap in a different one by changing what the world calls; the
/// signature is a plain function pointer, so there is no dispatch cost.
pub type RuleFn = fn(Cell, &Neighbours, u64) -> Cell;

/// One chance in this many, per generation, that a dead cell with nothing
/// alive beside it loses its owner.
///
/// Sixteen is about four seconds at the default rate: long enough that a
/// pattern flickering off and on keeps its ground, short enough that a glider's
/// trail fades behind it rather than staking a claim across the world. The one
/// number to move if territory feels too sticky or too slippery.
///
/// Granted ground is exempt — see [`super::bits::HOME`]. Without that floor a
/// player whose life died out would lose every square they had, and placing is
/// confined to your own territory, so they could never place again.
pub const DECAY_ODDS: u64 = 16;

/// Mix a value into a seed. SplitMix64's finaliser: cheap, and it decorrelates
/// the near-identical inputs (adjacent cells, consecutive ticks) that a plain
/// hash would leave visibly patterned.
#[inline]
pub const fn mix(seed: u64, value: u64) -> u64 {
    let mut z = seed ^ value.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Cell {
    /// Advance this cell one generation.
    ///
    /// The outer match is where behaviour that varies by cell type belongs: add
    /// an arm that inspects [`Cell::meta`] and the default falls through to
    /// Conway, so a new type costs one arm and disturbs nothing else.
    #[inline]
    pub fn update(mut self, neighbours: &Neighbours, seed: u64) -> Cell {
        // Under ice is time-stopped, whatever the cell is and whether or not
        // it is alive. Checked before the kind, so a pane freezes anything
        // without every kind having to remember to honour it.
        if self.is_ice() {
            return self;
        }
        // Territory: claimed by life growing over it, and lost when life goes
        // away. Either way the cell stays dead -- this sets the owner and
        // nothing else. Ice is handled above, so a pane's cover is neither
        // claimed nor lost while it stands.
        if !self.is_alive() {
            // A fixed array and a count rather than a `Vec`, because this runs
            // for every dead cell of every active chunk of every generation
            // and it was the one allocation in the hot loop.
            let mut claimants = [PlayerId::UNOWNED; 8];
            let mut found = 0usize;
            for n in neighbours {
                if n.is_alive() {
                    claimants[found] = n.player();
                    found += 1;
                }
            }

            if found > 0 {
                // A random living neighbour, so a cell between two players
                // goes to one of them rather than always to the first in
                // `Dir::ALL` order.
                if (seed >> 3) & 15 <= 9 {
                    self = self.with_player(claimants[(seed % found as u64) as usize]);
                }
            } else if self.player().is_owned() && !self.is_home() {
                // **Decay.** Nothing alive is touching this square, so whoever
                // holds it is holding it on memory alone. Territory used to
                // only ever spread, which meant a glider left a permanent
                // trail and an infinite world grew for as long as anything
                // moved: ground was won and never lost, and a map that only
                // fills up is not one anybody competes over.
                //
                // Slow, and seeded like everything else, so a patch fades over
                // a few seconds rather than blinking out. Its own slice of the
                // seed, so it is independent of the claim above.
                if (seed >> 9).is_multiple_of(DECAY_ODDS) {
                    self = self.with_player(PlayerId::UNOWNED);
                }
            }
        }

        match self.kind() {
            // No kind-specific rules yet; everything follows Conway.
            _ => self.conway(neighbours, seed),
        }
    }

    /// Conway's rules.
    ///
    /// Survival and death change the alive bit and nothing else. A birth also
    /// sets the owner, because it has none to keep and a live cell must have
    /// one.
    #[inline]
    pub fn conway(self, neighbours: &Neighbours, seed: u64) -> Cell {
        debug_assert!(
            !self.is_alive() || self.player().is_owned(),
            "a live cell must have a non-zero player"
        );

        let live = neighbours.iter().filter(|n| n.is_alive()).count();

        match (self.is_alive(), live) {
            // Survives. Only the alive bit is in play, and it is already set.
            (true, 2 | 3) => self,
            // Born, as a copy of one of its parents: owner, kind and all.
            //
            // The dead cell's own metadata is discarded, which is the whole of
            // how a kind spreads -- a mine's children are mines. Ice is
            // cleared because a parent may be under a pane and count as a live
            // neighbour while frozen, and a birth outside the pane must not
            // inherit the pane.
            (false, 3) => parent(neighbours, seed)
                .with_ice(false)
                // `HOME` marks the square, so it stays with the square. Every
                // other thing about a newborn comes from its parent, and this
                // is the one that must not.
                .with_home(self.is_home()),
            // Dies, or stays dead. Owner and metadata are left as they were.
            _ => self.with_alive(false),
        }
    }
}

/// Free-function form, so [`RuleFn`] can point at the default rule.
#[inline]
pub fn next_cell(cell: Cell, neighbours: &Neighbours, seed: u64) -> Cell {
    cell.update(neighbours, seed)
}

/// Pick one of the three parents to own the birth.
///
/// Scanning in a fixed order and indexing by `seed % 3` means the choice
/// depends only on the seed, never on iteration order — so it is the same on
/// every peer, and reproducible when replaying a tick.
fn parent(neighbours: &Neighbours, seed: u64) -> Cell {
    let mut parents = [&Cell([0, 0]); 3];
    let mut found = 0usize;
    for n in neighbours {
        if n.is_alive() && found < parents.len() {
            parents[found] = n;
            found += 1;
        }
    }
    debug_assert_eq!(found, 3, "a birth has exactly three parents");
    let chosen = parents[(seed % found.max(1) as u64) as usize];
    debug_assert!(
        chosen.player().is_owned(),
        "every parent is a live cell, so owned"
    );
    *chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Kind, PlayerId};

    fn neighbours(live: &[usize], player: u8) -> Neighbours {
        let mut n = [Cell::DEAD; 8];
        for &i in live {
            n[i] = Cell::alive(PlayerId(player));
        }
        n
    }

    #[test]
    fn survival_changes_only_the_alive_bit() {
        let me = Cell::alive(PlayerId(3)).with_kind(Kind(37));
        for live in [2, 3] {
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7), 0);
            assert_eq!(next, me, "{live} neighbours: nothing may change");
        }
    }

    #[test]
    fn death_keeps_owner_and_metadata() {
        let me = Cell::alive(PlayerId(3)).with_kind(Kind(37));
        for live in [0, 1, 4, 5, 6, 7, 8] {
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7), 0);
            assert!(!next.is_alive(), "{live} neighbours: should die");
            assert_eq!(next.player(), me.player(), "owner is kept");
            assert_eq!(next.kind(), me.kind(), "kind is kept");
        }
    }

    #[test]
    fn a_birth_takes_one_of_its_three_parents() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(4));
        n[3] = Cell::alive(PlayerId(7));
        n[6] = Cell::alive(PlayerId(9));
        for seed in 0..64 {
            let born = next_cell(Cell::DEAD, &n, seed);
            assert!(born.is_alive());
            assert!(
                [PlayerId(4), PlayerId(7), PlayerId(9)].contains(&born.player()),
                "seed {seed} gave {:?}, not a parent",
                born.player()
            );
        }
    }

    /// All three parents must be reachable, or "random" is a lie.
    #[test]
    fn every_parent_can_win() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(4));
        n[3] = Cell::alive(PlayerId(7));
        n[6] = Cell::alive(PlayerId(9));
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64 {
            seen.insert(next_cell(Cell::DEAD, &n, seed).player());
        }
        assert_eq!(seen.len(), 3, "only saw {seen:?}");
    }

    /// The same seed must always give the same answer, or clients desync.
    #[test]
    fn the_choice_is_reproducible() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(4));
        n[3] = Cell::alive(PlayerId(7));
        n[6] = Cell::alive(PlayerId(9));
        for seed in 0..64 {
            let first = next_cell(Cell::DEAD, &n, seed).player();
            for _ in 0..8 {
                assert_eq!(next_cell(Cell::DEAD, &n, seed).player(), first);
            }
        }
    }

    /// The invariant the whole ownership model rests on.
    #[test]
    fn every_live_cell_has_an_owner() {
        for pattern in 0u32..256 {
            let live: Vec<usize> = (0..8).filter(|i| pattern & (1 << i) != 0).collect();
            for player in 1..=PlayerId::MAX {
                for me in [Cell::DEAD, Cell::alive(PlayerId(1))] {
                    let next = next_cell(me, &neighbours(&live, player), pattern as u64);
                    if next.is_alive() {
                        assert!(
                            next.player().is_owned(),
                            "live cell with player 0 from {live:?}"
                        );
                    }
                }
            }
        }
    }

    /// Ice freezes what it covers, whatever surrounds it.
    #[test]
    fn a_cell_under_ice_never_changes() {
        for live in 0..=8 {
            let n = neighbours(&(0..live).collect::<Vec<_>>(), 7);
            for me in [
                // Alive under ice: would otherwise die or survive.
                Cell::alive(PlayerId(3)).with_ice(true),
                // Dead under ice: would otherwise be born at three.
                Cell::DEAD.with_ice(true),
                // Carrying a kind as well, to show the flag wins over it.
                Cell::alive(PlayerId(2)).with_kind(Kind(9)).with_ice(true),
            ] {
                assert_eq!(next_cell(me, &n, 0), me, "{live} live neighbours");
            }
        }
    }

    /// Ice and alive are independent: all four combinations are meaningful.
    #[test]
    fn ice_and_alive_are_independent() {
        for alive in [false, true] {
            for ice in [false, true] {
                let c = Cell::DEAD
                    .with_alive(alive)
                    .with_player(if alive {
                        PlayerId(1)
                    } else {
                        PlayerId::UNOWNED
                    })
                    .with_ice(ice);
                assert_eq!(c.is_alive(), alive);
                assert_eq!(c.is_ice(), ice);
            }
        }
        // A dead cell under ice stays dead even with three live neighbours,
        // where without the ice it would be born.
        let n = neighbours(&[0, 1, 2], 4);
        assert!(next_cell(Cell::DEAD, &n, 0).is_alive());
        assert!(!next_cell(Cell::DEAD.with_ice(true), &n, 0).is_alive());
    }

    /// A birth takes everything it is from a parent, and nothing from the
    /// ground it lands on. That is how a kind travels down a line, which is
    /// what makes a mine an investment rather than a square.
    #[test]
    fn a_birth_is_a_copy_of_a_parent_not_of_the_corpse() {
        let corpse = Cell::DEAD.with_kind(Kind(37)).with_player(PlayerId(6));
        let mut n = [Cell::DEAD; 8];
        for i in 0..3 {
            n[i] = Cell::alive(PlayerId(2)).with_kind(Kind::MINE);
        }
        let born = next_cell(corpse, &n, 1);
        assert!(born.is_alive());
        assert_eq!(born.player(), PlayerId(2), "a parent's number");
        assert_eq!(born.kind(), Kind::MINE, "and a parent's kind");
        assert!(!born.is_ice(), "never a parent's pane");
    }

    /// The kind is inherited from *the parent that was chosen*, so in a mixed
    /// neighbourhood it spreads rather than being handed down whole. One mine
    /// dropped into a growing pattern takes a share of the births, not all of
    /// them and not none.
    #[test]
    fn a_kind_spreads_through_a_mixed_neighbourhood() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(1)).with_kind(Kind::MINE);
        n[3] = Cell::alive(PlayerId(1));
        n[6] = Cell::alive(PlayerId(1));

        let mines = (0..300)
            .filter(|&seed| next_cell(Cell::DEAD, &n, seed).kind() == Kind::MINE)
            .count();
        assert!(
            (60..140).contains(&mines),
            "one parent in three should carry it, got {mines} in 300"
        );
    }
}
