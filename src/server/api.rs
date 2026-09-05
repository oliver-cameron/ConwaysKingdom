//! What an outside program may ask a server, and what it is told.
//!
//! **A request in, a reply out, and no socket anywhere near it.** [`handle`]
//! takes a [`Request`] and the [`Rooms`] and returns a [`Reply`], the way
//! [`crate::server::console::run`] takes a line — so the whole surface is a
//! pure function of a request and the worlds, and every route is tested here
//! without axum. What carries the bytes lives in [`http`], behind
//! `feature = "server"`, and is the only part that does.
//!
//! It runs on the simulation task, like the console and for the same reason:
//! seating a player and pricing an action are touching a world, and there is
//! exactly one place allowed to.
//!
//! **An outside engine's seat is a bot whose driver is the API** — see
//! [`crate::server::bot::Driver::External`]. One seat type, one removal path,
//! one flag in the lobby; and an action it posts goes through
//! [`Server::act`] the moment it arrives, so it is priced against the world
//! as it stands, exactly as a client's is.
//!
//! [`Server::act`]: crate::server::Server::act

#[cfg(feature = "server")]
pub mod http;

use serde_json::{json, Value};

use crate::net::{Action, Level, Stamped};
use crate::server::bot::Driver;
use crate::server::rooms::Rooms;
use crate::sim::{Cell, Chunk, Kind, PlayerId, CHUNK_CELLS, CHUNK_N};

/// One request, whichever route it came in on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// `GET /api/rooms`
    Rooms,
    /// `GET /api/rooms/{room}`
    Room { room: String },
    /// `GET /api/rooms/{room}/bots`
    Bots { room: String },
    /// `POST /api/rooms/{room}/bots`
    AddBot { room: String, name: Option<String>, level: Option<Level>, team: Option<PlayerId> },
    /// `DELETE /api/rooms/{room}/bots/{seat}`, and `DELETE
    /// /api/rooms/{room}/seats/{seat}` — one seat type, one way out.
    RemoveBot { room: String, seat: PlayerId },
    /// `POST /api/rooms/{room}/seats`: a seat something outside will play.
    Sit { room: String, name: String, team: Option<PlayerId> },
    /// `POST /api/rooms/{room}/seats/{seat}/act`
    Act { room: String, seat: PlayerId, action: Action },
    /// `GET /api/rooms/{room}/seats/{seat}`
    Seat { room: String, seat: PlayerId },
    /// `GET /api/rooms/{room}/chunks/{row}/{col}`
    Chunk { room: String, row: i32, col: i32 },
    /// `GET /api/rooms/{room}/cells?r0=&c0=&r1=&c1=`, both corners inclusive.
    Cells { room: String, r0: i32, c0: i32, r1: i32, c1: i32 },
    /// `GET /api/rooms/{room}/standings`
    Standings { room: String },
}

/// A status and a JSON body. Refusals are `{"error": "..."}` in the server's
/// own words — the ones a lobby or the console would have printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self { status: 200, body }
    }

    fn error(status: u16, why: impl Into<String>) -> Self {
        Self { status, body: json!({ "error": why.into() }) }
    }
}

/// The most cells one window may ask for: sixteen chunks' worth, a 256-square
/// window. Smaller than [`MOST_CHUNKS_AT_ONCE`] because this is JSON — see
/// [server.md].
///
/// [`MOST_CHUNKS_AT_ONCE`]: crate::server::MOST_CHUNKS_AT_ONCE
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#the-api
pub const MOST_CELLS_IN_A_WINDOW: usize = 16 * CHUNK_CELLS;

