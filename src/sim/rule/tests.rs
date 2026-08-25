//! Tests for the rule.
//!
//! In their own file so [`super`] is only the constants and what a cell does
//! from one generation to the next — which is a thing worth being able to read
//! in one screen, and cannot be read at all with two hundred lines of assertion
//! underneath it.

use super::*;
use crate::sim::{bits, Kind, PlayerId};
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

/// A kind that does not inherit passes over ownership alone. Without it a gun
/// would be a turret factory: a birth copies its parent whole, so a turret's
/// children would be turrets and whoever built one first would claim the map.
#[test]
fn a_turret_is_not_inherited() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(4)).with_kind(Kind::TURRET);
    }
    for seed in 0..64 {
        let born = next_cell(Cell::DEAD, &n, seed);
        assert!(born.is_alive(), "seed {seed}");
        assert_eq!(born.player(), PlayerId(4), "the ground still changes hands");
        assert_eq!(born.kind(), Kind::NORMAL, "seed {seed} bred a turret");
    }
}

/// And a mine still is, because that is the whole of what a mine is: what was
/// bought is a lineage, and it travels by being copied.
#[test]
fn a_mine_is_still_inherited() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(4)).with_kind(Kind::MINE);
    }
    for seed in 0..64 {
        assert_eq!(next_cell(Cell::DEAD, &n, seed).kind(), Kind::MINE, "seed {seed}");
    }
}

