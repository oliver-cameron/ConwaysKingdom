# Can the simulation move to a compute shader?

Short answer: on native and on WebGPU, yes. On WebGL2, not at all. And not with the texture format currently chosen.

## The blocking fact

Not every texture format can be bound as a storage texture, and a compute shader can only *write* to a storage texture. From `guaranteed_format_features` in wgpu-types 30 (`src/texture/format.rs`), where `attachment` does not include `STORAGE_BINDING` and `s_ro_wo` is `STORAGE_READ_ONLY | STORAGE_WRITE_ONLY`:

```
R8Uint    => (msaa,           attachment)      <- no storage
Rg8Uint   => (msaa,           attachment)      <- no storage
Rgba8Uint => (msaa | s_ro_wo, all_flags)       <- storage, read-only or write-only
R32Uint   => (s_all,          atomic)          <- storage incl. read-write, plus atomics
```

So **`Rg8Uint` cannot be a compute shader's output.** Choosing compute means choosing a different format, and the natural one is `Rgba8Uint` — four bytes, storage-capable, and exactly the four independent `u8` fields the format ladder in [01-cell-layout.md](01-cell-layout.md) already anticipates. No repacking, no bit math; `kind`, `player`, `age`, `flags` become R, G, B, A.

`Rgba8Uint` grants `STORAGE_READ_ONLY | STORAGE_WRITE_ONLY` but not `STORAGE_READ_WRITE`. That is fine for Conway, which needs ping-pong anyway: bind generation A read-only, generation B write-only, swap each tick.

`R32Uint` is the other candidate. It is fully read-write and supports atomics, which matters if kingdoms ever contest a cell and you want a well-defined winner without a second pass. Cost is manual packing, since one `u32` is either one fat cell or four thin ones.

## The WebGL2 wall

`Limits::downlevel_webgl2_defaults()` zeroes every relevant limit:

```
max_storage_buffers_per_shader_stage:   0
max_storage_textures_per_shader_stage:  0
max_compute_invocations_per_workgroup:  0
max_compute_workgroup_size_x/y/z:       0
max_compute_workgroups_per_dimension:   0
```

There is no partial credit here — compute is absent, not slow. Since `gpu.rs` currently requests `downlevel_webgl2_defaults()` for every wasm build, it also zeroes compute even when the adapter behind it is WebGPU. Enabling compute on the web means branching on what the adapter actually reports rather than assuming the floor.

This is the real decision. Compute on the GPU and a WebGL2 fallback are two different simulations that have to agree cell for cell, and keeping them in step is a permanent tax. Either drop WebGL2 and require WebGPU, or keep the simulation on the CPU.

## What it would cost architecturally

Moving the generation step to the GPU inverts who owns the truth. Today the CPU holds the cells and pushes them to VRAM; afterwards VRAM holds them and the CPU must pull back — via `copy_texture_to_buffer` plus `map_async`, which is asynchronous and lands a frame or more later. Anything reading cell state for game logic (scoring, territory counts, win conditions, AI, validating a player action) inherits that latency, and "what is the board right now" stops being a question the CPU can answer synchronously.

For a Conway variant with kingdoms and player actions, that is not a drop-in optimisation; it is a change to where the game lives.

## Is it warranted yet?

A byte-per-cell CPU Conway does roughly 20-50M cell-updates per second single-threaded, more with rayon across chunks. At ten generations per second that covers a few million live cells before it strains — and the zoom floor already caps what is *visible* far below that. Compute becomes interesting when the simulated world is much larger than the rendered one, or when tick rates climb.

The honest reading: not yet. The CPU path is simpler, keeps the CPU authoritative, and works on every backend including WebGL2. Revisit when there is a profile showing the generation step is actually the bottleneck.

## If and when it is done

Keep the door open cheaply by moving to `Rgba8Uint` now rather than later — it is a one-line format change plus a fourth field on the cell struct, and it is the difference between "swap a constant" and "rewrite the storage layer" when the day comes.

The shape it would take:

- Two `Rgba8Uint` array textures, ping-ponged, each with `STORAGE_BINDING | TEXTURE_BINDING | COPY_DST`.
- A storage buffer holding, per layer, the layer indices of its eight neighbours — this is how a workgroup on a chunk border reads across into an adjacent chunk. With the pentomino partition being a plain square lattice ([03-world-topology.md](03-world-topology.md)) that table is trivial to build.
- Workgroup size 8x8 or 16x16; `dispatch_workgroups(N/16, N/16, resident_layers)` handles every chunk in one dispatch, with the z dimension indexing the layer.
- Interior invocations read the eight neighbours directly; only invocations on a chunk edge consult the neighbour table. At 256x256 chunks that is 1.6% of them.
- Readback only of aggregates — per-chunk live counts and dominant owner for the minimap — rather than whole boards, to keep the `map_async` cost bounded.

## Where compute would help regardless

If a mip chain for zoomed-out views is ever needed, it must be built with a max-or-count reduction rather than an average, since automatic mipmapping averages and does not apply to `Uint` formats at all. That is a natural compute job and it does not invert data ownership, because it only reads what the renderer already has. Capping the zoom floor at one pixel per cell avoids needing it — see [02-texture-residency.md](02-texture-residency.md) — so this is contingent on that decision being revisited.
