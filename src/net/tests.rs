//! Tests for the wire types, the prices and the placement rules.
//!
//! In their own file for the reason [`crate::sim::rule`]'s are: what goes over
//! the wire and what a placement costs are worth being able to read without a
//! thousand lines of assertion between one type and the next.

use super::spawn::{crowding, SPAWN_CROWDED, SPAWN_GAP, SPAWN_PITCH, SPAWN_SEARCH};
use super::*;

/// **A name is a label, so it is clamped rather than refused.** What it is
/// clamped to is a width a row can hold; keeping it out of the separator is
/// [`jsonl`]'s job now and not this one.
#[test]
fn a_name_is_clamped_to_something_a_line_can_hold() {
    assert_eq!(player_name("  alice  "), "alice");
    assert_eq!(player_name("a\tb\nc"), "abc", "a name wrote its own field");
    assert_eq!(player_name(&"x".repeat(200)).chars().count(), PLAYER_NAME_MAX);
    // And nobody is kept out of a game over it, which is the difference
    // from `room_name` and `team_name`.
    assert_eq!(player_name(""), "");
}

fn paint(cells: Vec<(i32, i32)>, placement: Placement) -> Stamped {
    Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Paint { cells, placement },
    }
}

/// Why a client must not apply its own action a second time when the
/// server broadcasts it back.
///
/// A `Paint` is idempotent on the generation it was meant for and not one
/// generation later: by then the cells it named have moved, and laying
/// them again puts the original pattern back on top of where it went. The
/// symptom is a glider that turns into a blob and settles into a still
/// life, and then snaps back to a glider when the resync lands.
#[test]
fn a_paint_applied_late_is_not_the_paint_you_asked_for() {
    let glider = vec![(1, 2), (2, 3), (3, 1), (3, 2), (3, 3)];
    let paint = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Paint { cells: glider, placement: Placement::Life },
    };

    // The server. The action lands after it has already stepped, which is
    // the ordinary case as soon as there is any latency at all, so it lays
    // the cells on untouched ground and steps.
    let mut server = World::infinite_empty();
    server.step();
    apply(&mut server, &paint);
    server.step();

    // A client that predicted the paint a generation earlier, stepped when
    // it was told a generation had happened, and then applied the same
    // action again when the server broadcast it back.
    let mut twice = World::infinite_empty();
    apply(&mut twice, &paint);
    twice.step();
    apply(&mut twice, &paint);
    twice.step();

    // The same client, skipping what it had already predicted.
    let mut once = World::infinite_empty();
    apply(&mut once, &paint);
    once.step();
    once.step();

    assert_eq!(server.live_cells().len(), 5, "the server has a glider");
    assert_eq!(
        once.live_cells().len(),
        5,
        "and so does a client that predicted it: the same five cells, one \
         step out of phase, which is the error prediction is allowed"
    );
    assert!(
        twice.live_cells().len() > 5,
        "where applying it twice leaves {} cells -- the original pattern \
         stamped back over where it went",
        twice.live_cells().len()
    );
}

/// Ground already held by `player`, so a price is the base rate rather
/// than the outside one. Most of these tests are about what a placement
/// costs, not about where it is, and everywhere is outside on an empty
/// world.
fn hold(world: &mut World, cells: &[(i32, i32)], player: PlayerId) {
    for &(row, col) in cells {
        let cell = world.cell_at(row, col).unwrap_or(Cell::DEAD);
        world.set_cell_at(
            row,
            col,
            cell.with_player(player).with_level(crate::sim::bits::MAX_LEVEL),
        );
    }
}

/// The reason the pricing reads the world at all. A drag is extended by
/// sweeping the whole rectangle again, so every cell already laid would be
/// paid for a second time.
#[test]
fn painting_what_is_already_there_is_free() {
    let mut world = World::infinite_empty();
    let cells = vec![(0, 0), (0, 1), (0, 2), (0, 3)];
    hold(&mut world, &cells, PlayerId(1));
    let cells = vec![(0, 0), (0, 1), (0, 2)];

    let first = paint(cells.clone(), Placement::Ice);
    assert_eq!(value_delta(&world, &first), -3 * Placement::Ice.cost());
    apply(&mut world, &first);

    // The same rectangle again, plus one cell it did not cover.
    let mut wider = cells.clone();
    wider.push((0, 3));
    assert_eq!(
        value_delta(&world, &paint(wider, Placement::Ice)),
        -Placement::Ice.cost(),
        "only the cell that changed should be charged for"
    );
}

