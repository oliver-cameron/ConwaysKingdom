//! Headless authoritative server.
//!
//!     cargo run --bin server --no-default-features -- [generations]
//!
//! `--no-default-features` turns off `render`, so neither wgpu nor winit is
//! compiled. There is no transport yet; this steps the world and reports it.

use conwayskingdom::net::{Action, ClientMessage, Stamped};
use conwayskingdom::server::Server;
use conwayskingdom::sim::World;

fn main() {
    let gens: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200);

    let mut server = Server::new(World::infinite());
    let alice = server.join("alice").expect("first player");
    let bob = server.join("bob").expect("second player");
    println!("alice = {alice:?}, bob = {bob:?}");

    // Stand-in for input arriving over a socket: a blinker each, well apart.
    for (who, origin) in [(alice, (100, 100)), (bob, (-60, -60))] {
        server.handle(
            Some(who),
            ClientMessage::Act(Stamped {
                tick: server.tick(),
                player: who,
                action: Action::Paint {
                    cells: (0..3).map(|i| (origin.0, origin.1 + i)).collect(),
                },
            }),
        );
    }

    println!("{:>6} {:>7} {:>5} {:>18}", "tick", "chunks", "live", "digest");
    for _ in 0..gens {
        let out = server.step();
        if !out.is_empty() {
            println!("  -> {} message(s) to broadcast", out.len());
        }
        if server.tick() % (gens / 10).max(1) == 0 {
            println!(
                "{:>6} {:>7} {:>5} {:>18x}",
                server.tick(),
                server.world().stored_count(),
                server.world().live_cells().len(),
                server.world().digest(),
            );
        }
    }
    println!("{} players connected", server.player_count());
}
