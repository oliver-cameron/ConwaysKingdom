//! Who owns a cell.
//!
//! Part of the simulation rather than the network: a cell carries an owner,
//! so the world model needs the concept whether or not anyone is connected.
//! The network layer names players, it does not define them.

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

use super::cell::bits;

/// A player's number. Zero means unowned, so a zeroed cell is dead and
/// unclaimed. The cell only has five bits for this, hence [`PlayerId::MAX`].
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Pod, Zeroable, Serialize,
    Deserialize,
)]
pub struct PlayerId(pub u8);

impl PlayerId {
    pub const UNOWNED: Self = Self(0);
    /// Five bits in the cell, so 1..=31 are real players.
    pub const MAX: u8 = (1 << bits::PLAYER_WIDTH) - 1;

    pub const fn is_owned(self) -> bool {
        self.0 != 0
    }
}

/// A connected player.
///
/// The id is the same number a cell stores, so a cell's owner is looked up
/// without translation — which also means the world can only distinguish
/// [`PlayerId::MAX`] of them at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    /// Tick this player was last heard from, for timing out a dead
    /// connection. A plain counter here so `sim` owes `net` nothing.
    pub last_seen: u64,
}

impl Player {
    pub fn new(id: PlayerId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), last_seen: 0 }
    }
}

