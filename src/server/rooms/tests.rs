//! Tests for the several worlds behind one address.
//!
//! In their own file for the reason [`crate::sim::rule`]'s are: what a room
//! does with a message is worth being able to read without scrolling past
//! twelve hundred lines of assertion to find the next method.

use super::*;

/// A whole generation's worth of time, so one call to `Rooms::step` is one
/// generation whatever rate a room is set to — see `Server::owe`.
fn a_generation() -> std::time::Duration {
    std::time::Duration::from_secs_f32(crate::net::Rules::default().generation_span())
}
use crate::sim::World;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ck-rooms-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_declared_room_exists_and_an_undeclared_one_does_not() {
    let rooms = Rooms::open(
        temp_dir("declared"),
        &["lobby".into(), "arena".into()],
        WorldKind::Infinite,
        true,
    )
    .unwrap();

    assert_eq!(rooms.names().collect::<Vec<_>>(), ["arena", "lobby"]);
    assert_eq!(rooms.default_room().as_str(), "lobby", "the first declared is the default");
    assert_eq!(rooms.resolve(None).unwrap().as_str(), "lobby");
    assert_eq!(rooms.resolve(Some("ARENA")).unwrap().as_str(), "arena", "names fold to lowercase");

    // A typo is refused, and the refusal says what is actually here --
    // which, with no menu yet, is the only way a player finds out.
    let why = rooms.resolve(Some("loby")).unwrap_err();
    assert!(why.contains("arena") && why.contains("lobby"), "{why}");
}

#[test]
fn a_name_that_could_escape_the_directory_is_not_a_room() {
    let rooms = Rooms::open(temp_dir("escape"), &[], WorldKind::Infinite, true).unwrap();
    for bad in ["../elsewhere", "a/b", "", "with space", &"x".repeat(64)] {
        assert!(rooms.resolve(Some(bad)).is_err(), "{bad:?} should not be a room");
    }
}

/// The whole point of separate worlds: what happens in one is not visible
/// in the other, and a player number means nothing without its room.
#[test]
fn two_rooms_are_two_worlds() {
    let mut rooms =
        Rooms::open(temp_dir("two"), &["a".into(), "b".into()], WorldKind::Infinite, true).unwrap();

    let a = rooms.get_mut(&RoomId::from("a")).unwrap().join("alice").unwrap();
    let b = rooms.get_mut(&RoomId::from("b")).unwrap().join("bob").unwrap();
    assert_eq!((a, b), (PlayerId(1), PlayerId(1)), "numbers are per room");

    assert_eq!(rooms.get(&RoomId::from("a")).unwrap().player_count(), 1);
    assert_eq!(rooms.get(&RoomId::from("b")).unwrap().player_count(), 1);

    // Alice's ground is in her world and nowhere else. Both players hold
    // number one, so a shared world would have them standing on it
    // together and this would pass for the wrong reason -- hence the
    // second player's own room being checked for emptiness too.
    rooms.get_mut(&RoomId::from("a")).unwrap().step();
    let (row, col) = crate::net::spawn_for(a, rooms.get(&RoomId::from("a")).unwrap().world());
    assert!(rooms.get(&RoomId::from("a")).unwrap().world().cell_at(row, col).is_some());
    assert_eq!(
        rooms.get(&RoomId::from("b")).unwrap().world().generation,
        0,
        "stepping one room does not step the other"
    );
}

#[test]
fn every_room_steps_and_says_which_it_was() {
    let mut rooms =
        Rooms::open(temp_dir("step"), &["a".into(), "b".into()], WorldKind::Infinite, true)
            .unwrap();
    let stepped = rooms.step(a_generation());
    let names: Vec<&str> = stepped.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(names, ["a", "b"], "one Step per room, each labelled");
    assert_eq!(rooms.get(&RoomId::from("a")).unwrap().tick(), 1);
    assert_eq!(rooms.get(&RoomId::from("b")).unwrap().tick(), 1);
}