/// **Priced before it is applied, or it is free.** Every placement is an
/// idempotent setter, so once an action is down every cell it names
/// already reads as what it would put there — which is exactly the test
/// `value_delta` skips a cell on. Both sides of the wire depend on the
/// order: the client's `Acted` arm applied first and charged nothing, so
/// a teammate's spending never moved the purse it shares.
#[test]
fn pricing_an_action_after_applying_it_is_free() {
    let me = PlayerId(1);
    for placement in [Placement::Life, Placement::Factory, Placement::Turret, Placement::Ice] {
        let mut world = World::infinite_empty();
        let cells = vec![(0, 0), (0, 1)];
        hold(&mut world, &cells, me);

        let lay = paint(cells.clone(), placement);
        assert!(value_delta(&world, &lay) < 0, "{placement:?} costs nothing to lay");
        apply(&mut world, &lay);
        assert_eq!(value_delta(&world, &lay), 0, "{placement:?} priced after it was applied");

        if placement.can_be_taken() {
            let take = Stamped {
                tick: 0,
                player: me,
                seat: me,
                action: Action::Erase { cells: cells.clone(), placement },
            };
            assert!(value_delta(&world, &take) > 0, "{placement:?} reclaims nothing");
            apply(&mut world, &take);
            assert_eq!(value_delta(&world, &take), 0, "{placement:?} erased twice");
        }
    }
}

/// Life and a factory are different things to hold, so a click holding one
/// over the other replaces the kind rather than killing the cell — which
/// is what `is_on` answers and what `remove_from` could not, since both
/// are taken away by clearing the same bit.
#[test]
fn a_factory_held_over_life_is_not_already_there() {
    let me = PlayerId(1);
    let life = Placement::Life.apply_to(Cell::DEAD, me);
    let factory = Placement::Factory.apply_to(Cell::DEAD, me);

    assert!(Placement::Life.is_on(life), "life is what is on a living cell");
    assert!(!Placement::Factory.is_on(life), "so a factory held over it places");
    assert!(Placement::Factory.is_on(factory));
    assert!(!Placement::Life.is_on(factory), "and life held over a factory places");

    // And placing is what converts, at the price of what is being laid.
    let mut world = World::infinite_empty();
    apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
    assert_eq!(
        value_delta(&world, &paint(vec![(0, 0)], Placement::Factory)),
        -Placement::Factory.cost(),
        "converting life to a factory costs what a factory costs"
    );
    apply(&mut world, &paint(vec![(0, 0)], Placement::Factory));
    assert_eq!(world.cell_at(0, 0).unwrap().kind(), Kind::FACTORY);
    assert!(world.cell_at(0, 0).unwrap().is_alive(), "and leaves the cell living");
}

/// An overclocker is a machine placed in fours for a turret's reasons, and
/// it is put down, recognised and taken back the way a turret is.
#[test]
fn an_overclocker_is_placed_and_taken_back_like_a_turret() {
    assert!(OVERCLOCK_COST > FACTORY_COST, "an overclocker does not inherit, so it costs more");

    let mut world = World::infinite_empty();
    let block = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
    hold(&mut world, &block, PlayerId(1));
    assert_eq!(
        value_delta(&world, &paint(block.clone(), Placement::Overclock)),
        -4 * OVERCLOCK_COST,
        "an emplacement is four of them"
    );
    apply(&mut world, &paint(block.clone(), Placement::Overclock));
    for &(row, col) in &block {
        let cell = world.cell_at(row, col).unwrap();
        assert!(cell.is_alive() && cell.kind() == Kind::OVERCLOCK);
        assert!(Placement::Overclock.is_on(cell), "the square holds what was placed");
        assert!(!Placement::Turret.is_on(cell), "and not the other machine");
        let taken = Placement::Overclock.remove_from(cell);
        assert!(!taken.is_alive() && taken.player() == PlayerId(1));
    }
}

/// A turret is bought once per cell forever, where a factory is bought once
/// per lineage — so it is dearer than a factory, and the price to read is the
/// **emplacement**: one turret dies of loneliness, and the smallest one
/// that works is a block of four.
#[test]
fn a_turret_is_priced_per_cell_and_placed_in_fours() {
    assert!(TURRET_COST > FACTORY_COST, "a turret does not inherit, so it costs more");

    let mut world = World::infinite_empty();
    let block = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
    hold(&mut world, &block, PlayerId(1));
    assert_eq!(
        value_delta(&world, &paint(block.clone(), Placement::Turret)),
        -4 * TURRET_COST,
        "an emplacement is four of them"
    );

    apply(&mut world, &paint(block.clone(), Placement::Turret));
    for (row, col) in block {
        let cell = world.cell_at(row, col).unwrap();
        assert!(cell.is_alive());
        assert_eq!(cell.kind(), Kind::TURRET);
    }

    // And it is a third thing to hold, so life over a turret replaces it
    // exactly as life over a factory does.
    let placed = world.cell_at(0, 0).unwrap();
    assert!(Placement::Turret.is_on(placed));
    assert!(!Placement::Life.is_on(placed));
    assert!(!Placement::Factory.is_on(placed));
}

