//! Where a generation's time goes, at the sizes a server actually runs.
//!
//!     cargo run --release --no-default-features --example frametime
//!
//! The step is the server's whole per-generation cost and the client pays it
//! too, so it is the one number both sides share. `--span 250` is four a
//! second, so the budget is 250 ms; a client drawing at 60 fps has 16.7.
use conwayskingdom::sim::{PlayerId, World};
use std::time::Instant;

fn seeded(rows: i32, cols: i32, fill: u64) -> World {
    let mut w = World::toroidal(rows, cols);
    let n = 16;
    let mut state: u64 = 0x9E3779B97F4A7C15;
    for r in 0..rows * n {
        for c in 0..cols * n {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            if (state >> 33) % 100 < fill {
                let who = PlayerId(1 + ((state >> 17) % 4) as u8);
                w.set_cell_at(r, c, conwayskingdom::sim::Cell::alive(who));
            }
        }
    }
    w
}

fn main() {
    println!("{:>10} {:>8} {:>10} {:>10} {:>9}", "world", "chunks", "cells", "ms/step", "of 250ms");
    for &(rows, cols) in &[(4, 4), (8, 8), (12, 12), (24, 24), (48, 48)] {
        let mut w = seeded(rows, cols, 30);
        for _ in 0..20 {
            w.step();
        }
        let runs = 40;
        let t = Instant::now();
        for _ in 0..runs {
            w.step();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        let chunks = (rows * cols) as usize;
        println!(
            "{:>10} {:>8} {:>10} {:>10.2} {:>8.1}%",
            format!("{rows}x{cols}"),
            chunks,
            chunks * 256,
            ms,
            ms / 250.0 * 100.0
        );
    }
    println!("\nand the parts, on 24x24:");
    let mut w = seeded(24, 24, 30);
    for _ in 0..20 {
        w.step();
    }
    for (name, f) in [
        (
            "step",
            Box::new(|w: &mut World| {
                w.step();
            }) as Box<dyn Fn(&mut World)>,
        ),
        (
            "live_cells",
            Box::new(|w: &mut World| {
                std::hint::black_box(w.live_cells());
            }),
        ),
        (
            "standings",
            Box::new(|w: &mut World| {
                std::hint::black_box(conwayskingdom::net::standings(w));
            }),
        ),
    ] {
        let t = Instant::now();
        for _ in 0..20 {
            f(&mut w);
        }
        println!("  {name:<12} {:>8.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }
}
