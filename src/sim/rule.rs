//! The rule, evaluated one cell at a time.
//!
//! [`next_cell`] takes a cell and its eight neighbours — whole cells, not just
//! whether they are alive — so a rule can branch on what a cell *is*, not only
//! on how many live things surround it. That is the seam for behaviour that
//! varies by cell type: everything a rule needs is in its arguments, and it
//! knows nothing about chunks, worlds or topology.
//!
//! The default is plain Conway, and it touches **only the alive bit**. A cell
//! that dies keeps its owner and metadata, so "recently died, and whose it was"
//! survives without a field to store it. That is inert as far as the rules go:
//! nothing counts a dead cell, and births take their owner from live
//! neighbours only.
//!
//! One invariant holds throughout: **a live cell always has a non-zero player**.
//! Player zero means unowned, and unowned life would have nobody to attribute a
//! birth to.
//!
//! A birth picks its owner at random from the three parents — but *seeded*
//! random, not `rand`. Client-side prediction needs the step to be a pure
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

/// Conway's rules.
///
/// Survival and death change the alive bit and nothing else. A birth also sets
/// the owner, because it has none to keep and a live cell must have one.
#[inline]
pub fn next_cell(cell: Cell, neighbours: &Neighbours, seed: u64) -> Cell {
    debug_assert!(
        !cell.is_alive() || cell.player().is_owned(),
        "a live cell must have a non-zero player"
    );

    let live = neighbours.iter().filter(|n| n.is_alive()).count();

    match (cell.is_alive(), live) {
        // Survives. Only the alive bit is in play, and it is already set.
        (true, 2) | (true, 3) => cell,
        // Born. Keeps whatever metadata was left behind, gains an owner.
        (false, 3) => cell.with_alive(true).with_player(parent(neighbours, seed)),
        // Dies, or stays dead. Owner and metadata are left as they were.
        _ => cell.with_alive(false),
    }
}

/// Pick one of the three parents to own the birth.
///
/// Scanning in a fixed order and indexing by `seed % 3` means the choice
/// depends only on the seed, never on iteration order — so it is the same on
/// every peer, and reproducible when replaying a tick.
fn parent(neighbours: &Neighbours, seed: u64) -> PlayerId {
    let mut parents = [PlayerId::UNOWNED; 3];
    let mut found = 0usize;
    for n in neighbours {
        if n.is_alive() && found < parents.len() {
            parents[found] = n.player();
            found += 1;
        }
    }
    debug_assert_eq!(found, 3, "a birth has exactly three parents");
    let chosen = parents[(seed % found.max(1) as u64) as usize];
    debug_assert!(chosen.is_owned(), "every parent is a live cell, so owned");
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neighbours(live: &[usize], player: u8) -> Neighbours {
        let mut n = [Cell::DEAD; 8];
        for &i in live {
            n[i] = Cell::alive(PlayerId(player));
        }
        n
    }

    #[test]
    fn survival_changes_only_the_alive_bit() {
        let me = Cell::alive(PlayerId(3)).with_meta(0b101_0101_010);
        for live in [2, 3] {
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7), 0);
            assert_eq!(next, me, "{live} neighbours: nothing may change");
        }
    }

    #[test]
    fn death_keeps_owner_and_metadata() {
        let me = Cell::alive(PlayerId(3)).with_meta(0b101_0101_010);
        for live in [0, 1, 4, 5, 6, 7, 8] {
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7), 0);
            assert!(!next.is_alive(), "{live} neighbours: should die");
            assert_eq!(next.player(), me.player(), "owner is kept");
            assert_eq!(next.meta(), me.meta(), "metadata is kept");
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

    #[test]
    fn a_birth_keeps_whatever_metadata_the_dead_cell_carried() {
        let corpse = Cell::DEAD.with_meta(0b111_0000_111).with_player(PlayerId(6));
        let mut n = [Cell::DEAD; 8];
        for i in 0..3 {
            n[i] = Cell::alive(PlayerId(2));
        }
        let born = next_cell(corpse, &n, 1);
        assert!(born.is_alive());
        assert_eq!(born.meta(), corpse.meta(), "metadata survives");
        assert_eq!(born.player(), PlayerId(2), "but a parent takes ownership");
    }
}