/// Answer one request against the worlds.
pub fn handle(rooms: &mut Rooms, req: Request) -> Reply {
    match req {
        Request::Rooms => Reply::ok(json!({ "rooms": rooms.listing() })),
        Request::Room { room } => with_room(rooms, &room, |s, id| {
            let lobby = lobby_of(s);
            Reply::ok(json!({
                "id": id,
                "name": s.room(),
                "phase": s.phase(),
                "tick": s.tick(),
                "rules": s.rules(),
                "victory": s.victory(),
                "world": s.world().kind(),
                "players": lobby.players.iter().map(|p| seat_json(s, p.id)).collect::<Vec<_>>(),
                "teams": lobby.teams,
                "started_by": lobby.started_by,
                "standings": crate::net::standings(s.world()),
            }))
        }),
        Request::Bots { room } => with_room(rooms, &room, |s, _| {
            Reply::ok(json!({
                "bots": s.bots().map(|(seat, _)| seat_json(s, seat)).collect::<Vec<_>>(),
            }))
        }),
        Request::AddBot { room, name, level, team } => with_room(rooms, &room, |s, _| {
            let level = level.unwrap_or_default();
            let name = name.unwrap_or_else(|| format!("{} bot", level.name()));
            match s.add_bot(name, level, Driver::Book, team) {
                Ok(seat) => Reply::ok(json!({ "seat": seat })),
                Err(why) => Reply::error(409, why),
            }
        }),
        Request::Sit { room, name, team } => with_room(rooms, &room, |s, _| {
            match s.add_bot(name, Level::default(), Driver::External, team) {
                Ok(seat) => Reply::ok(json!({ "seat": seat })),
                Err(why) => Reply::error(409, why),
            }
        }),
        Request::RemoveBot { room, seat } => with_room(rooms, &room, |s, _| {
            if !s.is_bot(seat) {
                return not_the_apis(s, seat);
            }
            match s.remove_bot(seat) {
                Ok(()) => Reply::ok(json!({ "seat": seat })),
                Err(why) => Reply::error(409, why),
            }
        }),
        Request::Act { room, seat, action } => with_room(rooms, &room, |s, _| {
            match s.bot(seat).map(|b| &b.driver) {
                None => return not_the_apis(s, seat),
                Some(Driver::Book) => {
                    return Reply::error(409, format!("seat {} is played by the server", seat.0));
                }
                Some(Driver::External) => {}
            }
            let tick = s.tick();
            let stamped = Stamped { tick, player: s.plays_as(seat), seat, action };
            match s.act(stamped) {
                Ok(()) => Reply::ok(json!({ "accepted": true, "tick": tick })),
                Err(why) => Reply::ok(json!({ "accepted": false, "reason": why, "tick": tick })),
            }
        }),
        Request::Seat { room, seat } => with_room(rooms, &room, |s, _| {
            if !s.players().any(|p| p.id == seat) {
                return Reply::error(404, format!("nobody has seat {} here", seat.0));
            }
            Reply::ok(seat_json(s, seat))
        }),
        Request::Chunk { room, row, col } => with_room(rooms, &room, |s, _| {
            // An absent chunk is an empty one, which is an answer and not a
            // failure: nothing has ever lived there.
            let dead = Chunk::dead();
            let chunk = s.world().chunk_at((row, col)).unwrap_or(&dead);
            let n = CHUNK_N as i32;
            let cells: Vec<Vec<Value>> =
                (0..n).map(|r| (0..n).map(|c| cell_json(chunk.get(r, c))).collect()).collect();
            Reply::ok(json!({ "row": row, "col": col, "tick": s.tick(), "cells": cells }))
        }),
        Request::Cells { room, r0, c0, r1, c1 } => with_room(rooms, &room, |s, _| {
            if r1 < r0 || c1 < c0 {
                return Reply::error(
                    400,
                    "a window's far corner is below and right of its near one",
                );
            }
            let (rows, cols) = (r1 as i64 - r0 as i64 + 1, c1 as i64 - c0 as i64 + 1);
            let most = MOST_CELLS_IN_A_WINDOW as i64;
            // Each side against the cap before they are multiplied: the
            // corners are i32, so a window over the whole of that is 2^64
            // cells, and the product overflowed and took the simulation task
            // down with it.
            if rows > most || cols > most || rows * cols > most {
                return Reply::error(
                    413,
                    format!("a window may hold {most} cells; that is {rows} by {cols}"),
                );
            }
            let world = s.world();
            let cells: Vec<Vec<Value>> = (r0..=r1)
                .map(|r| {
                    (c0..=c1)
                        .map(|c| cell_json(world.cell_at(r, c).unwrap_or(Cell::DEAD)))
                        .collect()
                })
                .collect();
            Reply::ok(json!({
                "r0": r0, "c0": c0, "r1": r1, "c1": c1, "tick": s.tick(), "cells": cells,
            }))
        }),
        Request::Standings { room } => with_room(rooms, &room, |s, _| {
            Reply::ok(json!({ "tick": s.tick(), "held": crate::net::standings(s.world()) }))
        }),
    }
}

