//! Headless authoritative server.
//!
//!     cargo run --no-default-features --features server --bin server -- [OPTIONS]
//!
//!     --addr  ADDR   listen address           (default 0.0.0.0:8080)
//!     --world PATH   save file                (default world.ckw)
//!     --serve DIR    static files at /        (default: none)
//!     --span  MS     milliseconds per generation (default 250)
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

    let mut addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let mut world_path = PathBuf::from("world.ckw");
    let mut static_dir: Option<PathBuf> = None;
    let mut span = Duration::from_millis(250);

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
            other => panic!("unknown argument {other}"),
        }
    }

    let server = Server::load_or_new(&world_path, World::demo)?;
    log::info!(
        "world at tick {}, {} chunks, {} players",
        server.tick(),
        server.world().stored_count(),
        server.player_count()
    );

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
