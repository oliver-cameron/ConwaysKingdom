//! The seam between the simulation and the GPU.
//!
//! This is the only place that knows both what a chunk is and what a texture
//! layer is. Everything above it deals in chunks; everything below deals in
//! layers and draw calls.

use std::collections::{HashMap, HashSet};

use bytemuck::{Pod, Zeroable};

use crate::sim::{Cell, Coord, World, CHUNK_CELLS, CHUNK_N};

/// The WGSL both entry points live in.
pub const SHADER_SOURCE: &str = include_str!("shaders/grid.wgsl");

/// Upper bound on chunks drawn in one frame. Sizes the instance buffer.
pub const MAX_INSTANCES: usize = 1024;

/// Layer zero is never written after startup and holds nothing but dead cells.
/// One shared layer, because every unloaded chunk looks exactly the same.
pub const UNLOADED_LAYER: u32 = 0;

/// `meta.y` of an instance: what kind of quad it is.
pub const KIND_CHUNK: u32 = 0;
/// A single quad standing in for every unloaded chunk at once.
///
/// One instance rather than one per chunk, because the visible chunk count
/// grows as the square of zooming out: a 1920x1080 screen at one pixel per
/// cell covers over eight thousand of them, far past any sane instance budget,
/// so the far edges simply stopped being drawn. Since they all look identical
/// there is nothing to gain from drawing them separately.
pub const KIND_BACKDROP: u32 = 1;

/// Chunk store: a 2D array texture with one chunk per layer.
///
/// `Rgba8Uint`: R is the cell's owner byte, G its tile byte, B a neighbour
/// mask this layer derives, A spare. See [`ChunkTexture::FORMAT`] for why it is
/// four bytes and not the cell's two.
pub struct ChunkTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub layers: u32,
}

impl ChunkTexture {
    /// Four bytes rather than the cell's two.
    ///
    /// **The texture stopped being a copy of the world.** It was
    /// `bytemuck::bytes_of` over a `Chunk` — a reinterpret of simulation
    /// memory, which is why the byte layout was pinned to `sim::cell::bits`
    /// by a comment in the shader. Two of the bytes still are that. The third
    /// is a **neighbour mask**, which is not in a `Cell` and must never be:
    /// it is a fact about a cell *and its neighbours*, it is derived, and
    /// putting it in the wire format would make appearance something two
    /// clients can disagree about and desync over. It is computed here, on the
    /// way to the GPU, by the only thing that has both the whole world and no
    /// authority over it.
    ///
    /// The fourth is spare. Candidates, in the order they are worth having:
    /// the ownership level (byte 0 bits 1..4, which nothing currently reads),
    /// a second mask so a territory border can be drawn separately from a
    /// material edge, and a per-cell seed so a large flat region does not
    /// visibly repeat.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Uint;

    /// Bytes per cell in the texture, which is no longer `size_of::<Cell>()`.
    pub const STRIDE: usize = 4;

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

