//! Connect, take the server's world, and report what arrived.
use conwayskingdom::net::link::Link;
use conwayskingdom::net::{ClientMessage, ServerMessage};
use conwayskingdom::sim::{Chunk, World};
use std::time::{Duration, Instant};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).unwrap();
    let mut link = Link::connect(url);
    link.send(ClientMessage::Join { name: "late".into() });

    let mut world = World::infinite_empty();
    let mut chunks = 0;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        for msg in link.drain() {
            match msg {
                ServerMessage::Welcome { you, tick } => {
                    println!("joined as {you:?} at tick {tick}");
                    world.set_generation(tick);
                    let want = World::chunks_covering((-40, -40), (40, 40));
                    println!("subscribing to {} chunks", want.len());
                    link.send(ClientMessage::Subscribe { chunks: want });
                }
                ServerMessage::ChunkData { tick, chunk, cells } => {
                    let c: &Chunk = bytemuck::from_bytes(&cells);
                    world.set_generation(tick);
                    world.put_chunk(chunk, *c);
                    chunks += 1;
                }
                _ => {}
            }
        }
        if chunks > 0 { break; }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("received {chunks} chunk(s); local world now has {} live cells at tick {}",
             world.live_cells().len(), world.generation);
    println!("digest {:x}", world.digest());
}
