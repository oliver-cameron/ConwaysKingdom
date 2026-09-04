//! Prove a library actually survives leaving a server and coming back.
//!
//!     cargo run --example locker -- ws://127.0.0.1:8080/ws [ROOM]
//!
//! Joins with a fresh key, checks the server hands back an **empty** locker,
//! offers one, disconnects, then joins again with the *same* key on a *new*
//! socket and checks the same patterns come back.
//!
//! Here for the reason [`join`](../join.rs) is here, and one of its own:
//! `server::ws` is the only module behind the `server` feature, no test opens
//! a socket, and the round trip this checks is spread over `net::kept`,
//! `server::lockers`, `Rooms::handle` and the connection task — four places
//! that agree only if something actually asks them to.
//!
//! Run it against a server started with `--rooms` pointing somewhere
//! disposable; it writes a person and a library into that directory.

use conwayskingdom::net::kept::{Kept, Stamp};
use conwayskingdom::net::link::Link;
use conwayskingdom::net::{ClientMessage, RoomId, Secret, ServerMessage};
use std::time::{Duration, Instant};

/// Wait for the locker a join hands back, or say what arrived instead.
fn locker_after_joining(url: &str, room: Option<RoomId>, key: &Secret) -> Result<Kept, String> {
    let mut link = Link::connect(url.to_string());
    link.send(ClientMessage::Join { name: "locker".into(), room, person: Some(key.clone()) });

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        for msg in link.drain() {
            match msg {
                ServerMessage::Yours(kept) => return Ok(kept),
                ServerMessage::Rejected { reason } => return Err(format!("refused — {reason}")),
                _ => {}
            }
        }
        if link.is_closed() {
            return Err("the socket closed before a locker arrived".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("no locker arrived in twenty seconds".into())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: locker <ws url> [room]");
    let room = std::env::args().nth(2).map(RoomId);

    // A key nobody has used, so this is a person the server has not met and
    // its locker is empty for the honest reason rather than by luck.
    let key = Secret::new().expect("no entropy");
    println!("as a person this server has never met");

    let empty = match locker_after_joining(&url, room.clone(), &key) {
        Ok(kept) => kept,
        Err(why) => return println!("FAILED on the first join: {why}"),
    };
    if !empty.is_empty() {
        return println!("FAILED: a locker appeared from nowhere: {empty:?}");
    }
    println!("  the locker came back empty, which is what asks for one");

    // Offer one, on a second socket, the way a client does after being told
    // there is nothing here for it.
    let mut glider = Stamp::trimmed(vec![(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
    glider.name = "glider".into();
    let offered = Kept { stamps: vec![glider], games: Vec::new() };

    let mut link = Link::connect(url.clone());
    link.send(ClientMessage::Join {
        name: "locker".into(),
        room: room.clone(),
        person: Some(key.clone()),
    });
    // The `Keep` has to follow the `Welcome`, because that is when the
    // connection learns whose it is -- see `rooms::Caller::person`.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut sent = false;
    while Instant::now() < deadline && !sent {
        for msg in link.drain() {
            if matches!(msg, ServerMessage::Welcome { .. }) {
                link.send(ClientMessage::Keep(offered.clone()));
                link.send(ClientMessage::Leave);
                sent = true;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !sent {
        return println!("FAILED: never got a welcome to offer a locker after");
    }
    // Give the server a moment to apply it before the socket goes.
    std::thread::sleep(Duration::from_millis(300));
    drop(link);
    println!("  offered a library of one pattern, then left");

    // And back, on a new socket, as the same person.
    match locker_after_joining(&url, room, &key) {
        Ok(kept) if kept.stamps.len() == 1 && kept.stamps[0].name == "glider" => {
            println!("  it came back: {:?}", kept.stamps[0].name);
            println!("OK — a library survived the socket that made it");
        }
        Ok(kept) => println!("FAILED: the locker came back as {kept:?}"),
        Err(why) => println!("FAILED on the second join: {why}"),
    }
}
