//! Two clients on one server: does what one does reach the other?
//!
//!     cargo run --example two -- ws://127.0.0.1:8080/ws
//!
//! Uses the real link and the real protocol, because the question is about
//! what actually crosses the socket.

use conwayskingdom::net::link::Link;
use conwayskingdom::net::{Action, ClientMessage, Placement, ServerMessage, Stamped};
use conwayskingdom::sim::{PlayerId, World};
use std::time::Duration;

fn wait_for_welcome(link: &mut Link, who: &str) -> (PlayerId, u64, (i32, i32)) {
    for _ in 0..200 {
        for msg in link.drain() {
            if let ServerMessage::Welcome { you, tick, spawn, .. } = msg {
                println!("{who}: welcomed as {you:?} at tick {tick}, ground at {spawn:?}");
                return (you, tick, spawn);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("{who}: no welcome");
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let url = std::env::args().nth(1).expect("usage: two <ws url>");

    let mut a = Link::connect(url.clone());
    a.send(ClientMessage::Join { name: "alice".into(), token: None });
    let (alice, _, spawn) = wait_for_welcome(&mut a, "alice");

    let mut b = Link::connect(url);
    b.send(ClientMessage::Join { name: "bob".into(), token: None });
    let (bob, tick, _) = wait_for_welcome(&mut b, "bob");
    assert_ne!(alice, bob, "they must be different players");

    // Bob asks for the chunk alice's ground is in, so he is looking at it.
    let chunk = (spawn.0.div_euclid(16), spawn.1.div_euclid(16));
    b.send(ClientMessage::Subscribe { chunks: vec![chunk] });
    std::thread::sleep(Duration::from_millis(300));
    for msg in b.drain() {
        if let ServerMessage::ChunkData { chunk, .. } = msg {
            println!("bob: was sent chunk {chunk:?}");
        }
    }

    // Alice draws on her own ground.
    let cells: Vec<(i32, i32)> = (0..4).map(|i| (spawn.0 + 8, spawn.1 + i)).collect();
    println!("alice: painting {cells:?}");
    a.send(ClientMessage::Act(Stamped {
        tick,
        player: alice,
        action: Action::Paint { cells: cells.clone(), placement: Placement::Life },
    }));

    // Bob keeps a world and folds into it exactly what the client folds in:
    // adopt on welcome, take chunks, apply actions. If the cells show up here
    // the wire and the rules are fine and the problem is what is drawn.
    let mut world = World::infinite_empty();
    world.set_generation(tick);

    // And bob waits to be told.
    for round in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        for msg in b.drain() {
            match msg {
                ServerMessage::Step { tick, actions } => {
                    if actions.is_empty() { continue }
                    let _ = tick;
                    println!("bob: heard {} action(s) after {}ms", actions.len(), round * 100);
                    for s in &actions {
                        conwayskingdom::net::apply(&mut world, s);
                    }
                    let seen = cells
                        .iter()
                        .filter(|&&(r, c)| {
                            world.cell_at(r, c).is_some_and(|cell| cell.is_alive())
                        })
                        .count();
                    println!(
                        "bob: {seen} of alice's {} cells are alive in his world",
                        cells.len()
                    );
                    let owner = world.cell_at(cells[0].0, cells[0].1).map(|c| c.player());
                    println!("bob: and they belong to {owner:?}");
                    return;
                }
                ServerMessage::ChunkData { chunk, cells, .. } => {
                    if let Ok(c) = bytemuck::try_from_bytes::<conwayskingdom::sim::Chunk>(&cells) {
                        world.put_chunk(chunk, *c);
                    }
                    println!("bob: chunk {chunk:?}");
                }
                other => println!("bob: {other:?}"),
            }
        }
    }
    println!("bob: heard NOTHING about alice's paint");
}
