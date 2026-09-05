//! Tests for the authoritative side.
//!
//! In their own file for the reason [`crate::sim::rule`]'s are: what a server
//! does with a message is worth being able to read without scrolling past two
//! thousand lines of assertion to find the next method.

use super::*;

// Only the tests take a chunk apart; the server passes them through whole.
use crate::sim::{Cell, Chunk, CHUNK_N};

/// Cells inside a player's granted ground. Placing anywhere else is
/// refused now, so a test that wants a placement to land has to say where
/// relative to the grant rather than picking a coordinate off the map.
fn mine(id: PlayerId, offsets: &[(i32, i32)]) -> Vec<(i32, i32)> {
    // Every test here runs on an infinite world, whose grid of grants does
    // not depend on the world at all -- only a torus has to share out what
    // ground there is. So one is made here rather than threaded through
    // every call and fought with the borrow checker over.
    let (row, col) = crate::net::spawn_for(id, &World::infinite_empty());
    offsets.iter().map(|&(r, c)| (row + r, col + c)).collect()
}
use crate::net::{Action, Placement};

#[test]
/// Numbers start at one and are **never** reused, which is a change: they
/// used to fill the gap a departing player left. A number is written into
/// every cell that player owns, so handing it on hands over their
/// territory, and the ground outlives the connection. Coming back is what
/// the token is for.
fn player_numbers_start_at_one_and_are_never_reused() {
    let mut s = Server::new(World::infinite());
    let a = s.join("a").unwrap();
    let b = s.join("b").unwrap();
    assert_eq!((a, b), (PlayerId(1), PlayerId(2)));
    assert!(a.is_owned(), "zero is reserved for unowned cells");

    s.leave(a);
    assert_eq!(s.join("c").unwrap(), PlayerId(3), "a departed player's number is theirs still");
    assert!(!s.players().find(|p| p.id == a).unwrap().online, "and they are marked gone");
}

/// **A laboratory is a room, and its clock is a control.**
///
/// It used to be a mode the client was in with no server at all, which is
/// what made the two placing rules client-held flags — and a client that
/// answers those for itself predicts placements a server refuses. Held
/// here, several people can be in one laboratory and the answer is the
/// same for all of them.
#[test]
fn a_laboratory_opens_stopped_and_steps_when_it_is_told_to() {
    let mut s = Server::new(World::infinite());
    s.make_laboratory();
    assert!(s.rules().paused, "the first thing anybody does here is draw");

    let at = s.tick();
    assert!(s.step().is_empty(), "a stopped world does not step on the clock");
    assert_eq!(s.tick(), at);

    assert!(!s.step_once().is_empty(), "and does step when asked");
    assert_eq!(s.tick(), at + 1, "by exactly one generation");
    assert!(s.rules().paused, "and stays stopped afterwards");
}

/// The other half: a room that is a game says so rather than quietly
/// taking the rules off, because everywhere but a laboratory these *are*
/// the rules.
#[test]
fn only_a_laboratory_may_have_its_rules_changed() {
    let mut s = Server::new(World::infinite());
    let free = crate::net::Rules { place_free: true, ..Default::default() };
    assert!(s.set_rules(free).is_err(), "a world is a game");
    assert_eq!(s.rules(), crate::net::Rules::default());

    s.make_laboratory();
    let now = s.set_rules(free).expect("a laboratory's rules are its own");
    assert!(now.place_free && now.laboratory, "and it stays a laboratory");
}

/// **What the rules being off actually means**, which is two questions and
/// not a second simulation: where you may build, and what it costs.
#[test]
fn a_free_hand_places_off_your_own_ground_for_nothing() {
    let mut s = Server::new(World::infinite());
    let me = s.join("me").unwrap();
    let value = s.value_of(me).unwrap();
    // A long way from anything granted, so nothing of this player's
    // influence reaches it. A block rather than a cell, because the
    // assertion is read after a step and a lone cell dies of loneliness
    // before it can be looked at.
    let far = vec![(10_000, 10_000), (10_000, 10_001), (10_001, 10_000), (10_001, 10_001)];
    let act = |cells: Vec<(i32, i32)>| {
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint { cells, placement: Placement::Life },
        })
    };

    s.handle(Some(me), None, act(far.clone()));
    s.step();
    assert!(!s.world().live_cells().contains(&far[0]), "not yours to build on");

    s.make_laboratory();
    s.set_rules(crate::net::Rules {
        paused: true,
        place_anywhere: true,
        place_free: true,
        ..Default::default()
    })
    .unwrap();
    s.handle(Some(me), None, act(far.clone()));
    s.step_once();
    assert!(s.world().live_cells().contains(&far[0]), "with the rules off, anywhere");
    assert_eq!(s.value_of(me), Some(value), "and for nothing");
}

#[test]
fn the_server_is_full_at_the_cell_field_width() {
    let mut s = Server::new(World::infinite());
    for i in 1..=PlayerId::MAX {
        assert_eq!(s.join(format!("p{i}")).unwrap(), PlayerId(i));
    }
    assert!(s.join("one too many").is_err());
}

#[test]
fn a_painted_cell_belongs_to_the_player_who_painted_it() {
    let mut s = Server::new(World::infinite());
    let me = s.join("me").unwrap();
    // A blinker in the middle of this player's own ground, which is the
    // only place they may put one.
    let cells = mine(me, &[(5, 4), (5, 5), (5, 6)]);
    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint { cells: cells.clone(), placement: Placement::Life },
        }),
    );
    s.step();
    let live = s.world().live_cells();
    let (row, col) = (cells[1].0 - 1, cells[1].1);
    assert!(live.contains(&(row, col)), "the blinker should have rotated");
    let owner = s.world().cell_at(row, col).unwrap();
    assert_eq!(owner.player(), me, "live cells carry the painter's number");
}

/// The tick and the world's generation are the same number, and a load
/// has to restore it into the world rather than into a counter beside it.
///
/// Pinned on its own because the failure is silent: every seed is derived
/// from the generation, so a server restarted from a save rolls a
/// different sequence from the one that made the save, and a client — which
/// takes *this* number from `Welcome` — disagrees with the server from its
/// first step. `a_world_survives_a_save_and_load` catches it eventually,
/// as a divergence dozens of steps later that says nothing about why.
#[test]
fn a_loaded_world_is_on_the_tick_it_was_saved_at() {
    let dir = std::env::temp_dir().join("ck-tick-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tick.ckw");

    let mut s = Server::new(World::infinite_empty());
    let me = s.join("alice").unwrap();
    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint {
                cells: mine(me, &[(0, 0), (0, 1), (0, 2)]),
                placement: Placement::Life,
            },
        }),
    );
    for _ in 0..7 {
        s.step();
    }
    assert_eq!(s.tick(), s.world().generation, "they are one number");
    s.save(&path).unwrap();

    let back = Server::load_or_new(&path, DEFAULT_ROOM, World::infinite_empty).unwrap();
    assert_eq!(back.tick(), 7);
    assert_eq!(
        back.world().generation,
        7,
        "the world must come back on the generation it was saved on, or \
         every seed derived from it differs from here on"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_world_survives_a_save_and_load() {
    let dir = std::env::temp_dir().join("ck-persist-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("world.ckw");

    let mut s = Server::new(World::infinite());
    let me = s.join("alice").unwrap();
    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint {
                cells: mine(me, &[(4, 4), (4, 5), (4, 6)]),
                placement: Placement::Life,
            },
        }),
    );
    for _ in 0..25 {
        s.step();
    }
    s.save(&path).unwrap();

    let back = Server::load_or_new(&path, DEFAULT_ROOM, World::infinite).unwrap();
    assert_eq!(back.tick(), s.tick(), "tick is restored");
    assert_eq!(back.world().digest(), s.world().digest(), "world is restored");
    assert_eq!(back.world().live_cells(), s.world().live_cells());
    assert_eq!(back.player_count(), 1, "players are restored");
    assert_eq!(back.players().next().unwrap().name, "alice");

    // And it keeps stepping identically from there -- the whole point.
    let (mut a, mut b) = (s, back);
    for g in 0..50 {
        a.step();
        b.step();
        assert_eq!(a.world().digest(), b.world().digest(), "diverged at {g}");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_missing_file_starts_fresh_but_a_corrupt_one_does_not() {
    let dir = std::env::temp_dir().join("ck-persist-test");
    let _ = std::fs::create_dir_all(&dir);

    let missing = dir.join("does-not-exist.ckw");
    let _ = std::fs::remove_file(&missing);
    assert!(Server::load_or_new(&missing, DEFAULT_ROOM, World::infinite).is_ok());

    let corrupt = dir.join("corrupt.ckw");
    std::fs::write(&corrupt, b"not a world file at all").unwrap();
    assert!(
        Server::load_or_new(&corrupt, DEFAULT_ROOM, World::infinite).is_err(),
        "a bad file must not be silently replaced with an empty world"
    );
    let _ = std::fs::remove_file(&corrupt);
}

#[test]
fn reclaiming_your_own_cells_pays_and_placing_costs() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    let start = s.value_of(me).unwrap();

    let act = |s: &mut Server, action| {
        s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped { tick: s.tick(), player: me, seat: me, action }),
        );
        s.step();
    };

    // A 2x2 block: a still life, so it is still where it was put when the
    // next assertion looks. A blinker would have rotated out from under it.
    act(
        &mut s,
        Action::Paint {
            cells: mine(me, &[(0, 0), (0, 1), (1, 0), (1, 1)]),
            placement: Placement::Life,
        },
    );
    assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost()));

    // Reclaiming two of your own pays two back.
    // Reclaiming pays one each, well short of what they cost to place.
    act(&mut s, Action::Erase { cells: mine(me, &[(0, 0), (0, 1)]), placement: Placement::Life });
    assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost() + 2));

    // Erasing empty space is neither earned nor spent.
    act(&mut s, Action::Erase { cells: mine(me, &[(9, 9)]), placement: Placement::Life });
    assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost() + 2));
}

