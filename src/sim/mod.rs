//! The deterministic simulation: cells, chunks, worlds, and the rules.
//!
//! **This module runs on both the client and the server, and must produce
//! identical results on each.** Client-side prediction depends on it: a client
//! advances its own copy of the world and only needs the server for things it
//! cannot derive — another player's input, a global change, or chunk data from
//! outside the region it holds.
//!
//! What that buys is also what it costs. Nothing here may:
//!
//! - depend on iteration order of a `HashMap` or `HashSet`, whose order varies
//!   between processes and between runs;
//! - use floating point, wall-clock time, randomness, or thread scheduling;
//! - touch the GPU, the window, the filesystem or the network.
//!
//! The step is therefore a pure function of (world state, tick). Two worlds
//! given the same starting state and the same inputs stay byte-identical, and
//! [`World::digest`] exists so a client and server can cheaply confirm that.
//!
//! Nothing in [`crate::render`] may be referenced from here.

mod cell;
mod dir;
mod player;
mod rule;
mod seed;
mod world;

pub use cell::{bits, Cell, Chunk, Halo, Kind, Mined, CHUNK_CELLS, CHUNK_N, HALO_N};
pub use dir::Dir;
pub use player::{Player, PlayerId};
pub use rule::{
    next_cell, Chance, Neighbours, RuleFn, Then, BORN_ON, ICE_COST, LEVEL_ADJUST, LEVEL_FALL,
    LIFE_COST, MINE_COST, MINE_DRAIN, MINE_UPKEEP, MINE_YIELD, RECLAIM, RULES,
    STARTING_VALUE, SURVIVES_ON, THIN_MULTIPLIER, TURRET_COST, TURRET_DECAY, TURRET_POWER, TURRET_PUSH, TURRET_REACH,
};
pub use seed::{mix, Roll, OUT_OF};
pub use world::{parse_torus, Coord, World, WorldKind};
