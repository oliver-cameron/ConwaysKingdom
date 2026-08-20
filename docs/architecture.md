# Architecture

The crate is divided by **who needs the code**, not by what it does.

```
src/
  sim/      the rules                 client AND server
  net/      wire types and transport  client AND server
  server/   the authoritative side    server only
  render/   GPU and windowing         client only
  client/   what a player sees        client only
    views/  screens, and the egui they are drawn with
```

## The dependency rule

It runs one way, and the build enforces it rather than trusting convention.

- `sim` depends on `bytemuck` and `serde` and nothing else. No wgpu, no winit, no web-sys.
- `net` depends on `sim`.
- `render` depends on `sim`, and mentions egui nowhere — not in code, not in a comment. An interface is the client's business.
- `client` depends on all of them.
- `server` depends on `sim` and `net`.

Nothing in `sim` or `net` names anything from `render` or `client`.

## Feature gates

| feature | brings in | default |
|---|---|---|
| `render` | wgpu, winit, egui, egui-wgpu | on |
| `server` | axum, tower-http | off |

`cargo build --no-default-features --features server --bin server` compiles **zero GPU crates** — no wgpu, winit, naga, glow, ash or egui in the graph. That turns the module boundary from a convention into something the build checks, and it is worth re-checking after a refactor:

```
ls target/debug/deps | grep -ciE '^libwgpu|^libwinit|^libegui'
```

Transport (tokio, tokio-tungstenite, futures-util) is declared under `cfg(not(target_arch = "wasm32"))`, so a browser build pulls none of it. A browser has no sockets to give tokio; the web client uses `web_sys::WebSocket` instead, in `net/link_web.rs`, behind the same `Link` surface as the native one.

## The App seam

`render::app::App` is what the event loop calls. It describes what the loop needs without naming what provides it:

| | |
|---|---|
| `init`, `resize`, `update`, `draw_calls`, `clear_color` | the frame |
| `on_key`, `on_cursor`, `on_click`, `on_scroll`, `on_pinch`, `on_touch` | typed input |
| `on_window_event` | the raw event first; returning true suppresses the typed callbacks |
| `overlay` | record anything that sits on top, into the same pass |

`on_window_event` is how an interface layer keeps a click on a button from also acting on the world. `overlay` takes `&self`, not `&mut self`, because the frame holds an immutable borrow of the app for the whole pass — `draw_calls` returns references into it — so anything that mutates does it behind its own cell.

## Views

A view is a screen. `views::battle` is the game; a menu or lobby would sit beside it. `views::Views` is the egui plumbing they share, and `views::theme` is everything visual.

`views::camera` is split out of the battle view because it is the one part of it that is pure arithmetic — a position, a scale, and the mapping between the screen and the world. That mapping was written out at each of its four call sites, and none of it could be tested without a window to put it in. It knows nothing about what is drawn or what the pointer means: the view decides that a middle drag pans, the camera decides what panning is.

`Views` translates winit events into `egui::RawInput` by hand rather than using `egui-winit`, which does not compile for wasm32 — see [gotchas](gotchas.md).