    /// Upload one chunk's worth of texels into one layer. `Queue::write_texture`
    /// is exempt from the 256-byte `bytes_per_row` alignment, so a 64-byte row
    /// is fine.
    ///
    /// Takes texels rather than a `&Chunk`, because they are no longer the
    /// same thing — see [`Self::FORMAT`].
    pub fn upload(&self, queue: &wgpu::Queue, layer: u32, texels: &[u8]) {
        debug_assert!(layer < self.layers);
        debug_assert_eq!(texels.len(), CHUNK_CELLS * Self::STRIDE);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((CHUNK_N * Self::STRIDE) as u32),
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

/// A chunk as the GPU wants it: its two cell bytes, and what is next to it.
///
/// **This is the pass that used to be a memcpy**, and everything a
/// neighbour-sensitive sheet needs is here rather than in the shader. A
/// fragment knows only its own array layer, and the chunk-to-layer map is a
/// `HashMap` on this side, so a cell on a chunk edge has no way to reach its
/// neighbour's layer at all. Up here the whole world is in hand and the
/// coordinates are already folded for a torus.
///
/// A kilobyte per chunk across the fifty or so on screen, once per sync.
fn texels(world: &World, chunk_at: Coord, chunk: &crate::sim::Chunk) -> Vec<u8> {
    let (base_row, base_col) = (chunk_at.0 * CHUNK_N as i32, chunk_at.1 * CHUNK_N as i32);
    let mut out = Vec::with_capacity(CHUNK_CELLS * ChunkTexture::STRIDE);
    for row in 0..CHUNK_N as i32 {
        for col in 0..CHUNK_N as i32 {
            let cell = chunk[(row as usize, col as usize)];
            let bytes = bytemuck::bytes_of(&cell);
            out.push(bytes[0]);
            out.push(bytes[1]);
            out.push(neighbours(world, cell, (base_row + row, base_col + col)));
            // Spare. See `ChunkTexture::FORMAT` for what it is for.
            out.push(0);
        }
    }
    out
}

/// Which of the four sides of this cell have the same thing on them.
///
/// **"The same thing" is "would draw the same sprite"** — the same owner and
/// the same tile byte — because what this is for is making a mass of cells
/// read as one shape, and two cells that draw differently are not one shape
/// however related they are underneath. It is the relation an outline wants.
///
/// Four sides rather than eight: an edge mask needs sixteen variants and a
/// corner-aware one needs forty-seven, and the byte holds either whenever
/// somebody draws them. Nothing here has to change for that.
///
/// A cell in a chunk this client does not hold reads as **unlike**, which is
/// the same answer the backdrop already gives: ground that has not arrived is
/// drawn as ground that is not there, and an edge against it is honest rather
/// than a border that appears when a chunk loads.
fn neighbours(world: &World, cell: Cell, (row, col): (i32, i32)) -> u8 {
    const SIDES: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
    let mut mask = 0;
    for (bit, (dr, dc)) in SIDES.into_iter().enumerate() {
        // `cell_at` folds the chunk coordinate itself, so a neighbour off the
        // edge of a torus comes back from the other side without help.
        let like = world
            .cell_at(row + dr, col + dc)
            .is_some_and(|other| other.player() == cell.player() && other.tile() == cell.tile());
        if like {
            mask |= 1 << bit;
        }
    }
    mask
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
    /// Non-zero when the fragment shader must encode sRGB itself.
    ///
    /// The shader works in linear light and returns linear light, because that
    /// is what an sRGB surface format converts on the way out. Where no sRGB
    /// format is offered — the WebGL2 path, whose default framebuffer has no
    /// encode-on-write to give — `GpuState::new` falls back to a plain
    /// `Unorm` surface, and those linear numbers reach the display as though
    /// they were already encoded. Mid grey lands at 43% of the light it should
    /// emit, so the whole image reads dark and muddy.
    ///
    /// A flag rather than a second pipeline: it is one `select` in the
    /// fragment shader, fixed for the life of the surface, and the alternative
    /// is compiling the shader twice to change its last line.
    pub encode_srgb: f32,
    pub _pad: f32,
    /// A hue per player, as a turn in `0..1`, indexed by `PlayerId`.
    ///
    /// Worked out on the client — see `client::views::hue` — because where a
    /// player sits in their team's family of hue depends on who else is on
    /// that team, which no function of one player's number can answer. The
    /// shader looks it up and does nothing else with it.
    ///
    /// Four to a `vec4` because a uniform array of scalars has a 16-byte
    /// stride in WGSL: `array<f32, 16>` would spend 256 bytes carrying 64.
    pub hues: [[f32; 4]; 4],
}

const _: () = {
    // Must match `Camera` and the instance attributes in shaders/grid.wgsl.
    // WGSL requires a uniform struct's size to be a multiple of 16.
    assert!(size_of::<CameraUniform>() == 96);
    assert!(size_of::<Instance>() == 32);
};

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32x4, offset: 16, shader_location: 1 },
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
            // The sprite sheet, filtered: a cell is a 16x16 image, so it is
            // sampled rather than fetched, unlike the cell data itself.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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
    next_free: u32,
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
            // Layer zero is reserved, so allocation starts above it.
            next_free: UNLOADED_LAYER + 1,
            free: Vec::new(),
            instances: Vec::with_capacity(MAX_INSTANCES),
            buffer,
        }
    }

    /// Zero layer zero once. It stays dead for the life of the app.
    pub fn init_unloaded_layer(&self, queue: &wgpu::Queue) {
        self.texture.upload(queue, UNLOADED_LAYER, &[0u8; CHUNK_CELLS * ChunkTexture::STRIDE]);
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
    /// list. `visible` is the region on screen, in absolute cells, as
    /// (min, max). Everything in it that the world does not hold is covered by
    /// one backdrop quad rather than a quad per chunk.
    ///
    /// **A wrapping world is drawn by folding, not by tiling.** Every chunk
    /// position the viewport covers is asked which chunk actually fills it,
    /// which on a torus is many-to-one — so the world repeats for as far as
    /// anyone can pan, and the work is proportional to the screen rather than
    /// to the world. It used to draw a fixed number of copies either side of
    /// the original, which meant panning off the third copy fell into blank
    /// space forever, and a large torus paid for nine copies of every chunk
    /// whether or not any of them were on screen.
    pub fn sync(&mut self, queue: &wgpu::Queue, world: &World, visible: ((i32, i32), (i32, i32))) {
        // Only what is on screen gets a layer. Uploading every stored chunk
        // made the budget a limit on the size of the *world*, which an
        // infinite world hides -- it holds only what life has reached -- and a
        // torus does not: a 20x20 torus stores four hundred chunks whether you
        // are looking at them or not, and asked for a layer for each.
        let (min, max) = visible;
        let wanted: HashSet<Coord> = World::chunks_covering(min, max)
            .into_iter()
            .map(|c| world.canonical(c))
            .filter(|c| world.chunk_at(*c).is_some())
            .collect();

        let free = &mut self.free;
        self.layers.retain(|coord, layer| {
            let keep = wanted.contains(coord);
            if !keep {
                free.push(*layer);
            }
            keep
        });

        for (coord, chunk) in world.stored().into_iter().filter(|(c, _)| wanted.contains(c)) {
            let layer = match self.layers.get(&coord) {
                Some(&l) => l,
                None => {
                    let fresh = self.next_free;
                    let Some(l) = self.free.pop().or_else(|| {
                        (fresh < self.texture.layers).then(|| {
                            self.next_free += 1;
                            fresh
                        })
                    }) else {
                        log::warn!("layer budget exhausted; chunk {coord:?} not drawn");
                        continue;
                    };
                    self.layers.insert(coord, l);
                    l
                }
            };
            self.texture.upload(queue, layer, &texels(world, coord, chunk));
        }

        self.instances.clear();

        // First, so the chunks drawn after it paint over it. There is no depth
        // buffer and no blending, so order alone decides.
        let ((min_row, min_col), (max_row, max_col)) = visible;
        self.instances.push(Instance {
            rect: [
                min_col as f32,
                min_row as f32,
                (max_col - min_col + 1) as f32,
                (max_row - min_row + 1) as f32,
            ],
            meta: [UNLOADED_LAYER, KIND_BACKDROP, 0, 0],
        });

        // The same positions the layers were chosen from, so anything with a
        // layer has a quad and anything without one is left to the backdrop.
        for global in World::chunks_covering(min, max) {
            let Some(&layer) = self.layers.get(&world.canonical(global)) else {
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
                meta: [layer, KIND_CHUNK, 0, 0],
            });
        }

        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.instances));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::PlayerId;

    fn alive(world: &mut World, cells: &[(i32, i32)], who: u8) {
        for &(r, c) in cells {
            world.set_cell_at(r, c, Cell::alive(PlayerId(who)));
        }
    }

    /// North, east, south, west, in that order, and only where the neighbour
    /// would draw the same sprite.
    #[test]
    fn a_side_is_set_when_the_same_thing_is_on_it() {
        let mut world = World::infinite_empty();
        alive(&mut world, &[(5, 5), (4, 5), (5, 6)], 1);
        let cell = world.cell_at(5, 5).unwrap();

        // North and east are like it; south and west are empty ground.
        assert_eq!(neighbours(&world, cell, (5, 5)), 0b0011);
        // The neighbour to the north sees it back, and nothing else.
        let north = world.cell_at(4, 5).unwrap();
        assert_eq!(neighbours(&world, north, (4, 5)), 0b0100);
    }

    /// A cell with nothing round it is an island on every side, which is what
    /// makes a single cell draw a full outline rather than none.
    #[test]
    fn a_cell_on_its_own_is_open_on_all_four_sides() {
        let mut world = World::infinite_empty();
        alive(&mut world, &[(0, 0)], 1);
        assert_eq!(neighbours(&world, world.cell_at(0, 0).unwrap(), (0, 0)), 0);
    }

    /// **The relation is "would draw the same sprite"**, not "is alive": a
    /// mass of cells is one shape only where it looks like one, so somebody
    /// else's cell next to yours is an edge and so is a mine next to life.
    #[test]
    fn a_different_owner_or_a_different_kind_is_an_edge() {
        let mut world = World::infinite_empty();
        alive(&mut world, &[(0, 0)], 1);
        alive(&mut world, &[(0, 1)], 2);
        world.set_cell_at(1, 0, Cell::alive(PlayerId(1)).with_kind(crate::sim::Kind::MINE));
        let mine = world.cell_at(0, 0).unwrap();
        assert_eq!(neighbours(&world, mine, (0, 0)), 0, "an edge was missed");
    }

    /// A torus has no edge, so a cell against the seam is joined to whatever
    /// is on the other side of it -- `cell_at` folds the coordinate, so this
    /// needs no help here and is worth a test to keep it that way.
    #[test]
    fn a_torus_joins_across_its_seam() {
        let mut world = World::toroidal(1, 1);
        let n = CHUNK_N as i32;
        alive(&mut world, &[(0, 0), (n - 1, 0)], 1);
        let cell = world.cell_at(0, 0).unwrap();
        assert_eq!(neighbours(&world, cell, (0, 0)) & 1, 1, "the seam was an edge");
    }

    /// Ground this client has not been sent reads as unlike, which is the same
    /// answer the backdrop gives: an edge against ground that is not there is
    /// honest, and a border that appeared when a chunk loaded would not be.
    #[test]
    fn an_unheld_chunk_is_not_a_neighbour() {
        let mut world = World::infinite_empty();
        alive(&mut world, &[(0, 0)], 1);
        assert!(world.cell_at(-1, 0).is_none(), "the test needs an unheld chunk");
        assert_eq!(neighbours(&world, world.cell_at(0, 0).unwrap(), (0, 0)) & 1, 0);
    }

    /// Four bytes a cell, and the first two are still the cell's own -- the
    /// shader reads them by name and a swap here would be silent.
    #[test]
    fn a_chunk_becomes_four_bytes_a_cell_with_the_cell_first() {
        let mut world = World::infinite_empty();
        alive(&mut world, &[(0, 0), (0, 1)], 3);
        let chunk = world.chunk_at((0, 0)).unwrap().clone();
        let texels = texels(&world, (0, 0), &chunk);

        assert_eq!(texels.len(), CHUNK_CELLS * ChunkTexture::STRIDE);
        let cell = world.cell_at(0, 0).unwrap();
        assert_eq!(&texels[..2], bytemuck::bytes_of(&cell));
        assert_eq!(texels[2], neighbours(&world, cell, (0, 0)), "the mask is the third byte");
        assert_eq!(texels[3], 0, "the fourth is spare");
    }
}
