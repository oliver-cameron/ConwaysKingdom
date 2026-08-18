//! The seam between the simulation and the GPU.
//!
//! This is the only place that knows both what a chunk is and what a texture
//! layer is. Everything above it deals in chunks; everything below deals in
//! layers and draw calls.

use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};

use crate::sim::{Chunk, Coord, World, CHUNK_N};

/// The WGSL both entry points live in.
pub const SHADER_SOURCE: &str = include_str!("shaders/grid.wgsl");

/// Upper bound on chunks drawn in one frame. Sizes the instance buffer.
pub const MAX_INSTANCES: usize = 1024;

/// Chunk store: a 2D array texture with one chunk per layer.
///
/// `R16Uint`: one 16-bit integer per cell, matching `Cell`'s bit layout, so the
/// shader unpacks fields with shifts rather than reading channels.
///
/// Note this is *not* storage-capable — no 1- or 2-byte format is; the smallest
/// are `Rgba8Uint` and `R32Uint`. Moving the simulation to a compute shader
/// would therefore mean widening the cell, not just changing a constant.
pub struct ChunkTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub layers: u32,
}

impl ChunkTexture {
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

    /// Layers to allocate, the guaranteed floor for `max_texture_array_layers`.
    /// Allocated up front because an array texture cannot be resized.
    ///
    /// Must exceed 1: the GL backend picks its texture target from the texture
    /// descriptor, not the view, so `depth_or_array_layers == 1` makes a
    /// `TEXTURE_2D` and a `D2Array` view over it fails. See wgpu-hal
    /// `gles/mod.rs`, `get_info_from_desc`.
    pub const LAYER_BUDGET: u32 = 256;

    pub fn new(device: &wgpu::Device, layers: u32) -> Self {
        let layers = layers.clamp(2, device.limits().max_texture_array_layers.max(2));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("chunk store"),
            size: wgpu::Extent3d {
                width: CHUNK_N as u32,
                height: CHUNK_N as u32,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Spell the dimension out: the default would give a D2 view, which
        // fails validation against a `texture_2d_array<u32>` binding.
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        Self { texture, view, layers }
    }

    /// Upload one chunk into one layer. `Queue::write_texture` is exempt from
    /// the 256-byte `bytes_per_row` alignment, so a 64-byte row is fine.
    pub fn upload(&self, queue: &wgpu::Queue, layer: u32, chunk: &Chunk) {
        debug_assert!(layer < self.layers);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            chunk.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(Chunk::bytes_per_row()),
                rows_per_image: Some(CHUNK_N as u32),
            },
            wgpu::Extent3d {
                width: CHUNK_N as u32,
                height: CHUNK_N as u32,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// One drawn chunk.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    /// x, y, w, h in world cells.
    pub rect: [f32; 4],
    /// x = array layer; the rest reserved.
    pub meta: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct CameraUniform {
    pub origin: [f32; 2],
    pub viewport: [f32; 2],
    pub zoom: f32,
    pub chunk_n: f32,
    pub _pad: [f32; 2],
}

const _: () = {
    // Must match `Camera` and the instance attributes in shaders/grid.wgsl.
    // WGSL requires a uniform struct's size to be a multiple of 16.
    assert!(size_of::<CameraUniform>() == 32);
    assert!(size_of::<Instance>() == 32);
};

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

/// Binding 0 is the camera uniform, binding 1 the chunk array texture.
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

/// Holds the chunk texture, decides which chunk occupies which layer, and
/// builds the per-frame instance list.
///
/// Layers are allocated once and re-pointed as chunks come and go, which
/// matters because an infinite world drops chunks the moment life leaves them.
pub struct ChunkStore {
    texture: ChunkTexture,
    /// Canonical chunk coordinate -> array layer. Canonical, so a torus chunk
    /// drawn at nine global positions still occupies one layer.
    layers: HashMap<Coord, u32>,
    free: Vec<u32>,
    instances: Vec<Instance>,
    buffer: wgpu::Buffer,
}

impl ChunkStore {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk instances"),
            size: (MAX_INSTANCES * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture: ChunkTexture::new(device, ChunkTexture::LAYER_BUDGET),
            layers: HashMap::new(),
            free: Vec::new(),
            instances: Vec::with_capacity(MAX_INSTANCES),
            buffer,
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.texture.view
    }

    pub fn instance_buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn instance_count(&self) -> u32 {
        self.instances.len() as u32
    }

    /// Push every chunk the world holds to the GPU and rebuild the instance
    /// list. `repeats` is how many copies of a toroidal world to draw either
    /// side of the original; it is ignored for infinite worlds.
    pub fn sync(&mut self, queue: &wgpu::Queue, world: &World, repeats: i32) {
        let present: HashSet<Coord> = world.stored().iter().map(|&(c, _)| c).collect();
        let free = &mut self.free;
        self.layers.retain(|coord, layer| {
            let keep = present.contains(coord);
            if !keep {
                free.push(*layer);
            }
            keep
        });

        for (coord, chunk) in world.stored() {
            let layer = match self.layers.get(&coord) {
                Some(&l) => l,
                None => {
                    let next = self.layers.len() as u32;
                    let Some(l) = self
                        .free
                        .pop()
                        .or_else(|| (next < self.texture.layers).then_some(next))
                    else {
                        log::warn!("layer budget exhausted; chunk {coord:?} not drawn");
                        continue;
                    };
                    self.layers.insert(coord, l);
                    l
                }
            };
            self.texture.upload(queue, layer, chunk);
        }

        self.instances.clear();
        for (global, canonical) in world.render_tiles(repeats) {
            let Some(&layer) = self.layers.get(&canonical) else {
                continue;
            };
            if self.instances.len() == MAX_INSTANCES {
                log::warn!("instance budget exhausted; some chunks not drawn");
                break;
            }
            self.instances.push(Instance {
                rect: [
                    (global.1 * CHUNK_N as i32) as f32,
                    (global.0 * CHUNK_N as i32) as f32,
                    CHUNK_N as f32,
                    CHUNK_N as f32,
                ],
                meta: [layer, 0, 0, 0],
            });
        }

        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.instances));
    }
}
