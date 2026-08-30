# Defects in the current `src/`

As of the `init` commit (37bb285), the crate does not compile and the cell logic has never executed. All of the following were reproduced, not inferred.

## Blocking the build

**`src/gpu.rs:142` — `let view = texture.create_view;`** takes the value of a method instead of calling it. `error[E0615]: attempted to take value of method 'create_view' on type 'wgpu::Texture'`. This is the only hard error on the native target; with it patched the crate compiles clean with 7 warnings. `SizedTexture` is dead code (the compiler says so), so the thing blocking the build is a half-written struct nobody calls.

Fix: `texture.create_view(&wgpu::TextureViewDescriptor::default())`. The const generic parameters should also go — world size is a runtime decision. Its usage flags `TEXTURE_BINDING | COPY_DST` are already correct.

## Blocking the wasm target

**`src/lib.rs:24` — `run::<TriangleApp>()`** names a type that does not exist anywhere in the crate; grep finds that single reference. The type was renamed to `BattleApp` and `wasm_main` was not updated.

**`index.html:27` — `import init from "./pkg/game.js"`** but the crate is `conwayskingdom`, so wasm-bindgen emits `pkg/conwayskingdom.js` and the import 404s.

Neither shows up in a native `cargo build`, which is why both survive.

## The cell logic panics on any real board

`count_alive_neighbors` (`src/cell.rs:79`) resolves cross-chunk lookups with a match on `(dx, dy, x, y)` whose four diagonal arms guard on *both* coordinates being extreme, so they only cover the chunk's four literal corners. A cell on the top edge at `x == 0, y == 7` with offset `(-1, -1)` matches no arm and falls through to `self.cells[(0 - 1) as usize][6]`. Reproduced against an all-dead chunk:

```
thread 'edge_cell_non_corner_row0_col5' panicked at src/cell.rs:96:26:
index out of bounds: the len is 16 but the index is 18446744073709551615
```

Eight arms are missing — four edges times two diagonal directions, the cases where a diagonal move crosses exactly one boundary. Do not add them. Compute the neighbour's absolute position as `i32`, derive the chunk offset by flooring division by the chunk size, and take the coordinate `rem_euclid`. Six lines, no gaps by construction, and it does not grow when the chunk size changes.

Corner cells panic separately: `Index for Neighbor` maps `Neighbor::Unloaded => panic!()` (`src/cell.rs:52`). Every border cell of every chunk at the edge of the loaded world hits it. Unloaded should read as dead, and `is_loaded()` already exists and is never called.

## The chunk graph cannot represent a neighbourhood

`Neighbor::CellChunk(CellChunk)` holds a chunk by value, and `CellChunk.neighbors` is `[Rc<Neighbor>; 8]` with no interior mutability. `Rc` gives shared immutable access, so a chunk reached through a neighbour slot can never have `calc_generation(&mut self)` called on it. Building A holding B requires B fully constructed including its own neighbours; if B holds A back, A is needed first. Building A with all-`Unloaded` neighbours and handing that clone to B works, but then A's view of B contains a stale snapshot of A and every generation drifts further.

`GameState.chunks: Vec<Rc<Cell<cell::Neighbor>>>` is a different type from `[Rc<Neighbor>; 8]`, so a chunk in the game's list cannot be assigned into any chunk's neighbour array at all. `std::cell::Cell` only supports whole-value replace, which buys nothing here.

Replace with a `HashMap<(i32, i32), Chunk>` keyed by `loc` — which already exists on `CellChunk` and is unused — and look neighbours up by coordinate. `calc_generation` becomes a free function taking the map plus a key, since you cannot hold `&mut` on one chunk and `&` on another from the same map: read the eight neighbour edge strips into locals first, then mutate.

## The two halves do not touch

`BattleApp` initialises `game_state: GameState { chunks: vec![] }` and never touches it again — `field 'game_state' is never read`. `draw_calls` emits one hardcoded three-vertex triangle. `ticker` accumulates `dt` and nothing reads it. `grid.wgsl` and `triangle.wgsl` are byte-identical, and `game.rs` loads `triangle.wgsl` into a field called `grid_pipeline` labelled `"triangle pipeline"`.

## Smaller

- `now_secs()` on native uses `SystemTime::now()`, a wall clock subject to NTP steps. `.max(0.0)` guards negative `dt` but a spike still gets through. Use `Instant`.
- `ControlFlow::Poll` plus `request_redraw()` on every `AboutToWait` is an uncapped busy loop, and `present_mode: surface_caps.present_modes[0]` takes whatever the driver lists first rather than asking for `Fifo`.
- No tests. Blinker, block and glider over a single chunk would have caught both panics before they were committed.
- `mod conwayHandler` trips `non_snake_case`; `CellArray` is private but exposed through `pub` fields on `CellChunk`, tripping `private_interfaces`.
- `apply_generation` copies 256 cells individually where `mem::swap` moves two pointers.
- No LICENSE.

## Suggested order

1. Fix `create_view`, delete the duplicate shader.
2. Write blinker/block/glider tests against a single chunk.
3. Rework the neighbour lookup as coordinate arithmetic until they pass.
4. Replace the `Rc` chunk graph with the coordinate map.
5. Only then wire the cell data to the renderer.

Step 2 before step 3 is the point. The cell module has never executed; making it trustworthy is the prerequisite for connecting it to anything.
