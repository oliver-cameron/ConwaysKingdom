//! What ground actually does, over time and in shape.
//!
//!     cargo run --no-default-features --example territory
//!
//! Three rules pull against each other — ground is won from life, it creeps
//! across dead ground where enough of it agrees, and it fades where nothing is
//! alive — and whether the result is a country or a smear is not a thing
//! anybody can read off the constants. So this draws it.
//!
//! What the shapes should say: **a patch with life on it holds and pulls
//! itself smooth, a patch nobody is standing on fades, and a glider does not
//! stake a claim across the world behind it.** If the abandoned patch is still
//! there after a few hundred generations the creep is beating the decay, and
//! ground only ever grows again.

use conwayskingdom::sim::{Cell, PlayerId, World, CHUNK_N};

/// Owned cells, and how ragged the edge is — the count of owned cells with at
/// least one unowned neighbour, over the total. A solid blob has a small ratio;
/// a smear of speckle has a large one.
fn survey(world: &World, me: PlayerId) -> (usize, f32) {
    let mut owned = 0;
    let mut edge = 0;
    for (coord, chunk) in world.stored() {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                if chunk[(row, col)].player() != me {
                    continue;
                }
                owned += 1;
                let (r, c) = (
                    coord.0 * CHUNK_N as i32 + row as i32,
                    coord.1 * CHUNK_N as i32 + col as i32,
                );
                let ragged = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dr, dc)| {
                    world.cell_at(r + dr, c + dc).is_none_or(|n| n.player() != me)
                });
                if ragged {
                    edge += 1;
                }
            }
        }
    }
    (owned, if owned == 0 { 0.0 } else { edge as f32 / owned as f32 })
}

/// The claimed ground around the origin, as a picture.
fn draw(world: &World, me: PlayerId, half: i32) {
    for r in -half..=half {
        let mut line = String::from("      ");
        for c in -half..=half {
            let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
            line.push(match (cell.is_alive(), cell.player() == me) {
                (true, _) => '#',
                (false, true) => '+',
                (false, false) => '.',
            });
        }
        println!("{line}");
    }
}

fn run(name: &str, life: &[(i32, i32)], generations: u32, picture: bool) {
    let me = PlayerId(1);
    let mut world = World::infinite_empty();

    // A solid claimed patch to start from, with whatever life was asked for
    // standing on it.
    for r in -6..=6 {
        for c in -6..=6 {
            world.set_cell_at(r, c, Cell::DEAD.with_player(me));
        }
    }
    for &(r, c) in life {
        world.set_cell_at(r, c, Cell::alive(me));
    }

    println!("\n  {name}");
    println!("    {:>6} {:>8} {:>8}", "gen", "owned", "ragged");
    let every = (generations / 5).max(1);
    for g in 0..=generations {
        if g % every == 0 {
            let (owned, ragged) = survey(&world, me);
            println!("    {g:>6} {owned:>8} {ragged:>8.2}");
        }
        world.step();
    }
    if picture {
        draw(&world, me, 10);
    }
}

fn main() {
    println!(
        "  out of {}:  spread {}  creep {}  decay {}",
        conwayskingdom::sim::OUT_OF,
        conwayskingdom::sim::SPREAD,
        conwayskingdom::sim::CREEP,
        conwayskingdom::sim::DECAY,
    );
    run("held      a block stands on it", &[(0, 0), (0, 1), (1, 0), (1, 1)], 400, true);
    run("abandoned nothing alive at all", &[], 400, true);
    run("passed through   a glider crosses and leaves", &[(-5, -5), (-4, -4), (-3, -6), (-3, -5), (-3, -4)], 400, true);
}
