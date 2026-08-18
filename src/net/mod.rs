//! Wire types shared by client and server.
//!
//! Scaffolding: the shapes are here so the seams exist, but no transport,
//! encoding or session handling is implemented yet.
//!
//! The model this is shaped for: both sides hold a copy of the world and run
//! the same deterministic step from [`crate::sim`]. The client holds less of it
//! — roughly its viewport and a margin — and advances it locally. The server is
//! authoritative and is consulted only for what a client cannot derive:
//!
//! 1. other players' actions,
//! 2. changes with no local cause (spawns, scripted events, admin edits),
//! 3. chunks the client does not hold, when its viewport moves.
//!
//! Nothing here may depend on [`crate::render`].

pub mod codec;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;

use serde::{Deserialize, Serialize};

use crate::sim::{Coord, PlayerId};

/// A chunk is identified by where it is. There is no separate id to allocate,
/// keep unique, or reconcile after a reconnect — two peers naming the same
/// coordinate mean the same chunk. On a toroidal world, fold with
/// [`crate::sim::World::canonical`] before comparing.
pub type ChunkId = Coord;

/// Generation number. The unit of lockstep: an action is applied *at* a tick,
/// so both sides apply it at the same point in the sequence.
pub type Tick = u64;

/// Something a player did. Deliberately not raw keystrokes: input is resolved
/// to a world effect before it goes on the wire, so the server validates an
/// intent rather than replaying a keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Bring cells to life for this player, at absolute cell coordinates.
    Paint { cells: Vec<(i32, i32)> },
    /// Kill cells at absolute cell coordinates.
    Erase { cells: Vec<(i32, i32)> },
}

/// An action stamped with who did it and when, which is what makes replay on
/// another peer produce the same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    pub tick: Tick,
    pub player: PlayerId,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    Join { name: String },
    /// What this player did, and when they believe it happened.
    Act(Stamped),
    /// The chunks the client now needs, because its viewport moved.
    Subscribe { chunks: Vec<ChunkId> },
    /// Chunks the client has dropped and no longer wants updates for.
    Unsubscribe { chunks: Vec<ChunkId> },
    /// The client's digest at a tick, so the server can spot a desync.
    Checkpoint { tick: Tick, digest: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Accepted, and here is the number your cells will carry.
    Welcome { you: PlayerId, tick: Tick },
    Rejected { reason: String },
    /// Actions by other players, to be applied at the tick they carry.
    Actions(Vec<Stamped>),
    /// Full contents of a chunk the client does not hold. Bytes are a chunk's
    /// cells exactly as `Chunk::as_bytes` produces them.
    ChunkData { tick: Tick, chunk: ChunkId, cells: Vec<u8> },
    /// The client's copy of these chunks is wrong; here they are again.
    Resync { tick: Tick, chunks: Vec<ChunkId> },
}