/// Resolve a room the way a `Join` does — id, name or code — and answer with
/// it, or with the refusal a join would get.
fn with_room(
    rooms: &mut Rooms,
    asked: &str,
    then: impl FnOnce(&mut crate::server::Server, &crate::net::RoomId) -> Reply,
) -> Reply {
    match rooms.resolve(Some(asked)) {
        Ok(id) => {
            let server = rooms.get_mut(&id).expect("resolve only returns rooms that are here");
            then(server, &id)
        }
        Err(why) => Reply::error(404, why),
    }
}

/// A seat the API may not move or take away. **Nobody in it is a 404 and
/// somebody in it is a 409**, the line the room refusals draw between a room
/// that is not here and one that will not take it.
fn not_the_apis(server: &crate::server::Server, seat: PlayerId) -> Reply {
    if server.players().any(|p| p.id == seat && p.online) {
        Reply::error(409, format!("seat {} is a person's", seat.0))
    } else {
        Reply::error(404, format!("nobody is in seat {}", seat.0))
    }
}

fn lobby_of(server: &crate::server::Server) -> crate::net::Lobby {
    match server.lobby() {
        crate::net::ServerMessage::Match(lobby) => lobby,
        _ => unreachable!("a lobby is a Match"),
    }
}

/// One seat: who, what number its cells carry, where its ground is, what it
/// has to spend, and whether the server or something outside is playing it.
fn seat_json(server: &crate::server::Server, seat: PlayerId) -> Value {
    let row = server.players().find(|p| p.id == seat);
    let bot = server.bot(seat);
    let plays_as = server.plays_as(seat);
    json!({
        "seat": seat,
        "name": row.map(|p| p.name.as_str()),
        "online": row.is_some_and(|p| p.online),
        "forfeited": row.is_some_and(|p| p.forfeited),
        "plays_as": plays_as,
        // What a `Welcome` tells a client, for the same reason: an engine
        // that had to guess where it was granted ground would poll the wrong
        // region.
        "spawn": crate::net::spawn_for(plays_as, server.world()),
        "purse": server.value_of(seat),
        "bot": bot.is_some(),
        // **No level for a seat something outside plays.** A `Level` is how
        // often the server moves a seat and out of which book, and it moves
        // this one never -- so a number here was a claim about whatever is
        // driving it, which this server knows nothing about.
        "level": bot.filter(|b| matches!(b.driver, Driver::Book)).map(|b| b.level),
        "driver": bot.map(|b| match b.driver {
            Driver::Book => "book",
            Driver::External => "api",
        }),
    })
}

