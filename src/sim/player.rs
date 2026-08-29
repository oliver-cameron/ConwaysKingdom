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
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Hash,
    Pod,
    Zeroable,
    Serialize,
    Deserialize,
)]
pub struct PlayerId(pub u8);

impl PlayerId {
    pub const UNOWNED: Self = Self(0);
    /// Five bits in the cell, so 1..=31 are real players.
    pub const MAX: u8 = (1 << bits::PLAYER_WIDTH) - 1;
    /// Every number a cell can carry, zero included. The width of anything
    /// kept per player and indexed by the number the cell holds.
    pub const COUNT: usize = Self::MAX as usize + 1;

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
    /// The secret this player proves themselves with on a reconnect.
    ///
    /// Not authentication: it proves nothing to anybody else, and whoever
    /// holds it *is* this player. It is a claim ticket, and that is all a game
    /// with no accounts needs — what it buys is that a player who drops comes
    /// back to their own number, their own value and their own ground, rather
    /// than to a fresh player number beside a patch of land they can see and
    /// cannot build on.
    ///
    /// A name would not do: two players may pick the same one, and anybody
    /// could claim yours.
    pub token: String,
    /// Whether they are connected right now.
    ///
    /// A player who leaves is remembered rather than removed. Their number is
    /// their identity — every cell they own carries it — so handing it to
    /// somebody else would hand over their territory with it, and the ground
    /// outlives the connection.
    pub online: bool,
    /// **Who is sitting here**, as against which seat this is.
    ///
    /// The id alone -- never the proof, which lives in the server's own table
    /// and in the client's store and is not a thing a world has any business
    /// persisting. A `String` rather than `net::PersonId` because `sim` owes
    /// `net` nothing, the same reason `last_seen` is a plain counter.
    ///
    /// `None` for a seat filled before this existed, and for one filled by a
    /// client that has not been told who it is yet. A seat with no person is
    /// still a player: it plays, it holds ground, it comes back with its
    /// token. What it cannot do is carry anything that outlives the room.
    pub person: Option<String>,
}

impl Player {
    /// What a player joins with. The number itself lives with the other prices
    /// in [`crate::sim::rule`], which is where anybody balancing the game
    /// looks.
    pub const STARTING_VALUE: i32 = super::rule::STARTING_VALUE;

    pub fn new(id: PlayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            last_seen: 0,
            value: Self::STARTING_VALUE,
            token: String::new(),
            online: true,
            person: None,
        }
    }
}