/// And it has to survive the server closing, which is the case it was
/// failing.
///
/// A player is not saved as online — the flag is not in the format — so
/// one rebuilt from a file came back marked connected, because
/// **A seat survives the server closing**, and the person in it is what
/// finds it again.
///
/// Nobody is connected to a world read off a disk, which is what makes the
/// way back work: a player who is *online* is not returning, they are
/// here. `Player::new` is what a player joins with and joining means being
/// online, so a roster rebuilt from a file used to come back marked
/// connected — and everybody in it was then refused their own seat on the
/// next run and given a new one, beside territory they could see and could
/// not build on.
#[test]
fn a_seat_survives_the_server_closing() {
    let path = std::env::temp_dir().join(format!("ck-restart-{}.ckw", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let who = crate::net::PersonId("3f2a".into());

    let mut before = Server::named("arena", World::infinite_empty());
    let me = before.join_with("alice", Some(&who)).unwrap();
    before.players.get_mut(&me).unwrap().value = 42;
    assert!(before.players[&me].online, "connected while playing");
    before.save(&path).unwrap();

    // A new process, reading the file the old one left.
    let mut after = Server::load_or_new(&path, "arena", World::infinite_empty).unwrap();
    assert!(!after.players[&me].online, "nobody is connected to a world off a disk");

    let back = after.join_with("alice", Some(&who)).unwrap();
    assert_eq!(back, me, "the person brings them back to their own number");
    assert_eq!(after.players[&me].value, 42, "with what they had");

    // And somebody else is somebody else, which is the other half of it.
    let other = after.join_with("bob", Some(&crate::net::PersonId("aaaa".into()))).unwrap();
    assert_ne!(other, me, "a different person took the same seat");

    let _ = std::fs::remove_file(&path);
}

/// Ground far from anybody's granted patch, so it counts towards a score.
fn stake(s: &mut Server, id: PlayerId, at: (i32, i32), n: i32) {
    for r in at.0..at.0 + n {
        for c in at.1..at.1 + n {
            // At full influence, or the rule would let it go on the first
            // generation: ground is a level now, and a square holding
            // nothing is a square nobody is holding.
            s.world.set_cell_at(
                r,
                c,
                Cell::DEAD.with_player(id).with_level(crate::sim::bits::MAX_LEVEL),
            );
        }
    }
}

/// A match runs its length and names whoever holds most.
#[test]
fn a_timer_match_ends_and_names_a_winner() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 5 });
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();

    // Gathering holds still, which is what makes the opening drawn rather
    // than raced: nobody gains generations by arriving early.
    s.step();
    s.step();
    assert_eq!(s.tick(), 0, "a gathering match does not step");

    stake(&mut s, alice, (900, 900), 6);
    stake(&mut s, bob, (900, 940), 4);
    s.start_match(None).unwrap();

    for _ in 0..5 {
        s.step();
    }
    match s.phase() {
        Phase::Over { winner, held, at } => {
            assert_eq!(*winner, Some(alice), "alice staked more");
            assert!(*held >= 36, "she held {held}");
            assert_eq!(*at, 5, "decided at the generation the clock ran out");
        }
        other => panic!("should be over, not {other:?}"),
    }

    // And it stops: a decided match does not go on running.
    let stopped = s.tick();
    s.step();
    assert_eq!(s.tick(), stopped, "an over match holds still");
}

/// **A match's world does not exist until it starts.** Granting on
/// arrival would put the first player's block on a world the last player
/// has not seen yet, and would hand out ground in the order people
/// happened to click. So a gathering match is an empty world and a list of
/// names, and the whistle lays every seat at once.
#[test]
fn a_match_spawns_everybody_at_the_whistle_and_nobody_before_it() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 100 });

    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    assert!(s.world().live_cells().is_empty(), "no world yet");
    assert_eq!(s.territory().iter().sum::<usize>(), 0, "and no ground either");
    assert_eq!(s.value_of(alice), Some(0), "and nothing to spend");
    assert_eq!(s.value_of(bob), Some(0));

    s.start_match(None).unwrap();

    // Two blocks, one each, and each on its own granted patch.
    assert_eq!(s.world().live_cells().len(), 8, "a block each, laid together");
    for id in [alice, bob] {
        let (row, col) = crate::net::spawn_for(id, s.world());
        assert_eq!(s.world().cell_at(row, col).unwrap().player(), id);
    }
}

/// An ordinary room is unchanged: it grants on arrival and hands out
/// something to build with, because there is no whistle to wait for.
#[test]
fn an_ordinary_room_still_grants_on_arrival() {
    let mut s = Server::named("main", World::infinite_empty());
    let alice = s.join_with("alice", None).unwrap();
    assert_eq!(s.world().live_cells().len(), 4, "a block, at once");
    assert_eq!(s.value_of(alice), Some(Player::STARTING_VALUE));
}

/// **A gathering match does not step, so a lobby cannot be told on a
/// cadence.** There is no tick to hang "every so often" from, and a lobby
/// that only refreshed when the world moved would never refresh at all —
/// so it goes out when it changes, and a still world still sends one.
#[test]
fn a_lobby_is_told_when_it_changes_even_though_nothing_steps() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 100 });

    let lobby = |out: &[ServerMessage]| {
        out.iter().find_map(|m| match m {
            ServerMessage::Match(crate::net::Lobby { players, phase, .. }) => {
                Some((players.clone(), phase.clone()))
            }
            _ => None,
        })
    };

    // Making it is a change, and the world is frozen.
    let (players, phase) = lobby(&s.step()).expect("the making of it");
    assert_eq!(phase, Phase::Gathering);
    assert!(players.is_empty());
    assert_eq!(s.tick(), 0, "and still nothing stepped");

    // Quiet in between: a lobby nobody has touched is not resent.
    assert!(lobby(&s.step()).is_none(), "nothing changed, so nothing is said");

    let alice = s.join_with("alice", None).unwrap();
    let (players, _) = lobby(&s.step()).expect("somebody arrived");
    assert_eq!(players.len(), 1);
    assert_eq!((players[0].id, players[0].name.as_str()), (alice, "alice"));

    s.leave(alice);
    let (players, _) = lobby(&s.step()).expect("and left");
    assert!(players.is_empty(), "a lobby lists who is here now: {players:?}");

    // Starting is a change too, and it is the one a client must not miss:
    // a lobby still saying "waiting to start" after it has started is a
    // screen telling a lie.
    s.start_match(None).unwrap();
    let (_, phase) = lobby(&s.step()).expect("the whistle");
    assert!(matches!(phase, Phase::Running { .. }));
}

/// Most first, ties by number, and nobody holding nothing.
///
/// The order has to be the same on every peer or rows swap places at a tie
/// and the bars jump about; leaving out the empty is what stops a world
/// that has seen fifteen people showing a column of mostly nobody.
#[test]
fn the_standing_is_most_first_and_leaves_out_the_empty() {
    let mut s = Server::named("arena", World::infinite_empty());
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    let carol = s.join_with("carol", None).unwrap();

    // **A grant is not a score, and it is very much ground.** Every row
    // here is somebody with a patch and nothing won yet, so the scores are
    // nought and the ground is not — which is the whole reason there are
    // two numbers. The bar shows the second, and read nought for as long
    // as it showed the first.
    let ServerMessage::Standing { held, .. } = s.standing() else { panic!() };
    assert_eq!(held.len(), 3, "a grant is ground: {held:?}");
    for row in &held {
        assert_eq!(row.score, 0, "a grant scored: {row:?}");
        assert!(row.ground >= 100, "a granted patch is ground: {row:?}");
    }

    stake(&mut s, bob, (900, 900), 4);
    stake(&mut s, carol, (900, 940), 4);
    stake(&mut s, alice, (940, 900), 5);

    let ServerMessage::Standing { held, tick } = s.standing() else { panic!() };
    assert_eq!(tick, s.tick());
    let scores: Vec<(PlayerId, u32)> = held.iter().map(|h| (h.who, h.score)).collect();
    assert_eq!(
        scores,
        vec![(alice, 25), (bob, 16), (carol, 16)],
        "most first, and a tie by the lower number"
    );
    // And the ground each holds is their score plus the patch they were
    // given, which is what a player sees on the map.
    for row in &held {
        assert!(row.ground > row.score, "ground left out the grant: {row:?}");
    }
}

/// The standings go out on a cadence, and the moment a match is decided
/// whatever the cadence says — the last one is the result.
#[test]
fn the_standing_goes_out_on_a_cadence_and_at_the_whistle() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 3 });
    let alice = s.join_with("alice", None).unwrap();
    stake(&mut s, alice, (900, 900), 4);
    s.start_match(None).unwrap();

    let standing =
        |out: &[ServerMessage]| out.iter().any(|m| matches!(m, ServerMessage::Standing { .. }));
    assert!(!standing(&s.step()), "tick 1 is not on the cadence");
    assert!(!standing(&s.step()), "nor is tick 2");
    // Tick 3 is the whistle, which sends one whatever the cadence says.
    let last = s.step();
    assert!(standing(&last), "the result goes out at once");
    assert!(matches!(s.phase(), Phase::Over { .. }));
}

/// **Nothing happens before the whistle.** A match that let people place
/// while gathering would be fair in generations and unfair in *time*:
/// somebody who joined ten minutes early has had ten minutes to think and
/// draw, and holding the tick still does not hold a clock still.
#[test]
fn a_gathering_match_takes_no_actions() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 100 });
    let alice = s.join_with("alice", None).unwrap();
    let cells = mine(alice, &[(3, 3), (3, 4)]);

    let before = s.world().live_cells().len();
    s.handle(
        Some(alice),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: alice,
            seat: alice,
            action: Action::Paint { cells: cells.clone(), placement: Placement::Life },
        }),
    );
    s.step();
    assert_eq!(s.world().live_cells().len(), before, "nothing laid before the whistle");
    assert_eq!(s.value_of(alice), Some(0), "and a match starts you with nothing");
    assert_eq!(before, 0, "nor is there a world yet to lay it on");

    // The whistle: everybody is granted at once, and only then is there
    // anything to act on or with.
    s.start_match(None).unwrap();
    s.players.get_mut(&alice).unwrap().value = 100;
    let before = s.world().live_cells().len();
    assert_eq!(before, 4, "a block, laid at the whistle");
    s.handle(
        Some(alice),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: alice,
            seat: alice,
            action: Action::Paint { cells, placement: Placement::Life },
        }),
    );
    s.step();
    assert!(s.world().live_cells().len() > before, "and lands once it is running");
    assert!(s.value_of(alice).unwrap() < Player::STARTING_VALUE, "and is paid for");
}