/// A corpse holds no life for either placement to take, whatever kind it
/// kept — which is what stops a click over a dead factory handing out a free
/// one instead of charging for it.
#[test]
fn a_dead_mine_holds_neither_life_nor_a_mine() {
    let corpse = Placement::Factory.apply_to(Cell::DEAD, PlayerId(1)).with_alive(false);
    assert_eq!(corpse.kind(), Kind::FACTORY);
    assert!(!Placement::Factory.is_on(corpse));
    assert!(!Placement::Life.is_on(corpse));
}

/// The owner is no part of the question. Somebody else's life is still
/// life, so a click holding Life takes it — priced at RECLAIM rather than
/// converting it for what a cell costs.
#[test]
fn somebody_elses_life_is_still_life() {
    let theirs = Placement::Life.apply_to(Cell::DEAD, PlayerId(2));
    assert!(Placement::Life.is_on(theirs));
}

/// Ice is independent of life, so a pane is on a square whether or not
/// anything lives there — and life held over an iced living cell still
/// takes the life and leaves the pane.
#[test]
fn a_pane_is_on_a_square_whatever_lives_under_it() {
    let me = PlayerId(1);
    let iced_life = Placement::Ice.apply_to(Placement::Life.apply_to(Cell::DEAD, me), me);
    assert!(Placement::Ice.is_on(iced_life));
    assert!(Placement::Life.is_on(iced_life));
    assert!(Placement::Life.remove_from(iced_life).is_ice(), "the pane stands");
}

/// Ice and life are independent, so laying one over the other is a
/// change even though the cell was not empty.
#[test]
fn a_pane_over_a_living_cell_is_a_change() {
    let mut world = World::infinite_empty();
    apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
    assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Ice)), -Placement::Ice.cost());
    assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Life)), 0);
}

/// A pane belongs to whoever laid it, and there is one owner field per
/// cell, so icing someone else's ice takes it — and taking it is a
/// change, whatever the flags say.
#[test]
fn taking_over_another_players_pane_is_a_change() {
    let mut world = World::infinite_empty();
    let theirs = Stamped {
        tick: 0,
        player: PlayerId(2),
        seat: PlayerId(2),
        action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
    };
    apply(&mut world, &theirs);
    // Their pane, so their ground: laying over it is a change, and one
    // nobody else may make, since no influence of theirs reaches it.
    assert!(!may_place(&world, PlayerId(1), 0, 0), "not yours to build on");
}

/// The reason `Erase` carries a placement at all. Life and ice are
/// independent, so taking the life off an iced cell must leave the pane
/// standing — clearing the square outright destroyed a pane the player
/// never aimed at, at five a cell.
#[test]
fn taking_the_life_off_an_iced_cell_leaves_the_ice() {
    let mut world = World::infinite_empty();
    apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
    apply(&mut world, &paint(vec![(0, 0)], Placement::Ice));

    let take = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
    };
    assert_eq!(value_delta(&world, &take), 1, "reclaiming your own pays one");
    apply(&mut world, &take);

    let cell = world.cell_at(0, 0).unwrap();
    assert!(!cell.is_alive(), "the life should be gone");
    assert!(cell.is_ice(), "the pane should still be standing");
    assert_eq!(cell.player(), PlayerId(1), "and still belong to whoever laid it");
}

/// And the other way about, which is what gives a misplaced pane a way
/// back: holding Ice and clicking one lifts it, and the life under it
/// carries on.
#[test]
fn taking_the_ice_off_a_living_cell_leaves_the_life() {
    let mut world = World::infinite_empty();
    apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
    apply(&mut world, &paint(vec![(0, 0)], Placement::Ice));

    let take = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
    };
    apply(&mut world, &take);

    let cell = world.cell_at(0, 0).unwrap();
    assert!(cell.is_alive());
    assert!(!cell.is_ice());
}

/// Taking away what is not there is neither earned nor spent, and what
/// counts as "there" depends on what is being taken.
#[test]
fn taking_what_is_not_there_is_free() {
    let mut world = World::infinite_empty();
    apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
    let before = world.cell_at(0, 0).unwrap();

    let no_pane = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
    };
    assert_eq!(value_delta(&world, &no_pane), 0);
    apply(&mut world, &no_pane);
    assert_eq!(
        world.cell_at(0, 0).unwrap(),
        before,
        "there was no pane to lift, so nothing should have moved"
    );
}

/// Breaking someone else's costs one, because taking ground is not free —
/// and that now covers a pane as well as a cell, since both are theirs.
#[test]
fn breaking_another_players_pane_costs_one() {
    let mut world = World::infinite_empty();
    let theirs = Stamped {
        tick: 0,
        player: PlayerId(2),
        seat: PlayerId(2),
        action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
    };
    apply(&mut world, &theirs);

    let ours = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
    };
    assert_eq!(value_delta(&world, &ours), -1);
}

