//! What one dynamite turns over, and what that is worth against the other
//! ways of taking ground.
//!
//!     cargo run --no-default-features --example blast
//!
//! The whole price of a blast is paid when the stick is laid — see
//! `sim::rule::DYNAMITE_COST`, which says why — so the number wants measuring
//! rather than guessing, and wants re-measuring whenever the reach, the
//! density or the shape of a disc changes.
//!
//! Three tables. The first is squares turned over per stick, which is what
//! the price was folded from: `blast_reach` grows as the square root of a
//! blob, so the area grows as the blob and the figure is flat, except for two
//! sticks, whose reach rounds down to eight.
//!
//! The second is what a blast is **worth** rather than what it covers, which
//! area does not answer. A stick is laid on the frontier of somebody's field
//! and the world is stepped, beside a four-turret emplacement and beside the
//! same money spent on plain life in the same place — three ways of taking
//! ground on one page — against three kinds of field: ground they hold with
//! nothing on it, a field of blocks, and a soup at the blast's own density.
//! The third sweeps the density and the reach on the same measure, through
//! `World::detonate_with`, so no constant has to be edited between runs.
//!
//! Every figure is the mean of a few seeds, and the stick is laid **armed**:
//! a fresh fuse takes about twenty-five generations to burn and the stick has
//! to be kept alive for all of them, which is a cost the tables do not show.
//! Held is every square the bomber owns and lost is what the field's owner
//! holds in a run where nothing was laid, less what they hold in this one —
//! so an empty field that fades on its own does not count as taken.
//!
//! What the numbers should say: **a stick is the only one of the three that
//! takes ground somebody is standing on, and it is the dearest way to hold
//! ground at every generation.** Life at a cell a square holds the most for
//! the least from the first generation; turrets take a square a generation
//! for as long as they live, and nothing while anything lives in front of
//! them; a stick takes fifty squares off a field in the generation it goes
//! off, whatever stands there. If the stick ever holds ground cheaper than
//! life, or takes it cheaper than turrets on ground that does not fight
//! back, the price is wrong. What the tables say as of writing is in
//! `docs/planned.md`, under dynamite.

use conwayskingdom::sim::{
    bits, mix, Cell, Kind, PlayerId, Roll, World, CHUNK_N, DYNAMITE_COST, DYNAMITE_DENSITY,
    DYNAMITE_FUSE, DYNAMITE_REACH, DYNAMITE_WARN, OUT_OF, TURRET_COST,
};

/// When the census is taken.
const GENERATIONS: [u32; 5] = [1, 10, 25, 50, 100];
/// How many seeds a figure is the mean of.
const SEEDS: u64 = 4;
/// Where the stick stands, which is the frontier: the middle of a chunk, so a
/// run that stays in one is stepped as one.
const AT: (i32, i32) = (CHUNK_N as i32 / 2, CHUNK_N as i32 / 2);
/// Their field: this many rows either side of the stick, and this many
/// columns east of it, which keeps it inside the stick's chunk.
const HALF: i32 = 20;
const WIDTH: i32 = CHUNK_N as i32 / 2 - 1;

/// The disc a blast covers, which is the thing being priced.
fn disc(reach: i32) -> usize {
    (-reach..=reach)
        .flat_map(|dr| (-reach..=reach).map(move |dc| (dr, dc)))
        .filter(|(dr, dc)| dr * dr + dc * dc <= reach * reach)
        .count()
}

/// What a blob of sticks turns over, **counted rather than computed**: every
/// square the pass rewrote, read before Conway has a turn. Their ground all
/// round, so the blast goes off where the blob stands. Nothing is charged at
/// detonation, so `Takings` no longer tallies this; the world is asked.
fn turned_over(blob: usize) -> usize {
    let (me, them) = (PlayerId(1), PlayerId(2));
    let mut world = World::infinite_empty();
    let far = conwayskingdom::sim::blast_reach(blob) + 2;
    for r in -far..=far {
        for c in -far..=far {
            put(&mut world, (r, c), Cell::DEAD.with_player(them).with_level(bits::MAX_LEVEL));
        }
    }
    let side = (blob as f64).sqrt().ceil() as i32;
    let armed = Cell::alive(me).with_kind(Kind::DYNAMITE).with_age(bits::MAX_AGE);
    for i in 0..blob as i32 {
        put(&mut world, (i / side, i % side), armed);
    }
    let before = world.clone();
    world.detonate_with(DYNAMITE_DENSITY, DYNAMITE_REACH);
    (-far..=far)
        .flat_map(|r| (-far..=far).map(move |c| (AT.0 + r, AT.1 + c)))
        .filter(|&(r, c)| world.cell_at(r, c) != before.cell_at(r, c))
        .count()
}

