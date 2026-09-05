//! Turning messages into bytes and back.
//!
//! Postcard: compact, `no_std`-friendly so the wasm client uses the same code
//! as the server, and it does not embed field names the way a text format
//! would — a `ChunkData` frame is then barely larger than the chunk itself.
//!
//! Frames are binary, never text. A chunk is raw cell bytes and would be
//! mangled by a UTF-8 round trip.

use super::{ClientMessage, ServerMessage};

/// **What vocabulary these bytes are in.** One byte on the front of every
/// frame, and the whole reason it is there is that nothing detected a
/// mismatch.
///
/// Postcard writes an enum variant as its *index*, so inserting a message in
/// the middle of [`ClientMessage`] renumbers every one after it, and adding a
/// field to a struct that rides on one changes that message's shape. Both are
/// ordinary changes to make. What made them dangerous is that the browser
/// client is a **generated `pkg/` that a pull does not update** — see
/// [gotchas.md] — so a page four days old talks to a new server and neither
/// says anything: the frames decode to *something*, a join half-works, and
/// what the player sees is a profile that has forgotten them.
///
/// So: bump this whenever the vocabulary moves, and a stale client is told it
/// is stale instead of being quietly wrong.
///
/// **2**: three structs on the wire changed shape, and any one of them would
/// have needed this. [`Seat`] gained `bot`, so a page from before reads the
/// flag as the start of the next seat; [`RoomInfo`] gained `owner`, so a room
/// list is misread the same way; and `Create` gained `party`. The nine
/// messages that arrived beside them — `AddBot` and `RemoveBot`, `Hello`,
/// `Close`, `Invite`, and the four party verbs — joined the *end* of
/// [`ClientMessage`], which is the safe kind of change and would not have
/// needed a bump on its own.
///
/// [gotchas.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/gotchas.md
/// [`Seat`]: crate::net::Seat
/// [`RoomInfo`]: crate::net::RoomInfo
pub const PROTOCOL: u8 = 2;

#[derive(Debug)]
pub enum CodecError {
    Encode(postcard::Error),
    Decode(postcard::Error),
    /// The other end is speaking a different version of the vocabulary.
    Protocol {
        theirs: Option<u8>,
    },
}