/// Life is drawn by the stroke and ice is placed as a wall, so they are
/// not worth the same. Pinned because one flat constant is exactly what
/// this replaced, and it is an easy thing to fall back to.
#[test]
fn life_and_ice_are_priced_apart() {
    assert_eq!(Placement::Life.cost(), 1);
    assert_eq!(Placement::Ice.cost(), 5);

    let mut world = World::infinite_empty();
    let five: Vec<_> = (0..5).map(|c| (0, c)).collect();
    hold(&mut world, &five, PlayerId(1));
    assert_eq!(value_delta(&world, &paint(five.clone(), Placement::Life)), -5);
    assert_eq!(value_delta(&world, &paint(five, Placement::Ice)), -25);
}

/// **Placing is confined to ground your own influence reaches**, at the
/// placement's own price wherever that is. Both halves of the other
/// arrangement went together: a price that rose as influence thinned, and
/// permission to place anywhere for a multiple. Ten times a cell was no
/// obstacle to anybody with a factory running, and a cost that varied across
/// ground which all looks the same was one nobody could play around.
#[test]
fn placing_is_confined_to_ground_you_reach_and_costs_the_same_throughout() {
    let mut world = World::infinite_empty();
    let me = PlayerId(1);
    hold(&mut world, &[(0, 0)], me);

    // The middle of your ground and the thinnest edge of it: one price.
    world.set_cell_at(0, 1, Cell::DEAD.with_player(me).with_level(1));
    assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Life)), -LIFE_COST);
    assert_eq!(value_delta(&world, &paint(vec![(0, 1)], Placement::Life)), -LIFE_COST);
    assert!(may_place(&world, me, 0, 0) && may_place(&world, me, 0, 1));

    // And a square nothing of yours reaches is not for sale at any price.
    assert!(!may_place(&world, me, 0, 5));
    assert_eq!(reach(&world, me, 0, 5), 0);
}

/// Somebody else's ground is not yours however strong their claim is:
/// a square carries one owner, so two players' influence never sits on
/// the same one.
#[test]
fn somebody_elses_influence_is_not_yours() {
    let mut world = World::infinite_empty();
    let (me, them) = (PlayerId(1), PlayerId(2));
    hold(&mut world, &[(0, 0)], them);
    assert_eq!(reach(&world, them, 0, 0), crate::sim::bits::MAX_LEVEL);
    assert_eq!(reach(&world, me, 0, 0), 0);
    assert!(!may_place(&world, me, 0, 0));
}

/// Taking is not what changed. Erasing is priced on whose it is, at the
/// reclaim rate, wherever it is.
#[test]
fn taking_is_not_charged_the_outside_rate() {
    let mut world = World::infinite_empty();
    let them = PlayerId(2);
    apply(
        &mut world,
        &Stamped {
            tick: 0,
            player: them,
            seat: them,
            action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Life },
        },
    );
    let ours = Stamped {
        tick: 0,
        player: PlayerId(1),
        seat: PlayerId(1),
        action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
    };
    assert_eq!(value_delta(&world, &ours), -RECLAIM);
}

/// The grant is still what a player starts from — not because it is the
/// only ground they may build on any more, but because it is the ground
/// the cheap rate applies on.
#[test]
fn a_grant_is_ground_at_the_base_rate() {
    let mut world = World::infinite_empty();
    let (me, them) = (PlayerId(1), PlayerId(2));
    let (row, col) = spawn_for(me, &world);

    assert!(!may_place(&world, me, row, col), "nothing is owned yet");
    grant(&mut world, me);
    assert!(may_place(&world, me, row, col), "granted ground is buildable");
    assert!(!may_place(&world, them, row, col), "and only by its owner");

    // Ground at the edges, and a block standing in the middle of it.
    assert!(!world.cell_at(row, col).unwrap().is_alive(), "the corner is bare");
    let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
    let block: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 1)]
        .iter()
        .map(|(r, c)| world.cell_at(middle.0 + r, middle.1 + c).unwrap())
        .collect();
    assert!(block.iter().all(|c| c.is_alive() && c.player() == me), "a 2x2 block");

    // Beyond the patch is nobody's, and nobody's is closed to everyone.
    assert!(!may_place(&world, me, row, col + SPAWN_N));
    assert!(!may_place(&world, me, 10_000, 10_000));
}

/// Every player is within reach of several others. A line put the last
/// player thirty patches from the first, which is a corridor rather than a
/// map: two players at opposite ends could never meet.
#[test]
fn grants_are_laid_out_in_a_square() {
    let world = World::infinite_empty();
    let spots: Vec<(i32, i32)> =
        (1..=PlayerId::MAX).map(|p| spawn_for(PlayerId(p), &world)).collect();
    let rows: Vec<i32> = spots.iter().map(|s| s.0).collect();
    let cols: Vec<i32> = spots.iter().map(|s| s.1).collect();

    let span = |v: &[i32]| v.iter().max().unwrap() - v.iter().min().unwrap();
    assert!(span(&rows) > 0, "a line has no second axis");
    assert!(
        span(&rows).abs_diff(span(&cols)) <= SPAWN_PITCH as u32,
        "the layout should be square, got {}x{}",
        span(&rows),
        span(&cols)
    );

    // Every player has a neighbour one pitch away, which a line only gives
    // to the two beside you.
    for &(row, col) in &spots {
        let touching = spots
            .iter()
            .filter(|&&(r, c)| {
                let (dr, dc) = ((r - row).abs(), (c - col).abs());
                (dr, dc) != (0, 0) && dr <= SPAWN_PITCH && dc <= SPAWN_PITCH
            })
            .count();
        assert!(touching >= 2, "({row}, {col}) has only {touching} neighbours");
    }
}

