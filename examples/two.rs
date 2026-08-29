//! Two clients on one server: do they end up with the same world?
//!
//!     cargo run --example two -- ws://127.0.0.1:8080/ws [ROOM]
//!
//! The real link and the real protocol, because the question is about what
//! crosses the socket. Each side keeps a world and folds messages into it the
//! way the client does — adopt on welcome, take chunks, predict its own
//! actions, advance when told, checkpoint, resync — and one of them acts.
//!
//! What it measures is not that they never disagree. A client predicts, and
//! the server applies whenever the message lands, so a disagreement of one
//! generation is possible by design. What must hold is that a disagreement is
//! **found and put right**: so this reports the first generation they differ
//! at, and the generation by which they agree again.

use conwayskingdom::net::link::Link;
use conwayskingdom::net::{Action, ClientMessage, Placement, ServerMessage, Stamped};
use conwayskingdom::sim::{Chunk, PlayerId, World};
use std::time::Duration;

/// A client, minus everything that draws.
struct Peer {
    name: &'static str,
    link: Link,
    world: World,
    me: PlayerId,
    spawn: (i32, i32),
    /// Actions this peer has been told about, so the comparison can wait
    /// until both have heard the same news. A client predicts its own action
    /// the moment it is made, so before the server confirms it the two
    /// legitimately differ by exactly that action -- which is latency, not
    /// divergence.
    heard: usize,
    /// How many times the server has had to put this peer right.
    resyncs: usize,
}