impl CodecError {
    /// What to put in front of somebody, which for this one is an instruction
    /// rather than a diagnosis: there is exactly one thing to do about it.
    pub fn stale(&self) -> Option<String> {
        match self {
            CodecError::Protocol { theirs } => Some(format!(
                "this page speaks version {} and the server speaks {}.                  Reload; if that does not do it, the module needs rebuilding.",
                theirs.map(|v| v.to_string()).unwrap_or_else(|| "nothing".into()),
                PROTOCOL,
            )),
            _ => None,
        }
    }
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::Encode(e) => write!(f, "encoding message: {e}"),
            CodecError::Decode(e) => write!(f, "decoding message: {e}"),
            CodecError::Protocol { theirs } => {
                write!(f, "protocol {theirs:?}, and this build speaks {PROTOCOL}")
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Put the version on the front. One byte, on every frame rather than once at
/// the start, because a socket is not the only way a frame arrives and a
/// handshake is state both ends have to keep.
fn framed(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.insert(0, PROTOCOL);
    bytes
}

/// Take it off, or say the other end is speaking something else.
fn unframed(bytes: &[u8]) -> Result<&[u8], CodecError> {
    match bytes.split_first() {
        Some((&PROTOCOL, rest)) => Ok(rest),
        Some((&theirs, _)) => Err(CodecError::Protocol { theirs: Some(theirs) }),
        None => Err(CodecError::Protocol { theirs: None }),
    }
}

pub fn encode_client(msg: &ClientMessage) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(msg).map(framed).map_err(CodecError::Encode)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientMessage, CodecError> {
    postcard::from_bytes(unframed(bytes)?).map_err(CodecError::Decode)
}

pub fn encode_server(msg: &ServerMessage) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(msg).map(framed).map_err(CodecError::Encode)
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerMessage, CodecError> {
    postcard::from_bytes(unframed(bytes)?).map_err(CodecError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A frame from another vocabulary is refused, not misread.**
    ///
    /// This is the whole reason the byte is there. Postcard writes a variant
    /// as its index, so a message inserted in the middle renumbers every one
    /// after it — and the frames still decode, to the wrong thing. What that
    /// looked like was a join that half worked and a profile that had
    /// forgotten somebody, with a warning in a log nobody was reading.
    #[test]
    fn a_frame_from_another_version_says_so() {
        let mut bytes = encode_client(&ClientMessage::Rooms).expect("encode");
        assert_eq!(bytes[0], PROTOCOL, "the version is not on the front");

        bytes[0] = PROTOCOL.wrapping_add(1);
        let why = decode_client(&bytes).expect_err("a frame from the future was read");
        assert!(matches!(why, CodecError::Protocol { theirs: Some(_) }));

        // And it says what to do about it, because there is one thing.
        let told = why.stale().expect("a mismatch with nothing to tell anybody");
        assert!(told.contains("Reload"), "{told}");

        // An empty frame is the same answer rather than a panic.
        assert!(decode_client(&[]).is_err());
    }

    /// An ordinary frame is unaffected: the byte comes off and what is left is
    /// what was written.
    #[test]
    fn a_frame_of_this_version_round_trips() {
        let sent = ClientMessage::Leave;
        let bytes = encode_client(&sent).expect("encode");
        assert_eq!(decode_client(&bytes).expect("decode"), sent);
        assert!(decode_client(&bytes).unwrap().eq(&sent));
    }
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
                action: Action::Paint { cells: vec![(2, 2)], placement: Placement::Factory },
            }),
            // A world and a match, which differ on the wire by one field.
            ClientMessage::Create {
                name: "arena".into(),
                shape: crate::sim::WorldKind::Toroidal { rows: 6, cols: 8 },
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: None,
            },
            ClientMessage::Create {
                name: "cup".into(),
                shape: crate::sim::WorldKind::Infinite,
                victory: Some(crate::net::Victory::Timer { generations: 2000 }),
                teams: Some(2),
                private: true,
                laboratory: false,
                party: None,
            },
            // A laboratory, which is the third thing the form can ask for.
            ClientMessage::Create {
                name: "bench".into(),
                shape: crate::sim::WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: true,
                party: None,
            },
            // A party's world, which is the fourth.
            ClientMessage::Create {
                name: "den".into(),
                shape: crate::sim::WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: Some(crate::net::PartyId("p-3f2a91c4".into())),
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
                bpm: 120,
                laboratory: true,
            }),
            ClientMessage::StepOnce,
            ClientMessage::Wipe,
            ClientMessage::Profile { who: crate::net::PersonId("3f2a91c4".into()) },
            ClientMessage::People { like: "ali".into() },
            // The leaderboard, which is this with nothing asked.
            ClientMessage::People { like: String::new() },
            ClientMessage::AddBot { team: Some(PlayerId(2)), level: crate::net::Level::Hard },
            ClientMessage::AddBot { team: None, level: crate::net::Level::Easy },
            ClientMessage::RemoveBot { seat: PlayerId(5) },
            // Who I am, before any room is named.
            ClientMessage::Hello {
                name: "alice".into(),
                person: crate::net::Secret::read(&"a1".repeat(16)).expect("a secret"),
            },
            ClientMessage::Close { room: "r-abc234".into() },
            ClientMessage::Invite {
                who: crate::net::PersonId("3f2a91c4".into()),
                room: "r-abc234".into(),
            },
            ClientMessage::Parties,
            ClientMessage::MakeParty { name: "friday".into() },
            ClientMessage::InviteToParty {
                party: crate::net::PartyId("p-3f2a91c4".into()),
                who: crate::net::PersonId("3f2a91c4".into()),
            },
            ClientMessage::JoinParty { party: crate::net::PartyId("p-3f2a91c4".into()) },
            ClientMessage::LeaveParty { party: crate::net::PartyId("p-3f2a91c4".into()) },
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
                players: vec![
                    crate::net::Seat {
                        id: PlayerId(1),
                        name: "alice".into(),
                        who: Some(crate::net::PersonId("3f2a91c4".into())),
                        bot: false,
                    },
                    // Somebody with no key: a person this server will not
                    // remember, and so has nothing to say about.
                    crate::net::Seat { id: PlayerId(4), name: "bob".into(), who: None, bot: false },
                    // And a seat the server plays, which has no key either.
                    crate::net::Seat {
                        id: PlayerId(5),
                        name: "hard bot".into(),
                        who: None,
                        bot: true,
                    },
                ],
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
                profile: Some(crate::net::Profile {
                    who: crate::net::PersonId("3f2a91c4".into()),
                    name: "alice".into(),
                    rating: 1200,
                    provisional: true,
                    games: 3,
                    history: vec![1200, 1188, 1200],
                    best: 80,
                }),
                you: PlayerId(2),
                tick: 5,
                spawn: (-144, -96),
                value: 73,
                room: "main".into(),
                name: "main".into(),
                world: crate::sim::WorldKind::Infinite,
                rules: crate::net::Rules::default(),
            },
            // The shape of a wrapping world has to survive the round trip, or
            // a client is told the world ends somewhere it does not.
            ServerMessage::Welcome {
                // A client with no key: nobody this server remembers, and a
                // dashboard showing it the starting rating would invent one.
                profile: None,
                you: PlayerId(7),
                tick: 900,
                spawn: (0, 0),
                value: 0,
                room: "ring".into(),
                name: "ring".into(),
                world: crate::sim::WorldKind::Toroidal { rows: 18, cols: 24 },
                rules: crate::net::Rules {
                    paused: true,
                    place_anywhere: true,
                    place_free: false,
                    bpm: crate::net::FASTEST_BPM,
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
            ServerMessage::Rooms { rooms: vec![], hidden: crate::net::Hidden::default() },
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
            ServerMessage::Profile(Some(crate::net::Profile {
                who: crate::net::PersonId("3f2a91c4".into()),
                name: "alice".into(),
                rating: 1417,
                provisional: false,
                games: 22,
                history: vec![1200, 1216, 1204, 1240, 1417],
                best: 1204,
            })),
            // Somebody this server has never met, which is a real answer.
            ServerMessage::Profile(None),
            ServerMessage::People {
                like: "ali".into(),
                found: vec![crate::net::Profile {
                    who: crate::net::PersonId("3f2a91c4".into()),
                    name: "alice".into(),
                    rating: 1240,
                    provisional: false,
                    games: 9,
                    history: vec![1200, 1220, 1240],
                    best: 512,
                }],
            },
            ServerMessage::People { like: String::new(), found: Vec::new() },
            ServerMessage::Rules(crate::net::Rules {
                paused: false,
                place_anywhere: true,
                place_free: true,
                bpm: 120,
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
                        // Made by a player with a key, so a menu can offer them
                        // the door out.
                        owner: Some(crate::net::PersonId("3f2a91c4".into())),
                    },
                    crate::net::RoomInfo {
                        id: "lobby".into(),
                        name: "lobby".into(),
                        players: 0,
                        phase: crate::net::MatchPhase::Open,
                        victory: None,
                        world: crate::sim::WorldKind::Infinite,
                        rules: crate::net::Rules::default(),
                        owner: None,
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
                            bpm: 60,
                            laboratory: true,
                        },
                        owner: None,
                    },
                ],
                hidden: crate::net::Hidden { howto: true },
            },
            // The answer to a `Hello`, which the socket reads as well as the client.
            ServerMessage::You(crate::net::Profile {
                who: crate::net::PersonId("3f2a91c4".into()),
                name: "alice".into(),
                rating: 1200,
                provisional: true,
                games: 0,
                history: Vec::new(),
                best: 0,
            }),
            // Both arms, since the refusal is the one somebody reads.
            ServerMessage::Closed(Ok("r-abc234".into())),
            ServerMessage::Closed(Err("2 still in \"den\"".into())),
            ServerMessage::Invited {
                from: crate::net::Profile {
                    who: crate::net::PersonId("3f2a91c4".into()),
                    name: "alice".into(),
                    rating: 1240,
                    provisional: false,
                    games: 9,
                    history: vec![1200, 1220, 1240],
                    best: 512,
                },
                room: "r-xyz789".into(),
                name: "den".into(),
            },
            ServerMessage::NotDone { reason: "this server has never met them".into() },
            // A party with a world in it, and one with nothing but people.
            ServerMessage::Parties {
                parties: vec![
                    crate::net::PartyInfo {
                        id: crate::net::PartyId("p-3f2a91c4".into()),
                        name: "friday".into(),
                        members: vec![
                            crate::net::Member {
                                who: crate::net::PersonId("3f2a91c4".into()),
                                name: "alice".into(),
                                online: true,
                            },
                            crate::net::Member {
                                who: crate::net::PersonId("9b1c0d2e".into()),
                                name: "bob".into(),
                                online: false,
                            },
                        ],
                        rooms: vec![crate::net::RoomInfo {
                            id: "r-den123".into(),
                            name: "den".into(),
                            players: 1,
                            phase: crate::net::MatchPhase::Open,
                            victory: None,
                            world: crate::sim::WorldKind::Infinite,
                            rules: crate::net::Rules::default(),
                            owner: Some(crate::net::PersonId("3f2a91c4".into())),
                        }],
                    },
                    crate::net::PartyInfo {
                        id: crate::net::PartyId("p-77777777".into()),
                        name: "the others".into(),
                        members: Vec::new(),
                        rooms: Vec::new(),
                    },
                ],
            },
            ServerMessage::Parties { parties: Vec::new() },
            ServerMessage::PartyInvite {
                from: crate::net::Profile {
                    who: crate::net::PersonId("3f2a91c4".into()),
                    name: "alice".into(),
                    rating: 1240,
                    provisional: false,
                    games: 9,
                    history: vec![1200, 1220, 1240],
                    best: 512,
                },
                party: crate::net::PartyId("p-3f2a91c4".into()),
                name: "friday".into(),
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