/// And nothing after it either: a decided match cannot be played on.
#[test]
fn an_over_match_takes_no_actions() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 1 });
    let alice = s.join_with("alice", None).unwrap();
    s.start_match(None).unwrap();
    s.step();
    assert!(matches!(s.phase(), Phase::Over { .. }), "one generation, then over");

    let before = s.world().live_cells().len();
    s.handle(
        Some(alice),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: alice,
            seat: alice,
            action: Action::Paint { cells: mine(alice, &[(3, 3)]), placement: Placement::Life },
        }),
    );
    s.step();
    assert_eq!(s.world().live_cells().len(), before);
}

/// **A side is one seat, one platform and one purse.**
///
/// Allies build on each other's ground and cannot hurt each other, so
/// seating them separately hands one team two opening positions where a
/// solo player gets one -- twice the frontage, and a border between them
/// **Fifteen sides is allowed and will not start**, which is the point.
///
/// The cap used to be seven, on the arithmetic that every side needs
/// somebody on it and a side and a seat both cost a number. That is true,
/// and the form was the wrong place to spend it: how many people are
/// coming is not something the person describing a match knows yet.
/// `teams_are_fair` asks at the whistle, which is when it can be answered.
///
/// What is left is the only thing true when a match is being made: there
/// are fifteen numbers. Ask for all of them and there are none for people,
/// and the refusal to join says so rather than saying the server is full —
/// the sides are the thing whoever just made this can change.
#[test]
fn fifteen_sides_is_allowed_and_leaves_nowhere_to_sit() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 1000 });
    s.make_teams(crate::net::MAX_TEAMS).expect("fifteen sides was refused");
    assert_eq!(s.team_count(), crate::net::MAX_TEAMS);

    let why = s.join_with("me", None).expect_err("a number was left after fifteen sides");
    assert!(why.contains("sides"), "the refusal blamed the wrong thing: {why}");
}

/// One under, so somebody can sit down — and then the **whistle** is what
/// refuses, by naming a side nobody is on. That is the check the form used
/// to be standing in for, asked at the moment it can be answered.
#[test]
fn a_match_with_more_sides_than_players_is_refused_at_the_whistle() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 1000 });
    s.make_teams(3).expect("three sides");
    let me = s.join_with("me", None).expect("a seat");
    s.join_team(me, PlayerId(1)).expect("a side to be on");

    let why = s.start_match(None).expect_err("a match with two empty sides started");
    assert!(why.contains("nobody is on"), "{why}");
}

/// no rule will ever contest. The size of a side is meant to be the
/// advantage, not where it starts.
#[test]
fn a_team_shares_a_seat_a_platform_and_a_purse() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 1000 });
    s.make_teams(2).unwrap();
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    let carol = s.join_with("carol", None).unwrap();
    s.join_team(alice, PlayerId(1)).unwrap();
    s.join_team(bob, PlayerId(1)).unwrap();
    s.join_team(carol, PlayerId(2)).unwrap();
    s.start_match(None).unwrap();

    // A team is a player, so two people at one team's controls are
    // seated identically because they are asking about the same number.
    let seat = |id| crate::net::spawn_for(s.plays_as(id), s.world());
    assert_eq!(seat(alice), seat(bob), "allies were seated apart");
    assert_ne!(seat(alice), seat(carol), "two teams shared a seat");
    assert_eq!(s.plays_as(alice), s.plays_as(bob), "allies are not one player");
    assert_ne!(s.plays_as(alice), alice, "joining a team did not take its controls");

    // One platform: the second ally to be granted finds the ground already
    // held by their own side and leaves it alone, rather than laying a
    // second block on top of their team's opening.
    let home = seat(alice);
    let blocks = (home.0..home.0 + crate::net::SPAWN_N)
        .flat_map(|r| (home.1..home.1 + crate::net::SPAWN_N).map(move |c| (r, c)))
        .filter(|&(r, c)| s.world().cell_at(r, c).is_some_and(|cell| cell.is_alive()))
        .count();
    assert_eq!(blocks, 4, "a team has one 2x2 block, and found {blocks} live cells");

    // A match starts everybody at nothing, which is the invariant's
    // starting condition as well as the game's.
    s.step();
    for id in [alice, bob, carol] {
        assert_eq!(s.value_of(id), Some(0), "a match did not start everybody level");
    }

    // One purse, and it is the team's own — so this is not an invariant
    // being kept across two numbers, it is two clients reading one.
    s.credit(alice, 40);
    assert_eq!(s.value_of(alice), Some(40));
    assert_eq!(s.value_of(bob), Some(40), "an ally was not paid");
    assert_eq!(s.value_of(carol), Some(0), "the other side was");

    s.credit(bob, -15);
    assert_eq!(s.value_of(alice), Some(25), "an ally's spending did not come out of the purse");
    assert_eq!(s.value_of(bob), Some(25));
    assert_eq!(s.value_of(carol), Some(0));
}

/// **A world may have teams**, and its teams are never settled: there is
/// no whistle, so people join and leave one as they like. A match fixes
/// them at the start, or changing sides would hand your ground to the
/// people you were fighting.
#[test]
fn a_world_has_teams_and_they_are_never_settled() {
    let mut s = Server::new(World::infinite_empty());
    s.make_teams(2).expect("a world was refused teams");
    let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
    let alice = s.join_with("alice", None).unwrap();
    s.join_team(alice, teams[0]).unwrap();
    assert_eq!(s.plays_as(alice), teams[0]);

    // A world steps forever, and the teams still move.
    s.step();
    s.join_team(alice, teams[1]).expect("a world settled its teams");
    assert_eq!(s.plays_as(alice), teams[1]);
    s.join_team(alice, alice).expect("stepping off a team was refused");
    assert_eq!(s.plays_as(alice), alice);

    // And a match's do not, once it is running.
    let mut m = Server::new(World::infinite_empty());
    m.make_match(Victory::Timer { generations: 100 });
    m.make_teams(2).unwrap();
    let teams: Vec<PlayerId> = m.teams().iter().map(|t| t.id).collect();
    let bob = m.join_with("bob", None).unwrap();
    let carol = m.join_with("carol", None).unwrap();
    m.join_team(bob, teams[0]).unwrap();
    m.join_team(carol, teams[1]).unwrap();
    m.start_match(None).unwrap();
    assert!(m.join_team(bob, teams[1]).is_err(), "a running match let somebody change teams");
}

/// **Giving up is a seat's decision and being out is a player's**, which
/// is the distinction a team needs: one of two walking away leaves one
/// pair of hands on the team, and the team plays on.
#[test]
fn one_of_a_team_giving_up_does_not_concede_for_the_team() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 10_000 });
    s.make_teams(2).unwrap();
    let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    let carol = s.join_with("carol", None).unwrap();
    s.join_team(alice, teams[0]).unwrap();
    s.join_team(bob, teams[0]).unwrap();
    s.join_team(carol, teams[1]).unwrap();
    s.start_match(None).unwrap();

    s.forfeit(alice).unwrap();
    assert!(s.still_in(teams[0]), "a team conceded when one of two gave up");
    assert!(matches!(s.phase(), Phase::Running { .. }), "and the match stopped");
    // Twice is not a thing to do, and says so rather than doing nothing.
    assert!(s.forfeit(alice).is_err());

    // The second of them takes the team out, and with one number left the
    // match is over and the survivor has won it.
    s.forfeit(bob).unwrap();
    assert!(!s.still_in(teams[0]), "a team with nobody on it is still in");
    assert!(
        matches!(s.phase(), Phase::Over { winner: Some(w), .. } if *w == teams[1]),
        "the last team standing did not win: {:?}",
        s.phase()
    );
}

/// A match that simply *has* one player in it has not been won by them.
/// Putting the survivor check in `decide` ended every such match on its
/// first generation.
#[test]
fn a_match_with_one_player_is_not_over_before_it_begins() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 50 });
    let alice = s.join_with("alice", None).unwrap();
    s.start_match(None).unwrap();
    s.step();
    assert!(matches!(s.phase(), Phase::Running { .. }), "{:?}", s.phase());
    // And giving up when you are the only one there ends it with nobody.
    s.forfeit(alice).unwrap();
    assert!(matches!(s.phase(), Phase::Over { winner: None, .. }), "{:?}", s.phase());
}

/// **A seat that gave up stops placing.** A forfeit that left somebody
/// able to act would be a concession in the scoreboard and nowhere else.
#[test]
fn a_seat_that_gave_up_cannot_act() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 10_000 });
    let alice = s.join_with("alice", None).unwrap();
    let _bob = s.join_with("bob", None).unwrap();
    s.start_match(None).unwrap();
    s.credit(alice, 100);
    let at = crate::net::spawn_for(alice, s.world());
    let act = || {
        ClientMessage::Act(Stamped {
            tick: 0,
            player: alice,
            seat: alice,
            action: crate::net::Action::Paint {
                cells: vec![at],
                placement: crate::net::Placement::Life,
            },
        })
    };
    s.handle(Some(alice), None, act());
    assert_eq!(s.pending.len(), 1);
    s.forfeit(alice).unwrap();
    s.handle(Some(alice), None, act());
    assert_eq!(s.pending.len(), 1, "somebody who gave up went on placing");
}

/// Calling it off is a real result: whoever leads wins it, and it is over
/// rather than abandoned. A match nobody can be held to is not one worth
/// rating.
#[test]
fn ending_a_match_early_names_whoever_is_ahead() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 10_000 });
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    s.start_match(None).unwrap();
    stake(&mut s, alice, (5_000, 5_000), 6);
    stake(&mut s, bob, (9_000, 9_000), 2);

    s.end_match().unwrap();
    assert!(
        matches!(s.phase(), Phase::Over { winner: Some(w), held, .. } if *w == alice && *held == 36),
        "{:?}",
        s.phase()
    );
    // And only while one is running.
    assert!(s.end_match().is_err(), "a decided match was ended again");
}

