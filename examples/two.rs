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
//! **found and put right**: so this reports every generation they come to
//! differ at, and the generation by which they agree again.
//!
//! Four switches in the environment. `LIE=1` has one peer invent a block
//! nobody sent it, so the safety net is tested rather than assumed.
//! `OVERCLOCK=1` stands a block of overclockers over the blinker, so the
//! second pass runs on both sides of the socket and the same comparison
//! covers it. `DIFF=1` prints what a refetched chunk actually changed, cell
//! by cell, which is how a resync that is not a rules bug gets told apart
//! from one that is. `SNAP_ON_CHUNK=1` adopts a chunk's tick on arrival, the
//! way the client once did, for comparing the two behaviours.

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
        link.send(ClientMessage::Join { name: name.into(), room, person: None });
        for _ in 0..200 {
            for msg in link.drain() {
                if let ServerMessage::Welcome { you, tick, spawn, room, world: shape, .. } = msg {
                    // Built as the client builds it: to the shape the server
                    // named, and seeded from the room. Either left out is a
                    // divergence that is really a misunderstanding -- a plane
                    // against a torus folds no coordinates, and the wrong
                    // seed rolls different dice at every contested birth and
                    // every adjustment of the ground, which this reported as
                    // the server correcting both peers at every checkpoint.
                    let mut world = conwayskingdom::net::sane_world(shape, &room);
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
                        if std::env::var_os("DIFF").is_some() {
                            if let Some(old) = self.world.chunk_at(chunk) {
                                let mut n = 0;
                                for r in 0..conwayskingdom::sim::CHUNK_N {
                                    for k in 0..conwayskingdom::sim::CHUNK_N {
                                        if old[(r, k)] != c[(r, k)] {
                                            if n < 6 {
                                                println!(
                                                    "{}: {chunk:?} ({r},{k}) {:?} -> {:?}",
                                                    self.name,
                                                    old[(r, k)],
                                                    c[(r, k)]
                                                );
                                            }
                                            n += 1;
                                        }
                                    }
                                }
                                println!("{}: {chunk:?} at {tick}: {n} cells differ", self.name);
                            } else {
                                println!("{}: {chunk:?} at {tick}: not held before", self.name);
                            }
                        }
                        self.world.put_chunk(chunk, *c);
                    }
                }
                ServerMessage::Step { tick, actions } => {
                    if !actions.is_empty() {
                        self.heard += actions.len();
                    }
                    // **Not our own**, which were applied when they were made:
                    // a paint laid again a generation late is a different
                    // paint, and that is the gotcha this example exists to
                    // catch rather than commit.
                    for s in actions.iter().filter(|s| s.seat != self.me) {
                        conwayskingdom::net::apply(&mut self.world, s);
                    }
                    while self.world.generation < tick {
                        self.world.step();
                    }
                    if self.world.generation != tick {
                        self.world.set_generation(tick);
                    }
                    if self.world.generation.is_multiple_of(12) {
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
                    // A grant is announced this way too, so the first of these
                    // after joining is news rather than a correction.
                    println!(
                        "{}: server says {} chunks are wrong at {tick}: {chunks:?}; refetching",
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

    // Both look at both grants and the ring of chunks round each, so neither
    // is missing ground the other has -- a grant's influence has already crept
    // a chunk out by the time anybody joins, and a chunk the server holds and a
    // peer never asked for is a resync that says nothing about the rules.
    // Through `grant_chunks` rather than arithmetic of its own: this divided by
    // a chunk size of sixteen for four days after a chunk became sixty-four.
    let mut chunks: Vec<(i32, i32)> = [a.spawn, b.spawn]
        .iter()
        .flat_map(|&spawn| conwayskingdom::net::grant_chunks(&a.world, spawn))
        .flat_map(|(r, c)| (-1..=1).flat_map(move |dr| (-1..=1).map(move |dc| (r + dr, c + dc))))
        .map(|coord| a.world.canonical(coord))
        .collect();
    chunks.sort_unstable();
    chunks.dedup();
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
        seat: a.me,
        action: Action::Paint { cells, placement: Placement::Life },
    };
    // Predicted here and sent, which is what the client does.
    conwayskingdom::net::apply(&mut a.world, &stamped);
    a.link.send(ClientMessage::Act(stamped));

    // And a block of overclockers above it, with the blinker inside their
    // disc and the opening block outside anything they would feed a birth
    // to -- so both peers and the server run the second pass over something
    // that moves, and a digest that agrees is agreeing about it.
    if std::env::var_os("OVERCLOCK").is_some() {
        let cells: Vec<(i32, i32)> =
            [(2, 2), (2, 3), (3, 2), (3, 3)].map(|(r, c)| (a.spawn.0 + r, a.spawn.1 + c)).to_vec();
        println!("alice: standing overclockers at {cells:?}");
        let stamped = Stamped {
            tick: a.world.generation,
            player: a.me,
            seat: a.me,
            action: Action::Paint { cells, placement: Placement::Overclock },
        };
        conwayskingdom::net::apply(&mut a.world, &stamped);
        a.link.send(ClientMessage::Act(stamped));
    }

    // A deliberate lie, so the safety net is tested rather than assumed: bob
    // invents a cell nobody told him about. Nothing in the protocol can stop
    // this happening for real -- a dropped prediction, a bad step, a bug -- so
    // what matters is that it is noticed and put right.
    // With LIE=1 in the environment. Off by default so the example answers
    // "do two clients agree", and on when the question is "and what happens
    // when they do not".
    let mut lied = false;

    let (mut checked, mut disagreed) = (0, 0);
    // Every run of disagreement, as (split at, healed at). A prediction that
    // missed a server step is a run of one or two generations ending at the
    // next checkpoint; anything longer, or anything that never closes, is
    // the rules disagreeing.
    let mut runs: Vec<(u64, Option<u64>)> = Vec::new();
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
        let now = a.world.generation;
        let open = matches!(runs.last(), Some((_, None)));
        if a.world.digest() == b.world.digest() {
            if open {
                runs.last_mut().unwrap().1 = Some(now);
                println!("agreed again at generation {now}");
            }
        } else {
            disagreed += 1;
            if !open {
                println!("disagreement at generation {now}");
                runs.push((now, None));
            }
        }
    }

    println!("compared {checked} times at shared generations, {disagreed} of them disagreeing");
    println!("resyncs: alice {}, bob {}", a.resyncs, b.resyncs);
    if runs.is_empty() {
        println!("they never disagreed");
    }
    for (at, back) in runs {
        match back {
            Some(back) => {
                println!("disagreed at {at}, agreed again by {back} -- {} generations", back - at)
            }
            None => println!("DISAGREED at {at} AND NEVER RECOVERED"),
        }
    }
}
