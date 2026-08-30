# Cell and chunk memory layout

The goal is one allocation that the CPU indexes as a grid of cells and the GPU consumes as a texture, with no conversion step between them. This is achievable, and the whole thing turns on a single decision: **the cell must be a `#[repr(C)]` struct of `u8` fields, never an enum.**

`bytemuck::Pod` requires every bit pattern of a type to be a valid value. A two-variant enum has 254 invalid ones, so it can never be soundly reinterpreted as bytes. A byte struct can.

## The types

```rust
// Two bytes per cell -> Rg8Uint. Byte 0 is kind, byte 1 is player, on every target.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct Cell { pub kind: u8, pub player: u8 }

pub const N: usize = 16;                 // cells per chunk edge — the one knob
pub const CELLS: usize = N * N;

#[repr(transparent)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Chunk { cells: [Cell; CELLS] }

impl Chunk {
    pub fn zeroed() -> Box<Self> { bytemuck::zeroed_box() }
    pub fn as_bytes(&self) -> &[u8] { bytemuck::bytes_of(self) }
    pub fn as_bytes_mut(&mut self) -> &mut [u8] { bytemuck::bytes_of_mut(self) }
    pub const fn bytes_per_row() -> u32 { N as u32 * size_of::<Cell>() as u32 }
    pub fn row(&self, r: usize) -> &[Cell] { &self.cells[r * N..(r + 1) * N] }
}

// (row, col) == (texture Y, texture X). Stated once; the only way in.
impl Index<(usize, usize)> for Chunk {
    type Output = Cell;
    #[inline] fn index(&self, (r, c): (usize, usize)) -> &Cell { &self.cells[r * N + c] }
}
impl IndexMut<(usize, usize)> for Chunk {
    #[inline] fn index_mut(&mut self, (r, c): (usize, usize)) -> &mut Cell { &mut self.cells[r * N + c] }
}

const _: () = {
    assert!(size_of::<Cell>() == 2 && align_of::<Cell>() == 1);
    assert!(size_of::<Chunk>() == CELLS * 2);      // no padding, anywhere
};
```

The upload has no glue at all:

```rust
queue.write_texture(
    wgpu::TexelCopyTextureInfo { texture: &array, mip_level: 0,
        origin: wgpu::Origin3d { x: ox, y: oy, z: layer }, aspect: wgpu::TextureAspect::All },
    chunk.as_bytes(),                                  // <- the entire conversion
    wgpu::TexelCopyBufferLayout { offset: 0,
        bytes_per_row: Some(Chunk::bytes_per_row()),
        rows_per_image: Some(N as u32) },
    wgpu::Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
);
```

and the shader reads the two fields with no bit math:

```wgsl
let t      = textureLoad(blocks, coord, layer, 0).rg;   // vec2<u32>
let kind   = t.x;
let player = t.y;
if kind == 0u { return DEAD; }
```

## Why `repr(C)` and not a `u16` newtype

`#[repr(u16)]` is enum-only — on a struct it is `error[E0517]: attribute should be applied to an enum`.

The alternative is `#[repr(transparent)] struct Cell(u16)` with bit-packed accessors, and it carries an endianness hazard. A `u16`'s byte order in memory is the host's, but `Rg8Uint` reads byte 0 as R and byte 1 as G unconditionally, so the bit fields and the shader channels disagree. Measured on x86-64:

```
CellU16::new(kind=0x123, player=0x4) -> u16 0x4123 -> bytes [23, 41]
  as Rg8Uint the shader sees R=0x23 G=0x41, which is not the nibble split
```

`repr(C)` fixes field order to declaration order on every target, so there is nothing to get wrong. It also gives alignment 1 rather than 2, which matches the texture's model more closely.

## Verified properties

Each of these was compiled and asserted, at both `N = 16` and `N = 256`, with nothing changed but the const.

**Same allocation, two views.** `bytes_of(&chunk)` returns a reference at the identical address, not a copy — asserted by pointer equality. Note this is zero *conversion*, not zero *transfer*: `write_texture` still copies RAM to VRAM, but byte-for-byte with no transformation.

**Row-major order matches the texture.** `chunk[(0,1)]` is byte `size_of::<Cell>()`, `chunk[(1,0)]` is byte `N * size_of::<Cell>()` — exactly the order `write_texture` consumes with `bytes_per_row`. The current `src/cell.rs` calls the first index `x` and treats North as `x-1`; keeping that naming transposes the world. Rename to row/col.

**Zeroed memory is a valid empty world.** With `kind == 0` meaning dead, `bytemuck::zeroed_box()` hands back a legitimate empty chunk with no initialisation loop. Do not later give kind 0 a live meaning; free construction depends on it.

**Double buffering is a pointer swap.** `mem::swap` on two `Box<Chunk>` moves pointers, asserted by comparing addresses across the swap. The current `apply_generation` copies cell by cell instead.

**Rows are contiguous slices.** `chunk.row(r)` is a real `&[Cell]`, so the interior simulation loop can work on slices.

## Two traps

**bytemuck needs feature flags you do not currently have.** Without `min_const_generics`, `Pod` is only implemented for a fixed list of array lengths. `[Cell; 256]` happens to be on it and `[Cell; 65536]` is not, so this compiles at `N = 16` and fails with `the trait bound [Cell; 65536]: Pod is not satisfied` the day you scale up. `zeroed_box` needs `extern_crate_alloc`.

```toml
bytemuck = { version = "1.16", features = ["derive", "extern_crate_alloc", "min_const_generics"] }
```

**A `Vec<Chunk>` is contiguous but chunk-major, not layer-row-major.** Row 0 of chunk 1 does not follow row 0 of chunk 0 in memory, so a grid of chunks cannot be blitted into a texture layer with a single `write_texture`. Upload each chunk as its own sub-rect. This costs nothing in practice because only dirty chunks are ever uploaded, and it is legal at 16 or 32 bytes per row precisely because `Queue::write_texture` is exempt from the 256-byte alignment that `copy_buffer_to_texture` imposes — confirmed in the wgpu 30 docs on `TexelCopyBufferLayout::bytes_per_row`.

## The format ladder

Field widths grow independently, which is the actual defence against a Minecraft-style metadata ceiling — the problem there was not four bits as such, it was ID and metadata welded into one encoding that could not widen.

| Format | Bytes | Independent `u8` fields | Compute-writable |
|---|---|---|---|
| `R8Uint` | 1 | kind | no |
| `Rg8Uint` | 2 | kind, player | no |
| `Rgba8Uint` | 4 | kind, player, age, flags | yes |

Each step is the same `repr(C)` struct with another field plus a format constant; `bytes_per_row` recomputes from `size_of` so the upload path does not move. The compute column is the finding in [05-compute-feasibility.md](05-compute-feasibility.md) and it is the one reason to consider skipping straight to four bytes.

**bytemuck refuses to derive `Pod` on a type with padding** — `error[E0080]: derive(Pod) was applied to a type with padding`. So adding a misaligned field later fails loudly at compile time rather than silently uploading uninitialised bytes.
