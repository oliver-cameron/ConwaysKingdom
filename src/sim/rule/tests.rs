//! Tests for the rule.
//!
//! In their own file so [`super`] is only the constants and what a cell does
//! from one generation to the next — which is a thing worth being able to read
//! in one screen, and cannot be read at all with two hundred lines of assertion
//! underneath it.

use super::*;
use crate::sim::{bits, Chunk, Kind, PlayerId};
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

/// **An overclocker is inherited**, because it is part of a shape rather than
/// a machine standing beside one: a pattern built of them births more of them,
/// which is what lets an overclocked glider stay overclocked while it flies.
/// What stops it being free is the price on every such birth.
#[test]
fn an_overclocker_is_inherited() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(4)).with_kind(Kind::OVERCLOCK);
    }
    for seed in 0..64 {
        let born = next_cell(Cell::DEAD, &n, seed);
        assert!(born.is_alive(), "seed {seed}");
        assert_eq!(born.player(), PlayerId(4), "the ground still changes hands");
        assert_eq!(born.kind(), Kind::OVERCLOCK, "seed {seed} dropped the clock");
    }
}

/// And a turret still is not, because a turret claims ground by standing
/// there: a gun whose children were turrets would claim the map.
#[test]
fn a_turret_is_still_not_inherited() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(4)).with_kind(Kind::TURRET);
    }
    for seed in 0..64 {
        let born = next_cell(Cell::DEAD, &n, seed);
        assert_eq!(born.kind(), Kind::NORMAL, "seed {seed} bred a turret");
    }
}

