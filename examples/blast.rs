//! What one dynamite turns over, which is what it should cost.
//!
//!     cargo run --no-default-features --example blast
//!
//! The whole price of a blast is paid when the stick is laid — see
//! `sim::rule::DYNAMITE_COST`, which says why — so the number wants measuring
//! rather than guessing, and wants re-measuring whenever the reach or the
//! shape of a disc changes.
//!
//! What it prints is squares turned over per stick. `blast_reach` grows as the
//! square root of a blob, so the area grows as the blob and the figure is flat:
//! the one exception is two sticks, which sit inside one disc and so turn over
//! no more ground than one.

use conwayskingdom::sim::{bits, Cell, Kind, PlayerId, World};

/// The disc a blast covers, which is the thing being priced. Read here rather
/// than off `Takings`, which no longer counts it: nothing is charged at
/// detonation, so nothing tallies it either.
fn disc(reach: i32) -> usize {
    (-reach..=reach)
        .flat_map(|dr| (-reach..=reach).map(move |dc| (dr, dc)))
        .filter(|(dr, dc)| dr * dr + dc * dc <= reach * reach)
        .count()
}

fn main() {
    let me = PlayerId(1);
    println!(" sticks   ground turned over   a stick");
    for blob in [1usize, 2, 4, 9, 25, 100] {
        let mut world = World::infinite_empty();
        // Somebody else's ground all round, so there is something to take.
        for r in -40..40 {
            for c in -40..40 {
                world.set_cell_at(r, c, Cell::DEAD.with_player(PlayerId(2)));
            }
        }
        let side = (blob as f64).sqrt().ceil() as i32;
        let mut laid = 0;
        for r in 0..side {
            for c in 0..side {
                if laid == blob {
                    break;
                }
                let primed = Cell::alive(me).with_kind(Kind::DYNAMITE).with_age(bits::MAX_AGE);
                world.set_cell_at(r, c, primed);
                laid += 1;
            }
        }
        // Stepped, so the figure is measured against a world that really went
        // off rather than against arithmetic on its own.
        world.step();
        assert!(
            world.live_cells().len() != laid,
            "{blob} stick(s) did not go off, so the number below is arithmetic"
        );
        let reach = conwayskingdom::sim::blast_reach(blob);
        println!("{blob:7}   {:17}   {:7.1}", disc(reach), disc(reach) as f64 / blob as f64);
    }
    println!();
    println!(
        "one stick turns over {} squares, so it costs {} — see rule::DYNAMITE_COST",
        disc(conwayskingdom::sim::blast_reach(1)),
        conwayskingdom::sim::DYNAMITE_COST
    );
}