/// **An action says who sent it as well as what number it carries**, and
/// both are checked. They were one question until a team became a player:
/// several clients share `player`, so it can no longer say which of them
/// acted, and a client that could name any seat could act as a teammate.
#[test]
fn an_action_must_name_the_seat_that_sent_it() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 500 });
    s.make_teams(2).unwrap();
    let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    s.join_team(alice, teams[0]).unwrap();
    s.join_team(bob, teams[1]).unwrap();
    s.start_match(None).unwrap();

    let at = crate::net::spawn_for(teams[0], s.world());
    let tick = s.tick();
    let act = |seat, player| {
        ClientMessage::Act(Stamped {
            tick,
            player,
            seat,
            action: crate::net::Action::Paint {
                cells: vec![at],
                placement: crate::net::Placement::Life,
            },
        })
    };

    // A match starts everybody at nothing, and a paint costs.
    s.credit(teams[0], 100);
    s.credit(teams[1], 100);

    // Honest: alice, from alice's connection, playing her team.
    s.handle(Some(alice), None, act(alice, teams[0]));
    assert_eq!(s.pending.len(), 1, "an honest action was dropped");

    // A seat that is not the sender. Alice's connection cannot act as bob,
    // which is what the check was already for.
    s.handle(Some(alice), None, act(bob, teams[1]));
    assert_eq!(s.pending.len(), 1, "a connection acted as somebody else");

    // And the sender's own seat under a number they do not play: alice
    // putting cells down as the other team.
    s.handle(Some(alice), None, act(alice, teams[1]));
    assert_eq!(s.pending.len(), 1, "a seat placed under a number it does not play");
}

/// **The door is shut once a match is running**, and only somebody coming
/// back to their own seat gets through it.
///
/// The gate has to ask exactly what `join_with` asks — a gate that admits
/// on a weaker test than the one behind it is a gate with a hole in it,
/// and it had one: it asked whether the offered *token* matched anybody's,
/// and a `Player` started with an empty token, so a client sending an
/// empty one matched the first seat never issued a secret and was handed a
/// brand new player four hundred generations into a race.
#[test]
fn a_match_under_way_lets_nobody_new_in() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 100 });
    let alice = crate::net::PersonId("3f2a".into());
    s.join_with("alice", Some(&alice)).unwrap();
    s.start_match(None).unwrap();
    let before = s.players().count();

    let door = |who: Option<crate::net::PersonId>| {
        (who.clone(), ClientMessage::Join { name: "latecomer".into(), room: None, person: None })
    };
    // Nobody, and somebody this room has never seated.
    for offered in [None, Some(crate::net::PersonId("aaaa".into()))] {
        let (who, msg) = door(offered.clone());
        let out = s.handle(None, who.as_ref(), msg);
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "{offered:?} got in: {out:?}"
        );
    }
    assert_eq!(s.players().count(), before, "a refusal seated somebody anyway");

    // Somebody whose seat is here and *occupied* is a second machine, not
    // a reconnection, so the door refuses that too.
    let (who, msg) = door(Some(alice.clone()));
    let out = s.handle(None, who.as_ref(), msg);
    assert!(
        matches!(&out[..], [ServerMessage::Rejected { .. }]),
        "a second machine got in: {out:?}"
    );

    // But a player who actually dropped comes back, because this is the
    // door and not the room.
    let seat = s.players().next().map(|p| p.id).unwrap();
    s.leave(seat);
    let (who, msg) = door(Some(alice));
    let out = s.handle(None, who.as_ref(), msg);
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }]), "{out:?}");
    assert_eq!(s.players().count(), before, "coming back made a second player");
}

/// **The whistle says where everybody landed, and what to go and fetch.**
///
/// A match grants at the whistle rather than on arrival, by which time
/// every client has joined and subscribed -- so the chunks the grants
/// landed in do not change hands, nothing re-fetches them, and the ground
/// appeared for the server and for nobody else. Reloading the page fixed
/// it, which is what made it look like a bug in the client.
#[test]
fn a_whistle_says_where_everybody_landed() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 100 });
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();
    // Nothing is laid out while gathering, so nothing is announced either.
    assert!(!s.step().iter().any(|m| matches!(m, ServerMessage::Spawned { .. })));

    s.start_match(None).unwrap();
    let out = s.step();

    let spawned: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Spawned { player, at } => Some((*player, *at)),
            _ => None,
        })
        .collect();
    assert_eq!(spawned.len(), 2, "the whistle told nobody where they were: {out:?}");
    assert!(spawned.iter().any(|(p, _)| *p == alice));
    assert!(spawned.iter().any(|(p, _)| *p == bob));

    // And the ground itself, named so a client that already holds those
    // chunks knows they are wrong now.
    let resynced: Vec<_> = out
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Resync { chunks, .. } => Some(chunks.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    for (_, at) in &spawned {
        for chunk in crate::net::grant_chunks(s.world(), *at) {
            assert!(resynced.contains(&chunk), "{chunk:?} was granted and not resynced");
        }
    }
    // Once. A grant that announced itself every step would be a resync
    // storm for as long as the match ran.
    assert!(!s.step().iter().any(|m| matches!(m, ServerMessage::Spawned { .. })));
}

/// The other condition: first to a count rather than most at a whistle.
#[test]
fn a_territory_match_ends_when_somebody_reaches_the_count() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Territory { squares: 50 });
    let alice = s.join_with("alice", None).unwrap();
    s.start_match(None).unwrap();

    s.step();
    assert!(matches!(s.phase(), Phase::Running { .. }), "nobody holds fifty yet");

    stake(&mut s, alice, (900, 900), 8);
    s.step();
    match s.phase() {
        Phase::Over { winner, held, .. } => {
            assert_eq!(*winner, Some(alice));
            assert!(*held >= 50, "held {held}");
        }
        other => panic!("should be over, not {other:?}"),
    }
}

/// Granted ground never decays, so scoring it would be points for having
/// turned up. The floor stays — they can still build on it — it simply
/// does not win anything.
#[test]
fn granted_ground_does_not_count_towards_a_score() {
    let mut s = Server::named("arena", World::infinite_empty());
    let alice = s.join_with("alice", None).unwrap();
    assert_eq!(s.territory()[alice.0 as usize], 0, "a grant is not a score");

    stake(&mut s, alice, (900, 900), 3);
    assert_eq!(s.territory()[alice.0 as usize], 9, "ground won is");
}

/// **No late joining.** A match is a race from a shared start, and
/// somebody arriving at generation four hundred is not in it. Somebody
/// already seated is a different question: a refresh must still get them
/// back to their own seat.
#[test]
fn a_running_match_takes_no_newcomers_but_takes_its_own_back() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(matches::Victory::Timer { generations: 1000 });
    let who = crate::net::PersonId("3f2a".into());
    let alice = s.join_with("alice", Some(&who)).unwrap();
    s.start_match(None).unwrap();

    let refused =
        s.handle(None, None, ClientMessage::Join { name: "late".into(), room: None, person: None });
    assert!(
        matches!(refused.as_slice(), [ServerMessage::Rejected { reason }] if reason.contains("already under way")),
        "{refused:?}"
    );

    s.leave(alice);
    let back = s.handle(
        None,
        Some(&who),
        ClientMessage::Join { name: "alice".into(), room: None, person: None },
    );
    assert!(
        matches!(back.first(), Some(ServerMessage::Welcome { you, .. }) if *you == alice),
        "a player already in the match comes back: {back:?}"
    );
}

/// The whole point of being somebody: a player who drops comes back to
/// their own number, their own value and their own ground, rather than to
/// a fresh number beside a patch they can see and cannot build on.
#[test]
fn a_person_comes_back_to_themselves() {
    let mut s = Server::new(World::infinite_empty());
    let who = crate::net::PersonId("3f2a".into());
    let me = s.join_with("alice", Some(&who)).unwrap();

    // Spend some, so there is state worth coming back to.
    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint {
                cells: mine(me, &[(0, 0), (0, 1)]),
                placement: Placement::Life,
            },
        }),
    );
    s.step();
    let spent = s.value_of(me).unwrap();
    assert!(spent < Player::STARTING_VALUE, "something should have been spent");

    s.leave(me);

    // Coming back the way a client does, so the welcome itself is what is
    // checked: it has to carry the number, the secret *and* the value, or
    // the client returns believing it has the starting figure and offers
    // to spend money the server knows is gone.
    let welcome = s.handle(
        None,
        Some(&who),
        ClientMessage::Join { name: "alice".into(), room: None, person: None },
    );
    match welcome.as_slice() {
        [ServerMessage::Welcome { you, value, profile, .. }] => {
            assert_eq!(*you, me, "the same number");
            assert_eq!(*value, spent, "and the value they had");
            // A room does not fill this in. A profile outlives every room
            // on a server, so `Rooms` is what holds the table and what
            // stamps the answer — see `Rooms::profile_of`.
            assert!(profile.is_none());
        }
        other => panic!("expected a welcome, got {other:?}"),
    }
    assert_eq!(s.value_of(me), Some(spent));
}

