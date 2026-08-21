# Rendering

One pipeline, one bind group, one instanced draw call.

```rust
pass.set_pipeline(&pipeline);
pass.set_bind_group(0, &bind_group, &[]);
pass.set_vertex_buffer(0, instances.slice(..));
pass.draw(0..4, 0..instance_count);
```

A pipeline is the compiled shader plus fixed-function state; textures live in bind groups. Binding a different chunk costs nothing at all, because every chunk is a layer of the same array texture.

## The chunk store

A `texture_2d_array<u32>` in `Rgba8Uint`, one chunk per layer. R and G are the cell's sixteen bits, B and A its tile UV.

The array is a **cache of what is on screen**, not a limit on world size. `max_texture_array_layers` is 256 on the guaranteed minimums, and layers are recycled as chunks come and go.

That budget never binds, because the zoom floor is stricter. Visible chunks for a `W×H` viewport at zoom `Z` with chunk edge `N`:

```
(ceil(W / (Z·N)) + 1) × (ceil(H / (Z·N)) + 1)
```

At 16×16 chunks a 4K screen at one pixel per cell wants 160 layers. The floor of one pixel per cell exists for a different reason — below it, point sampling drops sparse cells and they flicker out rather than shrinking — but it happens to keep the layer count in range too.

`Queue::write_texture` is exempt from the 256-byte `bytes_per_row` alignment that `copy_buffer_to_texture` imposes, which is what makes a 64-byte chunk row a legal upload.

## Unloaded ground

One quad for all of it, drawn first, with the grid pattern computed from world position rather than sampled.

It used to be a quad per chunk, and the visible chunk count grows as the square of zooming out: a 1920×1080 screen at one pixel per cell covers over eight thousand chunks against an instance budget of a thousand, so the far edges simply stopped being drawn. Every chunk of empty ground looks identical, so there was never anything to gain from drawing them separately. Cost is now flat in the zoom.

## Sprites

One file per cell **state**, in `assets/sprites/`, each a 256×256 sheet of 16×16 tiles, loaded into its own layer of a texture array.

| state | file |
|---|---|
| dead | `dead.png` (deliberately blank) |
| alive | `alive.png` |
| dead under ice | `dead_ice.png` |
| alive under ice | `alive_ice.png` |

Four images rather than compositing a pane over a cell. That is partly an art decision — what an iced cell looks like is decided in the art — and partly a correctness one: compositing meant sampling inside an `if` on whether the cell was alive, and WGSL requires anything using implicit derivatives to sit in **uniform control flow**. One state, one unconditional sample.

The layer is `kind * 4 + state`, so a kind's four images sit together and a kind cannot name art that does not exist. `Kind::ALL` is walked by a test that fails if any state is blank.

A cell's own u,v picks the tile within its sheet, so a structure spanning several cells gives each one a different tile and the parts line up. Tile (0,0) is the default; the rest of the sheet is room for multi-cell pictures.

The PNGs are the source. There was a generator that drew them from ASCII art, in Python because the crate embeds these files with `include_bytes!` and so cannot build until they exist — which rules out a cargo example. It is gone: the art is edited directly now, and a generator that nobody runs is a second definition of the sprites waiting to disagree with the first.

**No anti-aliasing.** Nearest sampling, no mip chain, hard edges — a test asserts every alpha is one of a few fixed inks. The cost is that far-out zoom point-samples a 16×16 tile down to a pixel and will shimmer, which is why the camera does not go below one pixel per cell.

## Colour

Sprites carry **no hue**. A texel is saturation, lightness and coverage; the hue arrives at draw time from the cell's player, so one set of art serves all 31 players.

OKLab, not HSV: HSV's hues are not perceptually even — its yellows read far brighter than its blues at equal value, so players would not look equally prominent.

Asking for more chroma than sRGB can show is the *normal* case at useful saturations. Clamping fixes the range but bends hue, because red clips before blue, so two players drift towards each other — which defeats the point of choosing distinct hues. The chroma is **bisected down until it fits**, eight steps, keeping hue and lightness exactly. Across 31 players at four lightnesses nothing goes out of gamut; clamping at less than half the chroma still clipped 16 of 124 combinations.

Player colour has two axes. Hue is spaced by the golden ratio; saturation alternates between two tiers. Hue alone left the closest of 31 players 0.026 apart in OKLab; the tiers lift that to 0.037 over all 31 and 0.119 over the first eight. Spreading saturation *smoothly* measured worse than doing nothing, because lowering it shrinks the chroma radius and pulls colours together — the alternation is the point.

`client::views::hud::player_colour` reproduces the same arithmetic on the CPU so the HUD swatch and the board cannot disagree.

## The camera

Fixed centre and zoom, driven by input. The viewport is refreshed from the `GpuState` **every frame** rather than only on resize: two sources of truth meant the camera and the zoom anchoring disagreed about screen size, and zoom pulled towards the wrong point.

Zoom is clamped to [1, 64] pixels per cell and anchors on the cursor, or on the midpoint between two fingers, so what is under the pointer stays under it. Every path — wheel, ctrl+scroll, trackpad pinch, two-finger touch — goes through one `zoom_about`, so they cannot drift apart.

## The overlay

egui draws into the **same render pass** as the world, so there is no second surface and no compositing step. `Frame::submit` takes an overlay closure that runs after the draw calls and receives the encoder too.

The pass is `'static` because that is what egui's renderer takes: a pass keeps its referenced resources alive itself, and the only consequence is that touching the encoder while the pass is open is a runtime error rather than a compile one.

Whether a click belongs to the interface or the world is decided from **the rectangle the interface occupied last frame**, plus whether a widget is mid-drag. Not from egui's own `wants_pointer`, which depends on interaction state this integration feeds by hand: if any of that is wrong the answer sticks true and the world silently stops receiving clicks. A rectangle can be printed and reasoned about.

## Theme

`client::views::theme` holds a `Palette` and `Metrics`; no view names a colour. The world's clear colour comes from the same palette, so ground and panels are one colour rather than two guesses at it. Fonts are the gap — `ctx.set_fonts` in `Theme::apply` is where typography goes.
