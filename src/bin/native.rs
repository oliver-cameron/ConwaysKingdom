//! Native client.
//!
//!     cargo run --bin native -- [--ws ws://host:8080/ws] [--name NAME]
//!                                [--torus ROWSxCOLS]
//!
//! Without `--ws` it runs entirely locally: the simulation is deterministic, so
//! an unconnected client is a complete game, just a solitary one.
//!
//! `--torus` opens a world that wraps, sized in chunks. Only meaningful
//! offline: connected, the world is whatever the server is running.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        let mut ws = None;
        let mut name = "player".to_string();
        let mut world = conwayskingdom::sim::WorldMode::Infinite;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ws" => ws = Some(args.next().expect("--ws needs a URL")),
                "--name" => name = args.next().expect("--name needs a value"),
                "--torus" => {
                    let text = args.next().expect("--torus needs ROWSxCOLS");
                    world = conwayskingdom::sim::parse_torus(&text)
                        .unwrap_or_else(|e| panic!("--torus: {e}"));
                }
                other => panic!("unknown argument {other}"),
            }
        }

        if ws.is_some() && world != conwayskingdom::sim::WorldMode::Infinite {
            log::warn!("--torus is ignored when connected; the server's world is the world");
        }
        conwayskingdom::client::set_world(world);
        conwayskingdom::client::set_connection(ws, name);
        pollster::block_on(conwayskingdom::run::<conwayskingdom::BattleApp>());
    }
}
