//! Two bots on one board, and who won.
//!
//!     cargo run --no-default-features --example duel -- \
//!         [--a book|search] [--b book|search] [--level easy|normal|hard] \
//!         [--generations N] [--games K] [--seed S] [--territory N] \
//!         [--record DIR] [--every N]
//!
//! Two seats in one [`Server`], no socket and no clock: the same `add_bot`
//! the console calls, the same `step`, and every placement judged by the same
//! `act` a client's goes through. So what this measures is the bot and not a
//! harness — a book bot here plays exactly the book bot a room gets.
//!
//! It is how the search is judged, and the answer belongs in the commit
//! message whichever way it comes out. A seeded run repeats: `--seed` is the
//! world's dice and the bots', and game `k` runs on `seed + k`, so a
//! surprising game can be replayed on its own with `--games 1 --seed`.
//!
//! ## `--record DIR`
//!
//! One file per game, `duel-SEED-GAME.jsonl`, JSON one object a line with a
//! `kind` saying which. **This is the training corpus** for the learned judge
//! that goes behind `server::bot::Judge`, so it holds what a judge is shown
//! rather than what a person would like to read: the window round each seat's
//! home is exactly the crop `Ground::best` hands over, `SEARCH_SEEN` cells
//! each way from the middle of the patch.
//!
//! | line | fields |
//! |---|---|
//! | `game` | the settings, the seats, their homes, the window's half-width |
//! | `board` | `tick`, `purse`, `score`, `ground`, and `cells` per seat |
//! | `result` | `tick`, `winner` (an index, or null for a draw), and the final three |
//!
//! `cells[i]` is seat `i`'s window as one string a row: **the cell bytes
//! verbatim in hex**, four characters a cell, byte 0 then byte 1, which is
//! the order the save format and the wire both put them in — see
//! [docs/simulation.md]. Two bytes rather than a decoded record because
//! everything a judge could read is in them, and a decoding that had to keep
//! up with the cell layout is a second place for it to be written down.
//!
//! A window is about 84 kB and a `board` line carries one per seat, so a game
//! at the default cadence is a couple of megabytes. `--every` is what to turn
//! down.
//!
//! [docs/simulation.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/simulation.md#the-cell

use std::fmt::Write as _;
use std::io::Write as _;
use std::time::{Duration, Instant};

use conwayskingdom::net::{self, Level};
use conwayskingdom::server::bot::{Driver, SEARCH_SEEN};
use conwayskingdom::server::Server;
use conwayskingdom::sim::{Cell, PlayerId, World};

/// How often the ground is counted while a game runs. A pass over the world,
/// so not every generation; only a `--territory` target reads it.
const CHECK_EVERY: u64 = 8;

struct Args {
    drivers: [String; 2],
    level: Level,
    generations: u64,
    games: usize,
    seed: u64,
    territory: Option<usize>,
    record: Option<std::path::PathBuf>,
    every: u64,
}

impl Args {
    fn parse(mut words: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = Self {
            drivers: ["search".into(), "book".into()],
            level: Level::Normal,
            generations: 1000,
            games: 4,
            seed: 1,
            territory: None,
            record: None,
            every: 50,
        };
        while let Some(flag) = words.next() {
            let mut value =
                || words.next().ok_or_else(|| format!("{flag} wants something after it"));
            match flag.as_str() {
                "--a" => args.drivers[0] = value()?,
                "--b" => args.drivers[1] = value()?,
                "--level" => args.level = Level::parse(&value()?)?,
                "--generations" => args.generations = number(&value()?)?,
                "--games" => args.games = number(&value()?)? as usize,
                "--seed" => args.seed = number(&value()?)?,
                "--territory" => args.territory = Some(number(&value()?)? as usize),
                "--every" => args.every = number(&value()?)?.max(1),
                "--record" => args.record = Some(value()?.into()),
                _ => return Err(format!("no flag {flag}")),
            }
        }
        for driver in &args.drivers {
            Driver::parse(driver)?;
        }
        Ok(args)
    }
}

