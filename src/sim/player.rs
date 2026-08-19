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
    /// What this player has to spend. Reclaiming your own cells earns it;
    /// placing cells, or destroying someone else's, costs it. Signed because
    /// the arithmetic is naturally signed, though the rules never let it fall
    /// below zero -- an action that cannot be afforded is refused instead.
    pub value: i32,
}

impl Player {
    /// What a player joins with.
    ///
    /// Provisional. The intended opening is a small block of cells to mine
    /// rather than a number in hand, so this only needs to be enough to build
    /// something before the mining loop takes over -- twenty cells at the
    /// current price. Reclaiming pays one against a cost of five, so the ratio
    /// is what actually sets the pace; this only sets where it starts.
    pub const STARTING_VALUE: i32 = 100;

    pub fn new(id: PlayerId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), last_seen: 0, value: Self::STARTING_VALUE }
    }
}