/// Which parent is chosen must not depend on what kind it turned out to be:
/// the carve-out is after the roll, so every peer reaches the same parent and
/// only then asks whether its kind travels.
#[test]
fn not_inheriting_does_not_move_the_roll() {
    let owners = [PlayerId(4), PlayerId(7), PlayerId(9)];
    for seed in 0..64 {
        let mut plain = [Cell::DEAD; 8];
        let mut turrets = [Cell::DEAD; 8];
        for (i, &p) in [0usize, 3, 6].iter().zip(&owners) {
            plain[*i] = Cell::alive(p);
            turrets[*i] = Cell::alive(p).with_kind(Kind::TURRET);
        }
        assert_eq!(
            next_cell(Cell::DEAD, &plain, seed).player(),
            next_cell(Cell::DEAD, &turrets, seed).player(),
            "seed {seed} picked a different parent once the kind changed"
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

// --- territory, which is a level now rather than a flag ---------------------

/// A living cell is a **source**: it reads as full whatever is stored on its
/// square, and every step away costs [`LEVEL_FALL`].
#[test]
fn influence_falls_off_with_distance_from_a_source() {
    let me = PlayerId(1);
    let source = Cell::alive(me);
    assert_eq!(source.influence(), bits::MAX_LEVEL, "a living cell is a source");

    // Beside it: full, less one step.
    let mut beside = [Cell::DEAD; 8];
    beside[0] = source;
    let first = settled(Cell::DEAD, &beside);
    assert_eq!(first.player(), me);
    assert_eq!(first.level(), bits::MAX_LEVEL - LEVEL_FALL);

    // And a step further out, from that square rather than from the cell.
    let mut further = [Cell::DEAD; 8];
    further[0] = first;
    let second = settled(Cell::DEAD, &further);
    assert_eq!(second.level(), first.level() - LEVEL_FALL);

    // Until it runs out, and then the square belongs to nobody -- which is
    // what bounds a halo, with no rule about radius anywhere. Started from a
    // square that *is* held, so the letting go is something that happens
    // rather than something that was already true.
    let mut edge = [Cell::DEAD; 8];
    edge[0] = Cell::DEAD.with_player(me).with_level(LEVEL_FALL - 1);
    let held = Cell::DEAD.with_player(me).with_level(4);
    let past = settled(held, &edge);
    assert_eq!(past.player(), PlayerId::UNOWNED, "a claim that reaches nothing holds nothing");
    assert_eq!(past.level(), 0);
}

/// **The strongest claim wins**, which is what makes a front between two
/// players settle at the line equidistant between them without anything
/// working out where that is.
#[test]
fn the_strongest_claim_takes_the_square() {
    let (me, them) = (PlayerId(1), PlayerId(2));
    let mut n = [Cell::DEAD; 8];
    n[0] = Cell::DEAD.with_player(me).with_level(6);
    n[4] = Cell::DEAD.with_player(them).with_level(3);

    let out = settled(Cell::DEAD, &n);
    assert_eq!(out.player(), me, "nearer wins");
    assert_eq!(out.level(), 6 - LEVEL_FALL);

    // Mass does not beat distance: three weak claims lose to one strong one,
    // which is what keeps the number a distance and the map readable.
    let mut crowd = [Cell::DEAD; 8];
    crowd[0] = Cell::DEAD.with_player(me).with_level(6);
    for i in [2, 4, 6] {
        crowd[i] = Cell::DEAD.with_player(them).with_level(5);
    }
    assert_eq!(settled(Cell::DEAD, &crowd).player(), me);
}

/// Mass gets its say where distance cannot separate them: **a tie goes to
/// whoever is pushing hardest**, and a tie in that keeps the square where it
/// is, so a border between two exactly matched players does not flicker.
#[test]
fn a_tie_goes_to_the_heavier_push_and_then_stays_put() {
    let (me, them) = (PlayerId(1), PlayerId(2));
    let mut n = [Cell::DEAD; 8];
    n[0] = Cell::DEAD.with_player(me).with_level(5);
    n[2] = Cell::DEAD.with_player(me).with_level(5);
    n[4] = Cell::DEAD.with_player(them).with_level(5);
    assert_eq!(settled(Cell::DEAD, &n).player(), me, "two pushes beat one");

    // Dead level: whoever holds it keeps it, and both peers agree.
    let mut even = [Cell::DEAD; 8];
    even[0] = Cell::DEAD.with_player(me).with_level(5);
    even[4] = Cell::DEAD.with_player(them).with_level(5);
    let held_by_them = Cell::DEAD.with_player(them).with_level(1);
    assert_eq!(settled(held_by_them, &even).player(), them, "the holder keeps it");
    let held_by_me = Cell::DEAD.with_player(me).with_level(1);
    assert_eq!(settled(held_by_me, &even).player(), me);
}

/// **Granted ground is a spring**, not a carve-out. It reads as full whatever
/// is stored on it and the rule never works it out from its neighbours, so a
/// player whose life has gone out still has a patch with a live gradient on
/// it — which is the floor said in the same vocabulary as everything else.
#[test]
fn granted_ground_is_a_source_and_is_never_argued_away() {
    let (me, them) = (PlayerId(1), PlayerId(2));
    let home = Cell::DEAD.with_player(me).with_home(true).with_level(0);
    assert_eq!(home.influence(), bits::MAX_LEVEL, "stored level says nothing on a source");

    // Surrounded by somebody else at full strength, for as long as you like.
    let theirs = [Cell::alive(them); 8];
    let mut cell = home;
    for seed in 0..200 {
        cell = next_cell(cell, &theirs, seed);
        assert_eq!(cell.player(), me, "seed {seed}");
        assert!(cell.is_home());
    }

    // And it feeds the ground around it with nothing alive anywhere.
    let mut beside = [Cell::DEAD; 8];
    beside[0] = home;
    assert_eq!(settled(Cell::DEAD, &beside).player(), me);
}

/// The roll decides **when** a square works itself out, not what it decides.
/// Recomputed every generation the field would be an exact distance transform
/// that snaps the instant anything moves; this is what makes it lag and smear.
#[test]
fn the_roll_decides_the_rate_and_not_the_outcome() {
    let me = PlayerId(1);
    let mut n = [Cell::DEAD; 8];
    n[0] = Cell::alive(me);

    let mut moved = 0usize;
    for seed in 0..640 {
        let out = next_cell(Cell::DEAD, &n, seed);
        // Whenever it does update, it updates to the same thing. There is no
        // seed that gives a different owner or a different level.
        if out.player().is_owned() {
            assert_eq!(out.player(), me, "seed {seed}");
            assert_eq!(out.level(), bits::MAX_LEVEL - LEVEL_FALL, "seed {seed}");
            moved += 1;
        }
    }
    let expected = 640 * LEVEL_ADJUST as usize / crate::sim::seed::OUT_OF as usize;
    assert!(
        moved.abs_diff(expected) < 90,
        "{moved} of 640 settled, expected about {expected}"
    );
}

/// Run a square until it has taken whatever reaches it, so a test can say what
/// the field does without saying when.
fn settled(cell: Cell, neighbours: &Neighbours) -> Cell {
    for seed in 0..640 {
        let out = next_cell(cell, neighbours, seed);
        if out != cell {
            return out;
        }
    }
    panic!("nothing reached it in six hundred and forty tries")
}