fn number(word: &str) -> Result<u64, String> {
    word.parse().map_err(|_| format!("{word:?} is not a number"))
}

/// What one game came to.
struct Game {
    winner: Option<usize>,
    ticks: u64,
    score: [usize; 2],
    ground: [usize; 2],
    purse: [i32; 2],
    /// Wall clock on the steps a bot acted on, and on the rest. The first is
    /// what a search costs: a step is the same work either way, and only one
    /// of them has an act in it.
    acting: (Duration, u32),
    quiet: (Duration, u32),
}

fn main() {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(2);
        }
    };
    if let Some(dir) = &args.record {
        if let Err(why) = std::fs::create_dir_all(dir) {
            eprintln!("cannot write to {}: {why}", dir.display());
            std::process::exit(2);
        }
    }

    println!(
        "  {} against {}, {} level, {} generations, {} games from seed {}",
        args.drivers[0],
        args.drivers[1],
        args.level.name(),
        args.generations,
        args.games,
        args.seed
    );
    println!(
        "  {:>5} {:>7} {:>10} {:>13} {:>13} {:>13} {:>8} {:>8}",
        "game", "ticks", "winner", "score", "ground", "purse", "act ms", "step ms"
    );

    let mut won = [0usize; 2];
    let mut drawn = 0;
    let mut totals = [0usize; 2];
    let (mut acting, mut quiet) = ((Duration::ZERO, 0u32), (Duration::ZERO, 0u32));
    for game in 0..args.games {
        let played = play(&args, game);
        match played.winner {
            Some(i) => won[i] += 1,
            None => drawn += 1,
        }
        for i in 0..2 {
            totals[i] += played.score[i];
        }
        acting = (acting.0 + played.acting.0, acting.1 + played.acting.1);
        quiet = (quiet.0 + played.quiet.0, quiet.1 + played.quiet.1);
        println!(
            "  {:>5} {:>7} {:>10} {:>13} {:>13} {:>13} {:>8.2} {:>8.2}",
            game,
            played.ticks,
            match played.winner {
                Some(i) => args.drivers[i].as_str(),
                None => "drawn",
            },
            format!("{} - {}", played.score[0], played.score[1]),
            format!("{} - {}", played.ground[0], played.ground[1]),
            format!("{} - {}", played.purse[0], played.purse[1]),
            mean_ms(played.acting),
            mean_ms(played.quiet),
        );
    }

    let mean = |total: usize| total as f64 / args.games.max(1) as f64;
    println!(
        "\n  {} won {} and {} won {}, {drawn} drawn; mean score {:.0} against {:.0}",
        args.drivers[0],
        won[0],
        args.drivers[1],
        won[1],
        mean(totals[0]),
        mean(totals[1])
    );
    // The number the budget is set by. A step with an act in it is a step plus
    // whatever the driver did, and the difference is what one act costs
    // against the 250 ms a generation has.
    println!(
        "  a step with an act in it took {:.2} ms and one without {:.2}, so an act is {:.2} ms",
        mean_ms(acting),
        mean_ms(quiet),
        mean_ms(acting) - mean_ms(quiet)
    );
}

fn mean_ms((total, count): (Duration, u32)) -> f64 {
    if count == 0 {
        return 0.0;
    }
    total.as_secs_f64() * 1000.0 / count as f64
}

