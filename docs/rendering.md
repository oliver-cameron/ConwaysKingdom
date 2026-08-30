# Rendering

One pipeline, one bind group, one instanced draw call.

```rust
pass.set_pipeline(&pipeline);
pass.set_bind_group(0, &bind_group, &[]);
pass.set_vertex_buffer(0, instances.slice(..));
pass.draw(0..4, 0..instance_count);
```

A pipeline is the compiled shader plus fixed-function state; textures live in bind groups. Binding a different chunk costs nothing at all, because every chunk is a layer of the same array texture.

**That array texture is no longer a copy of the world.** It was `Rg8Uint` and a memcpy — `bytemuck::bytes_of` over a `Chunk`, which is why the shader pins its byte layout to `sim::cell::bits` with a comment. It is `Rgba8Uint` now: the first two bytes are still the cell's own, the third is a **neighbour mask**, and the fourth is spare.

The mask is which of a cell's four sides have something on them that would draw the same sprite, and it exists because a fragment cannot work that out. A fragment knows only its own array layer, and the chunk-to-layer map is a `HashMap` on the CPU, so a cell on a chunk edge has no way to reach the layer its neighbour is in. `render::chunks::texels` computes it on the way to the GPU, where the whole world is in hand and the coordinates are already folded for a torus — a kilobyte per chunk across the fifty or so on screen, once per sync.

It is **derived and it is not in `sim::Cell`**, which is the part worth stating. `Cell` goes over the wire and into the desync digest, so putting appearance in it would make how the world looks something two clients can argue about and resync over. A texture is a client-side view of state; the state is what the mask is computed *from*.

What the shader does with it today is draw a region's outline: a side with nothing like the cell on it gets a line, a side that continues into a neighbour does not, so a block of ice reads as one slab rather than as sixteen tiles that happen to touch. The usual answer is a sheet of sixteen variants per material with the mask choosing between them, and that is a question of art rather than of arithmetic — the byte holds the mask and the sheet has two hundred and forty free tiles, so the day those variants are drawn it becomes `tile + variant[mask]` and nothing else changes.

## The chunk store

A `texture_2d_array<u32>` in `Rgba8Uint`, one chunk per layer. R and G are the cell's sixteen bits, B and A its tile UV.

The array is a **cache of what is on screen**, not a limit on world size. `max_texture_array_layers` is 256 on the guaranteed minimums, and layers are recycled as chunks come and go.

That budget never binds, because the zoom floor is stricter. Visible chunks for a `W×H` viewport at zoom `Z` with chunk edge `N`:

```
(ceil(W / (Z·N)) + 1) × (ceil(H / (Z·N)) + 1)
```

At 16×16 chunks a 4K screen at one pixel per cell wants 160 layers. The floor of one pixel per cell exists for a different reason — below it, point sampling drops sparse cells and they flicker out rather than shrinking — but it happens to keep the layer count in range too.

`Queue::write_texture` is exempt from the 256-byte `bytes_per_row` alignment that `copy_buffer_to_texture` imposes, which is what makes a 64-byte chunk row a legal upload.

## A wrapping world is folded, not tiled

Every chunk position the viewport covers is asked which chunk actually fills it. On an infinite world that is the identity; on a torus it is many-to-one, so **the world repeats for as far as anyone can pan** and the work is proportional to the screen rather than to the world.

It used to draw a fixed number of copies either side of the original — `World::render_tiles(repeats)`, with `repeats` at 1. Two things were wrong with that. Panning off the third copy fell into blank space forever, which is not what a world with no edge should do. And a large torus paid for nine copies of every chunk whether or not any of them were on screen: a 12×12 world produced 1296 instances against a budget of 1024, and warned about it, while a viewport at normal zoom needs a couple of dozen.

The texture side already worked this way — `sync` chose which chunks get a layer by folding the visible region — so the fix was to make the instance list agree with it. `render_tiles` is gone and so is the repeat count.

## Unloaded ground

One quad for all of it, drawn first, with the grid pattern computed from world position rather than sampled.

