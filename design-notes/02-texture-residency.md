# Array textures, layer residency, and the zoom floor

Chunks live in a `texture_2d_array<u32>`, one chunk per layer. The array is not a cap on world size — it is a cache of what is currently on screen. Simulation runs over as many chunks as you like in CPU memory; only visible ones need a layer.

## Why an array and not an atlas

An atlas is bounded by `max_texture_dimension_2d`, which is 2048 on the downlevel path. A 2048x2048 atlas holds 4.2M cells no matter how it is tiled. An array holds `max_texture_array_layers` (256 guaranteed) times the layer area — 16.7M cells at 256x256 layers. The array wins by four times on the guaranteed minimums, and it avoids atlas coordinate math and bleed entirely.

## 256 is a floor, not hardware

`max_texture_array_layers` is 256 in both `Limits::default()` and `downlevel_defaults()`. Real native GPUs report 2048 or more. `gpu.rs` currently requests `wgpu::Limits::default()` on native, which imposes the 256 cap on hardware that would give far more. Query `adapter.limits().max_texture_array_layers` and request what is there, while still designing to 256 so the WebGL2 path works.

## The layer budget never binds

Visible chunks for a `W x H` viewport at zoom `Z` (screen pixels per cell) with chunk edge `N`, including partial chunks at the edges:

```
(ceil(W / (Z*N)) + 1) * (ceil(H / (Z*N)) + 1)
```

| viewport | N=128, Z=1 | N=128, Z=2 | N=256, Z=1 | N=256, Z=2 |
|---|---|---|---|---|
| 1080p | 160 | 54 | 54 | 20 |
| 1440p | 273 | 77 | 77 | 24 |
| 4K | 558 | 160 | 160 | 54 |
| ultrawide 5120x1440 | 533 | 147 | 147 | 44 |

Zoom at which a 256-layer budget saturates:

| viewport | N=128 | N=256 |
|---|---|---|
| 1080p | 0.80 px/cell | 0.40 px/cell |
| 1440p | 1.05 | 0.55 |
| 4K | 1.55 | 0.80 |
| ultrawide | 1.50 | 0.75 |

At 256x256 chunks the budget saturates below one pixel per cell on every viewport — and one pixel per cell is already the floor you want for a separate reason. So the visual constraint is stricter than the memory constraint and the layer limit stops being something to think about. At 128x128 it is the other way round and you would be forced to a 1.55 zoom floor for reasons unrelated to how it looks.

## Capping zoom retires the LOD problem

Below one pixel per cell, `textureLoad` point-samples and sparse live cells simply vanish as the camera pulls back. Fixing that properly means a hand-built mip chain using a max-or-count reduction, because automatic mipmapping averages and does not apply to `Uint` formats. Setting a floor of one pixel per cell means that subsystem never has to exist.

Wide views are served by the minimap instead, which is derived aggregate data at chunk granularity rather than a downscaled copy — see [04-rendering.md](04-rendering.md).

## Derive the floor, do not hardcode it

Neither 256 nor the zoom floor should be a literal in the code. Compute one from the other at startup and on resize, and overflow becomes unreachable rather than a case to handle.

```rust
/// Smallest zoom (screen px per cell) whose visible set fits in `layers`.
pub fn min_zoom(viewport: (u32, u32), chunk_n: u32, layers: u32) -> f32 {
    let mut z = 1.0_f32;                     // never below 1:1 — aliasing
    while visible_chunks(viewport, z, chunk_n) > layers { z += 0.05; }
    z
}

fn visible_chunks((w, h): (u32, u32), z: f32, n: u32) -> u32 {
    let per = z * n as f32;
    ((w as f32 / per).ceil() as u32 + 1) * ((h as f32 / per).ceil() as u32 + 1)
}
```

## The residency cache

```rust
pub struct LayerCache {
    resident:  HashMap<ChunkId, u32>,   // chunk -> layer
    occupant:  Vec<Option<ChunkId>>,    // layer -> chunk
    last_used: Vec<u64>,                // layer -> frame
    free:      Vec<u32>,
    frame:     u64,
}

impl LayerCache {
    pub fn begin_frame(&mut self) { self.frame += 1; }

    /// Returns the layer holding `id`, and whether it still needs uploading.
    pub fn acquire(&mut self, id: ChunkId) -> (u32, bool) {
        if let Some(&layer) = self.resident.get(&id) {
            self.last_used[layer as usize] = self.frame;
            return (layer, false);
        }
        let layer = self.free.pop().unwrap_or_else(|| {
            let (l, _) = self.last_used.iter().enumerate()
                .filter(|(_, &f)| f < self.frame)          // never evict this frame's work
                .min_by_key(|(_, &f)| f)
                .expect("visible set exceeds layer budget — zoom floor is wrong");
            let l = l as u32;
            if let Some(old) = self.occupant[l as usize].take() { self.resident.remove(&old); }
            l
        });
        self.resident.insert(id, layer);
        self.occupant[layer as usize] = Some(id);
        self.last_used[layer as usize] = self.frame;
        (layer, true)
    }
}
```

The `filter(|(_, &f)| f < self.frame)` is load-bearing. Without it the cache can evict a chunk already drawn this frame and thrash within a single frame rather than across frames. With the zoom floor derived correctly, the `expect` is unreachable.

Allocate the full layer budget up front — array textures cannot be resized, and 256 layers of 256x256 at two bytes per cell is 33.5 MB, which is not a number worth managing.

## Mechanical details

Layer selection on upload uses the `z` field of the origin. `Origin3d` is a plain `{ x, y, z }` and `Extent3d`'s third field is documented as "the depth of the extent **or the number of array layers**":

```rust
origin: wgpu::Origin3d { x: ox, y: oy, z: layer },
size:   wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
```

Spell out the view dimension rather than taking the default, or you risk a `D2` view failing validation against a `texture_2d_array<u32>` binding:

```rust
let view = array.create_view(&wgpu::TextureViewDescriptor {
    dimension: Some(wgpu::TextureViewDimension::D2Array),
    ..Default::default()
});
```
