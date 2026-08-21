//! Headless authoritative server.
//!
//!     cargo run --no-default-features --features server --bin server -- [OPTIONS]
//!
//!     --addr  ADDR   listen address           (default [::]:8080)
//!     --world PATH   save file                (default world.ckw)
//!     --serve DIR    static files at /        (default: none)
//!     --span  MS     milliseconds per generation (default 250)
//!     --fresh        ignore any existing save and start a new world
//!
//! With `--serve .` the browser client and the socket come from one origin, so
//! no separate static-file server is needed.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use conwayskingdom::server::{ws, Server};
use conwayskingdom::sim::World;

fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // `[::]` rather than `0.0.0.0`: unspecified **IPv6**, which on Linux
    // accepts IPv4 as well because `net.ipv6.bindv6only` is 0 by default. An
    // IPv4-only socket refuses every connection that arrives over IPv6, and a
    // machine that resolves this host by name will get its AAAA record and
    // come in that way -- which looks exactly like the server being
    // unreachable while anything bound to `::` answers fine.
    let mut addr: SocketAddr = "[::]:8080".parse().unwrap();
    let mut world_path = PathBuf::from("world.ckw");
    let mut static_dir: Option<PathBuf> = None;
    let mut span = Duration::from_millis(250);
    let mut fresh = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => addr = args.next().expect("--addr needs a value").parse().expect("bad addr"),
            "--world" => world_path = args.next().expect("--world needs a path").into(),
            "--serve" => static_dir = Some(args.next().expect("--serve needs a directory").into()),
            "--span" => {
                let ms: u64 = args.next().expect("--span needs milliseconds").parse().expect("bad span");
                span = Duration::from_millis(ms);
            }
            "--fresh" => fresh = true,
            other => panic!("unknown argument {other}"),
        }
    }

    // Without --fresh a save is authoritative, which is easy to forget: a
    // world file left over from an earlier run means World::demo never runs
    // and the gun never appears.
    let server = if fresh {
        log::info!("--fresh: ignoring any world at {}", world_path.display());
        Server::new(World::infinite_empty())
    } else {
        Server::load_or_new(&world_path, World::infinite_empty)?
    };

    let world = server.world();
    log::info!(
        "world: tick {}, {} chunks, {} live cells, {} players",
        server.tick(),
        world.stored_count(),
        world.live_cells().len(),
        server.player_count()
    );
    match world.live_bounds() {
        Some(((r0, c0), (r1, c1))) => log::info!(
            "life spans rows {r0}..={r1}, cols {c0}..={c1} (chunks {}..={}, {}..={}) — point a client there to see it",
            r0.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
            r1.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
            c0.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
            c1.div_euclid(conwayskingdom::sim::CHUNK_N as i32),
        ),
        None => log::warn!("the world is EMPTY — nothing to send a client. Try --fresh"),
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(ws::serve(
        server,
        ws::Config {
            addr,
            static_dir,
            save_path: Some(world_path),
            save_every: Duration::from_secs(30),
            generation_span: span,
        },
    ))
}