/// Another player's territory has to reach you, or you cannot see whose
/// ground you are standing next to — and, worse, your own does not reach
/// you either: `may_place` reads the owner off the cell, so a client that
/// never receives the chunk refuses to build on ground that is its own.
///
/// The case that nearly slipped through is a chunk holding *only*
/// territory. Chunks are sent when the world holds them, and it holds
/// anything not empty — which counts ownership now. A filter on liveness
/// would have dropped exactly the chunks this is about.
#[test]
fn territory_reaches_the_clients_that_ask_for_it() {
    let mut s = Server::new(World::infinite_empty());
    let alice = s.join_with("alice", None).unwrap();
    let bob = s.join_with("bob", None).unwrap();

    let (row, col) = crate::net::spawn_for(alice, s.world());
    let chunk = (row.div_euclid(CHUNK_N as i32), col.div_euclid(CHUNK_N as i32));

    let sent = s.handle(Some(bob), None, ClientMessage::Subscribe { chunks: vec![chunk] });
    let [ServerMessage::ChunkData { cells, .. }] = sent.as_slice() else {
        panic!("bob should have been sent alice's chunk, got {sent:?}");
    };
    let cells: &Chunk = bytemuck::from_bytes(cells);
    let hers = (0..CHUNK_N)
        .flat_map(|r| (0..CHUNK_N).map(move |c| (r, c)))
        .filter(|&(r, c)| cells[(r, c)].player() == alice)
        .count();
    assert!(hers > 0, "alice's ground should be in what bob was sent");

    // And once her life has gone, the ground still is: a chunk of bare
    // territory is exactly what a returning player needs to be able to
    // build on, and it has no life to be sent for.
    for r in 0..CHUNK_N as i32 {
        for c in 0..CHUNK_N as i32 {
            let at = (chunk.0 * CHUNK_N as i32 + r, chunk.1 * CHUNK_N as i32 + c);
            let cell = s.world().cell_at(at.0, at.1).unwrap();
            s.world_mut().set_cell_at(at.0, at.1, cell.with_alive(false));
        }
    }
    let sent = s.handle(Some(bob), None, ClientMessage::Subscribe { chunks: vec![chunk] });
    assert!(
        matches!(sent.as_slice(), [ServerMessage::ChunkData { .. }]),
        "bare territory must still be sent, got {sent:?}"
    );
}

/// **Nobody may be one person twice.** Somebody already in their seat is
/// refused a second one rather than handed a stranger's.
///
/// This is where a person is stricter than the token it replaced, and
/// deliberately so. A token said which *seat*, so two tabs sharing one
/// were honestly two players and the second quietly got a new number. A
/// person is not two players, and being told so beats finding out by
/// building on ground that turns out to be somebody else's.
#[test]
fn one_person_cannot_hold_two_seats() {
    let mut s = Server::new(World::infinite_empty());
    let who = crate::net::PersonId("3f2a".into());
    let alice = s.join_with("alice", Some(&who)).unwrap();

    assert!(s.join_with("alice again", Some(&who)).is_err(), "one person took two seats");
    assert_eq!(s.players().count(), 1, "a refused join seated somebody anyway");

    // Once she has gone, she comes back to her own.
    s.leave(alice);
    assert_eq!(s.join_with("alice", Some(&who)).unwrap(), alice);
}

/// A person this room has never seated is a new player, not an error.
/// Anything else would lock somebody out of a room they have not been in.
#[test]
fn an_unknown_person_joins_as_somebody_new() {
    let mut s = Server::new(World::infinite_empty());
    let first = s.join_with("alice", Some(&crate::net::PersonId("3f2a".into()))).unwrap();
    let second = s.join_with("bob", Some(&crate::net::PersonId("aaaa".into()))).unwrap();
    assert_ne!(first, second);
}

/// **A client with no person is still a player**, and a new one every
/// time: there is nothing to find a seat by, which is the honest outcome
/// for a browser that cannot keep a secret rather than a reason to refuse
/// to let anybody play.
#[test]
fn a_client_with_no_person_is_new_every_time() {
    let mut s = Server::new(World::infinite_empty());
    let first = s.join_with("alice", None).unwrap();
    let second = s.join_with("alice", None).unwrap();
    assert_ne!(first, second, "two anonymous joins became one player");
    assert_eq!(s.players().count(), 2);
}

/// Ice cannot be taken back, and the server is where that is decided. The
/// client refuses it too, but a client that sends whatever it likes is the
/// case this exists for — and a pane liftable by asking twice would be no
/// pane at all.
#[test]
fn the_server_refuses_to_lift_ice() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    let pane = mine(me, &[(0, 0), (0, 1), (0, 2)]);

    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: me,
            seat: me,
            action: Action::Paint { cells: pane.clone(), placement: Placement::Ice },
        }),
    );
    s.step();
    let (row, col) = pane[0];
    assert!(s.world().cell_at(row, col).unwrap().is_ice());
    let spent = s.value_of(me);

    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: me,
            seat: me,
            action: Action::Erase { cells: pane, placement: Placement::Ice },
        }),
    );
    s.step();
    assert!(s.world().cell_at(row, col).unwrap().is_ice(), "the pane should still be there");
    assert_eq!(s.value_of(me), spent, "and nothing should have been paid for it");
}

/// The whole of what a side buys: allies build on each other's ground and
/// score as one, and everything else stays exactly as it was.
#[test]
fn a_team_builds_together_and_scores_as_one() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Territory { squares: 1_000 });
    s.make_teams(2).unwrap();
    let a = s.join("a").unwrap();
    let b = s.join("b").unwrap();
    let c = s.join("c").unwrap();
    s.join_team(a, PlayerId(1)).unwrap();
    s.join_team(b, PlayerId(1)).unwrap();
    s.join_team(c, PlayerId(2)).unwrap();

    // A patch of the team's ground with nothing standing on it. Staked
    // under the *team's* number, because that is the number A places
    // under — which is the whole of what joining a team did.
    let team = s.plays_as(a);
    assert_eq!(team, s.plays_as(b), "two at one team's controls are two players");
    let at = (5_000, 5_000);
    stake(&mut s, team, at, 4);

    // Both may build on it, and the other team may not — and neither
    // question needs anybody to know a team exists. `may_place` takes the
    // number being played and compares it, exactly as it did before there
    // were teams at all.
    assert!(crate::net::may_place(s.world(), s.plays_as(a), at.0, at.1));
    assert!(crate::net::may_place(s.world(), s.plays_as(b), at.0, at.1), "an ally cannot");
    assert!(!crate::net::may_place(s.world(), s.plays_as(c), at.0, at.1), "an enemy can");

    // And it is scored as one. There is nothing to sum: the cells carry
    // the team's number, so `territory` counted them under it.
    let held = s.territory();
    assert_eq!(held[team.0 as usize], 16);
    assert_eq!(held[a.0 as usize], 0, "a seat holds nothing of its own in a team match");
    assert_eq!(crate::server::matches::leader(&held), (Some(team), 16));
}

/// Teams are settled once a match starts. Changing them mid-match would
/// hand your ground to the people you were fighting.
#[test]
fn teams_cannot_be_changed_once_the_whistle_has_gone() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    s.make_teams(2).unwrap();
    let a = s.join("a").unwrap();
    let b = s.join("b").unwrap();
    s.join_team(a, PlayerId(1)).unwrap();
    s.join_team(b, PlayerId(2)).unwrap();
    s.name_team(PlayerId(1), "Reds").unwrap();

    s.start_match(Some(a)).unwrap();
    assert!(s.join_team(a, PlayerId(2)).is_err(), "changed sides mid-match");
    assert!(s.name_team(PlayerId(1), "Blues").is_err(), "renamed a side mid-match");
}

/// **A room keeps its own clock, and banks what is left over.**
///
/// Every room used to step on one ticker at the server's span, so a
/// laboratory could not be slowed down and a rate was a launch flag rather
/// than a control. The ticker is a grain now and each room decides — the
/// same shape `World::update` already gave the client, deliberately,
/// because two clocks banking time differently are two clocks that drift.
#[test]
fn a_room_steps_at_its_own_rate_and_keeps_the_remainder() {
    use std::time::Duration;
    let mut s = Server::new(World::infinite_empty());
    s.rules.laboratory = true;
    s.set_rules(crate::net::Rules { laboratory: true, bpm: 60, ..Default::default() })
        .expect("a laboratory would not take a rate");

    // One a second now, so a quarter of one is not yet a generation.
    let was = s.tick();
    assert!(s.owe(Duration::from_millis(250)).is_empty());
    assert!(s.owe(Duration::from_millis(250)).is_empty());
    assert_eq!(s.tick(), was, "it stepped early");
    assert!(!s.owe(Duration::from_millis(500)).is_empty(), "a whole second did not step it");
    assert_eq!(s.tick(), was + 1);

    // At most one a tick however far behind: a server that stalled should
    // arrive late rather than hand every client four steps at once.
    let was = s.tick();
    s.owe(Duration::from_secs(10));
    assert_eq!(s.tick(), was + 1, "it caught up all at once");
}

/// **A rate off the wire is checked and the three flags are not**, because
/// a bool is one of two answers and either is a room somebody might want,
/// where `0` is a stopped world said twice and `65535` is a busy loop.
#[test]
fn a_laboratory_cannot_be_set_to_an_impossible_rate() {
    let mut s = Server::new(World::infinite_empty());
    s.rules.laboratory = true;
    for bad in [0, crate::net::FASTEST_BPM + 1, u16::MAX] {
        let asked = crate::net::Rules { laboratory: true, bpm: bad, ..Default::default() };
        assert!(s.set_rules(asked).is_err(), "{bad} was accepted");
    }
    assert_eq!(s.rules.bpm, crate::net::DEFAULT_BPM, "a refused rate changed the room");
}

/// **Arriving puts you on a side, and the lobby decides which.**
///
/// Nothing did, so the person who described a match, made it, and was
/// alone in it could not start it: the whistle refuses anybody on nobody's
/// side, and there was no way onto one but finding the list and pressing.
///
/// Empty sides first and in order, because an empty side is the other
/// thing that stops a whistle; then the smallest, so a room that fills up
/// stays roughly even without the server ever refusing the uneven match
/// people meant to arrange.
#[test]
fn arriving_at_a_match_puts_you_on_a_side() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    s.make_teams(3).unwrap();

    // The empty ones, in order, before anybody is doubled up.
    let first = s.join("a").unwrap();
    let second = s.join("b").unwrap();
    let third = s.join("c").unwrap();
    let plays = |s: &Server, id| s.players[&id].plays_as;
    assert_eq!(plays(&s, first), s.sides[0]);
    assert_eq!(plays(&s, second), s.sides[1]);
    assert_eq!(plays(&s, third), s.sides[2]);
    // Which is a match that can start, with nobody having pressed anything.
    s.start_match(Some(first)).expect("a full lobby would not start");

    // And then the smallest.
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    s.make_teams(2).unwrap();
    for who in ["a", "b", "c", "d"] {
        s.join(who).unwrap();
    }
    // Counting people rather than sides: a side is a `Player` row whose
    // `plays_as` is its own id, so a naive count counts it as one of its
    // own members and every answer here is one too many.
    let on = |s: &Server, side| {
        s.players.values().filter(|p| !s.sides.contains(&p.id) && p.plays_as == side).count()
    };
    assert_eq!((on(&s, s.sides[0]), on(&s, s.sides[1])), (2, 2), "four went in unevenly");
}

