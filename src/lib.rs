//! Conway's game of life, weaponised.
//!
//! Three modules, split by who needs them:
//!
//! - [`sim`] — the deterministic simulation. Client **and** server.
//! - [`net`] — wire types. Client **and** server.
//! - [`render`] — GPU and windowing. Client only.
//!
//! [`client`] wires all three together into the app that runs today.

pub mod net;
#[cfg(feature = "render")]
pub mod render;
pub mod server;
pub mod sim;

#[cfg(feature = "render")]
pub mod client;

#[cfg(feature = "render")]
pub use client::BattleApp;
#[cfg(feature = "render")]
pub use render::run;

// Re-exported so tests and downstream code need not spell out the module path.
pub use net::{Action, ChunkId, ClientMessage, ServerMessage, Stamped, Tick};
#[cfg(feature = "render")]
pub use render::{
    chunk_instance_layout, create_pipeline, create_pipeline_with, world_bind_group_layout, App,
    ChunkStore, ChunkTexture, Draw, DrawCall, Frame, FrameAcquire, GpuState, IndexBufferBinding,
    PipelineDescriptor, SHADER_SOURCE,
};
pub use sim::{
    bits, Cell, Chunk, Coord, Dir, Halo, Player, PlayerId, World, WorldKind, CHUNK_CELLS, CHUNK_N,
    HALO_N,
};

#[cfg(all(target_arch = "wasm32", feature = "render"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", feature = "render"))]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).expect("could not init logger");
    wasm_bindgen_futures::spawn_local(run::<BattleApp>());
}
