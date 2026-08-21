//! Prove a late join actually got the server's world.
//!
//!     cargo run --example join -- ws://127.0.0.1:8080/ws
//!
//! Joins, takes whatever chunks the server sends, then checkpoints its own
//! per-chunk digests back. A silent reply means every chunk it holds matches
//! the server byte for byte; a Resync names the ones that do not.

use conwayskingdom::net::link::Link;
use conwayskingdom::net::{ClientMessage, ServerMessage};
use conwayskingdom::sim::{Chunk, World};
use std::time::{Duration, Instant};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: join <ws url>");
    let mut link = Link::connect(url);
    link.send(ClientMessage::Join { name: "late".into(), token: None });

    let mut world = World::infinite_empty();
    let (mut tick, mut chunks, mut checkpointed, mut verdict) = (0, 0, false, None);

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && verdict.is_none() {
        for msg in link.drain() {
            match msg {
                ServerMessage::Welcome { you, tick: t, .. } => {
                    println!("joined as {you:?} at tick {t}");
                    tick = t;
                    world.set_generation(t);
                    link.send(ClientMessage::Subscribe {
                        chunks: World::chunks_covering((-64, -64), (64, 64)),
                    });
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
            println!("received {chunks} chunk(s), {} live cells at tick {tick}",
                     world.live_cells().len());
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
