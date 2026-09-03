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

/// What a viewport needs drawn, and what has to be resident to draw it.
///
/// **Two different numbers**, which is the whole point of separating them. A
/// quad is cheap and there is one per position on screen; a texture layer is
/// scarce and there is one per *distinct* chunk. On a plane the two are equal.
/// On a torus they are not, and that is what makes a wrapping world cost the
/// same however far anybody pans.
pub struct Covered {
    /// Every chunk position the viewport covers, in global coordinates. One
    /// quad each, and bounded by the screen.
    pub positions: Vec<Coord>,
    /// The distinct chunks behind them, folded onto what the world actually
    /// holds. One texture layer each, and on a torus bounded by the world.
    pub chunks: HashSet<Coord>,
}

/// Which chunks a viewport covers, and which of them are distinct.
///
/// **A wrapping world is drawn by folding, not by tiling.** Every position the
/// viewport covers is asked which chunk actually fills it, which on a torus is
/// many-to-one — so the world repeats for as far as anyone can pan, one copy
/// is uploaded however many times it appears, and a viewport straddling the
/// seam costs one extra quad rather than one extra resident.
///
/// Pure, and out here rather than inline in [`ChunkStore::sync`], because it
/// is the one arithmetic in this module that decides what is on screen and
/// there was no way to reach it without a device.
///
/// **It also says where low zoom stops working**, which is worth reading off
/// rather than discovering: `positions` grows as the square of zooming out,
/// so a 1920x1080 screen covers about `8100 / zoom²` of them. Against
/// [`ChunkTexture::LAYER_BUDGET`] that runs out at about zoom five, and
/// against [`MAX_INSTANCES`] at about zoom three. Below that a screen is
/// mostly backdrop however good the sampling is — see
/// [planned.md](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#zooming-out-without-lying).
pub fn covered(world: &World, visible: ((i32, i32), (i32, i32))) -> Covered {
    let positions = World::chunks_covering(visible.0, visible.1);
    let chunks = positions
        .iter()
        .map(|&c| world.canonical(c))
        .filter(|c| world.chunk_at(*c).is_some())
        .collect();
    Covered { positions, chunks }
}

/// `meta.y` of an instance: one quad standing in for the whole world, drawn
/// from the coarse texture rather than from a chunk.
pub const KIND_COARSE: u32 = 2;

/// Below this many pixels per cell, the world is drawn coarsely.
///
/// Four is where the fine path stops being able to be *resident*: one chunk is
/// one array layer, and a 1080p screen wants more than the guaranteed 256 of
/// them under about zoom five. It is also about where a sprite stops being
/// legible, which is the thing the coarse path drops.
///
/// **It is not where the picture stops being right**, and that gap is a known
/// fault. The world pass takes one reading a pixel, so below sixteen — one
/// screen pixel to a texel — there are texels no pixel ever reads, and the
/// sprites are one- and two-texel strokes. Everything between here and there
/// is drawn from a subset of the art it is meant to show. See
/// [planned.md](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#texels-nothing-samples).
pub const COARSE_BELOW: f32 = 1.5;

/// And back to the fine path only above this.
///
/// **Hysteresis, because two paths and one threshold flicker.** A scroll wheel
/// resting on the boundary would swap every frame, and the two paths do not
/// draw identically — one has sprites and outlines and the other has flat
/// colour — so the swap is visible and must not happen twice a second.
pub const FINE_ABOVE: f32 = 2.0;

/// The fine path is already drawing the coarse answer by the time either of
/// those is reached, so the swap has nothing left to show.
///
/// `grid.wgsl` fades a cell into its art-less colour between `FLAT_FROM` and
/// `FLAT_BY`, and `FLAT_BY` is under both thresholds above — which is what
/// makes the handover look the same going down as coming back up. **Kept in
/// step by hand**: lowering either of these under `FLAT_BY` puts the pop back.
pub const FLAT_BY_IN_SHADER: f32 = 1.5;

