//! What mining actually pays, per generation, for the patterns worth building.
//!
//!     cargo run --no-default-features --example balance
//!
//! Three constants decide whether mining is worth doing — `MINE_COST`,
//! `MINE_YIELD` and `rule::MINE_UPKEEP_ODDS` — and no amount of arguing about
//! them settles anything, because the answer depends on how many corpses a
//! pattern drags behind it and that is not a thing anybody can estimate. So
//! this runs the patterns and prints the table.
//!
//! What the numbers should say: **a compact machine pays and a mess does not.**
//! A blinker is three cells and two corpses and should be worth building; an
//! r-pentomino is two hundred live cells and eight hundred corpses of sprawl
//! and should not be free money. If sprawl is paying best, the upkeep is too
//! rare.

use conwayskingdom::net;
use conwayskingdom::sim::{Cell, Kind, Mined, PlayerId, World, CHUNK_N};

/// Live and dead mine cells this player owns. The dead ones are what is
/// charged for, and counting them is the whole point of the report.
fn census(world: &World, me: PlayerId) -> (usize, usize) {
    let mut alive = 0;
    let mut corpses = 0;
    for (_, chunk) in world.stored() {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let cell = chunk[(row, col)];
                if cell.kind() != Kind::MINE || cell.player() != me {
                    continue;
                }
                if cell.is_alive() {
                    alive += 1;
                } else {
                    corpses += 1;
                }
            }
        }
    }
    (alive, corpses)
}

fn run(name: &str, seed: &[(i32, i32)], generations: u32) {
    let me = PlayerId(1);
    let mut world = World::infinite_empty();
    for &(r, c) in seed {
        world.set_cell_at(r, c, Cell::alive(me).with_kind(Kind::MINE));
    }
    let placed = seed.len() as i32 * net::MINE_COST;

    let every = (generations / 6).max(1);
    println!("\n  {name}   {} mines, {placed} to place", seed.len());
    println!("    {:>5} {:>7} {:>8} {:>9} {:>9}", "gen", "alive", "corpses", "earned", "per gen");

    let (mut purse, mut last) = (0i32, 0i32);
    for g in 1..=generations {
        let mut tally = Mined::default();
        tally.add(&world.step());
        purse += net::earnings(&tally, me);
        if g % every == 0 {
            let (alive, corpses) = census(&world, me);
            println!(
                "    {g:>5} {alive:>7} {corpses:>8} {purse:>9} {:>9.2}",
                (purse - last) as f32 / every as f32
            );
            last = purse;
        }
    }
    println!("    net of the placement: {}", purse - placed);
}

fn main() {
    println!(
        "  cost {}  yield {}  drain {}  upkeep 1 in {}",
        net::MINE_COST,
        net::MINE_YIELD,
        net::MINE_DRAIN,
        conwayskingdom::sim::MINE_UPKEEP_ODDS,
    );
    run("block        still life, never dies", &[(0, 0), (0, 1), (1, 0), (1, 1)], 300);
    run("blinker      compact, pure churn", &[(0, 0), (0, 1), (0, 2)], 300);
    run("glider       travels, trails corpses", &[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)], 300);
    run("r-pentomino  sprawls for hundreds of generations", &[(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)], 600);
}