fn cell_json(cell: Cell) -> Value {
    let kind = match cell.kind() {
        Kind::NORMAL => "life",
        Kind::FACTORY => "factory",
        Kind::TURRET => "turret",
        Kind::DYNAMITE => "dynamite",
        _ => "unknown",
    };
    json!({
        "player": cell.player().0,
        "kind": kind,
        "alive": cell.is_alive(),
        "ice": cell.is_ice(),
        "age": cell.age(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Placement;
    use crate::server::Server;
    use crate::sim::World;

    fn rooms() -> Rooms {
        Rooms::just(Server::named("main", World::infinite_empty()))
    }

    fn seat_of(reply: &Reply) -> PlayerId {
        PlayerId(reply.body["seat"].as_u64().expect("no seat in the reply") as u8)
    }

    /// The listing is the one the menu gets, and a room that is not here is a
    /// 404 in the words a join would be refused with.
    #[test]
    fn the_listing_is_the_menus_and_an_unknown_room_is_a_404() {
        let mut rooms = rooms();
        let reply = handle(&mut rooms, Request::Rooms);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body["rooms"][0]["name"], "main");

        let reply = handle(&mut rooms, Request::Room { room: "nowhere".into() });
        assert_eq!(reply.status, 404);
        assert!(reply.body["error"].as_str().unwrap().contains("no room"), "{reply:?}");

        let reply = handle(&mut rooms, Request::Room { room: "main".into() });
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body["phase"], "Open");
        assert_eq!(reply.body["players"].as_array().unwrap().len(), 0);
    }

    /// A bot added here is a bot the lobby lists and the console would, and
    /// taking it away vacates the seat; a seat nobody is in is a 404, a
    /// person's is a 409, and so is a match under way.
    #[test]
    fn bots_are_added_listed_and_removed() {
        let mut rooms = rooms();
        let added = handle(
            &mut rooms,
            Request::AddBot {
                room: "main".into(),
                name: None,
                level: Some(Level::Hard),
                team: None,
            },
        );
        assert_eq!(added.status, 200, "{added:?}");
        let seat = seat_of(&added);

        let listed = handle(&mut rooms, Request::Bots { room: "main".into() });
        assert_eq!(listed.body["bots"][0]["seat"], seat.0);
        assert_eq!(listed.body["bots"][0]["level"], "hard");
        assert_eq!(listed.body["bots"][0]["name"], "hard bot");
        let room = handle(&mut rooms, Request::Room { room: "main".into() });
        assert_eq!(room.body["players"][0]["bot"], true);

        let gone = handle(&mut rooms, Request::RemoveBot { room: "main".into(), seat });
        assert_eq!(gone.status, 200);
        assert_eq!(
            handle(&mut rooms, Request::Bots { room: "main".into() }).body["bots"],
            json!([])
        );
        let again = handle(&mut rooms, Request::RemoveBot { room: "main".into(), seat });
        assert_eq!(again.status, 404, "{again:?}");

        rooms.get_mut(&"main".into()).unwrap().join_with("me", None).unwrap();
        let person = PlayerId(2);
        let not_a_bot =
            handle(&mut rooms, Request::RemoveBot { room: "main".into(), seat: person });
        assert_eq!(not_a_bot.status, 409, "{not_a_bot:?}");

        rooms
            .new_match(
                "dawn",
                crate::sim::WorldKind::Infinite,
                crate::net::Victory::Timer { generations: 10 },
            )
            .unwrap();
        rooms.start_match("dawn").unwrap();
        let late = handle(
            &mut rooms,
            Request::AddBot { room: "dawn".into(), name: None, level: None, team: None },
        );
        assert_eq!(late.status, 409, "{late:?}");
    }

    /// **An engine's seat is judged as a client is.** Its action is accepted
    /// or refused with the reason `act` gives, at the tick it was priced at;
    /// a seat the server plays and a person's are not the API's to move, and
    /// the standings are what a `Standing` carries.
    #[test]
    fn an_external_seat_acts_and_is_refused_in_the_servers_words() {
        let mut rooms = rooms();
        let sat = handle(
            &mut rooms,
            Request::Sit { room: "main".into(), name: "engine".into(), team: None },
        );
        assert_eq!(sat.status, 200, "{sat:?}");
        let seat = seat_of(&sat);
        let me = handle(&mut rooms, Request::Seat { room: "main".into(), seat });
        assert_eq!(me.body["driver"], "api");
        // A level says how often the server moves a seat, and it never moves
        // this one -- so reporting one was a claim about somebody else's
        // program.
        assert!(me.body["level"].is_null(), "an external seat has no level");
        assert_eq!(me.body["purse"], crate::sim::Player::STARTING_VALUE);

        let (row, col) = crate::net::spawn_for(seat, rooms.get(&"main".into()).unwrap().world());
        let act = |cells: Vec<(i32, i32)>, placement| Request::Act {
            room: "main".into(),
            seat,
            action: Action::Paint { cells, placement },
        };

        let taken =
            handle(&mut rooms, act(vec![(row + 2, col + 2), (row + 2, col + 3)], Placement::Life));
        assert_eq!(taken.body["accepted"], true, "{taken:?}");
        assert_eq!(taken.body["tick"], 0);

        let refused = handle(&mut rooms, act(vec![(row + 500, col + 500)], Placement::Life));
        assert_eq!(refused.status, 200);
        assert_eq!(refused.body["accepted"], false);
        assert_eq!(refused.body["reason"], "nothing of yours reaches there");

        let nobody = handle(
            &mut rooms,
            Request::Act {
                room: "main".into(),
                seat: PlayerId(7),
                action: Action::Paint { cells: vec![], placement: Placement::Life },
            },
        );
        assert_eq!(nobody.status, 404);
        let person = rooms.get_mut(&"main".into()).unwrap().join("someone").unwrap();
        let theirs = handle(
            &mut rooms,
            Request::Act {
                room: "main".into(),
                seat: person,
                action: Action::Paint { cells: vec![], placement: Placement::Life },
            },
        );
        assert_eq!(theirs.status, 409, "{theirs:?}");

        let book = handle(
            &mut rooms,
            Request::AddBot { room: "main".into(), name: None, level: None, team: None },
        );
        let not_ours = handle(
            &mut rooms,
            Request::Act {
                room: "main".into(),
                seat: seat_of(&book),
                action: Action::Paint { cells: vec![], placement: Placement::Life },
            },
        );
        assert_eq!(not_ours.status, 409, "{not_ours:?}");

        // Applied on the next step, where the cells are.
        rooms.get_mut(&"main".into()).unwrap().step();
        let window = handle(
            &mut rooms,
            Request::Cells {
                room: "main".into(),
                r0: row + 1,
                c0: col + 1,
                r1: row + 3,
                c1: col + 4,
            },
        );
        assert_eq!(window.status, 200, "{window:?}");
        let rows = window.body["cells"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].as_array().unwrap().len(), 4);
        assert_eq!(rows[1][1]["player"], seat.0, "the cell laid is not there");

        let standings = handle(&mut rooms, Request::Standings { room: "main".into() });
        assert_eq!(standings.status, 200, "{standings:?}");
        assert_eq!(standings.body["tick"], 1);
        assert!(standings.body["held"].is_array(), "{standings:?}");
    }

    /// A window is bounded, because it is JSON: sixteen chunks' worth and no
    /// more, and a corner the wrong way round is a bad request rather than an
    /// empty answer.
    #[test]
    fn a_window_is_capped_and_a_chunk_that_is_not_held_is_empty() {
        let mut rooms = rooms();
        let too_big = handle(
            &mut rooms,
            Request::Cells { room: "main".into(), r0: 0, c0: 0, r1: 300, c1: 300 },
        );
        assert_eq!(too_big.status, 413, "{too_big:?}");
        let backwards =
            handle(&mut rooms, Request::Cells { room: "main".into(), r0: 5, c0: 0, r1: 0, c1: 5 });
        assert_eq!(backwards.status, 400);
        // The corners of i32, which is 2^64 cells and does not fit the number
        // that used to hold the area; the task answering this panicked, and
        // every later request found nobody to answer it. Then rows over the
        // whole range and columns over half, which wrapped to i64::MIN and
        // passed.
        for (r0, r1, c0, c1) in
            [(i32::MIN, i32::MAX, i32::MIN, i32::MAX), (i32::MIN, i32::MAX, 0, i32::MAX / 2)]
        {
            let whole = handle(&mut rooms, Request::Cells { room: "main".into(), r0, c0, r1, c1 });
            assert_eq!(whole.status, 413, "{whole:?}");
        }
        let just = handle(
            &mut rooms,
            Request::Cells { room: "main".into(), r0: 0, c0: 0, r1: 255, c1: 255 },
        );
        assert_eq!(just.status, 200);

        let chunk = handle(&mut rooms, Request::Chunk { room: "main".into(), row: 40, col: -40 });
        assert_eq!(chunk.status, 200);
        let rows = chunk.body["cells"].as_array().unwrap();
        assert_eq!(rows.len(), CHUNK_N);
        assert_eq!(rows[0][0]["alive"], false);
        assert_eq!(rows[0][0]["kind"], "life");
    }
}