/// The world as one texel a cell: the cell without its art.
///
/// **What low zoom actually needs.** The fine path spends 16x16 texels of
/// sprite on every cell, which is the whole reason residency is one array
/// layer per chunk and the whole reason it runs out — and below about four
/// pixels a cell that sprite is not legible anyway. So this drops the art and
/// keeps the cell: one ordinary 2D texture, one quad, no sheet lookup, and a
/// count that stops growing with how far out anybody is.
///
/// `Rg8Uint`, which is the cell's own two bytes and nothing derived: R is the
/// owner byte, so `>> PLAYER_SHIFT` is the player exactly as the fine path
/// reads it, and G is the tile byte, whose bit 0 is alive and bit 1 is ice.
/// Nothing here is summarised, averaged or reduced, so there is nothing that
/// can disagree with what the fine path would have drawn.
pub struct CoarseTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// What the texture currently holds: the world rect it covers, in cells,
    /// as (row, col) of its top-left and its size.
    pub(crate) window: Option<((i32, i32), (i32, i32))>,
    /// The generation that window was filled at, so a still world is filled
    /// once rather than every frame.
    filled_at: u64,
}

impl CoarseTexture {
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Uint;
    pub const STRIDE: usize = 2;

    /// Cells a side. 1024 is two megabytes and covers a 64x64-chunk world
    /// whole, which is the largest a client may ask for — see
    /// `menu::draft::MAX_CHUNKS` — so any torus somebody makes fits in one
    /// texture with no window to scroll and no seam to get wrong.
    pub const SIDE: u32 = 1024;