impl Peer {
    fn join(url: &str, name: &'static str, room: Option<String>) -> Self {
        let room = room.map(conwayskingdom::net::RoomId);
        let mut link = Link::connect(url.to_string());
        link.send(ClientMessage::Join { name: name.into(), token: None, room, person: None });
        for _ in 0..200 {
            for msg in link.drain() {
                if let ServerMessage::Welcome { you, tick, spawn, room, world: shape, .. } = msg {
                    // Built to the shape the server named, as the client does.
                    // Assuming a plane against a wrapping server folds no
                    // coordinates, so the two peers would disagree about which
                    // chunk is which and this would report a divergence that
                    // was really a misunderstanding.
                    let mut world = shape.build();
                    world.set_generation(tick);
                    println!(
                        "{name}: {you:?} in room {room:?} at tick {tick}, ground at {spawn:?}"
                    );
                    return Self { name, link, world, me: you, spawn, heard: 0, resyncs: 0 };
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{name}: no welcome");
    }

    /// Everything waiting, folded in exactly as the client folds it.
    fn pump(&mut self) {
        for msg in self.link.drain() {
            match msg {
                ServerMessage::ChunkData { tick, chunk, cells } => {
                    if let Ok(c) = bytemuck::try_from_bytes::<Chunk>(&cells) {
                        if tick != self.world.generation {
                            println!(
                                "{}: chunk {chunk:?} is from tick {tick} and I am on {}",
                                self.name, self.world.generation
                            );
                        }
                        // What the client does, behind a switch, so the two
                        // behaviours can be compared rather than argued about.
                        if std::env::var_os("SNAP_ON_CHUNK").is_some() {
                            self.world.set_generation(tick);
                        }
                        self.world.put_chunk(chunk, *c);
                    }
                }
                ServerMessage::Step { tick, actions } => {
                    if !actions.is_empty() {
                        self.heard += actions.len();
                    }
                    for s in &actions {
                        conwayskingdom::net::apply(&mut self.world, s);
                    }
                    while self.world.generation < tick {
                        self.world.step();
                    }
                    if self.world.generation != tick {
                        self.world.set_generation(tick);
                    }
                    if self.world.generation % 12 == 0 {
                        let chunks: Vec<((i32, i32), u64)> = self
                            .world
                            .stored()
                            .iter()
                            .filter_map(|&(c, _)| Some((c, self.world.chunk_digest(c)?)))
                            .collect();
                        if !chunks.is_empty() {
                            self.link.send(ClientMessage::Checkpoint {
                                tick: self.world.generation,
                                chunks,
                            });
                        }
                    }
                }
                ServerMessage::Resync { tick, chunks } => {
                    println!(
                        "{}: server says {} chunks are wrong at {tick}; refetching",
                        self.name,
                        chunks.len()
                    );
                    self.resyncs += 1;
                    self.link.send(ClientMessage::Subscribe { chunks });
                }
                _ => {}
            }
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: two <ws url> [room]");
    // Both peers into the same room, or they are two worlds and cannot
    // disagree about anything.
    let room = std::env::args().nth(2);

    let mut a = Peer::join(&url, "alice", room.clone());
    let mut b = Peer::join(&url, "bob", room);
    assert_ne!(a.me, b.me);

    // Both look at both grants, so neither is missing ground the other has.
    let chunks: Vec<(i32, i32)> = [a.spawn, b.spawn]
        .iter()
        .flat_map(|&(r, c)| {
            (-1..=1).flat_map(move |dr| {
                (-1..=1).map(move |dc| (r.div_euclid(16) + dr, c.div_euclid(16) + dc))
            })
        })
        .collect();
    for p in [&mut a, &mut b] {
        p.link.send(ClientMessage::Subscribe { chunks: chunks.clone() });
    }
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(50));
        a.pump();
        b.pump();
    }

    // Alice draws something that will grow, on her own ground.
    let cells: Vec<(i32, i32)> = (0..3).map(|i| (a.spawn.0 + 8, a.spawn.1 + 2 + i)).collect();
    println!("alice: painting {cells:?}");
    let stamped = Stamped {
        tick: a.world.generation,
        player: a.me,
        action: Action::Paint { cells, placement: Placement::Life },
    };
    // Predicted here and sent, which is what the client does.
    conwayskingdom::net::apply(&mut a.world, &stamped);
    a.link.send(ClientMessage::Act(stamped));

    // A deliberate lie, so the safety net is tested rather than assumed: bob
    // invents a cell nobody told him about. Nothing in the protocol can stop
    // this happening for real -- a dropped prediction, a bad step, a bug -- so
    // what matters is that it is noticed and put right.
    // With LIE=1 in the environment. Off by default so the example answers
    // "do two clients agree", and on when the question is "and what happens
    // when they do not".
    let mut lied = false;

    let (mut checked, mut disagreed) = (0, 0);
    let mut first_split: Option<u64> = None;
    let mut healed_at: Option<u64> = None;
    for _ in 0..400 {
        std::thread::sleep(Duration::from_millis(50));
        a.pump();
        b.pump();
        // Not until both have been told about it: until then alice is ahead by
        // her own prediction, which is latency rather than divergence.
        if a.heard == 0 || b.heard == 0 || a.world.generation != b.world.generation {
            continue;
        }
        checked += 1;
        if std::env::var_os("LIE").is_some() && !lied && checked > 20 {
            // A block, because a lone cell dies of loneliness next
            // generation and heals the lie without anybody noticing. A still
            // life persists, so only the checkpoint can put it right.
            let (r, c) = (b.spawn.0 + 3, b.spawn.1 + 3);
            for (dr, dc) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                b.world.set_cell_at(r + dr, c + dc, conwayskingdom::sim::Cell::alive(b.me));
            }
            println!("bob: invented a block at ({r}, {c}) that nobody sent him");
            lied = true;
            continue;
        }
        if a.world.digest() == b.world.digest() {
            if first_split.is_some() && healed_at.is_none() {
                healed_at = Some(a.world.generation);
            }
        } else {
            disagreed += 1;
            if first_split.is_none() {
                println!("first disagreement at generation {}", a.world.generation);
                first_split = Some(a.world.generation);
            }
        }
    }

    println!("compared {checked} shared generations, {disagreed} of them disagreeing");
    println!("resyncs: alice {}, bob {}", a.resyncs, b.resyncs);
    match (first_split, healed_at) {
        (None, _) => println!("they never disagreed"),
        (Some(at), Some(back)) => {
            println!("disagreed at {at}, agreed again by {back} -- {} generations", back - at)
        }
        (Some(at), None) => println!("DISAGREED at {at} AND NEVER RECOVERED"),
    }
}
