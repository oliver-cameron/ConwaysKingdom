//! Prove a late join actually got the server's world.
//!
//!     cargo run --example join -- ws://127.0.0.1:8080/ws [ROOM]
//!
//! Joins, takes whatever chunks the server sends, then checkpoints its own
//! per-chunk digests back. A silent reply means every chunk it holds matches
//! the server byte for byte; a Resync names the ones that do not.
//!
//! `ROOM` picks which world on that server. Without it the server decides,
//! and either way the `Welcome` says which room it was and what shape that
//! room's world is — which this then **builds**, rather than assuming a plane.
//! Assuming one against a wrapping server is the failure this exists to catch:
//! nothing about the chunks that arrive says the ground ends, so the seam
//! shows only once something crosses it.

use conwayskingdom::net::link::Link;
use conwayskingdom::net::{ClientMessage, ServerMessage};
use conwayskingdom::sim::{Chunk, World, WorldKind};
use std::time::{Duration, Instant};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: join <ws url> [room]");
    let room = std::env::args().nth(2);
    let mut link = Link::connect(url);
    link.send(ClientMessage::Join { name: "late".into(), token: None, room });

    // Replaced on Welcome by a world of the shape the server named. A client
    // always opens with something to look at, because a socket may never
    // connect at all and an empty screen is worse than a local game.
    let mut world = World::infinite_empty();
    let (mut tick, mut chunks, mut checkpointed, mut verdict) = (0, 0, false, None);

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && verdict.is_none() {
        for msg in link.drain() {
            match msg {
                ServerMessage::Welcome { you, tick: t, room, world: shape, .. } => {
                    println!(
                        "joined room {room:?} as {you:?} at tick {t}, in {}",
                        match shape {
                            WorldKind::Infinite => "a boundless world".to_string(),
                            WorldKind::Toroidal { rows, cols } =>
                                format!("a {rows}x{cols} world that wraps"),
                        }
                    );
                    tick = t;
                    world = shape.build();
                    world.set_generation(t);
                    link.send(ClientMessage::Subscribe {
                        chunks: World::chunks_covering((-64, -64), (64, 64)),
                    });
                }
                ServerMessage::Rejected { reason } => {
                    verdict = Some(format!("REFUSED — {reason}"));
                }
                ServerMessage::ChunkData { tick: t, chunk, cells } => {
                    let c: &Chunk = bytemuck::from_bytes(&cells);
                    tick = t;
                    world.set_generation(t);
                    world.put_chunk(chunk, *c);
                    chunks += 1;
                }
                ServerMessage::Resync { chunks: wrong, .. } => {
                    verdict = Some(format!("MISMATCH on {} chunk(s): {wrong:?}", wrong.len()));
                }
                _ => {}
            }
        }

        // Once chunks have arrived and gone quiet, check them against the
        // server rather than trusting that they looked right.
        if chunks > 0 && !checkpointed {
            checkpointed = true;
            let held: Vec<_> = world
                .stored()
                .iter()
                .map(|&(coord, _)| (coord, world.chunk_digest(coord).unwrap()))
                .collect();
            println!(
                "received {chunks} chunk(s), {} live cells at tick {tick}",
                world.live_cells().len()
            );
            println!("checkpointing {} chunk digests", held.len());
            link.send(ClientMessage::Checkpoint { tick, chunks: held });
            // A silent server means agreement, so give it a moment to object.
            let quiet = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < quiet && verdict.is_none() {
                for msg in link.drain() {
                    if let ServerMessage::Resync { chunks: wrong, .. } = msg {
                        verdict = Some(format!("MISMATCH on {} chunk(s)", wrong.len()));
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            verdict.get_or_insert_with(|| "MATCH — every chunk agrees with the server".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    println!("{}", verdict.unwrap_or_else(|| "TIMED OUT — no chunks arrived".into()));
}
