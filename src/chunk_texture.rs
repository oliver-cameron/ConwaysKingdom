use crate::cell::{Chunk, CHUNK_N};

/// The chunk store: a 2D array texture with one chunk per layer.
///
/// `Rgba8Uint` rather than `Rg8Uint` because it is the narrowest format that
/// is also storage-capable, so moving the simulation to a compute shader later
/// stays a format-and-dispatch change rather than a storage rewrite.
pub struct ChunkTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub layers: u32,
}

impl ChunkTexture {
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;

    /// Layers to allocate. This is the guaranteed floor for
    /// `max_texture_array_layers`, and it is allocated up front because an
    /// array texture cannot be resized. Residency management arrives with
    /// chunk loading; for now only layer 0 is used.
    ///
    /// It must be greater than 1. The GL backend picks its texture target from
    /// the *texture* descriptor, not the view: `depth_or_array_layers == 1`
    /// makes it a `TEXTURE_2D`, and a `D2Array` view over that fails with
    /// "wgpu-hal heuristics assumed that the view dimension will be equal to
    /// `D2` rather than `D2Array`". See wgpu-hal `gles/mod.rs`,
    /// `get_info_from_desc`.
    pub const LAYER_BUDGET: u32 = 256;

    pub fn new(device: &wgpu::Device, layers: u32) -> Self {
        let layers = layers
            .clamp(2, device.limits().max_texture_array_layers.max(2));
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
    /// the 256-byte `bytes_per_row` alignment, so a 64-byte row is fine here.
    pub fn upload(&self, queue: &wgpu::Queue, layer: u32, chunk: &Chunk) {
        debug_assert!(layer < self.layers);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer }, // z selects the layer
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