/// A room with no sides puts nobody anywhere, and a match already running
/// takes nobody at all — so neither has a side to be put on.
#[test]
fn a_room_without_sides_seats_nobody_on_one() {
    let mut s = Server::new(World::infinite_empty());
    let alone = s.join("a").unwrap();
    assert_eq!(s.players[&alone].plays_as, alone, "a plain world put somebody on a side");
}

/// A match nobody would want to play is refused at the whistle rather than
/// in the lobby: a lobby that stops you joining your friend makes people
/// argue about the order they clicked in.
#[test]
fn a_lopsided_match_is_refused_at_the_whistle_and_not_before() {
    let mut s = Server::new(World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    s.make_teams(2).unwrap();
    let a = s.join("a").unwrap();
    let b = s.join("b").unwrap();

    // **Somebody who stepped off.** Joining puts you on a side now — see
    // `side_for_somebody_new` — so this is reached by leaving one rather
    // than by never having picked, which is the only way left to be on
    // nobody's and is still a match that cannot be scored.
    s.join_team(a, a).unwrap();
    let why = s.start_match(Some(b)).unwrap_err();
    assert!(why.contains("picked"), "{why}");

    // Everybody on one side, so the other is empty.
    s.join_team(a, PlayerId(1)).unwrap();
    s.join_team(b, PlayerId(1)).unwrap();
    let why = s.start_match(Some(a)).unwrap_err();
    assert!(why.contains("Team 2"), "{why}");

    // Three against one is *not* refused: people arrange that on purpose,
    // and a server that forbids it is one they work around.
    s.join_team(b, PlayerId(2)).unwrap();
    let c = s.join("c").unwrap();
    let d = s.join("d").unwrap();
    s.join_team(c, PlayerId(1)).unwrap();
    s.join_team(d, PlayerId(1)).unwrap();
    assert!(s.start_match(Some(a)).is_ok(), "three against one was refused");
}

/// An action belongs to the connection that sent it. Without this the
/// `player` field is a claim rather than an identity: anybody in the room
/// could act as anybody else in it, spending their value and placing their
/// cells, and a connection with no seat at all — a spectator — could act
/// as everybody.
///
/// Measured on the purse rather than on the world, because a single live
/// cell dies of loneliness in the same step that applies it. The value is
/// the honest witness: it moves exactly when an action was taken.
/// Coming back used to hand you a fresh 12×12 patch and a
/// brand-new 2×2 block on top of whatever you had built — so disconnecting
/// and returning conjured a still life out of nothing, for free, as often
/// as you liked.
#[test]
fn coming_back_does_not_grant_a_second_platform() {
    let mut s = Server::new(World::infinite_empty());
    let who = crate::net::PersonId("3f2a".into());
    let me = s.join_with("alice", Some(&who)).unwrap();
    let at = crate::net::spawn_for(me, s.world());

    // Clear the block they were given, which is what a player who has
    // played for a while and lost it looks like.
    let block = (at.0..at.0 + crate::net::SPAWN_N)
        .flat_map(|r| (at.1..at.1 + crate::net::SPAWN_N).map(move |c| (r, c)))
        .filter(|&(r, c)| s.world().cell_at(r, c).is_some_and(|x| x.is_alive()))
        .collect::<Vec<_>>();
    assert_eq!(block.len(), 4, "the grant stands a block");
    for (r, c) in block {
        let was = s.world().cell_at(r, c).unwrap();
        s.world_mut().set_cell_at(r, c, was.with_alive(false));
    }
    assert_eq!(s.world().live_cells().len(), 0, "nothing of theirs is alive");

    s.leave(me);
    let back = s.join_with("alice", Some(&who)).unwrap();
    assert_eq!(back, me, "they came back to themselves");
    assert_eq!(s.world().live_cells().len(), 0, "coming back built a fresh block out of nothing");
}

#[test]
fn an_action_attributed_to_somebody_else_is_dropped() {
    let mut s = Server::new(World::infinite_empty());
    let alice = s.join("alice").unwrap();
    let bob = s.join("bob").unwrap();
    // Ground Alice owns with nothing standing on it, so the only reason
    // an action there could fail is the one being tested.
    let at = (10_000, 10_000);
    stake(&mut s, alice, at, 3);
    let before = s.value_of(alice).unwrap();

    let forged = |tick| Stamped {
        tick,
        player: alice,
        seat: alice,
        action: Action::Paint { cells: vec![at], placement: Placement::Life },
    };

    // Bob's connection, claiming to be Alice.
    s.handle(Some(bob), None, ClientMessage::Act(forged(s.tick())));
    assert_eq!(s.value_of(alice).unwrap(), before, "Alice paid for Bob's action");

    // And a connection with no seat at all, which is what a spectator is.
    s.handle(None, None, ClientMessage::Act(forged(s.tick())));
    assert_eq!(s.value_of(alice).unwrap(), before, "a watcher acted");

    // The same action from Alice's own connection is taken, so this is a
    // test about attribution and not about the action being invalid.
    s.handle(Some(alice), None, ClientMessage::Act(forged(s.tick())));
    assert_eq!(
        s.value_of(alice).unwrap(),
        before - crate::sim::LIFE_COST,
        "Alice's own action was refused too"
    );
}

#[test]
fn destroying_another_players_cell_costs() {
    let mut s = Server::new(World::infinite_empty());
    let a = s.join("a").unwrap();
    let b = s.join("b").unwrap();
    // A block again, so a's cell survives long enough for b to attack it.
    s.handle(
        Some(a),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: a,
            seat: a,
            action: Action::Paint {
                cells: mine(a, &[(0, 0), (0, 1), (1, 0), (1, 1)]),
                placement: Placement::Life,
            },
        }),
    );
    s.step();
    let (row, col) = mine(a, &[(0, 0)])[0];
    assert_eq!(s.world().cell_at(row, col).map(|c| c.player()), Some(a));

    let before = s.value_of(b).unwrap();
    s.handle(
        Some(b),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: b,
            seat: b,
            action: Action::Erase { cells: mine(a, &[(0, 0)]), placement: Placement::Life },
        }),
    );
    s.step();
    assert_eq!(s.value_of(b), Some(before - 1), "taking ground is not free");
}

/// **Cost is no bound on length.** An `Erase` over ground nobody holds
/// prices at nothing however many cells it names, so affordability would
/// let a single message spend the room's whole tick — and then the room's
/// **A checkpoint naming chunks nobody holds is capped like a subscribe**,
/// and it is the sharper of the two: a coordinate the server does not hold
/// has no digest, so it always mismatches and always comes back. Uncapped,
/// a few kilobytes of made-up coordinates bought a walk of the map per
/// coordinate and a reply naming every one of them.
#[test]
fn a_checkpoint_cannot_ask_about_more_chunks_than_a_client_holds() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    let over = MOST_CHUNKS_AT_ONCE + 500;

    let out = s.handle(
        Some(me),
        None,
        // Every one of them invented, so every one of them mismatches --
        // which is what makes the reply as long as the request.
        ClientMessage::Checkpoint {
            tick: s.tick(),
            chunks: (0..over as i32).map(|c| ((9_000, c), 1u64)).collect(),
        },
    );
    let resync = out.iter().find_map(|m| match m {
        ServerMessage::Resync { chunks, .. } => Some(chunks),
        _ => None,
    });
    let named = resync.expect("a checkpoint of nothing the server holds answered nothing");
    assert_eq!(named.len(), MOST_CHUNKS_AT_ONCE, "the reply was not capped");
}

/// whole broadcast, since every client applies it too. Refused on the
/// length before anything walks the list.
#[test]
fn an_action_naming_more_cells_than_allowed_is_dropped() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    let over = crate::net::MOST_CELLS_AT_ONCE + 1;
    let before = s.value_of(me).unwrap();

    let out = s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            // Far from anything, and free at any length: the point is that
            // nothing about the price would have stopped it.
            action: Action::Erase {
                cells: (0..over as i32).map(|c| (100_000, c)).collect(),
                placement: Placement::Life,
            },
        }),
    );
    assert!(out.is_empty());
    assert!(s.take_announcements().is_empty(), "an over-long action was broadcast");
    s.step();
    assert_eq!(s.value_of(me), Some(before), "and nothing was charged for it");

    // One cell under the cap goes through, so it is the length being
    // refused rather than the shape of the message.
    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: s.tick(),
            player: me,
            seat: me,
            action: Action::Erase {
                cells: (0..crate::net::MOST_CELLS_AT_ONCE as i32).map(|c| (100_000, c)).collect(),
                placement: Placement::Life,
            },
        }),
    );
    assert!(!s.take_announcements().is_empty(), "an action within the cap was dropped");
}

#[test]
fn an_action_you_cannot_afford_is_refused() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    let purse = s.value_of(me).unwrap();
    let granted = s.world().live_cells();
    assert_eq!(granted.len(), 4, "the grant is a block, and only that");

    // Inside their own ground, so it is affordability being tested and
    // not the territory rule -- and skipping the block they already own,
    // since painting over what is already there is free and so would not
    // count towards the bill.
    let n = crate::net::SPAWN_N;
    let (row, col) = crate::net::spawn_for(me, &World::infinite_empty());
    let block = n / 2 - 1;
    let too_many: Vec<_> = (0..n)
        .flat_map(|r| (0..n).map(move |c| (r, c)))
        .filter(|&(r, c)| !((block..block + 2).contains(&r) && (block..block + 2).contains(&c)))
        .take((purse / Placement::Life.cost() + 1) as usize)
        .map(|(r, c)| (row + r, col + c))
        .collect();

    s.handle(
        Some(me),
        None,
        ClientMessage::Act(Stamped {
            tick: 0,
            player: me,
            seat: me,
            action: Action::Paint { cells: too_many, placement: Placement::Life },
        }),
    );
    s.step();
    assert_eq!(s.value_of(me), Some(purse), "nothing was spent");
    assert_eq!(s.world().live_cells(), granted, "and nothing was placed");
}

