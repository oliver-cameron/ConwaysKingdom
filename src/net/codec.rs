//! Turning messages into bytes and back.
//!
//! Postcard: compact, `no_std`-friendly so the wasm client uses the same code
//! as the server, and it does not embed field names the way a text format
//! would — a `ChunkData` frame is then barely larger than the chunk itself.
//!
//! Frames are binary, never text. A chunk is raw cell bytes and would be
//! mangled by a UTF-8 round trip.

use super::{ClientMessage, ServerMessage};

#[derive(Debug)]
pub enum CodecError {
    Encode(postcard::Error),
    Decode(postcard::Error),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::Encode(e) => write!(f, "encoding message: {e}"),
            CodecError::Decode(e) => write!(f, "decoding message: {e}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub fn encode_client(msg: &ClientMessage) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(msg).map_err(CodecError::Encode)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientMessage, CodecError> {
    postcard::from_bytes(bytes).map_err(CodecError::Decode)
}

pub fn encode_server(msg: &ServerMessage) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(msg).map_err(CodecError::Encode)
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerMessage, CodecError> {
    postcard::from_bytes(bytes).map_err(CodecError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Action, Placement, Stamped};
    use crate::sim::PlayerId;

    #[test]
    fn client_messages_round_trip() {
        let cases = vec![
            ClientMessage::Join { name: "alice".into(), room: Some("lobby".into()), person: None },
            ClientMessage::Join { name: "web".into(), room: None, person: None },
            ClientMessage::Act(Stamped {
                tick: 42,
                player: PlayerId(3),
                seat: PlayerId(3),
                action: Action::Paint { cells: vec![(1, 2), (-3, 4)], placement: Placement::Life },
            }),
            ClientMessage::Act(Stamped {
                tick: 7,
                player: PlayerId(1),
                seat: PlayerId(1),
                action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
            }),
            ClientMessage::Act(Stamped {
                tick: 8,
                player: PlayerId(2),
                seat: PlayerId(2),
                action: Action::Paint { cells: vec![(3, 3)], placement: Placement::Ice },
            }),
            ClientMessage::Subscribe { chunks: vec![(0, 0), (-1, 5)] },
            ClientMessage::Checkpoint {
                tick: 100,
                chunks: vec![((0, 0), 0xDEAD_BEEF), ((-1, 4), 7)],
            },
            ClientMessage::Rooms,
            ClientMessage::Act(Stamped {
                tick: 11,
                player: PlayerId(4),
                seat: PlayerId(4),
                action: Action::Paint { cells: vec![(2, 2)], placement: Placement::Mine },
            }),
            // A world and a match, which differ on the wire by one field.
            ClientMessage::Create {
                name: "arena".into(),
                shape: crate::sim::WorldKind::Toroidal { rows: 6, cols: 8 },
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
            },
            ClientMessage::Create {
                name: "cup".into(),
                shape: crate::sim::WorldKind::Infinite,
                victory: Some(crate::net::Victory::Timer { generations: 2000 }),
                teams: Some(2),
                private: true,
                laboratory: false,
            },
            // A laboratory, which is the third thing the form can ask for.
            ClientMessage::Create {
                name: "bench".into(),
                shape: crate::sim::WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: true,
            },
            ClientMessage::Leave,
            ClientMessage::Start,
            ClientMessage::Watch { room: "r-abc234".into() },
            ClientMessage::JoinTeam { team: PlayerId(2) },
            ClientMessage::NameTeam { team: PlayerId(1), name: "Reds".into() },
            ClientMessage::SetRules(crate::net::Rules {
                paused: true,
                place_anywhere: true,
                place_free: true,
                laboratory: true,
            }),
            ClientMessage::StepOnce,
        ];
        for msg in cases {
            let bytes = encode_client(&msg).unwrap();
            assert_eq!(decode_client(&bytes).unwrap(), msg, "{msg:?}");
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let cases = vec![
            // Most first, and a player holding nothing is simply absent.
            // A decided match, which is the shape with the most in it.
            ServerMessage::Match(crate::net::Lobby {
                teams: Vec::new(),
                started_by: None,
                owner: None,
                code: None,
                phase: crate::net::MatchPhase::Over {
                    winner: Some(PlayerId(4)),
                    held: 812,
                    at: 2000,
                },
                victory: Some(crate::net::Victory::Timer { generations: 2000 }),
                players: vec![(PlayerId(1), "alice".into()), (PlayerId(4), "bob".into())],
            }),
            ServerMessage::Match(crate::net::Lobby {
                teams: Vec::new(),
                started_by: None,
                owner: None,
                code: None,
                phase: crate::net::MatchPhase::Gathering,
                victory: Some(crate::net::Victory::Territory { squares: 500 }),
                players: vec![],
            }),
            ServerMessage::Standing {
                tick: 40,
                held: vec![
                    crate::net::Holding { who: PlayerId(3), score: 1200, ground: 1344 },
                    crate::net::Holding { who: PlayerId(1), score: 88, ground: 232 },
                ],
            },
            ServerMessage::Standing { tick: 0, held: Vec::new() },
            ServerMessage::Welcome {
                person: Some(crate::net::PersonId("3f2a".into())),
                you: PlayerId(2),
                tick: 5,
                spawn: (-144, -96),
                rating: 1200,
                value: 73,
                room: "main".into(),
                name: "main".into(),
                world: crate::sim::WorldKind::Infinite,
                rules: crate::net::Rules::default(),
            },
            // The shape of a wrapping world has to survive the round trip, or
            // a client is told the world ends somewhere it does not.
            ServerMessage::Welcome {
                person: Some(crate::net::PersonId("3f2a".into())),
                you: PlayerId(7),
                tick: 900,
                spawn: (0, 0),
                rating: 1200,
                value: 0,
                room: "ring".into(),
                name: "ring".into(),
                world: crate::sim::WorldKind::Toroidal { rows: 18, cols: 24 },
                rules: crate::net::Rules {
                    paused: true,
                    place_anywhere: true,
                    place_free: false,
                    laboratory: true,
                },
            },
            ServerMessage::Rejected { reason: "full".into() },
            ServerMessage::Step {
                tick: 9,
                actions: vec![Stamped {
                    tick: 1,
                    player: PlayerId(1),
                    seat: PlayerId(1),
                    action: Action::Paint { cells: vec![(5, 5)], placement: Placement::Life },
                }],
            },
            ServerMessage::ChunkData { tick: 3, chunk: (-2, 7), cells: vec![1, 2, 3, 4] },
            ServerMessage::Resync { tick: 9, chunks: vec![(0, 0)] },
            ServerMessage::Purse { value: -3 },
            ServerMessage::Rooms { rooms: vec![] },
            // Both arms of the answer to `Create`, since a refusal is the
            // common one and carries the only text a player will read.
            ServerMessage::Made(Ok(crate::net::Made {
                id: "r-abc234".into(),
                name: "arena".into(),
                code: None,
            })),
            ServerMessage::Made(Ok(crate::net::Made {
                id: "r-xyz789".into(),
                name: "private game".into(),
                code: Some("mn4p7q".into()),
            })),
            ServerMessage::Made(Err("there is already a room called \"arena\"".into())),
            ServerMessage::Watching {
                room: "r-abc234".into(),
                name: "arena".into(),
                tick: 900,
                world: crate::sim::WorldKind::Infinite,
                rules: crate::net::Rules::default(),
            },
            ServerMessage::Rules(crate::net::Rules {
                paused: false,
                place_anywhere: true,
                place_free: true,
                laboratory: true,
            }),
            ServerMessage::Rooms {
                rooms: vec![
                    // A match, so the list can say so before anybody clicks.
                    crate::net::RoomInfo {
                        id: "r-abc234".into(),
                        name: "arena".into(),
                        players: 3,
                        phase: crate::net::MatchPhase::Gathering,
                        victory: Some(crate::net::Victory::Territory { squares: 500 }),
                        world: crate::sim::WorldKind::Toroidal { rows: 6, cols: 6 },
                        rules: crate::net::Rules::default(),
                    },
                    crate::net::RoomInfo {
                        id: "lobby".into(),
                        name: "lobby".into(),
                        players: 0,
                        phase: crate::net::MatchPhase::Open,
                        victory: None,
                        world: crate::sim::WorldKind::Infinite,
                        rules: crate::net::Rules::default(),
                    },
                    // A laboratory, which the list tells apart from a world by
                    // these and nothing else.
                    crate::net::RoomInfo {
                        id: "r-bench1".into(),
                        name: "bench".into(),
                        players: 1,
                        phase: crate::net::MatchPhase::Open,
                        victory: None,
                        world: crate::sim::WorldKind::Infinite,
                        rules: crate::net::Rules {
                            paused: true,
                            place_anywhere: true,
                            place_free: true,
                            laboratory: true,
                        },
                    },
                ],
            },
        ];
        for msg in cases {
            let bytes = encode_server(&msg).unwrap();
            assert_eq!(decode_server(&bytes).unwrap(), msg, "{msg:?}");
        }
    }

    /// A chunk frame should be close to the chunk's own size, not multiples of
    /// it — the reason for a binary format rather than JSON.
    #[test]
    fn a_chunk_frame_is_barely_larger_than_the_chunk() {
        let cells = vec![0u8; crate::sim::CHUNK_CELLS * size_of::<crate::sim::Cell>()];
        let raw = cells.len();
        let msg = ServerMessage::ChunkData { tick: 1, chunk: (0, 0), cells };
        let encoded = encode_server(&msg).unwrap().len();
        assert!(encoded < raw + 32, "{encoded} bytes to carry {raw}");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode_client(&[0xFF; 8]).is_err() || decode_client(&[]).is_err());
        assert!(decode_server(&[]).is_err());
    }
}