    /// How far the view's middle must travel before the window follows it.
    ///
    /// Four chunks. Small against [`Self::SIDE`], so the window always keeps
    /// hundreds of cells of margin on every side and nothing can be seen to
    /// pop in at its edge; large enough that ordinary panning and zooming
    /// cross it rarely rather than every frame. See [`Self::window_for`].
    pub const STEP: i32 = CHUNK_N as i32 * 4;

    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("coarse world"),
            size: wgpu::Extent3d {
                width: Self::SIDE,
                height: Self::SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, window: None, filled_at: u64::MAX }
    }

    /// Which world rect the coarse texture should hold, given what is on
    /// screen.
    ///
    /// **A torus that fits is held whole**, which is the case worth optimising
    /// for and the one that makes a wrapping world repeat visibly: there is no
    /// window to recentre, no strips to upload as somebody pans, and the
    /// shader's wrap is the world's own. Anything larger, and any boundless
    /// world, gets a window centred on the view.
    pub fn window_for(
        world: &World,
        visible: ((i32, i32), (i32, i32)),
    ) -> ((i32, i32), (i32, i32)) {
        let side = Self::SIDE as i32;
        if let Some((height, width)) = world.size_in_cells() {
            if height <= side && width <= side {
                return ((0, 0), (height, width));
            }
        }
        let ((r0, c0), (r1, c1)) = visible;
        let middle = ((r0 + r1) / 2, (c0 + c1) / 2);
        // **Snapped, because the window is what decides whether to refill.**
        //
        // Centred exactly on the middle of the view, this moved whenever the
        // middle moved by one cell — which is every frame of a pan and every
        // frame of a zoom, since zooming about the pointer walks the centre
        // too. Each move rebuilds two megabytes and uploads them, so zooming
        // out cost a full texture a frame: about 120 MB/s at sixty frames,
        // and a walk of every cell in the world to build each one.
        //
        // A window a thousand cells wide does not need to be centred to a
        // cell. Snapping the centre to [`Self::STEP`] means a pan inside that
        // distance costs nothing at all, and the window still holds a wide
        // margin around the view on every side because it is far larger than
        // the step.
        let snap = |v: i32| v.div_euclid(Self::STEP) * Self::STEP;
        let middle = (snap(middle.0), snap(middle.1));
        ((middle.0 - side / 2, middle.1 - side / 2), (side, side))
    }

    /// Fill it, if what it holds is not already what is wanted.
    ///
    /// Skipped when the window and the generation both match, so a still world
    /// is uploaded once. Two megabytes at four generations a second is the
    /// worst case and only while zoomed out, which is the frame with nothing
    /// else to do.
    pub fn fill(&mut self, queue: &wgpu::Queue, world: &World, window: ((i32, i32), (i32, i32))) {
        if self.window == Some(window) && self.filled_at == world.generation {
            return;
        }
        self.window = Some(window);
        self.filled_at = world.generation;

        let ((row0, col0), (rows, cols)) = window;
        let mut texels = vec![0u8; (rows * cols) as usize * Self::STRIDE];
        // Walked by stored chunk rather than cell by cell: an infinite world
        // holds only what life has reached, so this is the resident set and
        // not the window. A torus holds everything, and then it is the world.
        // A torus of the largest shape holds sixteen thousand chunks and four
        // million cells, and all of them used to be walked whether or not the
        // window could show them. Skipped whole, before the inner loop, on the
        // cheap test — and never for a world that wraps, where a chunk outside
        // the window on one side is inside it on the other.
        let wraps = world.size_in_cells().is_some_and(|(h, w)| rows == h && cols == w);
        for (at, chunk) in world.stored() {
            let base = (at.0 * CHUNK_N as i32, at.1 * CHUNK_N as i32);
            if !wraps {
                let span = CHUNK_N as i32;
                let (r, c) = (base.0 - row0, base.1 - col0);
                if r + span <= 0 || c + span <= 0 || r >= rows || c >= cols {
                    continue;
                }
            }
            for cr in 0..CHUNK_N as i32 {
                for cc in 0..CHUNK_N as i32 {
                    let (row, col) = (base.0 + cr, base.1 + cc);
                    // Folded onto the window the same way the world folds, so
                    // a torus held whole needs no special case here either.
                    let (r, c) = (row - row0, col - col0);
                    let (r, c) = match world.size_in_cells() {
                        Some((h, w)) if rows == h && cols == w => {
                            (r.rem_euclid(h), c.rem_euclid(w))
                        }
                        _ => (r, c),
                    };
                    if r < 0 || c < 0 || r >= rows || c >= cols {
                        continue;
                    }
                    let cell = chunk[(cr as usize, cc as usize)];
                    let at = ((r * cols + c) as usize) * Self::STRIDE;
                    let bytes = bytemuck::bytes_of(&cell);
                    texels[at] = bytes[0];
                    texels[at + 1] = bytes[1];
                }
            }
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cols as u32 * Self::STRIDE as u32),
                rows_per_image: Some(rows as u32),
            },
            wgpu::Extent3d { width: cols as u32, height: rows as u32, depth_or_array_layers: 1 },
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
    /// Non-zero when the coarse window **is** the world, so the shader wraps
    /// it and a torus repeats for as far as anybody pans out of one texture
    /// and one quad. Zero for a window on a boundless world, which tiles
    /// nothing because there is nothing to tile.
    ///
    /// In what was the pad, because the struct has to be a multiple of sixteen
    /// bytes and this is a flag.
    pub coarse_wraps: f32,
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
    /// The world rect the coarse texture holds: top-left row and column, then
    /// how many rows and columns. Cells throughout.
    ///
    /// The shader needs it to turn a world position into a coarse texel, and
    /// needs the size to wrap — which is what makes a torus held whole repeat
    /// for as far as anybody pans, out of one texture and one quad.
    pub coarse: [f32; 4],
    /// **How many samples across one screen pixel the world is being drawn at.**
    ///
    /// The shader needs it because two different questions look like the same
    /// question once the world is drawn larger than the screen. Which level of
    /// detail to sample is about the *sample rate*, so it wants the zoom this
    /// pass is actually running at — `zoom` above, already multiplied. Whether
    /// the art has given out is about what the **screen** is showing, because
    /// it has to meet `COARSE_BELOW`, which is decided in screen pixels on the
    /// CPU. Dividing by this is how the shader gets back to that.
    ///
    /// Supersampling broke exactly that: every zoom-keyed threshold in the
    /// shader silently moved by this factor while the ones in `chunks.rs` did
    /// not, so the coarse path took over while the fine path still had full
    /// art on it and the handover popped again.
    pub over: f32,
    /// The struct has to be a multiple of sixteen bytes and `over` is one
    /// float. Nothing reads this.
    pub pad: [f32; 3],
}