/// A room is a file, so a restart finds it without being told again.
#[test]
fn a_saved_room_comes_back_without_being_declared() {
    let dir = temp_dir("saved");
    {
        let mut rooms = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
        rooms.get_mut(&RoomId::from("kept")).unwrap().join("alice").unwrap();
        rooms.step(a_generation());
        rooms.save().unwrap();
    }

    let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    assert!(back.get(&RoomId::from("kept")).is_some(), "the file is the declaration");
    assert_eq!(back.get(&RoomId::from("kept")).unwrap().tick(), 1, "and it kept its tick");
    assert!(
        back.get(&RoomId::from(DEFAULT_ROOM)).is_some(),
        "a server always has somewhere to put a client that named no room"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--fresh` is for starting over, so it must not leave one room's save
/// standing while the rest begin again.
#[test]
fn fresh_ignores_every_room_on_disk() {
    let dir = temp_dir("fresh");
    {
        let mut rooms = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
        rooms.step(a_generation());
        rooms.save().unwrap();
    }
    let back = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
    assert_eq!(back.get(&RoomId::from("kept")).unwrap().tick(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `Join` carries its own room and a `Hello` names no room at all, so
/// neither needs a seat and both are answered; anything else from a
/// connection that has not joined names no world and is dropped rather
/// than answered out of the default room.
#[test]
fn a_message_from_nobody_is_answered_only_if_it_names_no_world() {
    let mut rooms =
        Rooms::open(temp_dir("route"), &["hall".into()], WorldKind::Infinite, true).unwrap();

    let replies = rooms.handle(
        &Caller::nobody(),
        ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: None,
        },
    );
    let [ServerMessage::Welcome { room, world, .. }] = &replies[..] else {
        panic!("expected a welcome, got {replies:?}");
    };
    assert_eq!(room.as_str(), "hall", "the welcome names the room it let you into");
    assert_eq!(*world, WorldKind::Infinite);

    assert!(
        rooms
            .handle(&Caller::nobody(), ClientMessage::Subscribe { chunks: vec![(0, 0)] })
            .is_empty(),
        "an unjoined connection may not read a world"
    );

    // And a `Hello`, which names no world either: it says who is asking,
    // and is answered with who this server takes them to be.
    let replies = rooms.handle(
        &Caller::nobody(),
        ClientMessage::Hello { name: "alice".into(), person: Secret::new().unwrap() },
    );
    let [ServerMessage::You(profile)] = &replies[..] else {
        panic!("expected to be told who we are, got {replies:?}");
    };
    assert_eq!(profile.name, "alice", "the name rode with the hello");
    assert!(rooms.people.knows(&profile.who), "a hello is a meeting");
}

/// **A person on the menu is somebody.** A `Hello` names a person with no
/// seat, and whatever was waiting for them rides out with the answer rather
/// than with the next room list — so a challenge reaches a client that has
/// opened the page and joined nothing.
#[test]
fn a_hello_names_a_person_and_hands_over_what_is_waiting() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (a, b) = (Secret::new().unwrap(), Secret::new().unwrap());
    let hello = |s: &Secret| ClientMessage::Hello { name: "somebody".into(), person: s.clone() };

    // Both met by saying hello and nothing else: no room was ever joined.
    let out = rooms.handle(&Caller::new(1), hello(&a));
    let [ServerMessage::You(a_profile)] = &out[..] else { panic!("{out:?}") };
    let out = rooms.handle(&Caller::new(2), hello(&b));
    let [ServerMessage::You(b_profile)] = &out[..] else { panic!("{out:?}") };
    let (a_id, b_id) = (a_profile.who.clone(), b_profile.who.clone());
    assert_ne!(a_id, b_id);

    // The same secret is the same person, said twice.
    let out = rooms.handle(&Caller::new(3), hello(&a));
    let [ServerMessage::You(again)] = &out[..] else { panic!("{out:?}") };
    assert_eq!(again.who, a_id, "a second hello renamed somebody");

    rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id.clone() });

    // A fresh socket for b, which has said nothing yet: the hello is the
    // first word, and the challenge comes back with it.
    let out = rooms.handle(&Caller::new(4), hello(&b));
    assert!(matches!(&out[..], [ServerMessage::You(_), ..]), "{out:?}");
    let told = out.iter().find_map(|m| match m {
        ServerMessage::Challenged { from, .. } => Some(from.who.clone()),
        _ => None,
    });
    assert_eq!(told, Some(a_id), "the challenge did not ride out with the hello: {out:?}");
}

/// The menu's whole reason for being able to show anything. Asked before
/// joining, because a room is a world and picking one after you are in it
/// is picking too late.
#[test]
fn the_rooms_can_be_listed_without_joining_one() {
    let mut rooms = Rooms::open(
        temp_dir("listing"),
        &["lobby".into(), "arena".into()],
        WorldKind::Toroidal { rows: 4, cols: 4 },
        true,
    )
    .unwrap();
    rooms.get_mut(&RoomId::from("arena")).unwrap().join("alice").unwrap();

    let replies = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
    let [ServerMessage::Rooms { rooms: listed, .. }] = &replies[..] else {
        panic!("expected a listing, got {replies:?}");
    };
    assert_eq!(
        listed.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["arena", "lobby"],
        "in one order, so the buttons do not move under the pointer"
    );
    assert_eq!(listed[0].players, 1, "connected now");
    assert_eq!(listed[1].players, 0);
    assert_eq!(listed[0].world, WorldKind::Toroidal { rows: 4, cols: 4 });

    // A player who left is not a player who is there.
    rooms.get_mut(&RoomId::from("arena")).unwrap().leave(PlayerId(1));
    let [ServerMessage::Rooms { rooms: listed, .. }] =
        &rooms.handle(&Caller::nobody(), ClientMessage::Rooms)[..]
    else {
        panic!("expected a listing");
    };
    assert_eq!(listed[0].players, 0);
}

/// Joining twice on one connection is a room change, not a second player
/// left standing in the first room — where, being marked online, they
/// could never be returned to by their token.
#[test]
fn joining_again_leaves_the_room_it_came_from() {
    let mut rooms =
        Rooms::open(temp_dir("move"), &["a".into(), "b".into()], WorldKind::Infinite, true)
            .unwrap();

    let join = |room: &str| ClientMessage::Join {
        name: "alice".into(),
        room: Some(room.into()),
        person: None,
    };

    let replies = rooms.handle(&Caller::nobody(), join("a"));
    let [ServerMessage::Welcome { you, .. }] = &replies[..] else {
        panic!("expected a welcome, got {replies:?}");
    };
    let seat: Seat = ("a".into(), *you);

    rooms.handle(&Caller::sitting(1, seat.clone()), join("b"));
    assert!(
        !rooms.get(&RoomId::from("a")).unwrap().players().any(|p| p.online),
        "nobody is left standing in the room she left"
    );
    assert!(rooms.get(&RoomId::from("b")).unwrap().players().any(|p| p.online));

    // A refused change leaves her where she was. Her client learns where
    // it is from the Welcome it will not get, so anything else would have
    // the two disagreeing about which world she is in.
    let seat: Seat = ("b".into(), PlayerId(1));
    let replies = rooms.handle(&Caller::sitting(1, seat.clone()), join("nowhere"));
    assert!(matches!(replies[..], [ServerMessage::Rejected { .. }]));
    assert!(
        rooms.get(&RoomId::from("b")).unwrap().players().any(|p| p.online),
        "a refused join must not empty the room she was in"
    );
}

/// The shape of the world reaches the client, which is the only way it can
/// build one that folds at the same place the server's does.
#[test]
fn a_welcome_from_a_wrapping_room_says_so() {
    let mut rooms = Rooms::just(Server::named("ring", World::toroidal_empty(4, 6)));
    let replies = rooms.handle(
        &Caller::nobody(),
        ClientMessage::Join { name: "alice".into(), room: None, person: None },
    );
    let [ServerMessage::Welcome { world, .. }] = &replies[..] else {
        panic!("expected a welcome, got {replies:?}");
    };
    assert_eq!(*world, WorldKind::Toroidal { rows: 4, cols: 6 });
}

/// The whole of client-made rooms, in one exchange: ask, get a name back,
/// join that name. Making does not seat you, so the second half is the
/// same `Join` the room list sends.
#[test]
fn a_client_can_make_a_room_and_then_join_it() {
    let mut rooms =
        Rooms::open(temp_dir("made"), &["hall".into()], WorldKind::Infinite, true).unwrap();
    let me = Caller::new(7);

    let replies = rooms.handle(
        &me,
        ClientMessage::Create {
            // Typed with a capital and a space around it, because that is
            // what a text field hands you and the name that comes back is
            // the one that has to be joined.
            name: "  Arena  ".into(),
            shape: WorldKind::Toroidal { rows: 4, cols: 6 },
            victory: None,
            teams: None,
            private: false,
            laboratory: false,
            party: None,
        },
    );
    let [ServerMessage::Made(Ok(made))] = &replies[..] else {
        panic!("expected a room, got {replies:?}");
    };
    assert_eq!(made.name, "arena", "trimmed and lowercased, the way it will be shown");
    assert_eq!(made.code, None, "an open room needs no code");
    assert_eq!(rooms.made_by(&made.id), Some(7), "and it remembers who asked");

    let made_id = made.id.clone();
    let replies = rooms.handle(
        &me,
        ClientMessage::Join { name: "alice".into(), room: Some(made_id.clone()), person: None },
    );
    let [ServerMessage::Welcome { room, name, world, .. }] = &replies[..] else {
        panic!("expected a welcome, got {replies:?}");
    };
    assert_eq!(*room, made_id, "joined by the id it was given");
    assert_eq!(name, "arena", "and told what it is called");
    assert_eq!(*world, WorldKind::Toroidal { rows: 4, cols: 6 }, "the shape it asked for");
}

/// A win condition is the whole of the difference between a world and a
/// match, so one message makes either.
#[test]
fn a_victory_makes_a_match_and_no_victory_makes_a_world() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let me = Caller::new(1);

    rooms.make(1, "plain", WorldKind::Infinite, None, None, Reach::Listed, false).unwrap();
    rooms
        .make(
            1,
            "cup",
            WorldKind::Infinite,
            Some(Victory::Territory { squares: 500 }),
            None,
            Reach::Listed,
            false,
        )
        .unwrap();

    let [ServerMessage::Rooms { rooms: listed, .. }] = &rooms.handle(&me, ClientMessage::Rooms)[..]
    else {
        panic!("expected a listing");
    };
    let find = |name: &str| listed.iter().find(|r| r.name == name).expect(name).clone();
    assert_eq!(find("plain").phase, Phase::Open, "a world is open and stays open");
    assert_eq!(find("plain").victory, None);
    assert_eq!(find("cup").phase, Phase::Gathering, "a match waits for a whistle");
    assert_eq!(find("cup").victory, Some(Victory::Territory { squares: 500 }));
}

/// A name already taken is refused in the client's own words, because "there
/// is already a room called that" is the common failure and the one a player
/// can act on.
#[test]
fn a_name_that_is_taken_is_refused_and_nothing_is_made() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let before = rooms.len();

    let replies = rooms.handle(
        &Caller::new(2),
        ClientMessage::Create {
            name: "hall".into(),
            shape: WorldKind::Infinite,
            victory: None,
            teams: None,
            private: false,
            laboratory: false,
            party: None,
        },
    );
    let [ServerMessage::Made(Err(why))] = &replies[..] else {
        panic!("expected a refusal, got {replies:?}");
    };
    assert!(why.contains("hall"), "the refusal names the room: {why}");
    assert_eq!(rooms.len(), before, "and nothing was made");
    assert_eq!(rooms.made_by(&RoomId::from("hall")), None, "an existing room gets no owner");
}

/// The backstop. A server anybody can fill is a server that steps a
/// simulation four times a second for nobody, once per room, forever.
#[test]
fn the_cap_is_on_rooms_players_made_and_not_on_the_operators() {
    let mut rooms = Rooms::open(
        temp_dir("cap"),
        &["one".into(), "two".into(), "three".into()],
        WorldKind::Infinite,
        true,
    )
    .unwrap();
    rooms.cap_made(2);
    assert_eq!(rooms.len(), 3, "three declared, none of them counted");

    assert!(rooms.make(1, "a", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
    assert!(rooms.make(1, "b", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
    let (made, cap) = rooms.made_count();
    assert_eq!((made, cap), (2, 2));

    let refused =
        rooms.make(1, "c", WorldKind::Infinite, None, None, Reach::Listed, false).unwrap_err();
    assert!(refused.contains('2'), "the refusal says how many: {refused}");
    assert!(rooms.get(&RoomId::from("c")).is_none(), "and made none");

    // Deleting one frees a slot, or a server that had made and deleted its
    // cap's worth would refuse for ever while holding nothing.
    rooms.delete("a").unwrap();
    assert_eq!(rooms.made_count().0, 1);
    assert!(rooms.make(1, "c", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
}

/// **A bot is not somebody standing in a room.** Deleting is refused
/// while anybody is in it, because it cannot be taken back; a bot goes
/// with the room, and a person does not.
#[test]
fn a_room_of_bots_can_be_deleted_and_a_room_with_a_person_in_it_cannot() {
    use crate::net::Level;
    use crate::server::bot::Driver;
    let mut rooms =
        Rooms::open(temp_dir("bots-in"), &["hall".into()], WorldKind::Infinite, true).unwrap();
    let timer = Victory::Timer { generations: 10 };

    let dawn = rooms.new_match("dawn", WorldKind::Infinite, timer).unwrap();
    rooms.get_mut(&dawn).unwrap().add_bot("bot", Level::Hard, Driver::Book, None).unwrap();
    rooms.delete("dawn").unwrap();
    assert!(rooms.get(&dawn).is_none(), "a room holding only a bot was kept");

    let dusk = rooms.new_match("dusk", WorldKind::Infinite, timer).unwrap();
    let room = rooms.get_mut(&dusk).unwrap();
    room.add_bot("bot", Level::Hard, Driver::Book, None).unwrap();
    room.join_with("me", None).unwrap();
    let why = rooms.delete("dusk").unwrap_err();
    assert!(why.starts_with("1 still in"), "the bot was counted, or the person was not: {why}");
    assert!(rooms.get(&dusk).is_some());
}

/// A private room is reachable by its code and mentioned nowhere else —
/// including in the refusal a mistyped name gets back, which used to name
/// every room on the server and would have handed out every code.
#[test]
fn a_private_room_is_reachable_by_code_and_named_nowhere() {
    let mut rooms =
        Rooms::open(temp_dir("private"), &["hall".into()], WorldKind::Infinite, true).unwrap();

    let made =
        rooms.make(3, "friends-only", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
    let code = made.code.clone().expect("a private room gets a code");
    assert_eq!(code.len(), CODE_LEN);
    assert_ne!(code, made.id.as_str(), "a code is a credential, not an identity");
    assert_eq!(made.name, "friends-only", "a private room keeps the name it was given");
    assert_ne!(code, made.name, "and the code is not that name");
    assert!(rooms.is_unlisted(&made.id));

    let listing = rooms.listing();
    let listed: Vec<&str> = listing.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(listed, ["hall"], "the listing does not mention it");

    // The code still joins, which is the whole point of having one.
    assert_eq!(rooms.resolve(Some(&code)).unwrap(), made.id);
    assert_eq!(rooms.resolve(Some(made.id.as_str())).unwrap(), made.id);

    // And a wrong name is refused without naming it.
    let refused = rooms.resolve(Some("nowhere")).unwrap_err();
    assert!(refused.contains("hall"));
    assert!(!refused.contains(&code), "the refusal leaked a code: {refused}");
    assert!(!refused.contains(made.id.as_str()), "or an id: {refused}");
}

/// **A restart keeps what a client-made room was.** The world came back
/// from its file already; the code, the unlisting and the owner did not,
/// so a private world reopened listed, codeless and nobody's.
#[test]
fn a_restart_keeps_a_private_rooms_code_unlisting_and_owner() {
    let dir = temp_dir("meta");
    let key = Secret::new().unwrap();
    let (id, code, who) = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let out = rooms.handle(
            &Caller::new(1),
            ClientMessage::Hello { name: "maker".into(), person: key.clone() },
        );
        let [ServerMessage::You(profile)] = &out[..] else { panic!("{out:?}") };
        let who = profile.who.clone();
        let out = rooms.handle(
            &Caller::known(1, who.clone()),
            ClientMessage::Create {
                name: "den".into(),
                shape: WorldKind::Infinite,
                victory: None,
                teams: None,
                private: true,
                laboratory: false,
                party: None,
            },
        );
        let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
        // Owned at `Create`, before anybody has joined: the hello named
        // the maker, so there was somebody to record.
        assert_eq!(rooms.owned_by(&made.id), Some(&who), "a keyed maker owns it at once");
        rooms.save().unwrap();
        (made.id.clone(), made.code.clone().expect("a code"), who)
    };

    let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    assert!(back.get(&id).is_some(), "the world did not come back");
    assert!(back.is_unlisted(&id), "it came back listed");
    assert_eq!(back.code_of(&id), Some(code.as_str()), "it came back codeless");
    assert_eq!(back.owned_by(&id), Some(&who), "it came back nobody's");
    assert_eq!(back.made_count().0, 1, "it came back outside the cap");
    assert_eq!(back.made_by(&id), None, "a connection outlived the process");
    assert_eq!(back.resolve(Some(&code)).unwrap(), id, "the code stopped working");
    let refused = back.resolve(Some("nowhere")).unwrap_err();
    assert!(!refused.contains(&code) && !refused.contains(id.as_str()), "{refused}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A keyless maker's room keeps its code and its unlisting and has no
/// owner to keep, because a seat means nothing after a restart — and a
/// room deleted before the restart leaves no row to count against the cap.
#[test]
fn a_seat_owner_is_not_saved_and_a_deleted_room_leaves_no_row() {
    let dir = temp_dir("meta-seat");
    let id = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let made =
            rooms.make(4, "den", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
        let out = rooms.handle(
            &Caller::new(4),
            ClientMessage::Join { name: "maker".into(), room: Some(made.id.clone()), person: None },
        );
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        assert_eq!(rooms.owner.get(&made.id), Some(&Owner::Seat(*you)), "owned by seat");
        let gone =
            rooms.make(4, "gone", WorldKind::Infinite, None, None, Reach::Listed, false).unwrap();
        rooms.delete(gone.id.as_str()).unwrap();
        rooms.save().unwrap();
        made.id
    };

    let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    assert!(back.is_unlisted(&id) && back.code_of(&id).is_some(), "privacy was lost");
    assert_eq!(back.owner.get(&id), None, "a seat outlived the process");
    assert_eq!(back.made_count().0, 1, "a deleted room was counted");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Whoever made a room can close it from the menu, and nobody else can.**
/// Not while anybody is in it, the maker included; not a room the console
/// made; and the listing says whose each room is, so a menu offers the
/// door only on your own.
#[test]
fn only_whoever_made_a_room_can_close_it_and_only_once_it_is_empty() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (a_id, b_id) = two_people(&mut rooms);
    let out = rooms.handle(
        &Caller::known(1, a_id.clone()),
        ClientMessage::Create {
            name: "den".into(),
            shape: WorldKind::Infinite,
            victory: None,
            teams: None,
            private: false,
            laboratory: false,
            party: None,
        },
    );
    let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
    let den = made.id.clone();
    let close = |room: &RoomId| ClientMessage::Close { room: room.clone() };

    // The listing says whose it is, which is how a menu knows to offer it.
    let mine = rooms.listing().into_iter().find(|r| r.id == den).expect("listed");
    assert_eq!(mine.owner, Some(a_id.clone()));

    // Somebody else, and a room nobody made.
    let out = rooms.handle(&Caller::known(2, b_id), close(&den));
    let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
    assert!(why.contains("whoever made"), "{why}");
    let out = rooms.handle(&Caller::known(1, a_id.clone()), close(&RoomId::from("hall")));
    let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
    assert!(why.contains("console"), "{why}");

    // The maker, from inside: refused, and the reason is the room being
    // occupied rather than the key being wrong.
    let out = rooms.handle(
        &Caller::new(1),
        ClientMessage::Join { name: "maker".into(), room: Some(den.clone()), person: None },
    );
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let mut inside = Caller::sitting(1, (den.clone(), *you));
    inside.person = Some(a_id.clone());
    let out = rooms.handle(&inside, close(&den));
    let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
    assert!(why.contains("still in"), "{why}");
    assert!(rooms.get(&den).is_some(), "an occupied room was closed");

    // And once they have left, it goes.
    rooms.handle(&inside, ClientMessage::Leave);
    let out = rooms.handle(&Caller::known(1, a_id), close(&den));
    assert!(matches!(&out[..], [ServerMessage::Closed(Ok(id))] if *id == den), "{out:?}");
    assert!(rooms.get(&den).is_none(), "the room is still here");
    assert_eq!(rooms.made_count().0, 0, "and it still counts against the cap");
}

/// **A `Close` says no more about an unlisted room than a `Join` does.** A
/// stranger closing one by its id is told what a mistyped name is told,
/// word for word, so a forwarded id cannot be checked against the door.
#[test]
fn closing_a_room_you_cannot_see_is_told_it_is_not_here() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (_, a) = met(&mut rooms, 1);
    let (_, b) = met(&mut rooms, 2);
    let made = private_room(&mut rooms, &Caller::known(1, a));
    let close = |room: &str| ClientMessage::Close { room: RoomId::from(room) };
    let why = |out: Vec<ServerMessage>| match &out[..] {
        [ServerMessage::Closed(Err(why))] => why.clone(),
        other => panic!("{other:?}"),
    };
    let real = why(rooms.handle(&Caller::known(2, b.clone()), close(made.id.as_str())));
    let nothing = why(rooms.handle(&Caller::known(2, b), close("r-zzzzzz")));
    assert_eq!(real.replace(made.id.as_str(), "r-zzzzzz"), nothing, "the refusals differ");
    assert!(!real.contains(made.code.as_deref().unwrap()), "the refusal leaked the code");
    assert!(rooms.get(&made.id).is_some());
}

/// Somebody met by hello, with the secret still in hand for a join.
fn met(rooms: &mut Rooms, n: u64) -> (Secret, PersonId) {
    let key = Secret::new().unwrap();
    let out = rooms.handle(
        &Caller::new(n),
        ClientMessage::Hello { name: format!("p{n}"), person: key.clone() },
    );
    let [ServerMessage::You(profile), ..] = &out[..] else { panic!("{out:?}") };
    (key, profile.who.clone())
}

fn join_as(key: &Secret, room: &str) -> ClientMessage {
    ClientMessage::Join {
        name: "somebody".into(),
        room: Some(RoomId::from(room)),
        person: Some(key.clone()),
    }
}

fn private_room(rooms: &mut Rooms, by: &Caller) -> Made {
    let out = rooms.handle(
        by,
        ClientMessage::Create {
            name: "den".into(),
            shape: WorldKind::Infinite,
            victory: None,
            teams: None,
            private: true,
            laboratory: false,
            party: None,
        },
    );
    let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
    made.clone()
}

/// **An invitation names a person, where a code names nobody.** The id of
/// a private room stops being a way in on its own: it opens for its maker,
/// for whoever was invited, and by the code — and somebody who came in by
/// the code is in from then on, so a refresh is not a refusal.
#[test]
fn an_invitation_admits_the_person_it_names_and_the_id_alone_admits_nobody() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (ka, a) = met(&mut rooms, 1);
    let (kb, b) = met(&mut rooms, 2);
    let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
    let (den, code) = (made.id.clone(), made.code.expect("a code"));

    // A stranger with the id is told what anybody mistyping a name is
    // told, and no more: the refusal echoes what they typed and names the
    // listed rooms, and the code is in neither.
    let out = rooms.handle(&Caller::known(2, b.clone()), join_as(&kb, den.as_str()));
    let [ServerMessage::Rejected { reason }] = &out[..] else { panic!("{out:?}") };
    assert!(reason.contains("no room"), "{reason}");
    assert!(!reason.contains(&code), "the refusal leaked a code: {reason}");
    let out = rooms.handle(&Caller::new(9), ClientMessage::Watch { room: den.clone() });
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger watched: {out:?}");

    // The maker, by id: the key that made it.
    let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, den.as_str()));
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let mut inside = Caller::sitting(1, (den.clone(), *you));
    inside.person = Some(a.clone());

    // Invited, and told so with the next thing they say -- with the room's
    // name, which they have never been listed.
    let out = rooms.handle(&inside, ClientMessage::Invite { who: b.clone(), room: den.clone() });
    assert!(out.is_empty(), "{out:?}");
    let out = rooms.handle(&Caller::known(2, b.clone()), ClientMessage::Rooms);
    let told = out.iter().find_map(|m| match m {
        ServerMessage::Invited { from, room, name } => {
            Some((from.who.clone(), room.clone(), name.clone()))
        }
        _ => None,
    });
    assert_eq!(told, Some((a.clone(), den.clone(), "den".into())), "{out:?}");

    // And now the id is a way in for them.
    let out = rooms.handle(&Caller::known(2, b.clone()), join_as(&kb, den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");

    // Codes stay: a third person, by the code, and from then on by the id.
    let (kc, c) = met(&mut rooms, 3);
    let out = rooms.handle(&Caller::known(3, c.clone()), join_as(&kc, &code));
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    rooms.handle(&Caller::sitting(3, (den.clone(), *you)), ClientMessage::Leave);
    let out = rooms.handle(&Caller::known(3, c), join_as(&kc, den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "a refresh refused: {out:?}");
}

/// The five ways an invitation will not go, each a sentence that leaves the
/// asker where they were rather than back on the menu.
#[test]
fn an_invitation_nobody_can_use_is_refused_where_it_was_asked() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (ka, a) = met(&mut rooms, 1);
    let (_, b) = met(&mut rooms, 2);
    let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
    let den = made.id.clone();
    let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, den.as_str()));
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let mut inside = Caller::sitting(1, (den.clone(), *you));
    inside.person = Some(a.clone());
    let why = |out: &[ServerMessage]| match out {
        [ServerMessage::NotDone { reason }] => reason.clone(),
        other => panic!("not a refusal in place: {other:?}"),
    };
    let invite = |who: &PersonId, room: &RoomId| ClientMessage::Invite {
        who: who.clone(),
        room: room.clone(),
    };

    let mut keyless = inside.clone();
    keyless.person = None;
    assert!(why(&rooms.handle(&keyless, invite(&b, &den))).contains("no key"));
    assert!(why(&rooms.handle(&Caller::known(1, a.clone()), invite(&b, &den)))
        .contains("a room you are in"));
    assert!(why(&rooms.handle(&inside, invite(&b, &RoomId::from("hall"))))
        .contains("a room you are in"));
    assert!(why(&rooms.handle(&inside, invite(&a, &den))).contains("already here"));
    let stranger = PersonId("nobody-here".into());
    assert!(why(&rooms.handle(&inside, invite(&stranger, &den))).contains("never met"));
    assert!(
        rooms.admitted.get(&den).is_none_or(|in_| in_.is_empty()),
        "a refusal admitted somebody"
    );
}

/// **An invitation is one message, however often it is sent.** A repeat
/// into a room or a party is refused the way a second challenge is, and
/// the outbox has a ceiling: what waits there is memory held for somebody
/// who may never come back, and was the one thing a client could grow
/// without bound.
#[test]
fn an_invitation_is_one_message_however_often_it_is_sent() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (ka, a) = met(&mut rooms, 1);
    let (_, b) = met(&mut rooms, 2);
    let den = private_room(&mut rooms, &Caller::known(1, a.clone())).id;
    let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, den.as_str()));
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let mut inside = Caller::sitting(1, (den.clone(), *you));
    inside.person = Some(a.clone());
    let why = |out: &[ServerMessage]| match out {
        [ServerMessage::NotDone { reason }] => reason.clone(),
        other => panic!("not a refusal in place: {other:?}"),
    };

    let invite = ClientMessage::Invite { who: b.clone(), room: den.clone() };
    assert!(rooms.handle(&inside, invite.clone()).is_empty());
    for _ in 0..3 {
        assert!(why(&rooms.handle(&inside, invite.clone())).contains("already"));
    }
    assert_eq!(rooms.waiting[&b].len(), 1, "a repeat queued a message");

    let out = rooms.handle(&inside, ClientMessage::MakeParty { name: "friday".into() });
    let party = parties_in(&out)[0].id.clone();
    let ask = ClientMessage::InviteToParty { party: party.clone(), who: b.clone() };
    assert!(rooms.handle(&inside, ask.clone()).is_empty());
    assert!(why(&rooms.handle(&inside, ask)).contains("already"));
    assert_eq!(rooms.waiting[&b].len(), 2, "a repeat queued a message");

    // The ceiling. Reached honestly only by many rooms and many parties
    // asking for one person, so it is filled by hand here; what matters is
    // that a refusal at it changes nothing -- no door opens, nothing
    // stands.
    let (_, c) = met(&mut rooms, 3);
    let filler = rooms.waiting[&b][0].clone();
    rooms.waiting.entry(c.clone()).or_default().resize(MAX_WAITING, filler);
    let out = rooms.handle(&inside, ClientMessage::Invite { who: c.clone(), room: den.clone() });
    assert!(why(&out).contains("waiting"), "{out:?}");
    assert!(!rooms.admitted[&den].contains(&c), "a refused invitation opened the door");
    let out = rooms
        .handle(&inside, ClientMessage::InviteToParty { party: party.clone(), who: c.clone() });
    assert!(why(&out).contains("waiting"), "{out:?}");
    assert!(!rooms.parties.get(&party).unwrap().invited.contains(&c), "and one stands");
    assert_eq!(rooms.waiting[&c].len(), MAX_WAITING, "the ceiling gave");
}

/// A world of the party's, named apart from `private_room`'s so one test
/// can hold both.
fn party_room(rooms: &mut Rooms, by: &Caller, party: &PartyId) -> Result<Made, String> {
    let out = rooms.handle(
        by,
        ClientMessage::Create {
            name: "lair".into(),
            shape: WorldKind::Infinite,
            victory: None,
            teams: None,
            private: false,
            laboratory: false,
            party: Some(party.clone()),
        },
    );
    let [ServerMessage::Made(made)] = &out[..] else { panic!("{out:?}") };
    made.clone()
}

/// The parties in an answer, or a panic that says what came instead.
fn parties_in(out: &[ServerMessage]) -> Vec<crate::net::PartyInfo> {
    out.iter()
        .find_map(|m| match m {
            ServerMessage::Parties { parties } => Some(parties.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no party listing in {out:?}"))
}

/// **A party is a private set of worlds its members see and nobody else
/// does.** A member sees the party's world in the party listing and not in
/// the room list; a non-member sees neither and cannot join it by id; an
/// invitation reaches the person it names and lets them in; leaving takes
/// the worlds with it.
#[test]
fn a_party_is_a_private_set_of_worlds_only_its_members_see_or_join() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (ka, a) = met(&mut rooms, 1);
    let (kb, b) = met(&mut rooms, 2);
    let (_, c) = met(&mut rooms, 3);
    let me = Caller::known(1, a.clone());
    let them = Caller::known(2, b.clone());

    // Nobody has presented a key: on no list, and told so truthfully.
    let out = rooms.handle(&Caller::new(9), ClientMessage::Parties);
    assert!(parties_in(&out).is_empty());

    let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
    let listed = parties_in(&out);
    assert_eq!(listed.len(), 1, "making a party did not list it: {out:?}");
    let party = listed[0].id.clone();
    assert_eq!(listed[0].name, "friday");
    assert_eq!(listed[0].members.len(), 1, "the maker is its first member");
    assert_eq!(listed[0].members[0].who, a);

    // A world of the party's: no code, not in the room list, in the party's.
    let made = party_room(&mut rooms, &me, &party).expect("a member may make one");
    assert_eq!(made.code, None, "a party's world has a code");
    assert!(rooms.is_unlisted(&made.id));
    assert!(!rooms.listing().iter().any(|r| r.id == made.id), "it is in the room list");
    let mine = parties_in(&rooms.handle(&me, ClientMessage::Parties));
    assert_eq!(mine[0].rooms.len(), 1, "the party does not list its world");
    assert_eq!(mine[0].rooms[0].id, made.id);

    // A non-member sees nothing and gets in nowhere -- not by making a
    // world for it, not by the id, and not by watching.
    assert!(party_room(&mut rooms, &them, &party).is_err(), "a stranger made a party world");
    assert!(parties_in(&rooms.handle(&them, ClientMessage::Parties)).is_empty());
    let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
    let out = rooms.handle(&them, ClientMessage::Watch { room: made.id.clone() });
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger watched: {out:?}");
    // And a member cannot hand the door to a person by a room invitation,
    // which would be a way round the party.
    let out = rooms.handle(&me, join_as(&ka, made.id.as_str()));
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let mut inside = Caller::sitting(1, (made.id.clone(), *you));
    inside.person = Some(a.clone());
    let out = rooms.handle(&inside, ClientMessage::Invite { who: c, room: made.id.clone() });
    let [ServerMessage::NotDone { reason }] = &out[..] else { panic!("{out:?}") };
    assert!(reason.contains("party"), "{reason}");

    // An invitation reaches the person it names, rides out with their next
    // word, and is the only way in; the party then lists them both.
    let out = rooms.handle(&them, ClientMessage::JoinParty { party: party.clone() });
    assert!(matches!(&out[..], [ServerMessage::NotDone { .. }]), "joined uninvited: {out:?}");
    let out = rooms
        .handle(&inside, ClientMessage::InviteToParty { party: party.clone(), who: b.clone() });
    assert!(out.is_empty(), "{out:?}");
    let out = rooms.handle(&them, ClientMessage::Rooms);
    let asked = out.iter().find_map(|m| match m {
        ServerMessage::PartyInvite { from, party, name } => {
            Some((from.who.clone(), party.clone(), name.clone()))
        }
        _ => None,
    });
    assert_eq!(asked, Some((a.clone(), party.clone(), "friday".into())), "{out:?}");
    let out = rooms.handle(&them, ClientMessage::JoinParty { party: party.clone() });
    let theirs = parties_in(&out);
    assert_eq!(theirs.len(), 1);
    assert_eq!(theirs[0].members.len(), 2);
    assert!(theirs[0].members.iter().any(|m| m.who == a && m.online), "a is in the world");
    assert!(theirs[0].members.iter().any(|m| m.who == b && !m.online));
    let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "a member refused: {out:?}");

    // Leaving loses the worlds, which a code could never express -- from
    // the next join. The seat they hold lasts until they leave the room:
    // nothing reaches a seat from outside its room, so unseating them
    // would be a `Rejected` from nowhere.
    let out = rooms.handle(&them, ClientMessage::LeaveParty { party: party.clone() });
    assert!(parties_in(&out).is_empty(), "left and still listed");
    assert!(rooms.is_online(&b), "leaving the party pulled the chair");
    let seat = (made.id.clone(), PlayerId(2));
    rooms.handle(&Caller::sitting(2, seat), ClientMessage::Leave);
    let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "the door stayed open: {out:?}");

    // The last one out takes the party; its world stays its maker's, and
    // the maker's door with it -- ownership outranks membership.
    let out = rooms.handle(&me, ClientMessage::LeaveParty { party });
    assert!(parties_in(&out).is_empty());
    assert!(rooms.parties.is_empty(), "an empty party stayed");
    assert!(rooms.get(&made.id).is_some(), "the world went with the party");
    assert_eq!(rooms.owned_by(&made.id), Some(&a));
    rooms.handle(&inside, ClientMessage::Leave);
    let out = rooms.handle(&me, join_as(&ka, made.id.as_str()));
    assert!(
        matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
        "the maker lost their own door: {out:?}"
    );
}

/// **A party survives a restart** with its people, its standing
/// invitations and its worlds, and a world of its is still members-only.
#[test]
fn a_party_survives_a_restart() {
    let dir = temp_dir("parties");
    let (ka, kb, kc) = (Secret::new().unwrap(), Secret::new().unwrap(), Secret::new().unwrap());
    let hello = |rooms: &mut Rooms, n: u64, key: &Secret| -> PersonId {
        let out = rooms.handle(
            &Caller::new(n),
            ClientMessage::Hello { name: format!("p{n}"), person: key.clone() },
        );
        let [ServerMessage::You(profile), ..] = &out[..] else { panic!("{out:?}") };
        profile.who.clone()
    };
    let (party, den, b) = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let a = hello(&mut rooms, 1, &ka);
        let b = hello(&mut rooms, 2, &kb);
        hello(&mut rooms, 3, &kc);
        let me = Caller::known(1, a.clone());
        let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
        let party = parties_in(&out)[0].id.clone();
        let den = party_room(&mut rooms, &me, &party).unwrap().id;
        rooms.handle(&me, ClientMessage::InviteToParty { party: party.clone(), who: b.clone() });
        rooms.save().unwrap();
        (party, den, b)
    };

    let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    let a = hello(&mut back, 4, &ka);
    let mine = parties_in(&back.handle(&Caller::known(4, a), ClientMessage::Parties));
    assert_eq!(mine.len(), 1, "the party was lost");
    assert_eq!(mine[0].id, party);
    assert_eq!(mine[0].rooms.iter().map(|r| &r.id).collect::<Vec<_>>(), [&den]);

    // The invitation stood, and the door is still the party's.
    let out = back.handle(&Caller::known(5, b.clone()), ClientMessage::JoinParty { party });
    assert_eq!(parties_in(&out).len(), 1, "the invitation was lost: {out:?}");
    let out = back.handle(&Caller::known(5, b), join_as(&kb, den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");
    let out = back.handle(&Caller::new(6), join_as(&kc, den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **An invitation given before a restart stands after it**, because the
/// door is in `rooms.jsonl` beside the code.
#[test]
fn an_invitation_survives_a_restart() {
    let dir = temp_dir("admitted");
    let kb = Secret::new().unwrap();
    let den = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let (ka, a) = met(&mut rooms, 1);
        let out = rooms
            .handle(&Caller::new(2), ClientMessage::Hello { name: "b".into(), person: kb.clone() });
        let [ServerMessage::You(theirs)] = &out[..] else { panic!("{out:?}") };
        let b = theirs.who.clone();
        let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
        let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, made.id.as_str()));
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let mut inside = Caller::sitting(1, (made.id.clone(), *you));
        inside.person = Some(a);
        rooms.handle(&inside, ClientMessage::Invite { who: b, room: made.id.clone() });
        rooms.save().unwrap();
        made.id
    };

    let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    let out = back.handle(&Caller::new(5), join_as(&kb, den.as_str()));
    assert!(
        matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
        "the door forgot them: {out:?}"
    );
    let out = back.handle(&Caller::new(6), join_as(&Secret::new().unwrap(), den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "and let a stranger in: {out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A room a player made with no row is shown to nobody.** The row is
/// what says a room is private, so losing it — a truncated write, a line
/// this build cannot read — has to shut the door rather than open it: the
/// room comes back unlisted, codeless and nobody's, a party's world is
/// still its party's, and the next save writes the rows that were lacking.
#[test]
fn a_room_with_no_row_is_shown_to_nobody() {
    let dir = temp_dir("no-row");
    let kb = Secret::new().unwrap();
    let (den, lair, code) = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let (_, a) = met(&mut rooms, 1);
        let out = rooms
            .handle(&Caller::new(2), ClientMessage::Hello { name: "b".into(), person: kb.clone() });
        let [ServerMessage::You(theirs)] = &out[..] else { panic!("{out:?}") };
        let b = theirs.who.clone();
        let me = Caller::known(1, a.clone());
        let made = private_room(&mut rooms, &me);
        let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
        let party = parties_in(&out)[0].id.clone();
        rooms.handle(&me, ClientMessage::InviteToParty { party: party.clone(), who: b.clone() });
        rooms.handle(&Caller::known(2, b), ClientMessage::JoinParty { party: party.clone() });
        let lair = party_room(&mut rooms, &me, &party).unwrap().id;
        rooms.save().unwrap();
        (made.id, lair, made.code.expect("a code"))
    };
    std::fs::write(meta_path(&dir), "{not json\n").unwrap();

    let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    for room in [&den, &lair] {
        assert!(back.get(room).is_some(), "{room} did not come back");
        assert!(back.is_unlisted(room), "{room} came back listed");
        assert!(!back.listing().iter().any(|r| r.id == *room), "{room} is in the listing");
        assert_eq!(back.code_of(room), None, "{room} has a code from nowhere");
        assert_eq!(back.owned_by(room), None, "{room} has an owner from nowhere");
    }
    assert_eq!(back.made_count().0, 2, "they came back outside the cap");
    assert!(back.resolve(Some(&code)).is_err(), "the code still opened it");
    let out = back.handle(&Caller::new(5), join_as(&Secret::new().unwrap(), den.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
    // The party's world is still the party's.
    let out = back.handle(&Caller::new(6), join_as(&kb, lair.as_str()));
    assert!(
        matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
        "a member was refused: {out:?}"
    );
    let out = back.handle(&Caller::new(7), join_as(&Secret::new().unwrap(), lair.as_str()));
    assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
    back.save().unwrap();
    assert_eq!(load_meta(&meta_path(&dir)).unwrap().len(), 2, "the rows were not written");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **`--fresh` ignores a player's worlds and does not lose them.** It
/// opens none of them, so a save in between writes only what is in
/// memory — and the rows for what it did not open ride through, or the
/// next ordinary run found every private world listed, codeless and
/// nobody's, and a party's worlds no longer its party's.
#[test]
fn fresh_leaves_a_players_worlds_as_they_were() {
    let dir = temp_dir("fresh-keeps");
    let ka = Secret::new().unwrap();
    let (den, lair, code, a) = {
        let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        let out = rooms
            .handle(&Caller::new(1), ClientMessage::Hello { name: "a".into(), person: ka.clone() });
        let [ServerMessage::You(profile)] = &out[..] else { panic!("{out:?}") };
        let a = profile.who.clone();
        let me = Caller::known(1, a.clone());
        let made = private_room(&mut rooms, &me);
        let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
        let party = parties_in(&out)[0].id.clone();
        let lair = party_room(&mut rooms, &me, &party).unwrap().id;
        rooms.save().unwrap();
        (made.id, lair, made.code.expect("a code"), a)
    };

    // The diagnostic run: nothing of theirs is open, a room is made, and
    // it saves.
    {
        let mut fresh = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
        assert!(fresh.get(&den).is_none() && fresh.get(&lair).is_none(), "fresh opened them");
        assert_eq!(fresh.made_count().0, 0, "fresh counted them");
        fresh.make(9, "", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
        fresh.save().unwrap();
    }

    let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    assert!(back.is_unlisted(&den), "den came back listed");
    assert_eq!(back.code_of(&den), Some(code.as_str()), "den came back codeless");
    assert_eq!(back.owned_by(&den), Some(&a), "den came back nobody's");
    assert_eq!(back.made_count().0, 3, "the count is theirs and the one made meanwhile");
    let mine = parties_in(&back.handle(&Caller::known(2, a), ClientMessage::Parties));
    assert_eq!(mine.len(), 1, "the party was lost");
    assert_eq!(
        mine[0].rooms.iter().map(|r| &r.id).collect::<Vec<_>>(),
        [&lair],
        "the party lost its world"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whoever is running the server can read the save directory anyway, and
/// an operator who cannot see a room cannot delete one being misused.
#[test]
fn the_console_sees_private_rooms_and_the_wire_does_not() {
    let mut rooms =
        Rooms::open(temp_dir("console-sees"), &["hall".into()], WorldKind::Infinite, true).unwrap();
    let made = rooms.make(3, "", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
    assert!(made.code.is_some(), "a private room gets a code");

    let everything = rooms.everything();
    let found = everything.iter().find(|(r, _)| r.id == made.id).expect("the console sees it");
    assert!(found.1, "and knows it is private");
    assert_eq!(everything.len(), 2);

    assert_eq!(rooms.listing().len(), 1, "the wire sees only the open one");

    // What the console actually prints, since that is the thing being
    // claimed. Both rooms, and the private one said to be private.
    let printed = crate::server::console::run("rooms", &mut rooms, WorldKind::Infinite);
    let text = printed.lines.join("\n");
    assert!(text.contains("hall"), "{text}");
    assert!(text.contains(made.id.as_str()), "the console shows the id: {text}");
    assert!(text.contains("private"), "{text}");
    assert!(
        text.contains(made.code.as_ref().unwrap().as_str()),
        "and the code, which is why an operator looks a private room up: {text}"
    );
}

/// A code is read off one screen and typed into another. The five
/// characters that make that go wrong are not in it.
#[test]
fn a_code_has_nothing_confusable_in_it() {
    for _ in 0..500 {
        let c = code();
        assert_eq!(c.len(), CODE_LEN);
        assert!(!c.contains(['0', 'o', '1', 'i', 'l']), "confusable character in {c}");
        assert!(crate::net::room_name(&c).is_ok(), "a code must be a legal room name: {c}");
    }
}

/// A player can start the match they made, and nobody else can — not
/// another player in it, and not somebody who only joined.
#[test]
fn only_whoever_made_a_match_can_start_it() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let ours = Caller::new(5);
    let theirs = Caller::new(6);

    let made = rooms
        .make(
            5,
            "cup",
            WorldKind::Infinite,
            Some(Victory::Timer { generations: 50 }),
            None,
            Reach::Listed,
            false,
        )
        .unwrap();
    let join = |name: &str| ClientMessage::Join {
        name: name.into(),
        room: Some(made.id.clone()),
        person: None,
    };

    // Nobody owns it until the maker joins: the owner is a PlayerId, and
    // there is no player until somebody has one.
    let out = rooms.handle(&ours, join("owner"));
    let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };
    let owner = *you;

    let mut sitting = Caller::sitting(6, (made.id.clone(), PlayerId(0)));
    let out = rooms.handle(&theirs, join("guest"));
    let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };
    sitting.seat = Some((made.id.clone(), *you));
    assert_ne!(*you, owner, "two players, not one");

    // The guest cannot.
    let out = rooms.handle(&sitting, ClientMessage::Start);
    let [ServerMessage::NotStarted { reason }] = &out[..] else {
        panic!("a guest started somebody else's match: {out:?}");
    };
    assert!(reason.contains("made"), "{reason}");
    assert_eq!(*rooms.get(&made.id).unwrap().phase(), Phase::Gathering);

    // The owner can — and from a **different connection**, because a
    // reconnect gets a new socket and the same player, and losing your own
    // match to a refresh would be the obvious way for this to be wrong.
    let reconnected = Caller::sitting(99, (made.id.clone(), owner));
    let out = rooms.handle(&reconnected, ClientMessage::Start);
    assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
    assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
    assert_eq!(
        rooms.get(&made.id).unwrap().started_by(),
        Some(owner),
        "and it remembers who blew it"
    );
}

/// **A match is its maker's by person, not by seat.** A seat is a room's
/// number for somebody and a key is who they are on this server, so the
/// whistle answers to the key wherever it is presented from, and to
/// nobody holding the seat without it.
#[test]
fn a_keyed_maker_owns_their_match_from_any_seat() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let made = rooms
        .make(
            5,
            "cup",
            WorldKind::Infinite,
            Some(Victory::Timer { generations: 50 }),
            None,
            Reach::Listed,
            false,
        )
        .unwrap();
    let key = Secret::new().unwrap();
    let out = rooms.handle(
        &Caller::new(5),
        ClientMessage::Join {
            name: "maker".into(),
            room: Some(made.id.clone()),
            person: Some(key),
        },
    );
    let Some(ServerMessage::Welcome { you, profile: Some(profile), .. }) =
        out.iter().find(|m| matches!(m, ServerMessage::Welcome { .. }))
    else {
        panic!("{out:?}")
    };
    let (seat, who) = (*you, profile.who.clone());
    // The lobby names the maker's seat, which is how a client knows the
    // whistle is its own to blow. It goes out with the next step, as every
    // lobby does.
    let broadcast = rooms.step(std::time::Duration::from_secs(1));
    let named = broadcast.iter().find_map(|(_, m)| match m {
        ServerMessage::Match(lobby) => Some(lobby.owner),
        _ => None,
    });
    assert_eq!(named, Some(Some(seat)), "the lobby did not name the maker: {broadcast:?}");

    // Somebody in the maker's seat without the maker's key: a stranger.
    let mut impostor = Caller::sitting(9, (made.id.clone(), seat));
    let out = rooms.handle(&impostor, ClientMessage::Start);
    assert!(
        matches!(&out[..], [ServerMessage::NotStarted { .. }]),
        "a seat started a keyed match: {out:?}"
    );
    impostor.person = Some(PersonId("nobody".into()));
    let out = rooms.handle(&impostor, ClientMessage::Start);
    assert!(
        matches!(&out[..], [ServerMessage::NotStarted { .. }]),
        "the wrong key started it: {out:?}"
    );
    assert_eq!(*rooms.get(&made.id).unwrap().phase(), Phase::Gathering);

    // The maker's key on a new socket, given a seat the room never handed
    // out: the whistle is theirs anyway.
    let mut maker = Caller::sitting(99, (made.id.clone(), PlayerId(7)));
    maker.person = Some(who);
    let out = rooms.handle(&maker, ClientMessage::Start);
    assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
    assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
}

/// The whole flow a client actually walks: make a match, join it, and be
/// told — by the broadcast every client in the room gets — that it is
/// yours to start. The owner check has a unit test; this is about whether
/// the answer ever *reaches* the person who has to press the button.
#[test]
fn the_maker_of_a_match_is_told_it_is_theirs_to_start() {
    let mut rooms =
        Rooms::open(temp_dir("told"), &["hall".into()], WorldKind::Infinite, true).unwrap();
    let me = Caller::new(12);

    let made = rooms
        .handle(
            &me,
            ClientMessage::Create {
                name: "cup".into(),
                shape: WorldKind::Infinite,
                victory: Some(Victory::Timer { generations: 200 }),
                teams: None,
                private: false,
                laboratory: false,
                party: None,
            },
        )
        .into_iter()
        .find_map(|m| match m {
            ServerMessage::Made(Ok(made)) => Some(made),
            _ => None,
        })
        .expect("a room");

    let welcomed = rooms.handle(
        &me,
        ClientMessage::Join { name: "maker".into(), room: Some(made.id.clone()), person: None },
    );
    let you = welcomed
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { you, .. } => Some(*you),
            _ => None,
        })
        .expect("a welcome");

    // The lobby reaches everybody by broadcast rather than in the reply,
    // because a lobby full of people all changing sides needs one message
    // to all of them. A gathering match does not advance its world, and it
    // still has to produce this — which is the thing most likely to have
    // been got wrong.
    let broadcast = rooms.step(a_generation());
    let owner = broadcast
        .iter()
        .find_map(|(_, m)| match m {
            ServerMessage::Match(lobby) => Some(lobby.owner),
            _ => None,
        })
        .expect("a gathering match still broadcasts its lobby");
    assert_eq!(owner, Some(you), "the maker was not told the match is theirs");

    // And pressing it works from the seat that broadcast named.
    let out = rooms.handle(&Caller::sitting(12, (made.id.clone(), you)), ClientMessage::Start);
    assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
    assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
}

/// A room the console made is the operator's, and starts at the console.
#[test]
fn a_match_nobody_made_cannot_be_started_from_a_client() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    rooms.new_match("cup", WorldKind::Infinite, Victory::Timer { generations: 50 }).unwrap();
    let id = RoomId::from("cup");

    let out = rooms.handle(
        &Caller::new(3),
        ClientMessage::Join { name: "someone".into(), room: Some(id.clone()), person: None },
    );
    let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };

    let out = rooms.handle(&Caller::sitting(3, (id.clone(), *you)), ClientMessage::Start);
    let [ServerMessage::NotStarted { reason }] = &out[..] else { panic!("{out:?}") };
    assert!(reason.contains("console"), "{reason}");
}

/// **A join hands back the locker this server holds**, and an empty one is
/// how a client knows to offer what it is carrying — which is what makes a
/// library follow somebody to a server they have never played on, with no
/// two servers ever talking to each other.
#[test]
fn a_join_hands_back_the_locker_and_an_empty_one_asks_for_it() {
    use crate::net::kept::{Kept, Stamp};

    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let me = Secret::new().unwrap();
    let join = || ClientMessage::Join {
        name: "alice".into(),
        room: Some(RoomId::from("hall")),
        person: Some(me.clone()),
    };

    // Nothing held yet, so the answer is empty and the client seeds it.
    let out = rooms.handle(&Caller::new(1), join());
    let [.., ServerMessage::Yours(kept)] = &out[..] else { panic!("{out:?}") };
    assert!(kept.is_empty(), "a locker appeared from nowhere");

    let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let who = profile.clone().expect("no profile was issued").who;

    let mut library = Kept { stamps: vec![Stamp::trimmed(vec![(0, 0), (1, 1)])], games: vec![] };
    library.stamps[0].name = "corner".into();
    let caller = Caller::known(1, who.clone());
    assert!(rooms.handle(&caller, ClientMessage::Keep(library.clone())).is_empty());

    // And the next join gets it back. Seat given up first, or the second
    // connection is the same person arriving twice and is refused.
    let seat = (RoomId::from("hall"), crate::sim::PlayerId(1));
    rooms.handle(&Caller::sitting(1, seat), ClientMessage::Leave);
    let out = rooms.handle(&Caller::new(2), join());
    let [.., ServerMessage::Yours(kept)] = &out[..] else { panic!("{out:?}") };
    assert_eq!(kept.stamps.len(), 1, "the library did not come back");
    assert_eq!(kept.stamps[0].name, "corner");
}

/// **What a server asks a client not to offer travels with the room list**,
/// because that is the first thing a menu asks any server — so the answer
/// is known before the menu draws anything.
///
/// A request rather than a rule, and it cannot be anything else: the
/// client is somebody else's and every screen it hides is still compiled
/// into it. What it is for is copy nobody has written yet.
#[test]
fn a_room_list_says_what_this_server_would_rather_not_be_offered() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));

    let out = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
    let [ServerMessage::Rooms { hidden, .. }] = &out[..] else { panic!("{out:?}") };
    assert_eq!(*hidden, crate::net::Hidden::default(), "a fresh server hides nothing");

    rooms.hidden.hide("howto").expect("howto is a name");
    let out = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
    let [ServerMessage::Rooms { hidden, .. }] = &out[..] else { panic!("{out:?}") };
    assert!(hidden.howto, "the server's answer did not reach the list");
}

/// A name this build does not know is refused rather than quietly hiding
/// nothing, or `--hide howtoo` starts a server that ignores the flag.
#[test]
fn a_screen_this_build_has_no_name_for_is_refused() {
    let mut hidden = crate::net::Hidden::default();
    let why = hidden.hide("howtoo").expect_err("a typo was accepted");
    assert!(why.contains("howto"), "the refusal does not say what the names are: {why}");
    assert_eq!(hidden, crate::net::Hidden::default(), "a refused name changed something");
}

/// **A challenge is a room made and held for one named person**, and it
/// reaches them on the next thing they say — there is no channel to a
/// person, so it waits.
#[test]
fn a_challenge_makes_a_room_and_reaches_the_person_it_names() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (a, b) = (Secret::new().unwrap(), Secret::new().unwrap());
    let join = |s: &Secret| ClientMessage::Join {
        name: "somebody".into(),
        room: Some(RoomId::from("hall")),
        person: Some(s.clone()),
    };
    // Both have to be somebody this server has met.
    let out = rooms.handle(&Caller::new(1), join(&a));
    let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let a_id = profile.clone().expect("a profile").who;
    rooms.handle(
        &Caller::sitting(1, (RoomId::from("hall"), crate::sim::PlayerId(1))),
        ClientMessage::Leave,
    );
    let out = rooms.handle(&Caller::new(2), join(&b));
    let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let b_id = profile.clone().expect("a profile").who;

    let out = rooms
        .handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id.clone() });
    let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
    let room = made.id.clone();
    assert!(made.code.is_some(), "a challenge is not in the listing");

    // Nothing reaches them until they say something -- and then it does,
    // riding out with whatever they asked for.
    let out = rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
    let told = out.iter().find_map(|m| match m {
        ServerMessage::Challenged { from, room } => Some((from.clone(), room.clone())),
        _ => None,
    });
    let (from, told_room) = told.expect("the challenge never arrived");
    assert_eq!(from.who, a_id, "it came from the wrong person");
    assert_eq!(told_room, room);

    // Taken as it is handed over, so it is not shown twice.
    let out = rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
    assert!(!out.iter().any(|m| matches!(m, ServerMessage::Challenged { .. })), "shown twice");

    // Yes, and the answer reaches the person who asked.
    let out = rooms.handle(
        &Caller::known(2, b_id.clone()),
        ClientMessage::Answer { from: a_id.clone(), yes: true },
    );
    assert!(out.iter().any(|m| matches!(m, ServerMessage::Challenged { .. })), "{out:?}");

    let out = rooms.handle(&Caller::known(1, a_id), ClientMessage::Rooms);
    let answered = out.iter().find_map(|m| match m {
        ServerMessage::Answered { who, room } => Some((who.who.clone(), room.clone())),
        _ => None,
    });
    let (who, room_back) = answered.expect("the answer never arrived");
    assert_eq!(who, b_id);
    assert_eq!(room_back, Some(room), "a yes did not name the room");
}

/// **A no reaches somebody**, because the point of asking is finding out
/// and silence cannot be told from not having seen it.
#[test]
fn a_decline_reaches_the_person_who_asked() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (a_id, b_id) = two_people(&mut rooms);

    rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id.clone() });
    rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
    rooms.handle(
        &Caller::known(2, b_id.clone()),
        ClientMessage::Answer { from: a_id.clone(), yes: false },
    );

    // Searched rather than positioned: what is waiting is handed over
    // *before* what was asked for, so an answer arrives in front of the
    // room list it rode out with.
    let out = rooms.handle(&Caller::known(1, a_id), ClientMessage::Rooms);
    let answered = out.iter().find_map(|m| match m {
        ServerMessage::Answered { who, room } => Some((who.who.clone(), room.clone())),
        _ => None,
    });
    let (who, room) = answered.unwrap_or_else(|| panic!("no answer: {out:?}"));
    assert_eq!(who, b_id);
    assert!(room.is_none(), "a no named a room to join");
}

/// The five ways it will not go, each a sentence somebody can act on.
#[test]
fn a_challenge_nobody_can_answer_is_refused_with_a_reason() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let (a_id, b_id) = two_people(&mut rooms);
    let why = |out: &[ServerMessage]| match out {
        [ServerMessage::Rejected { reason }] => reason.clone(),
        other => panic!("not a refusal: {other:?}"),
    };

    // A client with no key is nobody, so there is nowhere to answer to.
    let out = rooms.handle(&Caller::new(9), ClientMessage::Challenge { who: b_id.clone() });
    assert!(why(&out).contains("no key"), "{:?}", why(&out));

    // Yourself.
    let out = rooms
        .handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: a_id.clone() });
    assert!(why(&out).contains("yourself"));

    // Somebody this server has never met.
    let stranger = crate::net::PersonId("nobody-here".into());
    let out =
        rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: stranger });
    assert!(why(&out).contains("never met"));

    // And twice over, so a challenge cannot fill somebody's screen.
    rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id.clone() });
    let out = rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id });
    assert!(why(&out).contains("already"), "{:?}", why(&out));

    // An answer to nothing.
    let out = rooms
        .handle(&Caller::known(1, a_id.clone()), ClientMessage::Answer { from: a_id, yes: true });
    assert!(why(&out).contains("no challenge"), "{:?}", why(&out));
}

/// Two people this server has met, each having left the room again.
fn two_people(rooms: &mut Rooms) -> (PersonId, PersonId) {
    let hall = RoomId::from("hall");
    let mut meet = |n: u64| {
        let key = Secret::new().unwrap();
        let out = rooms.handle(
            &Caller::new(n),
            ClientMessage::Join {
                name: "somebody".into(),
                room: Some(hall.clone()),
                person: Some(key),
            },
        );
        let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let (seat, who) = (*you, profile.clone().expect("a profile").who);
        rooms.handle(&Caller::sitting(n, (hall.clone(), seat)), ClientMessage::Leave);
        who
    };
    (meet(1), meet(2))
}

/// **A locker is nobody's to offer without a name.** `Keep` writes a
/// client's own words to this server's disk, so a connection that has
/// never joined has nowhere to put them and cannot say whose they are.
#[test]
fn a_locker_offered_by_nobody_is_dropped() {
    use crate::net::kept::{Kept, Stamp};

    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let offered = Kept { stamps: vec![Stamp::trimmed(vec![(0, 0)])], games: Vec::new() };
    assert!(rooms.handle(&Caller::new(9), ClientMessage::Keep(offered)).is_empty());
    assert!(rooms.lockers.is_empty(), "a nameless client filled a locker");
}

/// **A join that was refused is handed nothing.** The room can still turn
/// somebody away after this map has resolved it — a match under way, or a
/// person already sitting here in another tab — and a second tab that was
/// given a locker would go on to replace the library of the one holding
/// the seat.
#[test]
fn a_refused_join_is_handed_no_locker() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let me = Secret::new().unwrap();
    let join = || ClientMessage::Join {
        name: "alice".into(),
        room: Some(RoomId::from("hall")),
        person: Some(me.clone()),
    };
    assert!(rooms
        .handle(&Caller::new(1), join())
        .iter()
        .any(|m| { matches!(m, ServerMessage::Yours(_)) }));

    let out = rooms.handle(&Caller::new(2), join());
    let [ServerMessage::Rejected { .. }] = &out[..] else { panic!("{out:?}") };
}

/// **The same secret is the same person**, on a second connection and
/// after a restart. That is the whole of what an identity has to do: the
/// server issues an id the first time it sees a secret and gives the same
/// one back for ever after, so a rating filed against it does not move.
#[test]
fn one_secret_is_one_person() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let hall = RoomId::from("hall");
    let me = Secret::new().unwrap();
    let join = |secret: Option<Secret>| ClientMessage::Join {
        name: "alice".into(),
        room: Some(RoomId::from("hall")),
        person: secret,
    };
    let named = |rooms: &Rooms, you: &crate::sim::PlayerId| {
        rooms.get(&hall).unwrap().players().find(|p| p.id == *you).unwrap().person.clone()
    };

    let out = rooms.handle(&Caller::new(1), join(Some(me.clone())));
    let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let ours = profile.clone().expect("no profile was issued");
    let first = ours.who.clone();
    assert_eq!(ours.name, "alice", "the name a join was made under");
    assert!(ours.provisional, "a first join has no result behind it");
    assert_eq!(named(&rooms, you).as_deref(), Some(first.as_str()));

    // Away, and back: the same secret finds the same seat and the same
    // name. Nothing was presented and nothing was reissued.
    rooms.handle(&Caller::sitting(1, (hall.clone(), *you)), ClientMessage::Leave);
    let out = rooms.handle(&Caller::new(2), join(Some(me)));
    let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    assert_eq!(profile.as_ref().map(|p| &p.who), Some(&first), "one secret was two people");
    assert_eq!(named(&rooms, you).as_deref(), Some(first.as_str()));

    // And somebody else's secret is somebody else.
    let out = rooms.handle(&Caller::new(3), join(Some(Secret::new().unwrap())));
    let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
    assert_ne!(profile.as_ref().map(|p| &p.who), Some(&first), "two secrets were one person");

    // And so is who else plays here, for the same reason and one more:
    // this is how you find a person to look up in the first place, and the
    // menu is where you are standing when you do.
    let asked = rooms.handle(&Caller::nobody(), ClientMessage::People { like: "ali".into() });
    let [ServerMessage::People { like, found }] = &asked[..] else { panic!("{asked:?}") };
    assert_eq!(like, "ali", "the query comes back, so a stale answer can be dropped");
    assert!(found.iter().any(|p| p.who == first), "alice is not in a search for ali");
    assert!(found.iter().all(|p| p.name.to_lowercase().contains("ali")));

    // Nobody the server has never met, however the ratings table got their
    // fingerprint into it.
    let asked = rooms.handle(&Caller::nobody(), ClientMessage::People { like: "zzz".into() });
    let [ServerMessage::People { found, .. }] = &asked[..] else { panic!("{asked:?}") };
    assert!(found.is_empty(), "found somebody who is not here: {found:?}");

    // And what a server says about somebody is answerable from outside
    // every room, because that is where it is looked at from.
    let asked = rooms.handle(&Caller::nobody(), ClientMessage::Profile { who: first.clone() });
    let [ServerMessage::Profile(Some(found))] = &asked[..] else { panic!("{asked:?}") };
    assert_eq!(found.who, first);
    assert_eq!(found.label(), format!("alice·{}", first.short()));

    // Somebody this server never issued is "not here" rather than a
    // failure: a client may ask about anything.
    let none = rooms.handle(
        &Caller::nobody(),
        ClientMessage::Profile { who: crate::net::PersonId("nobody".into()) },
    );
    assert!(matches!(&none[..], [ServerMessage::Profile(None)]), "{none:?}");
}

/// A client with no key plays. It is nobody the server will remember,
/// which is the honest outcome for a browser that cannot keep one rather
/// than a reason to refuse to let anybody in.
#[test]
fn a_client_with_no_key_still_plays() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let out = rooms.handle(
        &Caller::new(1),
        ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: None,
        },
    );
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let seat = rooms.get(&RoomId::from("hall")).unwrap().players().find(|p| p.id == *you).unwrap();
    assert_eq!(seat.person, None);
}

/// **Leaving frees the seat, and the person still brings you back.**
///
/// Going back to the menu used to send nothing at all, so the player
/// stayed online: the room went on counting them, and the way back — which
/// only returns you to a player who is *not* online — found them online
/// and made a new one instead. Leave and come back three times and a room
/// with one person in it said three.
#[test]
fn leaving_frees_the_seat_and_the_person_still_comes_back() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let hall = RoomId::from("hall");
    let me = Caller::new(3);
    let secret = Secret::new().unwrap();
    let join = || ClientMessage::Join {
        name: "alice".into(),
        room: Some(RoomId::from("hall")),
        person: Some(secret.clone()),
    };

    let out = rooms.handle(&me, join());
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    let first = *you;
    assert_eq!(rooms.get(&hall).unwrap().players().filter(|p| p.online).count(), 1);

    // Back to the menu, still connected.
    rooms.handle(&Caller::sitting(3, (hall.clone(), first)), ClientMessage::Leave);
    assert_eq!(
        rooms.get(&hall).unwrap().players().filter(|p| p.online).count(),
        0,
        "the room still counts somebody who left"
    );

    // And back in: the same player, not a new one. Nothing was presented —
    // the secret this client already had is the whole of the way back.
    let out = rooms.handle(&me, join());
    let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
    assert_eq!(*you, first, "coming back made a second player");
    assert_eq!(
        rooms.get(&hall).unwrap().players().filter(|p| p.online).count(),
        1,
        "one person, however many times they have come and gone"
    );

    // Three times over, which is what the listing was counting.
    for _ in 0..3 {
        rooms.handle(&Caller::sitting(3, (hall.clone(), first)), ClientMessage::Leave);
        rooms.handle(&me, join());
    }
    assert_eq!(rooms.listing()[0].players, 1, "the room list counted the comings and goings");
}

/// **A person is not two players.** Somebody who has carried their secret
/// to a second machine and joined from both is told so, rather than being
/// handed a stranger's seat.
///
/// This is the one place a person is stricter than the token it replaced.
/// A token said which *seat*, so two tabs sharing one were honestly two
/// players and the second quietly got a new number — four hundred
/// generations into a match, if that is when they arrived.
#[test]
fn one_person_cannot_be_in_a_room_twice() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let secret = Secret::new().unwrap();
    let join = || ClientMessage::Join {
        name: "alice".into(),
        room: Some(RoomId::from("hall")),
        person: Some(secret.clone()),
    };
    let out = rooms.handle(&Caller::new(1), join());
    assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");

    let out = rooms.handle(&Caller::new(2), join());
    let [ServerMessage::Rejected { reason }] = &out[..] else { panic!("{out:?}") };
    assert!(reason.contains("already"), "{reason}");
    assert_eq!(
        rooms.get(&RoomId::from("hall")).unwrap().players().count(),
        1,
        "a refused join took a seat anyway"
    );
}

/// Late to a match, and what happens now: the join is **refused** and the
/// client is told why. It is not turned into a watch.
///
/// Deliberate rather than missing. A `Join` that quietly became a `Watch`
/// would put a player into a world they cannot act in without their having
/// asked for that, and the two are answered by different messages —
/// `Welcome` carries a player number, a purse and a spawn, and `Watching`
/// carries none of them. A client that asked to play and got a `Watching`
/// back would have to discover it had no seat by trying to use one.
///
/// So the server refuses and says so, and the **client** offers the watch:
/// the room list has a Watch button on every room, and the refusal names
/// the reason beside it. That keeps "you cannot play in this" and "would
/// you like to watch it" two separate answers, which is what they are.
#[test]
fn joining_a_running_match_is_refused_and_watching_it_is_not() {
    let mut rooms = Rooms::just(Server::named("cup", World::infinite_empty()));
    {
        let server = rooms.get_mut(&RoomId::from("cup")).unwrap();
        server.make_match(Victory::Timer { generations: 100 });
        server.join("early").unwrap();
        server.start_match(None).unwrap();
    }
    for _ in 0..40 {
        rooms.step(a_generation());
    }

    let late = Caller::new(11);
    let replies = rooms.handle(
        &late,
        ClientMessage::Join { name: "late".into(), room: Some(RoomId::from("cup")), person: None },
    );
    let [ServerMessage::Rejected { reason }] = &replies[..] else {
        panic!("expected a refusal, got {replies:?}");
    };
    assert!(reason.contains("cup"), "the refusal names the room: {reason}");

    // And the same connection may watch it, at the same generation, which
    // is the whole distinction: no late joining is a rule about players.
    let replies = rooms.handle(&late, ClientMessage::Watch { room: RoomId::from("cup") });
    let [ServerMessage::Watching { tick, .. }] = &replies[..] else {
        panic!("a late connection may still watch, got {replies:?}");
    };
    assert_eq!(*tick, 40, "and sees the world where it actually is");
}

/// The whole reason a spectator is not "a player with the actions taken
/// away": a seat is one of fifteen, and a match under way admits no new
/// players at all. Neither of those should keep somebody from watching.
#[test]
fn watching_needs_no_seat_and_no_room_in_the_roster() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    // Fill every seat there is. `PlayerId::MAX` is four bits of cell.
    for n in 0..PlayerId::MAX {
        rooms
            .get_mut(&RoomId::from("hall"))
            .unwrap()
            .join(format!("player{n}"))
            .unwrap_or_else(|e| panic!("seat {n}: {e}"));
    }
    assert!(
        rooms.get_mut(&RoomId::from("hall")).unwrap().join("one-too-many").is_err(),
        "the room is full, which is the situation being tested"
    );

    let replies =
        rooms.handle(&Caller::new(9), ClientMessage::Watch { room: RoomId::from("hall") });
    let [ServerMessage::Watching { room, world, .. }] = &replies[..] else {
        panic!("a full room still admits a watcher, got {replies:?}");
    };
    assert_eq!(room.as_str(), "hall");
    assert_eq!(*world, WorldKind::Infinite);
}

/// A watcher reads and does not act. Both halves matter: one that could
/// not read would be watching nothing, and one that could act would be a
/// player who never took a seat.
#[test]
fn a_watcher_is_sent_chunks_and_changes_nothing() {
    let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
    let seated = rooms.get_mut(&RoomId::from("hall")).unwrap().join("alice").unwrap();
    let watcher =
        Caller { connection: 4, seat: None, watching: Some(RoomId::from("hall")), person: None };

    let before = rooms.get(&RoomId::from("hall")).unwrap().world().digest();

    // Reads.
    let replies = rooms.handle(&watcher, ClientMessage::Subscribe { chunks: vec![(0, 0)] });
    assert!(
        replies.iter().any(|m| matches!(m, ServerMessage::ChunkData { .. })),
        "a watcher gets the chunks it asks for, got {replies:?}"
    );

    // And does not act. The action names a seated player, which is the
    // stronger version of the test: it is refused for coming from a
    // connection with no seat, not for naming a player who is not here.
    rooms.handle(
        &watcher,
        ClientMessage::Act(crate::net::Stamped {
            tick: rooms.get(&RoomId::from("hall")).unwrap().tick(),
            player: seated,
            seat: seated,
            action: crate::net::Action::Paint {
                cells: vec![(0, 0)],
                placement: crate::net::Placement::Life,
            },
        }),
    );
    rooms.step(a_generation());
    assert_eq!(
        rooms.get(&RoomId::from("hall")).unwrap().world().digest(),
        {
            let mut clean = Rooms::just(Server::named("hall", World::infinite_empty()));
            clean.get_mut(&RoomId::from("hall")).unwrap().join("alice").unwrap();
            clean.step(a_generation());
            clean.get(&RoomId::from("hall")).unwrap().world().digest()
        },
        "a watcher put something in the world"
    );
    let _ = before;
}

/// A room made while the server runs is a room, on disk immediately, and
/// the shape is its own rather than the one the server was started with.
#[test]
fn a_room_can_be_made_while_the_server_is_running() {
    let dir = temp_dir("create");
    let mut rooms = Rooms::open(&dir, &[], WorldKind::Infinite, true).unwrap();
    assert_eq!(rooms.names().collect::<Vec<_>>(), [DEFAULT_ROOM]);

    let made = rooms.create("Arena", WorldKind::Toroidal { rows: 4, cols: 4 }).unwrap();
    assert_eq!(made.as_str(), "arena", "names fold to lowercase, as they do on a join");
    assert_eq!(rooms.names().collect::<Vec<_>>(), ["arena", DEFAULT_ROOM]);
    assert_eq!(
        rooms.get(&RoomId::from("arena")).unwrap().world().kind(),
        WorldKind::Toroidal { rows: 4, cols: 4 },
        "its own shape, not the one the server was started with"
    );
    assert_eq!(
        rooms.resolve(Some("arena")).unwrap(),
        RoomId::from("arena"),
        "and joinable at once"
    );

    // On disk before anything is in it, so a crash does not take a room
    // somebody was told they had made.
    assert!(dir.join("arena.ckw").exists());

    // A name that is not one, and a name already taken, are both refused
    // rather than silently doing something else.
    assert!(rooms.create("../escape", WorldKind::Infinite).is_err());
    let taken = rooms.create("arena", WorldKind::Infinite).unwrap_err();
    assert!(taken.contains("already"), "{taken}");
    assert_eq!(
        rooms.get(&RoomId::from("arena")).unwrap().world().kind(),
        WorldKind::Toroidal { rows: 4, cols: 4 },
        "and the refusal left the existing world alone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that is not a room must not stop the server, and must not become
/// a room under a name nobody typed.
#[test]
fn a_stray_file_is_ignored_rather_than_opened() {
    let dir = temp_dir("stray");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
    std::fs::write(dir.join("Mixed Case.ckw"), b"not a world").unwrap();

    let rooms = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
    assert_eq!(rooms.names().collect::<Vec<_>>(), [DEFAULT_ROOM]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A world that cannot be read is an error naming the room, not a silent
/// reset and not an error naming nothing.
#[test]
fn a_corrupt_room_is_an_error_that_says_which_room() {
    let dir = temp_dir("corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken.ckw"), b"not a world file at all").unwrap();

    let Err(e) = Rooms::open(&dir, &[], WorldKind::Infinite, false) else {
        panic!("a corrupt room must not be opened");
    };
    assert!(e.to_string().contains("broken"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