/// Every player gets their square on a torus too, which is what a torus
/// makes hard: the ground is finite, so a fixed pitch would run off the
/// end and wrap one player's grant onto another's. The grid is spread over
/// whatever ground there is instead.
/// The bug that locked a player out of a world they were looking at.
///
/// Territory only ever spreads, so a world with an edge eventually
/// belongs to whoever got there first. A player joining after that used to
/// be granted nothing -- no ground, and so no block, since the block goes
/// only on ground they own -- and placing is confined to your own
/// territory, so they could never come to own anything.
#[test]
fn a_grant_on_ground_somebody_else_has_spread_over_still_works() {
    let mut world = World::toroidal_empty(12, 12);
    let first = PlayerId(1);

    // The first player's territory covers the whole world, as it does on
    // any torus that has been running.
    let (rows, cols) = world.size_in_cells().unwrap();
    for r in 0..rows {
        for c in 0..cols {
            world.set_cell_at(r, c, Cell::DEAD.with_player(first));
        }
    }

    let second = PlayerId(2);
    grant(&mut world, second);

    let (row, col) = spawn_for(second, &world);
    let ours = (row..row + SPAWN_N)
        .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
        .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == second)
        .count();
    assert_eq!(ours, (SPAWN_N * SPAWN_N) as usize, "the whole patch is theirs");

    let alive: Vec<(i32, i32)> = (row..row + SPAWN_N)
        .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
        .filter(|&(r, c)| world.cell_at(r, c).unwrap().is_alive())
        .collect();
    assert_eq!(alive.len(), 4, "and a block stands on it: {alive:?}");
    assert!(
        alive.iter().all(|&(r, c)| world.cell_at(r, c).unwrap().player() == second),
        "the block is theirs"
    );

    // And they can actually place, which is the whole point.
    assert!(may_place(&world, second, row, col));
}

/// A grant takes ground and never anybody's life or panes -- those are
/// won by playing, not by arriving.
#[test]
fn a_grant_steps_around_life_and_ice() {
    let mut world = World::infinite_empty();
    let second = PlayerId(2);
    let (row, col) = spawn_for(second, &world);

    // Somebody else's living cell and pane, right in the middle where the
    // block wants to go.
    let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
    world.set_cell_at(middle.0, middle.1, Cell::alive(PlayerId(1)));
    world.set_cell_at(middle.0, middle.1 + 1, Cell::DEAD.with_ice(true).with_player(PlayerId(1)));

    grant(&mut world, second);

    let theirs = world.cell_at(middle.0, middle.1).unwrap();
    assert!(theirs.is_alive() && theirs.player() == PlayerId(1), "their life is untouched");
    let pane = world.cell_at(middle.0, middle.1 + 1).unwrap();
    assert!(pane.is_ice() && pane.player() == PlayerId(1), "and their pane");

    // The block went somewhere else in the patch rather than nowhere.
    let alive: Vec<(i32, i32)> = (row..row + SPAWN_N)
        .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
        .filter(|&(r, c)| {
            world.cell_at(r, c).unwrap().player() == second
                && world.cell_at(r, c).unwrap().is_alive()
        })
        .collect();
    assert_eq!(alive.len(), 4, "a whole block, not three cells that die: {alive:?}");
}

/// Both sides work it out independently -- the server on a join, the
/// client for an offline game -- so it must not depend on iteration order.
#[test]
fn a_grant_lands_in_the_same_place_every_time() {
    let build = || {
        let mut world = World::toroidal_empty(8, 8);
        for r in 0..40 {
            world.set_cell_at(r, r, Cell::alive(PlayerId(1)));
        }
        grant(&mut world, PlayerId(3));
        world.live_cells()
    };
    let first = build();
    for _ in 0..8 {
        assert_eq!(build(), first);
    }
}

#[test]
fn a_torus_still_gives_everyone_a_square() {
    // Big enough that the grid fits without crowding.
    let mut world = World::toroidal_empty(24, 24);
    assert!(!too_cramped_for_grants(&world));
    for id in 1..=PlayerId::MAX {
        grant(&mut world, PlayerId(id));
    }

    for id in 1..=PlayerId::MAX {
        let (row, col) = spawn_for(PlayerId(id), &world);
        let ours = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == PlayerId(id))
            .count();
        assert_eq!(ours, (SPAWN_N * SPAWN_N) as usize, "player {id} did not get a whole square");
    }
}

