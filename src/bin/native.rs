//! Native client.
//!
//!     cargo run --bin native -- [--ws ws://host:8080/ws] [--name NAME]
//!                                [--room NAME] [--torus ROWSxCOLS]
//!                                [--token DIR]
//!
//! Without `--ws` it runs entirely locally: the simulation is deterministic, so
//! an unconnected client is a complete game, just a solitary one.
//!
//! `--room` picks which world on that server to join. Without it the server
//! decides, which is what makes a bare `--ws` still a game. A room is a
//! separate world with its own players, ground and value, so a name that
//! server does not have is refused rather than created — and the refusal says
//! what it does have.
//!
//! `--torus` opens a world that wraps, sized in chunks. Only meaningful
//! offline: connected, the world is whatever the server's room is running, and
//! the `Welcome` says which.
//!
//! `--token` says where to keep the secrets this client comes back with: a
//! directory, one file per room. Two clients on one machine otherwise share
//! one store and so try to be the same player; give them a directory each to
//! run them as two people.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        let mut ws = None;
        let mut name = "player".to_string();
        let mut room: Option<String> = None;
        let mut world = conwayskingdom::sim::WorldKind::Infinite;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ws" => ws = Some(args.next().expect("--ws needs a URL")),
                "--name" => name = args.next().expect("--name needs a value"),
                "--room" => {
                    let asked = args.next().expect("--room needs a name");
                    // Checked here as well as on the server, so a name that is
                    // not one is a message about the argument rather than a
                    // connection that opens and is turned away.
                    room = Some(
                        conwayskingdom::net::room_name(&asked)
                            .unwrap_or_else(|e| panic!("--room: {e}")),
                    );
                }
                "--token" => conwayskingdom::net::token::keep_in(
                    args.next().expect("--token needs a directory").into(),
                ),
                "--torus" => {
                    let text = args.next().expect("--torus needs ROWSxCOLS");
                    world = conwayskingdom::sim::parse_torus(&text)
                        .unwrap_or_else(|e| panic!("--torus: {e}"));
                }
                other => panic!("unknown argument {other}"),
            }
        }

        if ws.is_some() && world != conwayskingdom::sim::WorldKind::Infinite {
            log::warn!("--torus is ignored when connected; the server's world is the world");
        }
        if ws.is_none() && room.is_some() {
            log::warn!("--room is ignored offline; there is only the one world here");
        }
        conwayskingdom::client::set_world(world);
        conwayskingdom::client::set_connection(conwayskingdom::client::Connection {
            url: ws,
            name,
            room,
        });
        pollster::block_on(conwayskingdom::run::<conwayskingdom::BattleApp>());
    }
}