const _: () = {
    // Must match `Camera` and the instance attributes in shaders/grid.wgsl.
    // WGSL requires a uniform struct's size to be a multiple of 16.
    assert!(size_of::<CameraUniform>() == 128);
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
            // The world as one texel a cell, for low zoom. `Uint` and
            // fetched, like the chunk texture and for the same reason: these
            // are bit fields, and a blended kind is not a kind.
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
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
    coarse: CoarseTexture,
    /// Whether the last frame drew coarsely, so the swap has hysteresis and a
    /// zoom resting on the threshold does not flicker between two paths that
    /// do not look alike.
    was_coarse: bool,
    /// Canonical chunk coordinate -> array layer. Canonical, so a torus chunk
    /// drawn at nine global positions still occupies one layer.
    layers: HashMap<Coord, u32>,
    next_free: u32,
    free: Vec<u32>,
    instances: Vec<Instance>,
    buffer: wgpu::Buffer,
    /// What the last "does not fit" line said, so an unchanged view says it
    /// once rather than four times a second.
    told: Option<(usize, usize)>,
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
            coarse: CoarseTexture::new(device),
            was_coarse: false,
            layers: HashMap::new(),
            // Layer zero is reserved, so allocation starts above it.
            next_free: UNLOADED_LAYER + 1,
            free: Vec::new(),
            instances: Vec::with_capacity(MAX_INSTANCES),
            buffer,
            told: None,
        }
    }

    /// Say once, and again only when the numbers change, that a frame did not
    /// fit.
    ///
    /// **Not a failure and not rare.** One layer per chunk against a
    /// guaranteed floor of 256 of them means a screen wider than sixteen
    /// chunks cannot be fully resident, whatever else is true — see
    /// [`covered`]. What is missing is drawn as backdrop, which is empty
    /// ground, so the picture is wrong rather than broken and the log is how
    /// anybody finds out.
    fn report_budget(&mut self, chunks: usize, positions: usize, short_by: usize) {
        let over = positions.saturating_sub(MAX_INSTANCES);
        if short_by == 0 && over == 0 {
            self.told = None;
            return;
        }
        if self.told == Some((short_by, over)) {
            return;
        }
        self.told = Some((short_by, over));
        log::warn!(
            "the view does not fit: {positions} chunk positions over {chunks} chunks, \
             {short_by} without a layer ({} of {}) and {over} without a quad ({MAX_INSTANCES}). \
             The rest is drawn as empty ground.",
            self.layers.len(),
            self.texture.layers,
        );
    }

    /// Zero layer zero once. It stays dead for the life of the app.
    pub fn init_unloaded_layer(&self, queue: &wgpu::Queue) {
        self.texture.upload(queue, UNLOADED_LAYER, &[0u8; CHUNK_CELLS * ChunkTexture::STRIDE]);
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.texture.view
    }

    pub fn coarse_view(&self) -> &wgpu::TextureView {
        &self.coarse.view
    }

    /// Whether the world is being drawn coarsely, and the window it is drawn
    /// from — which the camera uniform has to carry so the shader can map a
    /// world position onto a texel.
    pub fn coarse_window(&self) -> Option<((i32, i32), (i32, i32))> {
        self.was_coarse.then(|| self.coarse.window).flatten()
    }

    /// Whether that window is the whole world, and so wraps.
    pub fn coarse_wraps(&self, world: &World) -> bool {
        match (self.coarse.window, world.size_in_cells()) {
            (Some((_, (rows, cols))), Some((h, w))) => rows == h && cols == w,
            _ => false,
        }
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
    pub fn sync(
        &mut self,
        queue: &wgpu::Queue,
        world: &World,
        visible: ((i32, i32), (i32, i32)),
        zoom: f32,
    ) {
        // **Which path, with hysteresis.** Two thresholds rather than one,
        // because the two paths do not draw alike -- one has sprites and
        // outlines and the other has flat colour -- so a zoom resting on a
        // single boundary would swap them back and forth every frame.
        self.was_coarse = if self.was_coarse { zoom < FINE_ABOVE } else { zoom < COARSE_BELOW };

        if self.was_coarse {
            // One quad for the whole world, out of one texture. **This is what
            // low zoom collapses to instead of the backdrop**: the fine path
            // cannot be resident at these zooms -- a 1080p screen wants
            // thousands of array layers under about zoom five and there are
            // 256 -- so what used to happen was that most of the screen fell
            // back to empty ground.
            let window = CoarseTexture::window_for(world, visible);
            self.coarse.fill(queue, world, window);
            // Every layer goes back: the fine path holds none of them while
            // this is what is drawn, and holding them would make zooming out
            // and back in the thing that exhausts the budget.
            self.free.extend(self.layers.values().copied());
            self.layers.clear();
            self.instances.clear();
            let ((min_row, min_col), (max_row, max_col)) = visible;
            self.instances.push(Instance {
                rect: [
                    min_col as f32,
                    min_row as f32,
                    (max_col - min_col + 1) as f32,
                    (max_row - min_row + 1) as f32,
                ],
                meta: [UNLOADED_LAYER, KIND_COARSE, 0, 0],
            });
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.instances));
            return;
        }

        // Only what is on screen gets a layer. Uploading every stored chunk
        // made the budget a limit on the size of the *world*, which an
        // infinite world hides -- it holds only what life has reached -- and a
        // torus does not: a 20x20 torus stores four hundred chunks whether you
        // are looking at them or not, and asked for a layer for each.
        let Covered { positions, chunks: wanted } = covered(world, visible);

        let free = &mut self.free;
        self.layers.retain(|coord, layer| {
            let keep = wanted.contains(coord);
            if !keep {
                free.push(*layer);
            }
            keep
        });

        // Counted rather than logged per chunk. Running out is the ordinary
        // state below about zoom five on a large screen -- see [`covered`] --
        // so a line per chunk per frame is thousands a second, and the number
        // that matters is how many were missed rather than which.
        let mut short_by = 0usize;
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
                        short_by += 1;
                        continue;
                    };
                    self.layers.insert(coord, l);
                    l
                }
            };
            self.texture.upload(queue, layer, &texels(world, coord, chunk));
        }

        self.report_budget(wanted.len(), positions.len(), short_by);
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
        for global in positions {
            let Some(&layer) = self.layers.get(&world.canonical(global)) else {
                continue;
            };
            if self.instances.len() == MAX_INSTANCES {
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

    /// **The shader compiles**, which nothing else here checked.
    ///
    /// `cargo check` only ever sees a string: WGSL is parsed and validated
    /// when the pipeline is built, so a typo or a type error in it is a panic
    /// on the first frame behind a green test run — right up until somebody
    /// opens the game. The tests that would have caught it need a browser and
    /// a GPU.
    ///
    /// Validated and not merely parsed, because parsing catches a missing
    /// bracket and validation catches the mistakes anybody actually makes:
    /// assigning to a `let`, a `vec3` where a `vec4` goes, a function called
    /// with the wrong arity.
    /// The resolve pass compiles too — it is the last thing that touches the
    /// world and nothing else would notice it failing until a frame was drawn.
    #[test]
    fn the_resolve_shader_compiles() {
        let source = include_str!("shaders/resolve.wgsl");
        let module = match wgpu::naga::front::wgsl::parse_str(source) {
            Ok(m) => m,
            Err(e) => panic!("resolve.wgsl does not parse: {}", e.emit_to_string(source)),
        };
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&module) {
            panic!("resolve.wgsl does not validate: {}", e.emit_to_string(source));
        }
    }

    #[test]
    fn the_shader_compiles() {
        let module = match wgpu::naga::front::wgsl::parse_str(SHADER_SOURCE) {
            Ok(m) => m,
            Err(e) => panic!("grid.wgsl does not parse: {}", e.emit_to_string(SHADER_SOURCE)),
        };
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&module) {
            panic!("grid.wgsl does not validate: {}", e.emit_to_string(SHADER_SOURCE));
        }
    }

    use super::*;
    use crate::sim::PlayerId;

    fn alive(world: &mut World, cells: &[(i32, i32)], who: u8) {
        for &(r, c) in cells {
            world.set_cell_at(r, c, Cell::alive(PlayerId(who)));
        }
    }

    /// **The camera uniform's fields are in the same order as the shader's.**
    ///
    /// Which the size assert beside the struct cannot tell you, and did not:
    /// `coarse` and `hues` were the wrong way round for a while and both
    /// orders are 112 bytes, so nothing complained. What the shader read as
    /// the hue table was the coarse rect, so player one's hue came out as the
    /// window's row — nought, which is red — and the whole board went pink.
    ///
    /// Parsed out of the WGSL rather than written down twice, because a copy
    /// of the layout is a third thing to keep in step. Offsets are computed by
    /// WGSL's own uniform rules: `f32` aligns to 4, `vec2` to 8, `vec4` and an
    /// array of them to 16.
    #[test]
    fn the_camera_uniform_matches_the_shader() {
        let body = SHADER_SOURCE
            .split_once("struct Camera {")
            .expect("no Camera struct in the shader")
            .1
            .split_once("};")
            .expect("unterminated Camera struct")
            .0;

        let mut at = 0usize;
        let mut seen: Vec<(String, usize)> = Vec::new();
        for line in body.lines() {
            let line = line.split("//").next().unwrap_or("").trim().trim_end_matches(',');
            let Some((name, kind)) = line.split_once(':') else { continue };
            let (name, kind) = (name.trim(), kind.trim());
            if name.is_empty() || kind.is_empty() {
                continue;
            }
            let (align, size) = match kind {
                "f32" => (4, 4),
                "vec2<f32>" => (8, 8),
                "vec4<f32>" => (16, 16),
                // Aligns to sixteen and occupies twelve, which is the one WGSL
                // type whose size and alignment differ — and the reason the
                // Rust side spells its padding out rather than trusting them
                // to agree.
                "vec3<f32>" => (16, 12),
                k if k.starts_with("array<vec4<f32>,") => {
                    let n: usize = k
                        .trim_start_matches("array<vec4<f32>,")
                        .trim_end_matches('>')
                        .trim()
                        .parse()
                        .expect("array length");
                    (16, 16 * n)
                }
                other => panic!("the test does not know how to size {other:?}"),
            };
            at = at.next_multiple_of(align);
            seen.push((name.to_string(), at));
            at += size;
        }

        // The shader's names, in its order, against the Rust field each one
        // is. Two differ by name and the pairing is what says so out loud.
        let rust = [
            ("origin", std::mem::offset_of!(CameraUniform, origin)),
            ("viewport", std::mem::offset_of!(CameraUniform, viewport)),
            ("zoom", std::mem::offset_of!(CameraUniform, zoom)),
            ("chunk_n", std::mem::offset_of!(CameraUniform, chunk_n)),
            ("encode", std::mem::offset_of!(CameraUniform, encode_srgb)),
            ("wraps", std::mem::offset_of!(CameraUniform, coarse_wraps)),
            ("hues", std::mem::offset_of!(CameraUniform, hues)),
            ("coarse", std::mem::offset_of!(CameraUniform, coarse)),
            ("over", std::mem::offset_of!(CameraUniform, over)),
            // `pad` is Rust's alone: WGSL rounds the struct up to a multiple of
            // sixteen without being told, and a pad field written there would
            // align to sixteen and make it larger rather than the same size.
        ];

        assert_eq!(
            seen.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            rust.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            "the shader's fields are not the ones this expects, in that order"
        );
        for ((name, wgsl), (_, ours)) in seen.iter().zip(rust) {
            assert_eq!(*wgsl, ours, "{name} is at {ours} in Rust and {wgsl} in the shader");
        }
        // And the whole thing is the size the struct says it is.
        assert_eq!(at.next_multiple_of(16), size_of::<CameraUniform>());
    }

    /// **The world collapses into itself, not into the backdrop.**
    ///
    /// Which is the whole point of the coarse path. A torus small enough to
    /// hold whole is held whole, so there is no window to recentre and no seam
    /// to get wrong, and the shader wraps it — one texture, one quad, and the
    /// world repeats for as far as anybody pans.
    #[test]
    fn a_torus_that_fits_is_the_coarse_window() {
        // Three chunks a side, which is small enough to be held whole at any
        // chunk size worth having — written that way rather than as a number
        // of cells, which is what went stale when a chunk grew.
        let world = World::toroidal_empty(3, 3);
        let side = 3 * CHUNK_N as i32;
        let (at, size) = CoarseTexture::window_for(&world, ((-500, -500), (500, 500)));
        assert_eq!(at, (0, 0), "a world held whole starts where the world does");
        assert_eq!(size, (side, side), "and is exactly the world");

        // Anything larger gets a window on the view instead, because it cannot
        // be held whole — 1024 cells is 64 chunks and that is the largest a
        // client may ask for, so this is the case that does not arise from the
        // menu and does from a command line.
        // Larger than the coarse texture can hold whole, which is what puts a
        // window on the view instead. `SIDE / CHUNK_N` chunks is exactly the
        // texture, so one more than that is the first world that cannot fit.
        let too_wide = (CoarseTexture::SIDE as i32 / CHUNK_N as i32) + 1;
        let big = World::toroidal_empty(too_wide, too_wide);
        let (at, size) = CoarseTexture::window_for(&big, ((0, 0), (99, 99)));
        assert_eq!(size, (CoarseTexture::SIDE as i32, CoarseTexture::SIDE as i32));
        assert_ne!(at, (0, 0), "a window is centred on what is being looked at");
    }

    /// A boundless world has no size, so it is always a window — and what the
    /// window does not cover reads as dead, which is what the backdrop is.
    #[test]
    fn a_boundless_world_gets_a_window_on_the_view() {
        let world = World::infinite_empty();
        let visible = ((100, 200), (300, 400));
        let ((row, col), size) = CoarseTexture::window_for(&world, visible);
        assert_eq!(size, (CoarseTexture::SIDE as i32, CoarseTexture::SIDE as i32));
        // On the middle of what is on screen, to within the step it snaps to
        // — see `window_for`, where the snapping is what stops a pan of one
        // cell rebuilding two megabytes.
        let side = CoarseTexture::SIDE as i32;
        let centre = (row + side / 2, col + side / 2);
        assert!(
            (centre.0 - 200).abs() < CoarseTexture::STEP
                && (centre.1 - 300).abs() < CoarseTexture::STEP,
            "{centre:?} is not on (200, 300)"
        );
    }

    /// **The window is what decides whether to refill**, and each refill
    /// rebuilds two megabytes and uploads them — so what has to be true is
    /// that it moves rarely, not that it never moves.
    ///
    /// Unsnapped it followed the view's middle exactly, so it changed on every
    /// frame of a pan and every frame of a zoom, which is a full texture a
    /// frame. Snapped it can change only when the middle crosses a step, so a
    /// pan of a thousand cells costs at most a refill every `STEP` of it.
    #[test]
    fn the_coarse_window_moves_at_most_once_a_step() {
        let world = World::infinite();
        let view = |r: i32, c: i32| ((r, c), (r + 200, c + 200));
        let travel = CoarseTexture::STEP * 10;
        let mut last = CoarseTexture::window_for(&world, view(0, 0));
        let mut moves = 0;
        for step in 1..=travel {
            let now = CoarseTexture::window_for(&world, view(step, 0));
            if now != last {
                moves += 1;
                last = now;
            }
        }
        assert_eq!(moves, 10, "{travel} cells of pan cost {moves} refills");
    }

    /// And it does follow, eventually — a window that never moved would leave
    /// the view behind.
    #[test]
    fn the_coarse_window_follows_a_long_pan() {
        let world = World::infinite();
        let view = |r: i32, c: i32| ((r, c), (r + 200, c + 200));
        let first = CoarseTexture::window_for(&world, view(0, 0));
        let far = CoarseTexture::window_for(&world, view(CoarseTexture::STEP * 4, 0));
        assert_ne!(far, first, "the window never followed the view");
        // And it still surrounds what is being looked at, with room to spare.
        let ((row0, col0), (rows, cols)) = far;
        let ((r0, c0), (r1, c1)) = view(CoarseTexture::STEP * 4, 0);
        assert!(r0 > row0 && r1 < row0 + rows, "the view is outside the window's rows");
        assert!(c0 > col0 && c1 < col0 + cols, "the view is outside the window's columns");
    }

    /// **The swap has hysteresis**, or a zoom resting on the threshold flips
    /// between two paths that do not draw alike — and it no longer needs to
    /// carry the whole burden, because by the time either threshold is reached
    /// the fine path has already faded into what the coarse one draws.
    #[test]
    fn the_two_paths_do_not_flicker_at_the_boundary() {
        assert!(COARSE_BELOW < FINE_ABOVE, "one threshold is not hysteresis");
        // The shader has finished fading the art out by the time either side
        // of that hysteresis is crossed, so neither crossing is a visible one.
        assert!(
            FLAT_BY_IN_SHADER <= COARSE_BELOW,
            "the swap happens while the fine path still has art on it"
        );
        // Coming down: fine until below COARSE_BELOW. Written against the two
        // thresholds rather than as a list of numbers, which is what went
        // stale when they moved with the chunk size.
        let mut coarse = false;
        for zoom in [FINE_ABOVE * 4.0, FINE_ABOVE, COARSE_BELOW, COARSE_BELOW * 0.9] {
            coarse = if coarse { zoom < FINE_ABOVE } else { zoom < COARSE_BELOW };
        }
        assert!(coarse, "never became coarse on the way down");
        // And going back up: coarse until above FINE_ABOVE, so the band
        // between them holds whichever it already was.
        for zoom in [(COARSE_BELOW + FINE_ABOVE) / 2.0, FINE_ABOVE * 0.99] {
            coarse = if coarse { zoom < FINE_ABOVE } else { zoom < COARSE_BELOW };
            assert!(coarse, "swapped back inside the band");
        }
        coarse = if coarse { 5.1 < FINE_ABOVE } else { 5.1 < COARSE_BELOW };
        assert!(!coarse, "never came back to the fine path");
    }

    /// **A torus is drawn by folding.** A viewport wider than the world covers
    /// the same chunks over and over, and each of them is one resident with
    /// several quads — so the texture cost is bounded by the world while the
    /// quad cost is bounded by the screen.
    #[test]
    fn a_wrapping_world_repeats_without_repeating_its_textures() {
        // Every chunk stored and non-empty, so residency is about folding
        // rather than about what the world happens to hold.
        let mut world = World::toroidal_empty(4, 4);
        for r in 0..4 * CHUNK_N as i32 {
            for c in 0..4 * CHUNK_N as i32 {
                alive(&mut world, &[(r, c)], 1);
            }
        }

        // Three worlds across and three down, starting off the origin so the
        // seam falls inside the view rather than on its edge.
        let n = CHUNK_N as i32;
        let wide = covered(&world, ((-2 * n, -2 * n), (10 * n - 1, 10 * n - 1)));

        assert_eq!(wide.positions.len(), 12 * 12, "one quad per position on screen");
        assert_eq!(wide.chunks.len(), 4 * 4, "and one layer per chunk the world actually has");
        assert!(
            wide.chunks.iter().all(|&(r, c)| (0..4).contains(&r) && (0..4).contains(&c)),
            "every resident should be a folded coordinate"
        );

        // Panning does not grow it. This is the property the whole thing is
        // for: how far somebody has walked stops being a cost.
        let far = covered(&world, ((1000 * n, 1000 * n), (1012 * n - 1, 1012 * n - 1)));
        assert_eq!(far.chunks, wide.chunks, "panning a thousand worlds along found new chunks");
    }

    /// A plane folds nothing, so the two numbers are the same and residency is
    /// bounded by the screen alone.
    #[test]
    fn a_boundless_world_has_one_layer_per_position_it_holds() {
        let mut world = World::infinite_empty();
        let n = CHUNK_N as i32;
        for c in 0..3 {
            alive(&mut world, &[(0, c * n)], 1);
        }
        let seen = covered(&world, ((0, 0), (n - 1, 5 * n - 1)));
        assert_eq!(seen.positions.len(), 5, "five positions across");
        assert_eq!(seen.chunks.len(), 3, "and only the three the world holds");
    }

    /// **Where low zoom stops working, in numbers.** One layer per chunk
    /// against a guaranteed floor of 256 means a view wider than sixteen
    /// chunks each way cannot be fully resident — which on an ordinary screen
    /// is reached well inside the zoom range, long before sampling is what is
    /// wrong. Pinned here because it is the argument for a coarse level of
    /// detail rather than a better sampler.
    #[test]
    fn a_screen_at_low_zoom_wants_more_layers_than_there_are() {
        let n = CHUNK_N as f32;
        let chunks_on_screen =
            |w: f32, h: f32, zoom: f32| ((w / zoom / n).ceil() * (h / zoom / n).ceil()) as usize;
        let (w, h) = (1920.0, 1080.0);

        assert!(
            chunks_on_screen(w, h, 16.0) < ChunkTexture::LAYER_BUDGET as usize,
            "the zoom the client opens at should fit"
        );
        // **And there is a zoom that does not fit**, which is the whole reason
        // the coarse path exists. Where that zoom *is* moved a long way when a
        // chunk went from sixteen cells a side to sixty-four: a chunk now
        // covers sixteen times the screen, so the budget lasts sixteen times
        // longer and the fine path reaches down to about one and a half pixels
        // a cell instead of five. `COARSE_BELOW` followed it down, which is
        // most of what that change bought.
        assert!(
            chunks_on_screen(w, h, COARSE_BELOW) <= ChunkTexture::LAYER_BUDGET as usize,
            "the fine path is asked for more layers than there are at its own floor"
        );
        assert!(
            chunks_on_screen(w, h, COARSE_BELOW / 4.0) > ChunkTexture::LAYER_BUDGET as usize,
            "nothing on this screen ever runs out of layers, so the coarse path is dead code"
        );
        // And at the very bottom of the range it is out of both, which is what
        // the coarse path is a backstop for. Written against the budget rather
        // than as a number of chunks: the number depends on `CHUNK_N`, and the
        // fact does not.
        let floor = crate::client::views::game::camera::ZOOM_RANGE.0;
        assert!(
            chunks_on_screen(w, h, floor) > ChunkTexture::LAYER_BUDGET as usize,
            "at the zoom floor a 1080p screen fits in {} layers, so nothing needs the coarse path",
            ChunkTexture::LAYER_BUDGET,
        );
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
