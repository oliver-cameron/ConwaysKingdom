//! Run the simulation with no GPU and report how the world grows.
//!
//!     cargo run --example headless -- [generations] [infinite|torus] [rows] [cols]

use conwayskingdom::World;

fn main() {
    let mut args = std::env::args().skip(1);
    let gens: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(400);
    let kind = args.next().unwrap_or_else(|| "infinite".into());
    let rows: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4);
    let cols: i32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4);

    let mut world = match kind.as_str() {
        "torus" | "toroidal" | "tiled" => World::toroidal(rows, cols),
        _ => World::demo(),
    };
    println!("{:?}", world.kind());
    println!("{:>6} {:>7} {:>7} {:>5}", "gen", "stored", "active", "live");

    for g in 0..=gens {
        if g % (gens / 10).max(1) == 0 {
            println!(
                "{:>6} {:>7} {:>7} {:>5}",
                g,
                world.stored_count(),
                world.active_count(),
                world.live_cells().len()
            );
        }
        world.step();
    }
}