/// And a factory still is, because that is the whole of what a factory is: what was
/// bought is a lineage, and it travels by being copied.
#[test]
fn a_mine_is_still_inherited() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(4)).with_kind(Kind::FACTORY);
    }
    for seed in 0..64 {
        assert_eq!(next_cell(Cell::DEAD, &n, seed).kind(), Kind::FACTORY, "seed {seed}");
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
        let mut overclockers = [Cell::DEAD; 8];
        for (i, &p) in [0usize, 3, 6].iter().zip(&owners) {
            plain[*i] = Cell::alive(p);
            turrets[*i] = Cell::alive(p).with_kind(Kind::TURRET);
            overclockers[*i] = Cell::alive(p).with_kind(Kind::OVERCLOCK);
        }
        let chosen = next_cell(Cell::DEAD, &plain, seed).player();
        assert_eq!(
            next_cell(Cell::DEAD, &turrets, seed).player(),
            chosen,
            "seed {seed} picked a different parent once the kind changed"
        );
        assert_eq!(
            next_cell(Cell::DEAD, &overclockers, seed).player(),
            chosen,
            "seed {seed} picked a different parent among overclockers"
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
                    assert!(next.player().is_owned(), "live cell with player 0 from {live:?}");
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
                .with_player(if alive { PlayerId(1) } else { PlayerId::UNOWNED })
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
/// what makes a factory an investment rather than a square.
#[test]
fn a_birth_is_a_copy_of_a_parent_not_of_the_corpse() {
    let corpse = Cell::DEAD.with_kind(Kind(37)).with_player(PlayerId(6));
    let mut n = [Cell::DEAD; 8];
    for i in 0..3 {
        n[i] = Cell::alive(PlayerId(2)).with_kind(Kind::FACTORY);
    }
    let born = next_cell(corpse, &n, 1);
    assert!(born.is_alive());
    assert_eq!(born.player(), PlayerId(2), "a parent's number");
    assert_eq!(born.kind(), Kind::FACTORY, "and a parent's kind");
    assert!(!born.is_ice(), "never a parent's pane");
}

/// The kind is inherited from *the parent that was chosen*, so in a mixed
/// neighbourhood it spreads rather than being handed down whole. One factory
/// dropped into a growing pattern takes a share of the births, not all of
/// them and not none.
#[test]
fn a_kind_spreads_through_a_mixed_neighbourhood() {
    let mut n = [Cell::DEAD; 8];
    n[0] = Cell::alive(PlayerId(1)).with_kind(Kind::FACTORY);
    n[3] = Cell::alive(PlayerId(1));
    n[6] = Cell::alive(PlayerId(1));

    let factories =
        (0..300).filter(|&seed| next_cell(Cell::DEAD, &n, seed).kind() == Kind::FACTORY).count();
    assert!(
        (60..140).contains(&factories),
        "one parent in three should carry it, got {factories} in 300"
    );
}

// --- territory, which is a level now rather than a flag ---------------------

/// A living cell is a **source**: it reads as full whatever is stored on its
/// square, and what reaches out from it is the sum of what pushes, capped so a
/// step always costs at least [`LEVEL_FALL`].
#[test]
fn influence_comes_from_the_sum_of_what_pushes() {
    let me = PlayerId(1);
    let source = Cell::alive(me);
    assert_eq!(source.influence(), bits::MAX_LEVEL, "a living cell is a source");

    // One neighbour is one push: seven of influence, which buys a single level
    // at LEVEL_SPREAD apiece.
    let mut one = [Cell::DEAD; 8];
    one[0] = source;
    let alone = settled(Cell::DEAD, &one);
    assert_eq!(alone.player(), me);
    assert_eq!(alone.level(), bits::MAX_LEVEL / LEVEL_SPREAD);

    // **Mass buys reach.** The same square with more of the same pushing on it
    // takes a stronger claim, which is the whole of why this is a sum.
    let mut crowd = [Cell::DEAD; 8];
    for i in 0..5 {
        crowd[i] = source;
    }
    let pressed = settled(Cell::DEAD, &crowd);
    assert!(
        pressed.level() > alone.level(),
        "five pushes gave {} against one push's {}",
        pressed.level(),
        alone.level()
    );

    // But never as much as what feeds it. Without that a square with four
    // neighbours at its own level sums to more than that level, the field
    // feeds itself, and the map saturates.
    assert!(pressed.level() <= bits::MAX_LEVEL - LEVEL_FALL, "a step always costs");

    // And a claim that buys nothing leaves the square to nobody, which is what
    // bounds a halo with no rule about radius anywhere.
    let mut faint = [Cell::DEAD; 8];
    faint[0] = Cell::DEAD.with_player(me).with_level(1);
    let held = Cell::DEAD.with_player(me).with_level(4);
    let past = settled(held, &faint);
    assert_eq!(past.player(), PlayerId::UNOWNED);
    assert_eq!(past.level(), 0);
}

/// **Everybody else counts against you.** A player's net is their own total
/// less all the others', so a square with more pushing on it from elsewhere is
/// not theirs however much they have on it.
#[test]
fn the_heaviest_net_takes_the_square() {
    let (me, them) = (PlayerId(1), PlayerId(2));

    // Outnumbered: three of theirs against two of factory, so it goes to them.
    let mut n = [Cell::DEAD; 8];
    n[0] = Cell::DEAD.with_player(me).with_level(6);
    n[1] = Cell::DEAD.with_player(me).with_level(6);
    for i in 3..6 {
        n[i] = Cell::DEAD.with_player(them).with_level(6);
    }
    assert_eq!(settled(Cell::DEAD, &n).player(), them, "weight of numbers");

    // And what they take is what is *left* after factory is counted against it,
    // which is less than they would hold unopposed.
    let contested = settled(Cell::DEAD, &n).level();
    let mut alone = [Cell::DEAD; 8];
    for i in 3..6 {
        alone[i] = Cell::DEAD.with_player(them).with_level(6);
    }
    assert!(settled(Cell::DEAD, &alone).level() > contested, "being pushed back should cost them");

    // Evenly matched is nobody's: the nets cancel.
    let mut even = [Cell::DEAD; 8];
    even[0] = Cell::DEAD.with_player(me).with_level(7);
    even[4] = Cell::DEAD.with_player(them).with_level(7);
    let held = Cell::DEAD.with_player(me).with_level(3);
    assert_eq!(settled(held, &even).player(), PlayerId::UNOWNED, "a dead heat holds nothing");
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
/// Recomputed every generation the field would be an exact transform that
/// snaps the instant anything moves; this is what makes it lag and smear.
#[test]
fn the_roll_decides_the_rate_and_not_the_outcome() {
    let me = PlayerId(1);
    let mut n = [Cell::DEAD; 8];
    for i in 0..4 {
        n[i] = Cell::alive(me);
    }

    let mut moved = 0usize;
    let mut seen = None;
    for seed in 0..640 {
        let out = next_cell(Cell::DEAD, &n, seed);
        // Whenever it does settle, it settles to the same thing. There is no
        // seed that gives a different owner or a different level.
        if out.player().is_owned() {
            assert_eq!(out.player(), me, "seed {seed}");
            let level = *seen.get_or_insert(out.level());
            assert_eq!(out.level(), level, "seed {seed}");
            moved += 1;
        }
    }
    let expected = 640 * LEVEL_ADJUST as usize / crate::sim::seed::OUT_OF as usize;
    assert!(moved.abs_diff(expected) < 90, "{moved} of 640 settled, expected about {expected}");
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

/// **The curve has a peak, and that is the point.** A yield that only fell
/// would make every factory worth most on the generation it was laid, and the only
/// decision left would be to lay more. A peak means a field is worth letting
/// mature and then worth retiring.
#[test]
fn a_mine_pays_best_at_its_prime() {
    let best = super::factory_chance(super::FACTORY_PRIME);
    assert_eq!(best, super::FACTORY_BEST, "the prime is not the peak");
    for age in 0..=bits::MAX_AGE {
        assert!(super::factory_chance(age) <= best, "age {age} beats the prime");
    }
    assert!(
        super::factory_chance(0) < best,
        "a fresh factory is already at its best, so there is nothing to mature into"
    );
}

/// **And it falls, and keeps falling.** Past the prime every step is worse than
/// the last, which is what bounds what one lineage can ever be worth.
#[test]
fn past_its_prime_a_mine_only_gets_worse() {
    for age in super::FACTORY_PRIME..bits::MAX_AGE {
        assert!(
            super::factory_chance(age + 1) < super::factory_chance(age),
            "a factory at {} pays no less than at {age}",
            age + 1
        );
    }
    assert_eq!(
        super::factory_chance(bits::MAX_AGE),
        super::FACTORY_SPENT,
        "spent is not the floor"
    );
}

/// **Never nothing.** A factory that could not pay again is a cell worth telling
/// somebody about, and it is told by the sprite rather than by a surprise — so
/// the floor is small and real rather than zero.
#[test]
fn a_spent_mine_still_pays_sometimes() {
    for age in 0..=bits::MAX_AGE {
        assert!(super::factory_chance(age) > 0, "a factory at {age} can never pay");
        assert!(super::factory_chance(age) <= crate::sim::OUT_OF, "a chance out of range at {age}");
    }
}

/// **A factory takes the square's wear, not its parent's.** What has to be bounded
/// is a pattern re-birthing over the same cells; a lineage that travels to
/// fresh ground is the thing this must not punish, and it is what the game
/// wants people doing.
#[test]
fn a_mine_inherits_the_ground_it_is_born_on() {
    let me = PlayerId(1);
    let worn = Cell::DEAD.with_player(me).with_kind(Kind::FACTORY).with_age(4);
    let parents = [
        Cell::alive(me).with_kind(Kind::FACTORY),
        Cell::alive(me).with_kind(Kind::FACTORY),
        Cell::alive(me).with_kind(Kind::FACTORY),
        Cell::DEAD,
        Cell::DEAD,
        Cell::DEAD,
        Cell::DEAD,
        Cell::DEAD,
    ];
    let born = super::next_cell(worn, &parents, 7);
    assert!(born.is_alive() && born.kind() == Kind::FACTORY, "no factory was born");
    assert_eq!(born.age(), 5, "the square's wear did not carry into the birth");

    // Fresh ground starts fresh, however worn the parents are.
    let born = super::next_cell(Cell::DEAD.with_player(me), &parents, 7);
    assert_eq!(born.age(), 1, "a factory on clean ground started worn");
}

/// **Wear survives dying and clears only when the corpse goes.** Otherwise
/// letting a field die and regrow is a way to reset the meter, which is exactly
/// the loop this is here to close.
#[test]
fn wear_outlives_the_mine_and_goes_with_the_corpse() {
    let me = PlayerId(1);
    let corpse = Cell::DEAD.with_player(me).with_kind(Kind::FACTORY).with_age(6);
    let alone = [Cell::DEAD; 8];
    let next = super::next_cell(corpse, &alone, 3);
    assert_eq!(next.age(), 6, "a corpse forgot how worn its square was");
    assert_eq!(next.kind(), Kind::FACTORY, "and stopped being a factory too early");
}

/// **A fuse burns on the roll**, about [`DYNAMITE_FUSE`] in sixty-four of the
/// generations it lives through, and one step at a time. The chance is what
/// scatters four laid in one gesture.
#[test]
fn a_fuse_burns_on_a_chance_and_the_chance_is_the_rate() {
    let stick = Cell::alive(PlayerId(1)).with_kind(Kind::DYNAMITE);
    // Two neighbours, so it lives to burn.
    let n = neighbours(&[0, 4], 1);
    let mut burnt = 0usize;
    for seed in 0..640 {
        let out = next_cell(stick, &n, seed);
        assert!(out.is_alive() && out.kind() == Kind::DYNAMITE, "seed {seed}: not a live dynamite");
        assert!(out.age() <= 1, "seed {seed}: burnt {} steps in one generation", out.age());
        burnt += out.age() as usize;
    }
    let expected = 640 * DYNAMITE_FUSE as usize / crate::sim::seed::OUT_OF as usize;
    assert!(burnt.abs_diff(expected) < 40, "{burnt} of 640 burnt, expected about {expected}");
}

/// **The last step is certain**, so the sprite for "about to go" is on screen
/// for exactly one generation, always. A weapon with a random warning is a
/// weapon with no warning.
#[test]
fn the_last_step_of_a_fuse_is_certain() {
    let warned = Cell::alive(PlayerId(1)).with_kind(Kind::DYNAMITE).with_age(DYNAMITE_WARN);
    let n = neighbours(&[0, 4], 1);
    for seed in 0..640 {
        assert_eq!(next_cell(warned, &n, seed).age(), bits::MAX_AGE, "seed {seed}");
    }
}

/// **A fuse that has run out is left exactly as it is.** Going off is the
/// pass's business — see [`crate::sim::World`] — and the rule leaves a full
/// fuse alive, a dynamite and full for the generation its last sprite shows.
#[test]
fn a_fuse_that_has_run_out_is_left_for_the_pass() {
    let full = Cell::alive(PlayerId(1)).with_kind(Kind::DYNAMITE).with_age(bits::MAX_AGE);
    let n = neighbours(&[0, 4], 1);
    for seed in 0..640 {
        assert_eq!(next_cell(full, &n, seed), full, "seed {seed}");
    }
}

/// **Ice stops a fuse**, at every age including the certain step. A pane stops
/// time over what it covers and that is every rule, which is what makes ice
/// the counter to a dynamite.
#[test]
fn a_fuse_does_not_burn_under_ice() {
    let n = neighbours(&[0, 4], 1);
    for age in 0..bits::MAX_AGE {
        let iced = Cell::alive(PlayerId(1)).with_kind(Kind::DYNAMITE).with_age(age).with_ice(true);
        for seed in 0..64 {
            assert_eq!(next_cell(iced, &n, seed), iced, "age {age} seed {seed}");
        }
    }
}

/// **A dead dynamite is ordinary ground at once**, whether it died this
/// generation or was already lying there: [`Kind::leaves_a_corpse`] says no
/// for it, so the step sweeps it to `NORMAL` at age nought. An armed corpse
/// would take away the one answer that needs no ice, which is that a dynamite
/// has to be kept alive.
#[test]
fn a_dead_dynamite_is_ordinary_ground_at_age_nought() {
    let me = PlayerId(1);
    let mut chunk = Chunk::dead();
    // Alone, so it dies this generation; and a corpse, part burnt.
    chunk[(8, 8)] = Cell::alive(me).with_kind(Kind::DYNAMITE).with_age(5);
    chunk[(20, 20)] = Cell::DEAD.with_player(me).with_kind(Kind::DYNAMITE).with_age(3);
    let mut next = Chunk::dead();
    chunk.step(&mut next);
    for at in [(8, 8), (20, 20)] {
        let cell = next[at];
        assert!(!cell.is_alive(), "{at:?} is alive");
        assert_eq!(cell.kind(), Kind::NORMAL, "{at:?} is still a dynamite");
        assert_eq!(cell.age(), 0, "{at:?} kept its fuse");
    }
}

/// **A birth from a dynamite arrives with a fresh fuse.** The kind travels
/// and the age does not, so a glider that picks one up arms itself from
/// nought rather than arriving about to go off.
#[test]
fn a_birth_from_a_dynamite_arrives_with_a_fresh_fuse() {
    let mut n = [Cell::DEAD; 8];
    for i in [0, 3, 6] {
        n[i] = Cell::alive(PlayerId(1)).with_kind(Kind::DYNAMITE).with_age(DYNAMITE_WARN);
    }
    for seed in 0..64 {
        let born = next_cell(Cell::DEAD, &n, seed);
        assert!(born.is_alive() && born.kind() == Kind::DYNAMITE, "seed {seed}: no dynamite born");
        assert_eq!(born.age(), 0, "seed {seed}: the fuse travelled with the kind");
    }
}
