//! Tests for the rule.
//!
//! In their own file so [`super`] is only the constants and what a cell does
//! from one generation to the next — which is a thing worth being able to read
//! in one screen, and cannot be read at all with two hundred lines of assertion
//! underneath it.

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

/// Creep cuts both ways, which is the whole of why a border settles rather than
/// running away or rotting: a dead cell takes a neighbour's owner, and one of
/// those neighbours may be nobody.
#[test]
fn creep_takes_whoever_is_beside_it_including_nobody() {
    let mine = Cell::DEAD.with_player(PlayerId(1));
    let nobody = Cell::DEAD;
    let outcomes = |cell: Cell, around: &Neighbours| {
        (0..2000u64).filter(|&s| next_cell(cell, around, s).player() == PlayerId(1)).count()
    };

    // Deep inside my ground there is nothing else to take, so unclaimed ground
    // there only ever becomes mine -- at the creep rate, not at once.
    let inside = [mine; 8];
    let claimed = outcomes(nobody, &inside);
    assert!((180..340).contains(&claimed), "claimed {claimed} of 2000, want about 250");
    for seed in 0..2000 {
        let next = next_cell(nobody, &inside, seed).player();
        assert!(next == PlayerId(1) || next == PlayerId::UNOWNED, "seed {seed}: {next:?}");
    }

    // Out where nothing is claimed, my ground only ever becomes nobody's --
    // and that is the erosion, with no rule of its own.
    let outside = [nobody; 8];
    let kept = outcomes(mine, &outside);
    assert!(kept < 2000, "ground surrounded by nothing should sometimes go, kept {kept}");
    assert!(kept > 1500, "and should not go all at once, kept {kept}");

    // On a border it goes both ways, which is what makes the edge a walk.
    let mut edge = [nobody; 8];
    for n in edge.iter_mut().take(5) {
        *n = mine;
    }
    let held = outcomes(mine, &edge);
    assert!((1500..2000).contains(&held), "a border should move both ways, held {held} of 2000");
}

/// Granted ground answers to life alone: it neither creeps nor fades, or a
/// player whose life went out would lose the only ground they may build on.
#[test]
fn granted_ground_is_not_taken_by_the_ground_around_it() {
    let home = Cell::DEAD.with_player(PlayerId(1)).with_home(true);
    let nothing = [Cell::DEAD; 8];
    for seed in 0..2000 {
        let next = next_cell(home, &nothing, seed);
        assert_eq!(next.player(), PlayerId(1), "seed {seed}");
        assert!(next.is_home());
    }
}
