use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::app::App;
use crate::cell::{Chunk, CHUNK_N};
use crate::chunk_texture::ChunkTexture;
use crate::frame::{Draw, DrawCall};
use crate::gpu::GpuState;
use crate::pipeline::{create_pipeline, PipelineDescriptor};
use crate::world::{ChunkId, Neighbour, World};

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

/// Upper bound on chunks drawn in one frame. Sizes the instance buffer.
const MAX_INSTANCES: usize = 1024;

/// Cells of empty space to keep around the live pattern when framing.
const VIEW_PADDING: f32 = 24.0;

/// Layer 0 is permanently zeroed and shared by every chunk with no cells, so
/// Idle slots can be drawn (showing the world's structure) without each one
/// consuming a layer of its own.
const DEAD_LAYER: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    origin: [f32; 2],
    viewport: [f32; 2],
    zoom: f32,
    chunk_n: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    /// x, y, w, h in world cells.
    pub rect: [f32; 4],
    /// x = array layer; the rest is reserved.
    pub meta: [u32; 4],
}

// These must match the `Camera` struct and the instance attribute offsets in
// shaders/grid.wgsl. WGSL requires a uniform struct's size to be a multiple of
// 16, which is what the explicit padding is for.
const _: () = {
    assert!(size_of::<CameraUniform>() == 32);
    assert!(size_of::<Instance>() == 32);
};


/// The WGSL both entry points live in.
pub const SHADER_SOURCE: &str = include_str!("shaders/grid.wgsl");

/// Binding 0 is the camera uniform, binding 1 the chunk array texture.
/// Shared with the tests so a regression here fails a test, not a demo.
pub fn world_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("world"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32x4,
        offset: 16,
        shader_location: 1,
    },
];

pub fn chunk_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Instance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRS,
    }
}

pub struct BattleApp {
    pipeline: wgpu::RenderPipeline,
    bind_groups: Vec<wgpu::BindGroup>,
    vertex_buffers: Vec<wgpu::Buffer>,
    camera_buffer: wgpu::Buffer,
    chunks: ChunkTexture,
    world: World,
    /// Slot id -> array layer. Assigned on first sight and kept; eviction
    /// arrives with a residency cache, once worlds outgrow the layer budget.
    layers: HashMap<ChunkId, u32>,
    next_layer: u32,
    instances: Vec<Instance>,
}

impl BattleApp {
    /// Frame the live pattern, so the view follows it as it travels rather
    /// than shrinking towards nothing as the world grows.
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
        // Never below one pixel per cell: point sampling drops sparse cells
        // under that, which is the aliasing floor the design settles on.
        let zoom = (vw / span_x).min(vh / span_y).clamp(1.0, 64.0);

        let centre = ((min_col + max_col) * 0.5, (min_row + max_row) * 0.5);
        let origin = [
            centre.0 - vw / (2.0 * zoom),
            centre.1 - vh / (2.0 * zoom),
        ];

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

    /// Push every chunk the world currently holds to the GPU and rebuild the
    /// instance list. Called only when the world reports itself dirty.
    fn sync_world(&mut self, gpu: &GpuState) {
        self.instances.clear();

        for id in 0..self.world.slot_count() {
            let slot = self.world.slot(id);
            let (row, col) = self.world.loc(id);

            let layer = match slot {
                // Placeholders have no position worth drawing.
                Neighbour::Unloaded => continue,
                Neighbour::Idle { .. } => DEAD_LAYER,
                Neighbour::CellChunk { cells, .. } => {
                    let layer = match self.layers.get(&id) {
                        Some(&l) => l,
                        None => {
                            if self.next_layer >= self.chunks.layers {
                                log::warn!("layer budget exhausted; chunk {id} not drawn");
                                continue;
                            }
                            let l = self.next_layer;
                            self.next_layer += 1;
                            self.layers.insert(id, l);
                            l
                        }
                    };
                    self.chunks.upload(&gpu.queue, layer, cells);
                    layer
                }
            };

            if self.instances.len() == MAX_INSTANCES {
                log::warn!("instance budget exhausted; some chunks not drawn");
                break;
            }
            self.instances.push(Instance {
                rect: [
                    (col * CHUNK_N as i32) as f32,
                    (row * CHUNK_N as i32) as f32,
                    CHUNK_N as f32,
                    CHUNK_N as f32,
                ],
                meta: [layer, 0, 0, 0],
            });
        }

        gpu.queue.write_buffer(
            &self.vertex_buffers[0],
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }
}

impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let world = World::infinite();
        let chunks = ChunkTexture::new(&gpu.device, ChunkTexture::LAYER_BUDGET);

        // Layer 0 stays zeroed for the lifetime of the app.
        chunks.upload(&gpu.queue, DEAD_LAYER, &Chunk::zeroed());

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
                    resource: wgpu::BindingResource::TextureView(&chunks.view),
                },
            ],
        });

        let instance_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk instances"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        let mut app = Self {
            pipeline,
            bind_groups: vec![bind_group],
            vertex_buffers: vec![instance_buffer],
            camera_buffer,
            chunks,
            world,
            layers: HashMap::new(),
            next_layer: DEAD_LAYER + 1,
            instances: Vec::with_capacity(MAX_INSTANCES),
        };
        app.sync_world(gpu);
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
            self.sync_world(gpu);
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
                instances: 0..self.instances.len() as u32,
            },
        }]
    }

    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })
    }
}
