//! Everything that touches the GPU or the window.
//!
//! **Client-only.** A headless server links [`crate::sim`] and [`crate::net`]
//! and never compiles this module, so nothing here may leak into either.
//!
//! The split runs one way: rendering reads the simulation, the simulation
//! knows nothing about rendering. [`chunks::ChunkStore`] is the seam — it
//! turns chunks into texture layers, and is the only place that knows both.

pub mod app;
pub mod chunks;
pub mod context;
pub mod pipeline;

pub use app::{run, App};
pub use chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, ChunkTexture,
    Instance, MAX_INSTANCES, SHADER_SOURCE,
};
pub use context::{Draw, DrawCall, Frame, FrameAcquire, GpuState, IndexBufferBinding};
pub use pipeline::{create_pipeline, create_pipeline_with, PipelineDescriptor};
