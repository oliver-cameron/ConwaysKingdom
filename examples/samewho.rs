//! Does a person survive the server closing? Run twice against two runs of a
//! server pointed at the same --rooms directory, with the same key.
//!
//!     cargo run --example samewho -- ws://127.0.0.1:8102/ws <32 hex chars>
use conwayskingdom::net::link::Link;
use conwayskingdom::net::{ClientMessage, RoomId, Secret, ServerMessage};
use std::time::{Duration, Instant};

fn main() {
    let url = std::env::args().nth(1).expect("usage: samewho <ws url> <key>");
    let key = Secret::read(&std::env::args().nth(2).expect("a key")).expect("not a key");
    let mut link = Link::connect(url);
    link.send(ClientMessage::Join {
        name: "hugh".into(),
        room: Some(RoomId::from("main")),
        person: Some(key),
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        for msg in link.drain() {
            if let ServerMessage::Welcome { you, profile, value, .. } = msg {
                println!(
                    "seat {you:?}  purse {value}  person {}",
                    profile.map(|p| p.who.to_string()).unwrap_or_else(|| "NONE".into())
                );
                return;
            }
            if let ServerMessage::Rejected { reason } = msg {
                println!("refused: {reason}");
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("no welcome");
}
