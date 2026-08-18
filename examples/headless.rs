//! Run the simulation with no GPU and report how the world grows.
//!
//!     cargo run --example headless -- [generations] [infinite|tiled]

use conwayskingdom::{Neighbour, World};

fn main() {
    let mut args = std::env::args().skip(1);
    let gens: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(400);
    let kind = args.next().unwrap_or_else(|| "infinite".into());

    let mut world = match kind.as_str() {
        "tiled" => World::tiled(4, 4),
        _ => World::infinite(),
    };

    println!("{:>6}  {:>6} {:>6} {:>6} {:>5}", "g", "slots", "loaded", "idle", "live");
    for g in 0..=gens {
        if g % (gens / 10).max(1) == 0 {
            let idle = (0..world.slot_count())
                .filter(|&id| matches!(world.slot(id), Neighbour::Idle { .. }))
                .count();
            println!(
                "{:>6}  {:>6} {:>6} {:>6} {:>5}",
                g,
                world.slot_count(),
                world.loaded_count(),
                idle,
                world.live_cells().len()
            );
        }
        world.step();
    }
}
