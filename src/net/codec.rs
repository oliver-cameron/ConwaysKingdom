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
            ClientMessage::Join { name: "alice".into() },
            ClientMessage::Act(Stamped {
                tick: 42,
                player: PlayerId(3),
                action: Action::Paint { cells: vec![(1, 2), (-3, 4)], placement: Placement::Life },
            }),
            ClientMessage::Act(Stamped {
                tick: 7,
                player: PlayerId(1),
                action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
            }),
            ClientMessage::Act(Stamped {
                tick: 8,
                player: PlayerId(2),
                action: Action::Paint {
                    cells: vec![(3, 3)],
                    placement: Placement::Ice,
                },
            }),
            ClientMessage::Subscribe { chunks: vec![(0, 0), (-1, 5)] },
            ClientMessage::Unsubscribe { chunks: vec![(9, 9)] },
            ClientMessage::Checkpoint { tick: 100, chunks: vec![((0, 0), 0xDEAD_BEEF), ((-1, 4), 7)] },
        ];
        for msg in cases {
            let bytes = encode_client(&msg).unwrap();
            assert_eq!(decode_client(&bytes).unwrap(), msg, "{msg:?}");
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let cases = vec![
            ServerMessage::Welcome { you: PlayerId(2), tick: 5 },
            ServerMessage::Rejected { reason: "full".into() },
            ServerMessage::Actions(vec![Stamped {
                tick: 1,
                player: PlayerId(1),
                action: Action::Paint { cells: vec![(5, 5)], placement: Placement::Life },
            }]),
            ServerMessage::ChunkData { tick: 3, chunk: (-2, 7), cells: vec![1, 2, 3, 4] },
            ServerMessage::Resync { tick: 9, chunks: vec![(0, 0)] },
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
