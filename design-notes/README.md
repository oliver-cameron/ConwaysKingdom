# ConwaysKingdom — design notes

Working notes for the renderer and world-storage design. Nothing here is in the repo yet; it is a staging area for decisions that have been checked against wgpu 30 and, where possible, compiled and run.

Pinned versions: `wgpu 30.0.0`, `winit 0.29.15`, `bytemuck 1.25.2`, edition 2024.

| Note | Subject |
|---|---|
| [01-cell-layout.md](01-cell-layout.md) | The cell and chunk types; one allocation readable as both a Rust grid and a texture |
| [02-texture-residency.md](02-texture-residency.md) | Array textures, the layer cache, and deriving the zoom floor |
| [03-world-topology.md](03-world-topology.md) | The F-pentomino partition and the adjacency graph |
| [04-rendering.md](04-rendering.md) | One pipeline, one bind group, instanced draw; the minimap |
| [05-compute-feasibility.md](05-compute-feasibility.md) | Whether the simulation can move to a compute shader |
| [06-open-bugs.md](06-open-bugs.md) | Defects in the current `src/` that block any of this |

## The one-paragraph version

Cells are `#[repr(C)]` byte structs, so a chunk is a flat byte array that `Queue::write_texture` consumes with no conversion. Chunks live in a `texture_2d_array` whose layers are a cache of what is currently visible, sized so the layer budget never binds before the anti-aliasing zoom floor does. The world is an ordinary square lattice; the F-pentomino tiling is a partition drawn over it, resolved by a ten-entry lookup table rather than a graph walk. Rendering is one pipeline, one bind group, one instanced draw call.

## Status

Verified by compiling and running: the cell/chunk layout, the nibble and two-byte packing, the byte aliasing, the pentomino tiling and its lookup table, and the `repr`/padding failure modes.

Verified by reading wgpu 30 sources: `write_texture` row-alignment exemption, storage-capable formats, array layer limits, downlevel WebGL2 limits, bind group and texel-copy struct shapes.

Not yet built: anything in the repo. `src/` still does not compile — see [06-open-bugs.md](06-open-bugs.md).