It used to be a quad per chunk, and the visible chunk count grows as the square of zooming out: a 1920×1080 screen at one pixel per cell covers over eight thousand chunks against an instance budget of a thousand, so the far edges simply stopped being drawn. Every chunk of empty ground looks identical, so there was never anything to gain from drawing them separately. Cost is now flat in the zoom.

## Sprites

**One sheet**, `assets/sprites/sheet.png`: 256×256, a 16×16 grid of 16×16 tiles. A cell's tile byte is the index into it — low nibble across, high nibble down — and that byte already carries alive, ice, kind and age, so there is nothing to look up.

The fields are placed so the sheet reads as a grid rather than as a list. Alive and ice are the bottom two bits, so a kind's **four states are four columns**; age is the low three bits of the high nibble, so its **eight ages are eight rows** under them. The kind's third bit is the top bit of the byte, which splits the sheet in half: kinds 0–3 above, 4–7 below.

Nothing advances age yet — see [payloads](planned.md#payloads), which is what it is for. What that costs today is nothing: every kind in play is 0–3 and every cell is age nought, so all the art that exists is in the first row, exactly where the old `kind * 4 + state` mapping put it.

| tile | state |
|---|---|
| `kind * 4 + 0` | dead |
| `kind * 4 + 1` | alive |
| `kind * 4 + 2` | dead under ice |
| `kind * 4 + 3` | alive under ice |

A tile per state rather than compositing a pane over a cell. That is partly an art decision — what an iced cell looks like is decided in the art — and partly a correctness one: compositing meant sampling inside an `if` on whether the cell was alive, and WGSL requires anything using implicit derivatives to sit in **uniform control flow**. One tile, one unconditional sample, and now not even a layer index to compute.

The sheet in the repo is **provisional**: four flat tiles so the states are told apart, all in the first row. Redraw it and drop it in; no code changes, because the mapping is the tile byte and nothing else.

The PNGs are the source, and `cnvt` converts between what you draw and what the shader reads, in both directions:

```
cargo run --bin cnvt -- art.png assets/sprites/sheet.png            # forward
cargo run --bin cnvt -- --back assets/sprites/sheet.png art.png     # and back
cargo run --bin cnvt -- --back --player 3 sheet.png as-p3.png       # as the game draws it
```

The reverse exists because a sheet cannot be opened: three of its channels are numbers fed to a colour model, so a paint program shows something that looks nothing like the art. Converting back gives a picture you can look at, edit, and convert forward again — a round trip is exact to within a step of rounding, and a test in the tool pins that, because the pair silently stops being a pair if its `shade` and the shader's ever drift.

`--player N` reverses it the way the game will draw it, taking hue and saturation tier from player N rather than from the sheet. `--player 0` is unowned, which is grey.

`strip` clears a channel:

```
cargo run --bin strip -- b assets/sprites/sheet.png assets/sprites/sheet.png
```

Written for blue, which is hue. `cnvt` writes it because it is the honest decomposition of a pixel, but the shader never reads it — hue comes from the cell's player, so one sheet serves every player. A sheet carrying hue carries a number nothing will look at, and one that *reads* as though the art chose a colour it has no say over. It reports how much it found before clearing it, because whether a sheet carries any is a question worth an answer; in and out may be the same file. Any channel, not only blue: clearing saturation makes a sheet greyscale, which is a real thing to want. The atlas is not a picture: its channels are the arguments to `shade()` — R saturation, G lightness, B hue, A coverage — so art is drawn in ordinary colours in any editor and converted, rather than authored channel by channel in a space nobody can see. The tool reports the worst round trip it caused, because the format cannot express everything: `shade` tapers chroma towards black and white, so a vivid colour at an extreme lightness clamps to full saturation and comes back duller. The shader ignores B today, taking hue from the cell's player instead; the channel is written anyway because it is the honest decomposition and costs nothing.

 There was a generator that drew them from ASCII art, in Python because the crate embeds these files with `include_bytes!` and so cannot build until they exist — which rules out a cargo example. It is gone: the art is edited directly now, and a generator that nobody runs is a second definition of the sprites waiting to disagree with the first.

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