#[test]
fn a_matching_digest_asks_for_no_resync() {
    let mut s = Server::new(World::infinite());
    let me = s.join("me").unwrap();
    let held: Vec<_> = s
        .world()
        .stored()
        .iter()
        .map(|&(coord, _)| (coord, s.world().chunk_digest(coord).unwrap()))
        .collect();

    // Agreement is silence about chunks -- but never silence, because the
    // purse rides on every checkpoint now that value is not a thing a
    // client can work out for itself.
    let replies =
        s.handle(Some(me), None, ClientMessage::Checkpoint { tick: 0, chunks: held.clone() });
    assert!(
        !replies.iter().any(|m| matches!(m, ServerMessage::Resync { .. })),
        "matching digests asked for a resync: {replies:?}"
    );
    assert!(
        replies.iter().any(|m| matches!(m, ServerMessage::Purse { .. })),
        "and the purse should come back with it: {replies:?}"
    );

    // One chunk wrong: only that one comes back.
    let mut bad = held.clone();
    bad[0].1 = !bad[0].1;
    let replies = s.handle(Some(me), None, ClientMessage::Checkpoint { tick: 0, chunks: bad });
    let resyncs: Vec<_> = replies
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Resync { chunks, .. } => Some(chunks.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(resyncs, vec![vec![held[0].0]], "only the disagreeing chunk");
}

/// A compact machine pays. Three cells and two corpses is the cheapest
/// thing that keeps giving birth, and it is meant to be worth building.
#[test]
fn a_blinker_of_mines_pays_because_it_is_compact() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();

    // Three in a row, which flips end over end forever. Clear of the
    // grant's own block, which sits in the middle of the patch.
    place_mines(&mut s, me, &[(1, 1), (2, 1), (3, 1)]);
    s.step();
    // Measured from after the cost, which `handle` charges on receipt.
    let purse = s.value_of(me).unwrap();

    for _ in 0..20 {
        s.step();
    }
    assert!(
        s.value_of(me).unwrap() > purse,
        "two births a generation against two corpses charged one time in \
         eight should pay: {purse} -> {}",
        s.value_of(me).unwrap()
    );
}

/// And a mess does not pay, which is the point of charging for corpses.
///
/// An r-pentomino of factories grows into a couple of hundred live cells
/// dragging eight hundred corpses behind it. Every one of those is charged
/// one generation in eight, so sprawl costs far more than its own births
/// bring in — measured at about twenty a generation against it. Without
/// the upkeep it was the best investment in the game.
#[test]
fn sprawling_mines_cost_more_than_they_earn() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    place_mines(&mut s, me, &[(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)]);
    s.step();

    // Given plenty to spend, so the floor at zero does not hide the drain.
    s.players.get_mut(&me).unwrap().value = 100_000;
    let purse = s.value_of(me).unwrap();
    for _ in 0..300 {
        s.step();
    }
    assert!(
        s.value_of(me).unwrap() < purse,
        "sprawl should bleed: {purse} -> {}",
        s.value_of(me).unwrap()
    );
}

/// Nothing dies on a still life, so nothing is charged. A block of factories
/// is free to hold and earns nothing, which is the honest answer for
/// something that never does anything.
#[test]
fn a_block_of_mines_costs_nothing_to_hold() {
    let mut s = Server::new(World::infinite_empty());
    let me = s.join("me").unwrap();
    place_mines(&mut s, me, &[(1, 1), (1, 2), (2, 1), (2, 2)]);
    s.step();
    let purse = s.value_of(me).unwrap();
    for _ in 0..50 {
        s.step();
    }
    assert_eq!(s.value_of(me).unwrap(), purse, "no births and no corpses");
}

/// Lay factories at offsets inside this player's granted ground, and apply
/// them, without advancing the world.
fn place_mines(s: &mut Server, id: PlayerId, offsets: &[(i32, i32)]) {
    let tick = s.tick();
    s.handle(
        Some(id),
        None,
        ClientMessage::Act(Stamped {
            tick,
            player: id,
            seat: id,
            action: Action::Paint { cells: mine(id, offsets), placement: Placement::Factory },
        }),
    );
    // `handle` queues; `step` is what applies. Stepping once here would
    // also advance the world, so the pending action is drained by the
    // caller's own first step.
}

/// Step until a `Step` carries an action from this seat, and say what
/// number its cells carried; `None` if it never moved in `within` steps.
fn first_act_of(s: &mut Server, seat: PlayerId, within: usize) -> Option<PlayerId> {
    for _ in 0..within {
        for out in s.step() {
            if let ServerMessage::Step { actions, .. } = out {
                if let Some(a) = actions.iter().find(|a| a.seat == seat) {
                    return Some(a.player);
                }
            }
        }
    }
    None
}

/// **A bot is a seat the server plays.** It takes a number like anybody,
/// acts on its own within its cadence, and what it laid is priced against
/// its purse and stands in the world under its number.
#[test]
fn a_bot_takes_a_seat_and_acts_within_its_cadence() {
    let mut s = Server::new(World::infinite_empty());
    let bot = s.add_bot("easy bot", Level::Easy, Driver::Book, None).unwrap();
    assert!(s.is_bot(bot));
    assert!(s.players().any(|p| p.id == bot && p.online), "a bot is a player here");
    let purse = s.value_of(bot).unwrap();

    // Three cadences, because the dice may find nowhere to build once.
    assert!(first_act_of(&mut s, bot, 3 * 16).is_some(), "an easy bot never acted");
    assert!(s.value_of(bot).unwrap() < purse, "its factories were free");
    let standing = s
        .world()
        .live_cells()
        .iter()
        .filter(|&&(r, c)| {
            let cell = s.world().cell_at(r, c).unwrap();
            cell.player() == bot && cell.kind() == crate::sim::Kind::FACTORY
        })
        .count();
    assert!(standing > 0, "nothing of the bot's is standing");
}

/// **A bot's move rides the `Step` and nothing else.** An `Acted` for it
/// would leave with the next thing anybody said, after the `Step` that
/// already carried it, and a paint laid a generation late is a different
/// paint.
#[test]
fn a_bots_action_is_never_announced() {
    let mut s = Server::new(World::infinite_empty());
    let bot = s.add_bot("bot", Level::Hard, Driver::Book, None).unwrap();
    assert!(first_act_of(&mut s, bot, 3 * 4).is_some(), "the bot never acted");
    assert!(
        s.take_announcements().iter().all(|m| !matches!(m, ServerMessage::Acted(_))),
        "a bot's action was announced"
    );
}

/// **A peer built from nothing but the `Step`s is the server's world**,
/// generation for generation, with a bot in the room: what it chose
/// reaches a client as ordinary actions and nothing else, so nothing a
/// client does not hear can move the server's copy.
#[test]
fn a_peer_built_from_steps_agrees_with_the_server_with_a_bot_in_the_room() {
    let mut s = Server::new(World::infinite_empty());
    let bot = s.add_bot("hard bot", Level::Hard, Driver::Book, None).unwrap();
    s.credit(bot, 5_000);
    let mut peer = s.world().clone();
    let mut acted = 0;
    for _ in 0..200 {
        for out in s.step() {
            if let ServerMessage::Step { tick, actions } = out {
                acted += actions.len();
                for stamped in &actions {
                    crate::net::apply(&mut peer, stamped);
                }
                while peer.generation < tick {
                    peer.step();
                }
            }
        }
        for (coord, _) in s.world().stored() {
            assert_eq!(
                peer.chunk_digest(coord),
                s.world().chunk_digest(coord),
                "chunk {coord:?} differs at tick {}",
                s.tick()
            );
        }
    }
    assert!(acted >= 10, "the bot acted {acted} times in 200 generations");
}

/// On a side it plays as the side's number, so its cells and its purse
/// are the team's — the same rule as a person at the team's controls.
#[test]
fn a_bot_on_a_team_plays_as_the_team() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(Victory::Timer { generations: 1000 });
    s.make_teams(2).unwrap();
    let me = s.join_with("me", None).unwrap();
    s.join_team(me, PlayerId(1)).unwrap();
    let bot = s.add_bot("hard bot", Level::Hard, Driver::Book, Some(PlayerId(2))).unwrap();
    assert_eq!(s.plays_as(bot), PlayerId(2));
    let why = s.add_bot("x", Level::Easy, Driver::Book, Some(PlayerId(9))).unwrap_err();
    assert!(why.contains("teams"), "a side this match does not have: {why}");
    assert_eq!(s.player_count(), 4, "a refused side cost a number");

    s.start_match(None).unwrap();
    // No hand-out. A match opens every purse at nought, and a bot that needed
    // one to move was this test standing in for the fault below.
    assert_eq!(first_act_of(&mut s, bot, 8 * 4), Some(PlayerId(2)), "not the team's number");
}

/// **In a world a removed bot's seat stays spent**, as a person's who left
/// does, because its ground carries its number; so a room with fifteen
/// seats taken refuses a person with the words it refuses a sixteenth
/// person with, and one removed bot, me and thirteen more is fifteen.
#[test]
fn a_removed_bots_seat_stays_spent_in_a_world_and_a_room_of_bots_is_full() {
    let mut s = Server::new(World::infinite_empty());
    let bot = s.add_bot("bot", Level::Normal, Driver::Book, None).unwrap();
    assert!(s.remove_bot(PlayerId(9)).is_err(), "nobody is in seat 9");
    let me = s.join("me").unwrap();
    assert!(s.remove_bot(me).is_err(), "a person is not a bot");
    s.remove_bot(bot).unwrap();
    assert!(!s.is_bot(bot));
    assert!(!s.players().find(|p| p.id == bot).unwrap().online, "the seat is still taken");
    let ServerMessage::Match(lobby) = s.lobby() else { panic!("not a lobby") };
    assert!(lobby.players.iter().all(|p| p.id != bot), "a removed bot is still listed");

    for i in 0..PlayerId::MAX as usize - 2 {
        s.add_bot(format!("bot {i}"), Level::Easy, Driver::Book, None).unwrap();
    }
    let why = s.join("late").unwrap_err();
    assert!(why.contains("full"), "{why}");
    let why = s.add_bot("one more", Level::Easy, Driver::Book, None).unwrap_err();
    assert!(why.contains("full"), "{why}");
}

