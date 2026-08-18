mod app;
mod cell;
mod chunk_texture;
mod frame;
mod game;
mod gpu;
mod pipeline;
mod world;

pub use app::{run, App};
pub use cell::{Cell, Chunk, CHUNK_CELLS, CHUNK_N};
pub use chunk_texture::ChunkTexture;
pub use frame::{Draw, DrawCall, Frame, FrameAcquire, IndexBufferBinding};
pub use game::{
    chunk_instance_layout, world_bind_group_layout, BattleApp, Instance, GENERATION_SPAN,
    SHADER_SOURCE,
};
pub use gpu::GpuState;
pub use pipeline::{create_pipeline, create_pipeline_with, PipelineDescriptor};
pub use world::World;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).expect("could not init logger");
    wasm_bindgen_futures::spawn_local(run::<BattleApp>());
}
