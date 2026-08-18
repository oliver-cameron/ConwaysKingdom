//! Native client.
//!
//!     cargo run --bin native -- [--ws ws://host:8080/ws] [--name NAME]
//!
//! Without `--ws` it runs entirely locally: the simulation is deterministic, so
//! an unconnected client is a complete game, just a solitary one.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        let mut ws = None;
        let mut name = "player".to_string();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ws" => ws = Some(args.next().expect("--ws needs a URL")),
                "--name" => name = args.next().expect("--name needs a value"),
                other => panic!("unknown argument {other}"),
            }
        }

        conwayskingdom::client::set_connection(ws, name);
        pollster::block_on(conwayskingdom::run::<conwayskingdom::BattleApp>());
    }
}