/// **No world a client or server can make is too small any more**, and
/// that is worth a test rather than a deletion.
///
/// The smallest torus there is, is one chunk. At sixteen cells a side that
/// was 256 cells and could not seat everybody, so `too_cramped_for_grants`
/// had a case to answer; at sixty-four it is 4096, which holds a five by
/// five grid of patches against a ceiling of fifteen players. The guard is
/// still right and is now unreachable from outside, which is the state to
/// know about — if either number moves back it starts mattering again, and
/// this is what would notice.
#[test]
fn no_world_anybody_can_make_is_too_small_to_go_round() {
    let smallest = World::toroidal_empty(1, 1);
    assert!(
        !too_cramped_for_grants(&smallest),
        "the smallest world there is cannot seat everybody"
    );
    let roomy = World::toroidal_empty(24, 24);
    assert!(!too_cramped_for_grants(&roomy));
    assert!(!too_cramped_for_grants(&World::infinite_empty()), "infinite has room");
}

/// Two players' grants must not overlap, or one would be building on the
/// other from the first move.
#[test]
fn grants_do_not_overlap() {
    let mut world = World::infinite_empty();
    for id in 1..=PlayerId::MAX {
        grant(&mut world, PlayerId(id));
    }
    for id in 1..=PlayerId::MAX {
        let (row, col) = spawn_for(PlayerId(id), &world);
        for r in row..row + SPAWN_N {
            for c in col..col + SPAWN_N {
                assert_eq!(
                    world.cell_at(r, c).unwrap().player(),
                    PlayerId(id),
                    "({r}, {c}) should belong to {id}"
                );
            }
        }
    }
}

/// The mark is on the **square**, not on what is standing on it, so the
/// block does not rub out the `HOME` under its own four cells — which
/// would leave the middle of a granted patch decaying like ordinary ground
/// once the block died, with the ring around it permanent.
#[test]
fn the_block_stands_on_home_ground_like_the_rest_of_the_patch() {
    let mut world = World::infinite_empty();
    let me = PlayerId(1);
    grant(&mut world, me);
    let (row, col) = spawn_for(me, &world);

    let mut live = 0;
    for r in row..row + SPAWN_N {
        for c in col..col + SPAWN_N {
            let cell = world.cell_at(r, c).unwrap();
            assert!(cell.is_home(), "({r}, {c}) in the patch should be home ground");
            live += cell.is_alive() as usize;
        }
    }
    assert_eq!(live, 4, "and the block is on it");
}

/// Neighbouring grants are a patch apart plus the gap, and the gap is in
/// chunks — which is the unit "how far away is my neighbour" is a question
/// about. Pinned because the spacing is the one number a player feels
/// before they have done anything.
#[test]
fn neighbouring_grants_are_a_gap_apart() {
    let world = World::infinite_empty();
    let (row, col) = spawn_for(PlayerId(1), &world);
    let (next_row, next_col) = spawn_for(PlayerId(2), &world);
    assert_eq!(next_row, row, "the first two are side by side");
    assert_eq!(next_col - col, SPAWN_PITCH);

    // What is between them is the gap: the pitch less the patch they each
    // stand on.
    assert_eq!(SPAWN_PITCH - SPAWN_N, SPAWN_GAP);
    // In cells, and stated as a distance rather than as a count of chunks
    // — the number was always about how far a glider travels, and tying it
    // to `CHUNK_N` quadrupled it the day a chunk grew.
    assert_eq!(SPAWN_GAP, 48, "forty-eight cells of no-man's-land");
}

/// **The grid grows with the roster.** Six seats filled in reading order
/// put the first six players in a line, and a line is the arrangement the
/// layout exists to avoid: your only neighbours are the two beside you and
/// the ends can never reach each other. A spiral fills a square at every
/// size, so however many turn up everybody has neighbours on more than one
/// side.
#[test]
fn the_grid_is_a_square_at_every_size() {
    let world = World::infinite_empty();
    let seats =
        |n: u8| -> Vec<(i32, i32)> { (1..=n).map(|p| spawn_for(PlayerId(p), &world)).collect() };

    for (players, side) in [(4u8, 2), (9, 3), (16, 4), (25, 5)] {
        let spots = seats(players);
        let span =
            |v: Vec<i32>| (v.iter().max().unwrap() - v.iter().min().unwrap()) / SPAWN_PITCH + 1;
        assert_eq!(span(spots.iter().map(|s| s.0).collect()), side, "{players} players");
        assert_eq!(span(spots.iter().map(|s| s.1).collect()), side, "{players} players");

        // Filled, not just bounded: a square with holes in it is a
        // different arrangement from a square.
        let mut distinct = spots.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), players as usize, "{players} seats should be distinct");
    }
}

