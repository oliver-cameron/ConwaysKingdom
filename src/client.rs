//! The client: a world, a view of it, and the loop that keeps them in step.
//!
//! Deliberately thin. The rules live in [`crate::sim`] and the GPU work in
//! [`crate::render`]; this only decides *policy* — how fast generations run,
//! which world to open, and where to point the camera. A headless server would
//! keep the [`World`] and drop everything else in this file.

use crate::render::app::App;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, CHUNK_N};

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

/// Cells of empty space to keep around the live pattern when framing.
const VIEW_PADDING: f32 = 24.0;

/// How many copies of a toroidal world to draw either side of the original, so
/// the tiling can be seen tiling. Ignored for infinite worlds.
const TORUS_REPEATS: i32 = 1;

/// Which world the app opens.
const WORLD: WorldMode = WorldMode::Infinite;

/// A toroidal world's size, as (chunks high, chunks wide).
const TORUS_CHUNKS: (i32, i32) = (16, 16);

#[derive(Clone, Copy, PartialEq, Eq)]
// `WORLD` is a const, so whichever arm is not selected reads as dead.
#[allow(dead_code)]
pub enum WorldMode {
    Infinite,
    Torus,
}

pub struct BattleApp {
    pipeline: wgpu::RenderPipeline,
    bind_groups: Vec<wgpu::BindGroup>,
    vertex_buffers: Vec<wgpu::Buffer>,
    camera_buffer: wgpu::Buffer,
    chunks: ChunkStore,
    world: World,
}

impl BattleApp {
    /// Frame the live pattern, so the view follows it rather than shrinking
    /// towards nothing as the world grows.
    fn write_camera(&self, gpu: &GpuState) {
        let (vw, vh) = (gpu.size.0 as f32, gpu.size.1 as f32);

        let live = self.world.live_cells();
        let (min_row, min_col, max_row, max_col) = if live.is_empty() {
            (0.0, 0.0, CHUNK_N as f32, CHUNK_N as f32)
        } else {
            let (mut r0, mut c0, mut r1, mut c1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for &(r, c) in &live {
                r0 = r0.min(r as f32);
                c0 = c0.min(c as f32);
                r1 = r1.max(r as f32 + 1.0);
                c1 = c1.max(c as f32 + 1.0);
            }
            (r0, c0, r1, c1)
        };

        let span_x = (max_col - min_col) + 2.0 * VIEW_PADDING;
        let span_y = (max_row - min_row) + 2.0 * VIEW_PADDING;
        // Never below one pixel per cell: point sampling drops sparse cells.
        let zoom = (vw / span_x).min(vh / span_y).clamp(1.0, 64.0);

        let centre = ((min_col + max_col) * 0.5, (min_row + max_row) * 0.5);
        let origin = [centre.0 - vw / (2.0 * zoom), centre.1 - vh / (2.0 * zoom)];

        gpu.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                origin,
                viewport: [vw, vh],
                zoom,
                chunk_n: CHUNK_N as f32,
                _pad: [0.0; 2],
            }),
        );
    }
}

impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let world = match WORLD {
            WorldMode::Infinite => World::infinite(),
            WorldMode::Torus => World::toroidal(TORUS_CHUNKS.0, TORUS_CHUNKS.1),
        };
        let mut chunks = ChunkStore::new(&gpu.device);
        chunks.sync(&gpu.queue, &world, TORUS_REPEATS);

        let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = world_bind_group_layout(&gpu.device);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(chunks.view()),
                },
            ],
        });

        let pipeline = create_pipeline(
            gpu,
            &PipelineDescriptor {
                label: "chunk pipeline",
                shader_source: SHADER_SOURCE,
                vertex_buffers: &[chunk_instance_layout()],
                bind_group_layouts: &[Some(&bgl)],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
        );

        let vertex_buffers = vec![chunks.instance_buffer().clone()];
        let mut app = Self {
            pipeline,
            bind_groups: vec![bind_group],
            vertex_buffers,
            camera_buffer,
            chunks,
            world,
        };
        app.world.dirty = false;
        app.write_camera(gpu);
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS);
            self.world.dirty = false;
            self.write_camera(gpu);
        }
    }

    fn draw_calls(&self) -> Vec<DrawCall<'_>> {
        vec![DrawCall {
            pipeline: &self.pipeline,
            bind_groups: &self.bind_groups,
            vertex_buffers: &self.vertex_buffers,
            index_buffer: None,
            draw: Draw::Vertices {
                vertices: 0..4,
                instances: 0..self.chunks.instance_count(),
            },
        }]
    }

    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })
    }
}
