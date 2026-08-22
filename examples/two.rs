//! Two clients on one server: do they end up with the same world?
//!
//!     cargo run --example two -- ws://127.0.0.1:8080/ws
//!
//! The real link and the real protocol, because the question is about what
//! crosses the socket. Each side keeps a world and folds messages into it the
//! way the client does — adopt on welcome, take chunks, apply actions, advance
//! when told — and one of them acts. Then their digests are compared every
//! generation. Two peers running the same deterministic step from the same
//! state must stay byte-identical; the first generation at which they do not
//! is the whole answer.

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
}

impl Peer {
    fn join(url: &str, name: &'static str) -> Self {
        let mut link = Link::connect(url.to_string());
        link.send(ClientMessage::Join { name: name.into(), token: None });
        for _ in 0..200 {
            for msg in link.drain() {
                if let ServerMessage::Welcome { you, tick, spawn, .. } = msg {
                    let mut world = World::infinite_empty();
                    world.set_generation(tick);
                    println!("{name}: {you:?} at tick {tick}, ground at {spawn:?}");
                    return Self { name, link, world, me: you, spawn, heard: 0 };
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
                        println!(
                            "{}: server says {tick}, I am on {}",
                            self.name, self.world.generation
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: two <ws url>");

    let mut a = Peer::join(&url, "alice");
    let mut b = Peer::join(&url, "bob");
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
    // Sent, not applied: the server applies it and says so, which is what
    // keeps every peer on the same generation for it. See `BattleApp::commit`.
    a.link.send(ClientMessage::Act(stamped));

    let mut checked = 0;
    for round in 0..120 {
        std::thread::sleep(Duration::from_millis(50));
        a.pump();
        b.pump();
        // Not until both have been told about it: until then alice is ahead
        // by her own prediction, which is latency rather than divergence.
        if a.heard == 0 || b.heard == 0 {
            continue;
        }
        if a.world.generation != b.world.generation {
            continue; // mid-flight; only compare when both are on the same tick
        }
        checked += 1;
        if a.world.digest() != b.world.digest() {
            println!(
                "DIVERGED at generation {} after {}ms",
                a.world.generation,
                round * 50
            );
            println!("  alice {} live, bob {} live", a.world.live_cells().len(), b.world.live_cells().len());
            return;
        }
    }
    println!(
        "agreed on all {checked} shared generations, up to {}",
        a.world.generation
    );
}
