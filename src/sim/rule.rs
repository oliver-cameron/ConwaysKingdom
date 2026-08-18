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

use super::cell::Cell;
use super::player::PlayerId;

/// Neighbours in [`super::Dir::ALL`] order: N, NE, E, SE, S, SW, W, NW.
pub type Neighbours = [Cell; 8];

/// A rule. Swap in a different one by changing what the world calls; the
/// signature is a plain function pointer, so there is no dispatch cost.
pub type RuleFn = fn(Cell, &Neighbours) -> Cell;

/// Conway's rules.
///
/// Survival and death change the alive bit and nothing else. A birth also sets
/// the owner, because it has none to keep and a live cell must have one.
#[inline]
pub fn next_cell(cell: Cell, neighbours: &Neighbours) -> Cell {
    debug_assert!(
        !cell.is_alive() || cell.player().is_owned(),
        "a live cell must have a non-zero player"
    );

    let live = neighbours.iter().filter(|n| n.is_alive()).count();

    match (cell.is_alive(), live) {
        // Survives. Only the alive bit is in play, and it is already set.
        (true, 2) | (true, 3) => cell,
        // Born. Keeps whatever metadata was left behind, gains an owner.
        (false, 3) => cell.with_alive(true).with_player(dominant_player(neighbours)),
        // Dies, or stays dead. Owner and metadata are left as they were.
        _ => cell.with_alive(false),
    }
}

/// The player holding most of the live neighbours; ties go to the lowest
/// number, so the outcome does not depend on iteration order.
///
/// Only live neighbours are counted, and every live cell is owned, so the
/// result is non-zero whenever there is any life to attribute — which is
/// guaranteed here, since this is only reached with exactly three live
/// neighbours.
fn dominant_player(neighbours: &Neighbours) -> PlayerId {
    let mut tally = [0u8; (PlayerId::MAX as usize) + 1];
    for n in neighbours {
        if n.is_alive() {
            tally[n.player().0 as usize] += 1;
        }
    }
    // Skip index 0: unowned cannot own a birth.
    let (best, count) = tally
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|&(player, &count)| (count, std::cmp::Reverse(player)))
        .map(|(player, &count)| (player as u8, count))
        .unwrap_or((0, 0));

    debug_assert!(count > 0, "a birth needs at least one owned live neighbour");
    PlayerId(best)
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
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7));
            assert_eq!(next, me, "{live} neighbours: nothing may change");
        }
    }

    #[test]
    fn death_keeps_owner_and_metadata() {
        let me = Cell::alive(PlayerId(3)).with_meta(0b101_0101_010);
        for live in [0, 1, 4, 5, 6, 7, 8] {
            let next = next_cell(me, &neighbours(&(0..live).collect::<Vec<_>>(), 7));
            assert!(!next.is_alive(), "{live} neighbours: should die");
            assert_eq!(next.player(), me.player(), "owner is kept");
            assert_eq!(next.meta(), me.meta(), "metadata is kept");
        }
    }

    #[test]
    fn a_birth_takes_the_majority_owner() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(4));
        n[1] = Cell::alive(PlayerId(4));
        n[2] = Cell::alive(PlayerId(9));
        let born = next_cell(Cell::DEAD, &n);
        assert!(born.is_alive());
        assert_eq!(born.player(), PlayerId(4));
    }

    #[test]
    fn a_tie_goes_to_the_lower_number_whatever_the_order() {
        let mut n = [Cell::DEAD; 8];
        n[0] = Cell::alive(PlayerId(9));
        n[1] = Cell::alive(PlayerId(2));
        n[2] = Cell::alive(PlayerId(9));
        let a = next_cell(Cell::DEAD, &n);
        n.reverse();
        let b = next_cell(Cell::DEAD, &n);
        assert_eq!(a.player(), b.player(), "order must not matter");
    }

    /// The invariant the whole ownership model rests on.
    #[test]
    fn every_live_cell_has_an_owner() {
        for pattern in 0u32..256 {
            let live: Vec<usize> = (0..8).filter(|i| pattern & (1 << i) != 0).collect();
            for player in 1..=PlayerId::MAX {
                for me in [Cell::DEAD, Cell::alive(PlayerId(1))] {
                    let next = next_cell(me, &neighbours(&live, player));
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
        let born = next_cell(corpse, &n);
        assert!(born.is_alive());
        assert_eq!(born.meta(), corpse.meta(), "metadata survives");
        assert_eq!(born.player(), PlayerId(2), "but the new owner takes it");
    }
}