/// **A bot taken out of a lobby gives its number back.** Nothing is laid
/// out before the whistle, so the number is in no cell -- and a seat that
/// stayed spent let anybody seated lock a room to newcomers with fifteen
/// presses of add-and-remove from one connection.
#[test]
fn a_bot_removed_while_gathering_gives_its_number_back() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    s.make_teams(2).unwrap();
    let me = s.join_with("me", None).unwrap();
    let bot = s.add_bot("bot", Level::Normal, Driver::Book, Some(PlayerId(2))).unwrap();
    s.remove_bot(bot).unwrap();
    assert!(!s.is_bot(bot));
    assert!(s.players().all(|p| p.id != bot), "the row stayed behind");
    for _ in 0..2 * PlayerId::MAX as usize {
        let again = s.add_bot("bot", Level::Normal, Driver::Book, Some(PlayerId(2))).unwrap();
        assert_eq!(again, bot, "the number was not given back");
        s.remove_bot(again).unwrap();
    }
    assert_eq!(s.player_count(), 3, "two sides and me");
    let ServerMessage::Match(lobby) = s.lobby() else { panic!("not a lobby") };
    assert_eq!(lobby.players.iter().map(|p| p.id).collect::<Vec<_>>(), [me]);
    assert_eq!(s.join_with("late", None).unwrap(), bot, "the seat went to nobody");
}

/// **Never before the whistle and never after the end.** A gathering
/// match holds still and a decided one has stopped; a bot in either does
/// nothing, and neither admits or releases one once it is running.
#[test]
fn a_bot_does_nothing_before_the_whistle_or_after_the_end() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(Victory::Timer { generations: 2 });
    s.join_with("me", None).unwrap();
    let bot = s.add_bot("bot", Level::Hard, Driver::Book, None).unwrap();
    for _ in 0..20 {
        let out = s.step();
        assert!(out.iter().all(|m| !matches!(m, ServerMessage::Step { .. })), "gathering stepped");
    }
    assert!(s.pending.is_empty() && s.announce.is_empty(), "a bot acted before the whistle");
    assert!(s.add_bot("second", Level::Easy, Driver::Book, None).is_ok(), "gathering admits");

    s.start_match(None).unwrap();
    s.credit(bot, 500);
    assert!(s.add_bot("late", Level::Easy, Driver::Book, None).is_err(), "late joining");
    assert!(s.remove_bot(bot).is_err(), "a seat leaving mid-match is a forfeit");
    for _ in 0..2 {
        s.step();
    }
    assert!(matches!(s.phase(), Phase::Over { .. }));
    s.take_announcements();
    for _ in 0..20 {
        let out = s.step();
        assert!(out.iter().all(|m| !matches!(m, ServerMessage::Step { .. })), "over stepped");
    }
    assert!(s.announce.is_empty() && s.pending.is_empty(), "a bot acted after the end");
}

/// **An external seat is judged exactly as a client is**: its action goes
/// through `act`, is refused off its ground and beyond its purse, and is
/// taken otherwise. The server never moves for it.
#[test]
fn an_external_seat_is_priced_like_anybody() {
    let mut s = Server::new(World::infinite_empty());
    let engine = s.add_bot("engine", Level::Normal, Driver::External, None).unwrap();
    let paint = |cells: Vec<(i32, i32)>, placement| Stamped {
        tick: 0,
        player: engine,
        seat: engine,
        action: Action::Paint { cells, placement },
    };

    let far = vec![(9_000, 9_000), (9_000, 9_001), (9_001, 9_000), (9_001, 9_001)];
    assert_eq!(s.act(paint(far, Placement::Life)), Err("nothing of yours reaches there"));

    // A pane over the whole patch: more than the purse holds.
    let patch: Vec<(i32, i32)> = (0..crate::net::SPAWN_N)
        .flat_map(|r| (0..crate::net::SPAWN_N).map(move |c| (r, c)))
        .collect();
    assert_eq!(s.act(paint(mine(engine, &patch), Placement::Ice)), Err("you cannot afford that"));

    let purse = s.value_of(engine).unwrap();
    assert_eq!(s.act(paint(mine(engine, &[(2, 2), (2, 3), (2, 4)]), Placement::Factory)), Ok(()));
    assert_eq!(s.value_of(engine), Some(purse - 3 * crate::net::FACTORY_COST));
    assert_eq!(first_act_of(&mut s, engine, 1), Some(engine), "the posted action was not applied");
    assert!(first_act_of(&mut s, engine, 40).is_none(), "the server moved for an engine");
    let ServerMessage::Match(lobby) = s.lobby() else { panic!("not a lobby") };
    assert!(lobby.players.iter().any(|p| p.id == engine && p.bot), "an engine is a bot seat");
}

/// **From the wire, a bot is a seated player's to add and take away**, and
/// nobody else's: a spectator is dropped, a refusal is answered into the
/// lobby rather than closing the door, and the next lobby says who is a
/// bot.
#[test]
fn a_seated_player_adds_and_removes_bots_from_the_lobby() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(Victory::Timer { generations: 100 });
    let me = s.join_with("me", None).unwrap();
    let add = |team| ClientMessage::AddBot { team, level: Level::Normal };

    assert!(s.handle(None, None, add(None)).is_empty(), "a spectator was answered");
    assert_eq!(s.player_count(), 1, "a spectator seated a bot");

    assert!(s.handle(Some(me), None, add(None)).is_empty(), "a good press is not answered");
    let ServerMessage::Match(lobby) = s.lobby() else { panic!("not a lobby") };
    let bot = lobby.players.iter().find(|p| p.bot).expect("no bot in the lobby");
    assert!(!lobby.players.iter().find(|p| p.id == me).unwrap().bot, "a person marked as a bot");

    let refused = s.handle(Some(me), None, add(Some(PlayerId(3))));
    assert!(matches!(&refused[..], [ServerMessage::NotStarted { .. }]), "{refused:?}");

    let seat = bot.id;
    s.start_match(None).unwrap();
    let refused = s.handle(Some(me), None, ClientMessage::RemoveBot { seat });
    assert!(matches!(&refused[..], [ServerMessage::NotStarted { .. }]), "{refused:?}");
    assert!(s.is_bot(seat), "a running match let a bot go");
}

/// **A block heals, so one cell a generation is a coin that never runs out.**
/// A match hands out no value, and what makes nought recoverable rather than
/// stuck is that a grant is a block: take a corner out and the three that are
/// left give the empty corner exactly three neighbours, so it is born again on
/// the next generation. Mine one, wait one, mine again.
#[test]
fn a_mined_block_heals_itself_the_next_generation() {
    let me = PlayerId(1);
    let mut world = World::infinite_empty();
    crate::net::grant(&mut world, me);
    let mine = |w: &World| {
        w.live_cells().into_iter().find(|&(r, c)| {
            w.cell_at(r, c).is_some_and(|cell| cell.is_alive() && cell.player() == me)
        })
    };
    let before = world.live_cells().len();
    let at = mine(&world).expect("a grant stands somebody up");
    world.set_cell_at(at.0, at.1, crate::sim::Cell::DEAD.with_player(me));
    assert_eq!(world.live_cells().len(), before - 1, "one taken");
    world.step();
    assert_eq!(world.live_cells().len(), before, "the block did not heal");
}

/// **A bot that runs dry mines rather than stopping**, which is what a person
/// does with a purse at nought; and one with money never pulls up its own
/// cells, because mining is what happens when nothing can be afforded rather
/// than one move among the others.
#[test]
fn a_bot_mines_only_when_it_can_afford_nothing() {
    use crate::server::bot::Bot;
    let mut world = World::infinite_empty();
    let me = PlayerId(1);
    crate::net::grant(&mut world, me);
    let rules = crate::net::Rules::default();

    let mut rich = Bot::new(Level::Hard, Driver::Book, 7, me, 0);
    let mined = (0..200)
        .filter_map(|tick| rich.choose(&world, &rules, me, 1_000, tick))
        .filter(|a| matches!(a, crate::net::Action::Erase { .. }))
        .count();
    assert_eq!(mined, 0, "a bot with a purse took its own cells back {mined} times");

    let mut broke = Bot::new(Level::Hard, Driver::Book, 7, me, 0);
    let took: Vec<_> = (0..200)
        .filter_map(|tick| broke.choose(&world, &rules, me, 0, tick))
        .filter_map(|a| match a {
            crate::net::Action::Erase { cells, .. } => Some(cells.len()),
            _ => None,
        })
        .collect();
    assert!(!took.is_empty(), "a bot with nothing never tried to mine");
    assert!(took.iter().all(|&n| n == 1), "it took more than the block can heal: {took:?}");
}

/// **And a broke bot in a match digs itself out.** One coin a generation from
/// a block that heals is enough to reach a factory, which is what turns the
/// opening from a grind into a game.
#[test]
fn a_broke_bot_in_a_match_mines_its_way_to_something_standing() {
    let mut s = Server::named("arena", World::infinite_empty());
    s.make_match(Victory::Timer { generations: 4000 });
    let bot = s.add_bot("hard bot", Level::Hard, Driver::Book, None).unwrap();
    assert_eq!(s.value_of(bot), Some(0), "a match opens broke, which is the premise");
    s.start_match(None).unwrap();
    for _ in 0..600 {
        s.step();
    }
    assert!(s.value_of(bot).unwrap() > 0, "it never earned a coin");
    let standing = s
        .world()
        .live_cells()
        .iter()
        .filter(|&&(r, c)| s.world().cell_at(r, c).is_some_and(|cell| cell.player() == bot))
        .count();
    assert!(standing > 0, "it mined itself down to nothing");
}