/// What their field has standing on it.
#[derive(Clone, Copy)]
enum Field {
    /// Held ground with nothing on it, which is what most of a country is.
    Empty,
    /// Blocks on a four-square pitch: a still life, so it holds until hit.
    Blocks,
    /// Random life at the blast's own density.
    Soup,
}

/// The three ways of spending the money at the frontier.
#[derive(Clone, Copy)]
enum Spend {
    /// One stick, armed, on the last square before their ground.
    Stick,
    /// The smallest emplacement that lives: a block of four.
    Turrets,
    /// `DYNAMITE_COST` cells of plain life, as a soup against the frontier.
    Life,
}

impl Spend {
    fn cost(self) -> i32 {
        match self {
            Self::Stick | Self::Life => DYNAMITE_COST,
            Self::Turrets => 4 * TURRET_COST,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Census {
    live: f64,
    held: f64,
    theirs: f64,
}

fn census(world: &World, me: PlayerId, them: PlayerId) -> Census {
    let mut out = Census::default();
    for (_, chunk) in world.stored() {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let cell = chunk[(row, col)];
                if cell.player() == me {
                    out.held += 1.0;
                    if cell.is_alive() {
                        out.live += 1.0;
                    }
                } else if cell.player() == them {
                    out.theirs += 1.0;
                }
            }
        }
    }
    out
}

/// A seeded shuffle, so a soup is the same soup every run.
fn shuffled(mut squares: Vec<(i32, i32)>, seed: u64) -> Vec<(i32, i32)> {
    for i in (1..squares.len()).rev() {
        let j = (mix(seed, i as u64) % (i as u64 + 1)) as usize;
        squares.swap(i, j);
    }
    squares
}

/// A square relative to the stick.
fn put(world: &mut World, (r, c): (i32, i32), cell: Cell) {
    world.set_cell_at(AT.0 + r, AT.1 + c, cell);
}

fn lay_field(world: &mut World, field: Field, them: PlayerId, seed: u64) {
    for r in -HALF..=HALF {
        for c in 1..=WIDTH {
            let alive = match field {
                Field::Empty => false,
                Field::Blocks => r.rem_euclid(4) < 2 && c.rem_euclid(4) < 2,
                Field::Soup => {
                    let square = mix(seed, ((r as u32 as u64) << 32) | (c as u32 as u64));
                    Roll::new(square).chance(0, DYNAMITE_DENSITY)
                }
            };
            let held = Cell::DEAD.with_player(them).with_level(bits::MAX_LEVEL);
            put(world, (r, c), if alive { Cell::alive(them) } else { held });
        }
    }
}

fn lay(world: &mut World, spend: Spend, me: PlayerId, seed: u64) {
    match spend {
        Spend::Stick => {
            let armed = Cell::alive(me).with_kind(Kind::DYNAMITE).with_age(bits::MAX_AGE);
            put(world, (0, 0), armed);
        }
        Spend::Turrets => {
            // Two squares back, or it is one shape with whatever stands on
            // their first column; it reaches four columns in from there.
            for at in [(0, -3), (0, -2), (1, -3), (1, -2)] {
                put(world, at, Cell::alive(me).with_kind(Kind::TURRET));
            }
        }
        Spend::Life => {
            // A box the size a blast's density would fill with this many
            // cells, so the soup laid is the soup a blast leaves — bought a
            // cell at a time rather than a disc at a time.
            let side = ((DYNAMITE_COST as f64 * OUT_OF as f64 / DYNAMITE_DENSITY as f64)
                .sqrt()
                .ceil()) as i32;
            let squares = (-side / 2..side - side / 2)
                .flat_map(|r| (1 - side..=0).map(move |c| (r, c)))
                .collect();
            for at in shuffled(squares, seed).into_iter().take(DYNAMITE_COST as usize) {
                put(world, at, Cell::alive(me));
            }
        }
    }
}

/// One run: their field, what was laid against it, and the census at each
/// generation in [`GENERATIONS`]. `blast` overrides the constants a stick
/// goes off with, for the sweep.
fn run(field: Field, spend: Option<Spend>, blast: Option<(u64, i32)>, seed: u64) -> Vec<Census> {
    let (me, them) = (PlayerId(1), PlayerId(2));
    let mut world = World::infinite_empty();
    world.set_seed(seed);
    lay_field(&mut world, field, them, seed);
    if let Some(spend) = spend {
        lay(&mut world, spend, me, seed);
    }
    if let Some((density, reach)) = blast {
        // What the top of the first step would do, with other numbers; the
        // step then finds nothing left to set off.
        world.detonate_with(density, reach);
    }
    let mut out = Vec::new();
    for g in 1..=GENERATIONS[GENERATIONS.len() - 1] {
        world.step();
        if GENERATIONS.contains(&g) {
            out.push(census(&world, me, them));
        }
    }
    out
}

/// A field with nothing laid against it, once per seed: what its owner holds
/// when left alone, which is what a spend is measured against.
fn controls(field: Field) -> Vec<Vec<Census>> {
    (0..SEEDS).map(|seed| run(field, None, None, seed)).collect()
}

/// The mean over [`SEEDS`] of a run, less its control: `theirs` becomes what
/// they lost to the spend rather than what they hold.
fn measured(
    field: Field,
    spend: Spend,
    blast: Option<(u64, i32)>,
    controls: &[Vec<Census>],
) -> Vec<Census> {
    let mut out = vec![Census::default(); GENERATIONS.len()];
    for (seed, control) in controls.iter().enumerate() {
        let with = run(field, Some(spend), blast, seed as u64);
        for (i, (c, w)) in control.iter().zip(&with).enumerate() {
            out[i].live += w.live / SEEDS as f64;
            out[i].held += w.held / SEEDS as f64;
            out[i].theirs += (c.theirs - w.theirs) / SEEDS as f64;
        }
    }
    out
}

fn main() {
    println!(" sticks   ground turned over   a stick");
    for blob in [1usize, 2, 4, 9, 25, 100] {
        let turned = turned_over(blob);
        assert_eq!(
            turned,
            disc(conwayskingdom::sim::blast_reach(blob)),
            "{blob}: not a whole disc"
        );
        println!("{blob:7}   {turned:17}   {:7.1}", turned as f64 / blob as f64);
    }
    println!();
    println!(
        "one stick turns over {} squares, so it costs {} — see rule::DYNAMITE_COST",
        turned_over(1),
        DYNAMITE_COST
    );
    println!(
        "a fresh fuse burns in about {:.0} generations, which the tables below skip",
        DYNAMITE_WARN as f64 * OUT_OF as f64 / DYNAMITE_FUSE as f64 + 1.0
    );
    println!("held is every square the bomber owns; lost is what the field's owner");
    println!("holds when left alone, less what they hold now, so a minus is ground they gained");

    let fields = [
        (Field::Empty, "empty     held ground with nothing standing on it, which fades on its own"),
        (Field::Blocks, "blocks    a still life on a four-square pitch"),
        (Field::Soup, "soup      random life at the blast's own density"),
    ];
    let spends = [(Spend::Stick, "stick"), (Spend::Turrets, "turrets"), (Spend::Life, "life")];
    let controls: Vec<Vec<Vec<Census>>> =
        fields.iter().map(|&(field, _)| controls(field)).collect();

    let a_square = |cost: i32, held: f64| {
        if held >= 1.0 {
            format!("{:9.1}", cost as f64 / held)
        } else {
            format!("{:>9}", "-")
        }
    };
    for (&(field, name), control) in fields.iter().zip(&controls) {
        println!("\n  {name}");
        println!(
            "    {:<8}{:>5} {:>5} {:>6} {:>6} {:>6} {:>9}",
            "", "cost", "gen", "live", "held", "lost", "a square"
        );
        for (spend, label) in spends {
            let rows = measured(field, spend, None, control);
            for (g, c) in GENERATIONS.iter().zip(&rows) {
                let first = *g == GENERATIONS[0];
                println!(
                    "    {:<8}{:>5} {g:>5} {:>6.0} {:>6.0} {:>6.0} {}",
                    if first { label } else { "" },
                    if first { spend.cost().to_string() } else { String::new() },
                    c.live,
                    c.held,
                    c.theirs,
                    a_square(spend.cost(), c.held)
                );
            }
        }
    }

    // What a stick does at other numbers, on the same measure. Every row is
    // priced at `DYNAMITE_COST` so the disc says what it would cost instead.
    let shown = [1u32, 25, 100];
    for (&(field, name), control) in fields.iter().zip(&controls) {
        println!(
            "\n  a stick at other numbers, against {}",
            name.split_whitespace().next().unwrap()
        );
        let mut head = format!("    {:>7} {:>5} {:>4}", "density", "reach", "disc");
        for g in shown {
            head.push_str(&format!("   {:>7} {:>6}", format!("held {g}"), format!("lost {g}")));
        }
        println!("{head}");
        for density in [16u64, 21, 24, 32] {
            for reach in [5i32, 6, 8] {
                let rows = measured(field, Spend::Stick, Some((density, reach)), control);
                let mut line = format!("    {density:>7} {reach:>5} {:>4}", disc(reach));
                for g in shown {
                    let c = rows[GENERATIONS.iter().position(|&x| x == g).unwrap()];
                    line.push_str(&format!("   {:>7.0} {:>6.0}", c.held, c.theirs));
                }
                println!("{line}");
            }
        }
    }
}