/// A seat inside somebody's country is not a seat. A latecomer is put out
/// where there is room rather than into ground they could not build on
/// without paying the outside rate for every cell.
#[test]
fn a_seat_inside_somebody_elses_country_is_given_up() {
    let mut world = World::infinite_empty();
    let (me, them) = (PlayerId(2), PlayerId(1));
    let wanted = spawn_for(me, &world);

    // Their ground over the whole of it and well past its edges.
    for r in wanted.0 - SPAWN_N..wanted.0 + 2 * SPAWN_N {
        for c in wanted.1 - SPAWN_N..wanted.1 + 2 * SPAWN_N {
            world.set_cell_at(r, c, Cell::DEAD.with_player(them));
        }
    }

    let moved = spawn_for(me, &world);
    assert_ne!(moved, wanted, "a seat buried in their country should be given up");
    assert_eq!(crowding(&world, moved, me), 0, "and the one taken instead should be nobody's");

    // But a couple of stray cells is not a country: `grant` claims dead
    // ground whoever held it and steps the block around anything alive, so
    // a seat with a few of somebody's squares in it is still a seat.
    let mut sparse = World::infinite_empty();
    let spot = spawn_for(me, &sparse);
    sparse.set_cell_at(spot.0 + 1, spot.1 + 1, Cell::alive(them));
    sparse.set_cell_at(spot.0 + 2, spot.1 + 2, Cell::DEAD.with_player(them));
    assert_eq!(spawn_for(me, &sparse), spot, "two cells should not move anybody");
}

/// **Crowded means held, not inhabited.**
///
/// Territory *is* the owner field on dead squares, so a seat can be
/// entirely somebody's country with not one living cell in it — which is
/// what most of a country looks like most of the time, since life is
/// sparse and the ground it claimed is not. A crowding check that counted
/// life would call that seat empty and drop a latecomer into the middle of
/// somebody's territory, where every square is owned and they can build
/// nothing.
#[test]
fn a_seat_is_crowded_by_ground_even_with_nothing_alive_on_it() {
    let mut world = World::infinite_empty();
    let (me, them) = (PlayerId(2), PlayerId(1));
    let at = spawn_for(me, &world);

    // Their ground, at full influence, and **nothing alive anywhere**.
    for r in at.0..at.0 + SPAWN_N {
        for c in at.1..at.1 + SPAWN_N {
            world.set_cell_at(
                r,
                c,
                Cell::DEAD.with_player(them).with_level(crate::sim::bits::MAX_LEVEL),
            );
        }
    }
    assert!(world.live_cells().is_empty(), "the test is about ground, not life");

    assert!(
        crowding(&world, at, me) > SPAWN_CROWDED,
        "a seat full of somebody's territory read as empty because nothing stood on it"
    );
    assert_ne!(spawn_for(me, &world), at, "and so nobody was moved off it");

    // The converse, so this is a test about the owner field rather than
    // about any ground at all: the player's *own* territory does not
    // crowd them out of their own seat.
    let mut ours = World::infinite_empty();
    let seat = spawn_for(me, &ours);
    for r in seat.0..seat.0 + SPAWN_N {
        for c in seat.1..seat.1 + SPAWN_N {
            ours.set_cell_at(
                r,
                c,
                Cell::DEAD.with_player(me).with_level(crate::sim::bits::MAX_LEVEL),
            );
        }
    }
    assert_eq!(crowding(&ours, seat, me), 0, "your own ground is not a crowd");
    assert_eq!(spawn_for(me, &ours), seat);
}

/// The other half of giving a crowded seat up: the cure must not be worse.
///
/// An infinite plane has unlimited emptiness, so a search that simply
/// walked until it found quiet would put a latecomer so far from everybody
/// that they are alone in a multiplayer game. The seats are a **bounded**
/// spiral — `SPAWN_SEARCH` of them, `SPAWN_PITCH` apart — so however
/// crowded the world is, the furthest anybody lands is a distance that can
/// be written down.
#[test]
fn a_crowded_world_does_not_fling_anybody_into_nowhere() {
    let mut world = World::infinite_empty();
    let me = PlayerId(2);
    let them = PlayerId(1);

    // Everything the search can reach, owned by somebody else. There is
    // no uncrowded seat at all, which is the worst case: it has to settle
    // for the emptiest rather than return one.
    let reach = SPAWN_PITCH * 8;
    for r in -reach..reach {
        for c in -reach..reach {
            world.set_cell_at(r, c, Cell::DEAD.with_player(them));
        }
    }

    let at = spawn_for(me, &world);
    let bound = SPAWN_PITCH * SPAWN_SEARCH;
    assert!(
        at.0.abs() <= bound && at.1.abs() <= bound,
        "spawned at {at:?}, beyond the {bound} the spiral can reach"
    );
    // And it is still a seat somebody could be granted: `grant` claims
    // dead ground whoever held it, so a crowded patch is buildable even
    // though it was not empty.
    grant(&mut world, me);
    assert!(may_place(&world, me, at.0, at.1), "granted ground nobody can build on");
}