fn play(args: &Args, game: usize) -> Game {
    let seed = args.seed.wrapping_add(game as u64);
    let mut world = World::infinite_empty();
    // The world's dice and, through `add_bot`, the bots' own — so a game is
    // one number and repeats from it.
    world.set_seed(seed);
    let mut server = Server::new(world);

    let mut seats = [PlayerId::UNOWNED; 2];
    for (i, driver) in args.drivers.iter().enumerate() {
        let name = format!("{driver} {}", if i == 0 { "a" } else { "b" });
        let driver = Driver::parse(driver).expect("checked when the flags were read");
        seats[i] = server.add_bot(name, args.level, driver, None).expect("a room with two in it");
    }
    let homes = seats.map(|seat| {
        let (row, col) = net::spawn_for(seat, server.world());
        (row + net::SPAWN_N / 2, col + net::SPAWN_N / 2)
    });

    let mut file = args.record.as_ref().map(|dir| {
        let path = dir.join(format!("duel-{seed}-{game}.jsonl"));
        let file = std::fs::File::create(&path)
            .unwrap_or_else(|why| panic!("cannot write {}: {why}", path.display()));
        let mut file = std::io::BufWriter::new(file);
        say(
            &mut file,
            serde_json::json!({
                "kind": "game", "record": 1, "game": game, "seed": seed,
                "level": args.level.name(), "drivers": args.drivers,
                "seats": seats.map(|s| s.0), "homes": homes,
                "window": SEARCH_SEEN, "every": args.every,
                "generations": args.generations,
            }),
        );
        file
    });

    // Asked of a seat rather than worked out, because how often a bot acts is
    // the bot's own first dial.
    let cadence = server.bot(seats[0]).expect("just seated").cadence();
    let (mut acting, mut quiet) = ((Duration::ZERO, 0u32), (Duration::ZERO, 0u32));
    let mut ticks = 0;
    for tick in 0..args.generations {
        if let Some(file) = &mut file {
            if tick % args.every == 0 {
                let held = standing(&server, seats);
                say(
                    file,
                    serde_json::json!({
                        "kind": "board", "tick": tick,
                        "purse": held.2, "score": held.0, "ground": held.1,
                        "cells": homes.map(|at| window(server.world(), at, SEARCH_SEEN)),
                    }),
                );
            }
        }
        let started = Instant::now();
        server.step();
        let took = started.elapsed();
        // A bot's `next_at` starts at the tick it was seated on and advances
        // by its cadence, and both seats share one, so these are exactly the
        // generations an act was made on.
        let landed = if tick % cadence == 0 { &mut acting } else { &mut quiet };
        (landed.0, landed.1) = (landed.0 + took, landed.1 + 1);
        ticks = tick + 1;
        if let Some(target) = args.territory {
            if tick % CHECK_EVERY == 0 && standing(&server, seats).0.iter().any(|&n| n >= target) {
                break;
            }
        }
    }

    let (score, ground, purse) = standing(&server, seats);
    let winner = match score[0].cmp(&score[1]) {
        std::cmp::Ordering::Greater => Some(0),
        std::cmp::Ordering::Less => Some(1),
        std::cmp::Ordering::Equal => None,
    };
    if let Some(file) = &mut file {
        say(
            file,
            serde_json::json!({
                "kind": "result", "tick": ticks, "winner": winner,
                "purse": purse, "score": score, "ground": ground,
            }),
        );
        file.flush().expect("the last line of a game");
    }
    Game { winner, ticks, score, ground, purse, acting, quiet }
}

/// What each seat holds and has in hand: the score a match is won on, every
/// square including the grant, and the purse.
fn standing(server: &Server, seats: [PlayerId; 2]) -> ([usize; 2], [usize; 2], [i32; 2]) {
    let score = server.territory();
    let ground = server.ground();
    (
        seats.map(|s| score[s.0 as usize]),
        seats.map(|s| ground[s.0 as usize]),
        seats.map(|s| server.value_of(s).unwrap_or(0)),
    )
}

/// A square of the world round one seat, one string a row, four hex
/// characters a cell.
fn window(world: &World, centre: (i32, i32), half: i32) -> Vec<String> {
    (centre.0 - half..=centre.0 + half)
        .map(|row| {
            let mut line = String::with_capacity((2 * half as usize + 1) * 4);
            for col in centre.1 - half..=centre.1 + half {
                let cell = world.cell_at(row, col).unwrap_or(Cell::DEAD);
                write!(line, "{:02x}{:02x}", cell.0[0], cell.0[1]).expect("a string");
            }
            line
        })
        .collect()
}

fn say(file: &mut impl std::io::Write, line: serde_json::Value) {
    writeln!(file, "{line}").expect("a line of the record");
}
