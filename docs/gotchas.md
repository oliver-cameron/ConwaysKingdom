# Gotchas

Things that cost a day here, written down so they only cost one. Each of these produced a symptom that pointed somewhere else.

## `TexturesDelta` asserts on drop

egui hands over a `TexturesDelta`, and dropping one that still holds deltas panics. Applying every change is **not enough** — reading it through a reference leaves it full. It must be `clear()`ed.

*Symptom:* the client dies on startup at `epaint/src/textures.rs`, on the first frames when the font atlas arrives and the surface is still reporting `Skip` so nothing has drawn. And because the panic kills the app before any input is dispatched, it looks exactly like the world has stopped responding to clicks.

*Also:* apply textures when they are **produced**, not when they are drawn. A frame is not always drawn.

## `egui-winit` does not build for wasm32

At 0.36.1, `egui::DroppedFile` declares `bytes_async` under `cfg(wasm32)` and egui-winit's impl provides only the native `bytes`, so the trait is unimplemented on that target. Upstream bug, not a wiring mistake.

We translate winit events into `egui::RawInput` by hand instead — about a hundred lines, one code path for both targets. A HUD needs pointer, wheel and modifiers; the IME and clipboard handling egui-winit exists for is not in play.

## A canvas has two sizes, and nothing sets the one that matters

`width`/`height` are the pixels drawn into; the CSS box is where those pixels are stretched to. Styling the canvas `100vw` by `100vh` sets the box only. **Nothing sets the backing store.** winit's resize observer reports the box and emits `Resized`, and wgpu configures a surface, but neither writes `canvas.width`, so it keeps whatever it had — 300×150 by default, or 1×1 once winit has applied its own zero-sized idea of the window.

Do not write the canvas directly either: wgpu's WebGL backend sets the backing store to match whatever the **surface** is configured to, so a write to the canvas is undone on the next frame. Configure the surface from the canvas's client box each frame and the canvas follows it.

winit's `inner_size` is no help: it starts at zero, so the surface is configured 1×1 before any resize observation lands.

*Symptom:* the whole game drawn into a handful of pixels and scaled up, **and it comes right the instant you open the web inspector** — devtools changes the window size, which is what finally gets a resize past whatever guard was blocking it. That tell is worth more than any amount of reading.

## `navigator.gpu` existing does not mean WebGPU works

On a secure origin — `localhost` counts — Chrome exposes `navigator.gpu` and then returns **null** from `requestAdapter` whenever no GPU is usable: a blocklisted driver, a crashed GPU process, a VM, a headless browser. wgpu hands that null back as an `Adapter` anyway, and the first method called on it throws

```
TypeError: Cannot read properties of null (reading 'info')
```

which kills the page before the WebGL2 fallback is reached. Ask the browser yourself and only name `BROWSER_WEBGPU` if the answer is a real adapter.

*Also:* web-sys's WebGPU bindings are behind `--cfg=web_sys_unstable_apis`, so that check goes through `js_sys::Reflect` rather than putting a build flag between the crate and compiling.

## Setting a canvas's size clears it

`Window::inner_size` on the web returns a value winit updates from its **own** resize observer, so it never reflects a request in the same frame. Comparing against it means requesting a resize every frame, and setting a canvas's size clears it — the picture never survives to be shown.

Compare against the last size *requested* instead.

*Symptom:* a completely blank browser canvas, with no error anywhere.

## A one-layer array texture is not an array on GL

wgpu-hal's GL backend picks its texture target from the **texture** descriptor, not the view: `depth_or_array_layers == 1` makes a `TEXTURE_2D`, and a `D2Array` view over it mismatches.

```
wgpu-hal heuristics assumed that the view dimension will be equal to `D2` rather than `D2Array`
```

Allocate the real layer budget. Native Vulkan does not care, so this only shows in a browser.

*Also:* that message arrives through `log::error!`, so it is non-fatal and its console stack points at the wasm-bindgen shim that forwards it, not at anything of ours.

## `textureSample` needs uniform control flow

WGSL forbids anything using implicit derivatives inside a conditional that varies per fragment. **Naga accepts it; Tint does not**, and it is undefined on the GL path.

That is one reason each cell state has its own sprite rather than compositing: one state, one unconditional sample.

## `write_texture` is exempt from the 256-byte row alignment

`copy_buffer_to_texture` and `copy_texture_to_buffer` require `bytes_per_row` to be a multiple of 256. `Queue::write_texture` does not, which is what makes a 64-byte chunk row a legal upload.

## Alignment decides whether a cast is free

`[u8; 4]` has alignment 1; a `u16` field forces alignment 2. Only the first can be reinterpreted from an arbitrary byte offset, which is what a save file and a wire frame hand you. With alignment 2, `bytemuck::from_bytes` panics on an odd offset.

## Reserved words

- `meta` is reserved in **WGSL**. It fails at pipeline creation, not compile time.
- `gen` is reserved in **Rust edition 2024**. `for gen in 0..n` will not parse.

## bytemuck needs feature flags

Without `min_const_generics`, `Pod` is implemented only for a fixed list of array lengths. `[Cell; 256]` happens to be on it and `[Cell; 65536]` is not, so code compiles at one chunk size and fails at another. `zeroed_box` needs `extern_crate_alloc`.

An enum can never be `Pod`: it has invalid bit patterns. And bytemuck refuses to derive `Pod` on anything with padding, which is a guardrail worth having.

## Two versions of one crate cannot share a window

`egui-winit` tracks winit and `egui-wgpu` tracks wgpu. If the versions disagree with yours, the `Window` and device types are unrelated and nothing will bridge them. Check with `cargo tree -i winit` and `cargo tree -i wgpu` — one entry each.

## `is_empty` is three different questions

For a chunk, "nothing alive" discards panes for good. "Every cell exactly `DEAD`" keeps every chunk life ever passed through, because a cell keeps its owner when it dies, and an infinite world grows without bound. It has to be "no life and no structure".

## A drag is not a sequence of small moves

Deciding that a press has become a drag from the distance between **one pointer event and the next** does not work. A slow, deliberate sweep arrives as a stream of one- and two-pixel moves, and no single one of them clears any threshold worth setting — while a hand that shakes on the button produces one that does. Measure from where the press landed instead.

*Symptom:* dragging out a rectangle places a single cell at the release point, and the faster you drag the more likely it is to work. It reads as the fill being broken rather than the classification.

## Serving over plain HTTP costs you WebGPU

`navigator.gpu` requires a secure context. `http://host:8080` is not one, so a browser falls back to WebGL2 — which works, but is a different backend with lower limits. `localhost` **is** a secure context, so an SSH tunnel gets you WebGPU without TLS.