/// A granted patch keeps its `HOME` marks and they never decay, so a
/// spawn stays put however the world around it changes — `grant` runs
/// again on every rejoin, and a seat that wandered would hand a returning
/// player a second patch every time.
#[test]
fn no_two_grants_on_a_torus_are_neighbours() {
    // How far apart two positions are on a ring, which is the only measure
    // that means anything on a world you can walk off the edge of.
    let apart = |a: i32, b: i32, extent: i32| {
        let d = (a - b).abs();
        d.min(extent - d)
    };
    // A quarter the chunks a side for the same worlds, since a chunk grew
    // fourfold on the edge — and the largest is the cap, which is the size
    // this most wants to be checked at.
    for chunks in [2, 3, 4, 6, 10] {
        let extent = chunks * CHUNK_N as i32;
        let world = World::toroidal(chunks, chunks);
        assert!(!too_cramped_for_grants(&world), "{chunks} chunks was called cramped");

        // **Every number, not as many as the grid felt like seating.**
        // This used to ask only about the seats a comfortable pitch fit,
        // and quietly accept that the numbers past that shared a patch.
        let spawns: Vec<(i32, i32)> =
            (1..=PlayerId::MAX).map(|n| spawn_for(PlayerId(n), &world)).collect();
        for (i, a) in spawns.iter().enumerate() {
            for b in &spawns[i + 1..] {
                let (down, along) = (apart(a.0, b.0, extent), apart(a.1, b.1, extent));
                // **Patches never overlap.** Two of them do overlap only
                // if they are close on *both* axes, so one axis clearing a
                // patch's width is the whole of it -- neighbours in a row
                // share a row by construction.
                assert!(
                    down >= SPAWN_N || along >= SPAWN_N,
                    "on a {chunks}-chunk torus {a:?} and {b:?} are {down} and {along} apart, \
                     and a patch is {SPAWN_N} wide"
                );
            }
        }
    }
}

/// **A small torus seats everybody closer together rather than seating
/// some of them on top of each other**, which is the trade this makes and
/// the reverse of the one it used to.
///
/// A 128-cell torus passes the cramped check and has room for four seats
/// at a comfortable pitch and fifteen at a tight one. It used to take the
/// four, fold the other eleven numbers onto them with `%`, and hand each
/// new arrival a patch somebody was already standing on — and because
/// `grant` claims dead ground whoever held it, the newcomer took all but
/// the four squares under the last one's block. Players 1, 5, 9 and 13 all
/// sat at (0, 64).
#[test]
fn a_small_torus_seats_everybody_closer_rather_than_twice_over() {
    let world = World::toroidal(8, 8);
    let seats: std::collections::HashSet<(i32, i32)> =
        (1..=PlayerId::MAX).map(|n| spawn_for(PlayerId(n), &world)).collect();
    assert_eq!(seats.len(), PlayerId::MAX as usize, "two numbers shared a patch");

    // And where there *is* room, the comfortable spacing is untouched.
    let roomy = World::toroidal(10, 10);
    // The world's own extent, not a number repeated from the line above
    // — which is how this came to describe a world four times the size of
    // the one it was measuring, and wrapped every distance the wrong way.
    let extent = roomy.size_in_cells().expect("a torus has a size").1;
    let (first, second) = (spawn_for(PlayerId(1), &roomy), spawn_for(PlayerId(2), &roomy));
    let along = (first.1 - second.1).abs().min(extent - (first.1 - second.1).abs());
    let down = (first.0 - second.0).abs().min(extent - (first.0 - second.0).abs());
    assert!(down.max(along) >= SPAWN_PITCH, "{first:?} and {second:?} on a roomy torus");
}

#[test]
fn a_granted_seat_does_not_wander() {
    let mut world = World::infinite_empty();
    let (me, them) = (PlayerId(2), PlayerId(1));
    let home = spawn_for(me, &world);
    grant(&mut world, me);

    // Their country arrives afterwards, all around and over the top of it.
    for r in home.0 - SPAWN_N..home.0 + 2 * SPAWN_N {
        for c in home.1 - SPAWN_N..home.1 + 2 * SPAWN_N {
            let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
            if !cell.is_home() {
                world.set_cell_at(r, c, cell.with_player(them));
            }
        }
    }

    assert_eq!(spawn_for(me, &world), home, "their home is where it was granted");
}

/// Ground nobody holds prices as empty, which is what `apply` writes into
/// it. The two must agree or a client would be charged for one thing and
/// given another.
#[test]
fn unheld_ground_prices_as_empty() {
    let world = World::infinite_empty();
    let far = [(100_000, 100_000)];
    assert!(world.cell_at(far[0].0, far[0].1).is_none());
    // Nothing of anybody's reaches it, so nobody may build there. A
    // client cannot know what it does not hold, and reading unheld ground
    // as its own would predict a placement the server refuses.
    assert!(!may_place(&world, PlayerId(1), far[0].0, far[0].1));
    assert_eq!(reach(&world, PlayerId(1), far[0].0, far[0].1), 0);
}
