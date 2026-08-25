//! Headless authoritative server.
//!
//!     cargo run --no-default-features --features server --bin server -- [OPTIONS]
//!
//!     --addr  ADDR   listen address              (default [::]:8080)
//!     --rooms DIR    where rooms are saved       (default rooms)
//!     --room  NAME   declare a room; repeatable  (default one called main)
//!     --torus RxC    a world that wraps, in chunks (default infinite)
//!     --serve DIR    static files at /           (default: none)
//!     --span  MS     milliseconds per generation (default 250)
//!     --fresh        ignore every existing save and start new worlds
//!     --max-rooms N  how many rooms players may make (default 32)
//!
//! With `--serve .` the browser client and the socket come from one origin, so
//! no separate static-file server is needed.
//!
//! A room is a whole separate world. `--room` declares one; every `<name>.ckw`
//! already in the rooms directory is one too, so a restart keeps what a
//! previous run was asked for. Joining a name nobody declared is refused, and
//! the refusal says what is actually here.
//!
//! The server reads its own terminal as well: `help` lists the commands,
//! `new NAME [ROWSxCOLS]` makes a room without a restart, and `stop` saves
//! every room and shuts down. So do SIGINT and SIGTERM.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use conwayskingdom::server::rooms::Rooms;
use conwayskingdom::server::ws;

fn main() -> std::io::Result<()> {
    // Through the console's printer, so a log line arriving while somebody is
    // typing appears *above* the half-typed command rather than through the
    // middle of it. With no terminal there is no prompt to protect and it
    // falls through to stderr, which is where these always went.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(conwayskingdom::server::ws::ConsoleLog::target())
        .init();

    // `[::]` rather than `0.0.0.0`: unspecified **IPv6**, which on Linux
    // accepts IPv4 as well because `net.ipv6.bindv6only` is 0 by default. An
    // IPv4-only socket refuses every connection that arrives over IPv6, and a
    // machine that resolves this host by name will get its AAAA record and
    // come in that way -- which looks exactly like the server being
    // unreachable while anything bound to `::` answers fine.
    let mut addr: SocketAddr = "[::]:8080".parse().unwrap();
    let mut rooms_dir = PathBuf::from("rooms");
    let mut declared: Vec<String> = Vec::new();
    let mut static_dir: Option<PathBuf> = None;
    let mut span = Duration::from_millis(250);
    let mut fresh = false;
    let mut shape = conwayskingdom::sim::WorldKind::Infinite;
    let mut max_rooms = conwayskingdom::server::rooms::MAX_MADE_ROOMS;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => {
                addr = args.next().expect("--addr needs a value").parse().expect("bad addr")
            }
            "--rooms" => rooms_dir = args.next().expect("--rooms needs a directory").into(),
            "--room" => declared.push(args.next().expect("--room needs a name")),
            "--serve" => static_dir = Some(args.next().expect("--serve needs a directory").into()),
            "--span" => {
                let ms: u64 =
                    args.next().expect("--span needs milliseconds").parse().expect("bad span");
                span = Duration::from_millis(ms);
            }
            "--fresh" => fresh = true,
            // Rooms made by clients only. A `--room` on this line, or a name
            // typed at the console, is a decision somebody made and is not
            // counted against it.
            "--max-rooms" => {
                max_rooms = args
                    .next()
                    .expect("--max-rooms needs a number")
                    .parse()
                    .expect("--max-rooms takes a number");
            }
            // Skipped rather than refused. The `cargo serve` alias already
            // ends in `--`, so typing a second one -- which is the habit --
            // would otherwise hand the binary a bare `--` and panic on it.
            "--" => continue,
            "--torus" => {
                let text = args.next().expect("--torus needs ROWSxCOLS");
                shape = conwayskingdom::sim::parse_torus(&text)
                    .unwrap_or_else(|e| panic!("--torus: {e}"));
            }
            // Gone rather than quietly reinterpreted. A world is now one room
            // among several and lives in a directory under its room's name, so
            // a path that used to mean "the world" now means nothing -- and
            // silently treating it as the default room's file would put the
            // save somewhere the next run does not look.
            "--world" => {
                let path = args.next().unwrap_or_else(|| "world.ckw".into());
                panic!(
                    "--world is gone: worlds are rooms now, one file each under --rooms DIR. \
                     Move {path} to rooms/main.ckw -- the format is unchanged -- or pass \
                     --rooms to say where they live."
                );
            }
            other => panic!("unknown argument {other}"),
        }
    }

    // Without --fresh a save is authoritative, which is easy to forget: a room
    // file left over from an earlier run is that room, whatever --torus says,
    // because the shape of a world is not something a flag can change after
    // cells have been written into it.
    if fresh {
        log::info!("--fresh: ignoring any rooms saved in {}", rooms_dir.display());
    }
    // Spelled out rather than `?`. A room file from an older build is the most
    // likely thing to go wrong here, and `Error: Custom { kind: InvalidData,
    // .. }` on the way out tells a person nothing about what to do -- which
    // reads as saving being broken rather than as a file needing to be moved
    // out of the way.
    let mut rooms = match Rooms::open(&rooms_dir, &declared, shape, fresh) {
        Ok(r) => r,
        Err(e) => {
            log::error!("cannot open rooms in {}: {e}", rooms_dir.display());
            log::error!(
                "if that world is from an older build, the format has changed and it cannot \
                 be converted. Move it aside, or pass --fresh to start new worlds."
            );
            std::process::exit(1);
        }
    };

    rooms.cap_made(max_rooms);

    log::info!(
        "{} room(s) in {}: {} -- a client naming none gets \"{}\"",
        rooms.len(),
        rooms_dir.display(),
        rooms.names().collect::<Vec<_>>().join(", "),
        rooms.default_room(),
    );

    for name in rooms.names().collect::<Vec<_>>() {
        let server = rooms.get(name).expect("just listed");
        let world = server.world();
        // Asked of the world rather than of the flag, because a save is
        // authoritative: the world that comes back may not be the shape
        // --torus asked for, and it is the one players will be granted ground
        // in.
        if conwayskingdom::net::too_cramped_for_grants(world) {
            log::warn!(
                "room \"{name}\" is too small to give every player a square of their own; \
                 the later ones will get what is left"
            );
        }
        log::info!(
            "  {name}: {}, tick {}, {} chunks, {} live cells, {} players",
            match world.kind() {
                conwayskingdom::sim::WorldKind::Infinite => "infinite".to_string(),
                conwayskingdom::sim::WorldKind::Toroidal { rows, cols } =>
                    format!("{rows}x{cols} torus"),
            },
            server.tick(),
            world.stored_count(),
            world.live_cells().len(),
            server.player_count(),
        );
        match world.live_bounds() {
            Some(((r0, c0), (r1, c1))) => log::info!(
                "    life spans rows {r0}..={r1}, cols {c0}..={c1} (chunks {}..={}, {}..={}) — point a client there to see it",
                r0.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
                r1.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
                c0.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
                c1.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
            ),
            // Not a warning. An empty world is where a new one starts: there
            // is no seeded pattern, and the first life arrives with the first
            // player, who is granted ground and a block on joining.
            None => log::info!("    empty; the first player to join will bring a block"),
        }
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(ws::serve(
        rooms,
        ws::Config {
            addr,
            static_dir,
            save_every: Duration::from_secs(30),
            generation_span: span,
            // What `new NAME` at the console makes when it is not given a
            // size, so typing it means what --room would have meant.
            shape,
        },
    ))
}
