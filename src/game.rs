use bytemuck::{Pod, Zeroable};

use crate::app::App;
use crate::cell::CHUNK_N;
use crate::chunk_texture::ChunkTexture;
use crate::frame::{Draw, DrawCall};
use crate::gpu::GpuState;
use crate::pipeline::{create_pipeline, PipelineDescriptor};
use crate::world::World;

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

/// Fraction of the shorter viewport axis the chunk should occupy. Fixed for
/// now — pan and zoom are handled by the camera uniform but not yet driven by
/// input.
const FIT_MARGIN: f32 = 0.8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    origin: [f32; 2],
    viewport: [f32; 2],
    zoom: f32,
    _pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    /// x, y, w, h in world cells.
    rect: [f32; 4],
    /// x = array layer; the rest is reserved.
    meta: [u32; 4],
}

// These must match the `Camera` struct and the instance attribute offsets in
// shaders/grid.wgsl. WGSL requires a uniform struct's size to be a multiple of
// 16, which is what the explicit padding is for.
const _: () = {
    assert!(size_of::<CameraUniform>() == 32);
    assert!(size_of::<Instance>() == 32);
};

pub struct BattleApp {
    pipeline: wgpu::RenderPipeline,
    bind_groups: Vec<wgpu::BindGroup>,
    vertex_buffers: Vec<wgpu::Buffer>,
    camera_buffer: wgpu::Buffer,
    chunks: ChunkTexture,
    world: World,
    instance_count: u32,
}

impl BattleApp {
    /// Centre the single chunk in the viewport at a zoom that fits it.
    fn write_camera(&self, gpu: &GpuState) {
        let (w, h) = (gpu.size.0 as f32, gpu.size.1 as f32);
        let span = CHUNK_N as f32;
        let zoom = (FIT_MARGIN * w.min(h) / span).max(1.0);
        // Put the chunk's centre at the viewport's centre.
        let origin = [span * 0.5 - w / (2.0 * zoom), span * 0.5 - h / (2.0 * zoom)];

        gpu.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                origin,
                viewport: [w, h],
                zoom,
                _pad: [0.0; 3],
            }),
        );
    }
}

impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let world = World::new();
        let chunks = ChunkTexture::new(&gpu.device, 1);

        let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            });

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

        // One instance: the single chunk, occupying cells [0, 16) on both axes.
        let instances = [Instance {
            rect: [0.0, 0.0, CHUNK_N as f32, CHUNK_N as f32],
            meta: [0, 0, 0, 0],
        }];
        let instance_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk instances"),
            size: size_of_val(&instances) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue
            .write_buffer(&instance_buffer, 0, bytemuck::cast_slice(&instances));

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
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
            ],
        };

        let pipeline = create_pipeline(
            gpu,
            &PipelineDescriptor {
                label: "chunk pipeline",
                shader_source: include_str!("shaders/grid.wgsl"),
                vertex_buffers: &[instance_layout],
                bind_group_layouts: &[Some(&bgl)],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
        );

        let app = Self {
            pipeline,
            bind_groups: vec![bind_group],
            vertex_buffers: vec![instance_buffer],
            camera_buffer,
            chunks,
            world,
            instance_count: instances.len() as u32,
        };
        app.write_camera(gpu);
        app.chunks.upload(&gpu.queue, 0, app.world.chunk());
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            self.chunks.upload(&gpu.queue, 0, self.world.chunk());
            self.world.dirty = false;
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
                instances: 0..self.instance_count,
            },
        }]
    }

    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })
    }
}
