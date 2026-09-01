# Architecture

The crate is divided by **who needs the code**, not by what it does.

```
src/
  sim/      the rules                 client AND server
  net/      wire types and transport  client AND server
  server/   the authoritative side    server only
    rooms.rs  several worlds behind one address
  render/   GPU and windowing         client only
  client/   what a player sees        client only
    views/  screens, and the egui they are drawn with
```

## The dependency rule

It runs one way, and the build enforces it rather than trusting convention.

- `sim` depends on `bytemuck` and `serde` and nothing else. No wgpu, no winit, no web-sys.

`sim::Player` is the one player type, and **a team is one of them**. It has a number, a purse and a patch of granted ground like anybody else; `plays_as` says which player a client is driving, and it is the client's own number unless they have joined a team. Nothing below `server` learns that teams exist — every rule takes a `PlayerId` and compares it — which is what makes a team cost no new code rather than a comparison threaded through placement, pricing, spawning, mining, scoring and colour. See [server.md](server.md#teams).

Inside `sim`, `rule.rs` is deliberately thin. **Every tunable number in the game is a constant there** — the survival counts, how fast ground changes hands, what everything costs, what mining pays — and every rule is one named entry in an ordered list. Nothing else is: the seeded dice are `sim::seed`, the tests are `sim/rule/tests.rs`, and the list-to-chain macro is `sim/rule/order.rs`. The point is that the rules of the game can be read on one screen and changed by editing a number.

A **room** carries `net::Rules` beside its world: `paused`, `place_anywhere`, `place_free`, and `laboratory`, which says whether the first three are anybody's to change. They are the room's rather than the client's for the reason everything authoritative is — a client that answered "may I place here" for itself would predict placements the server refuses and resync every time it drew, which is what kept a laboratory offline for as long as it was a mode rather than a kind of room. `net::RoomKind` reads a room's kind off those and its victory condition, and is what the make-a-world form asks first.

That includes the **prices**, which used to live in `net` beside the actions that spend them. "Life costs one" is the same kind of statement as "a cell survives on two or three", and somebody balancing the game should not have to look in two files. `net` names the actions and reads the numbers.
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

**Two screens, and what they share.** `views::game` is the world and everything drawn over it — the HUD, the hotbar, the lobby, the clock, the key list, the overlay, the stamp library, the camera. `views::menu` is what comes before it. A module lives under the screen that uses it and comes back up the moment two do.

What is left at the top is what both need: `views::theme` is every colour and measurement, `views::hue` a player's colour and its conversion to sRGB, `views::icons` the sprite sheet tinted on the CPU the way the shader tints it on the GPU so a button can show the cell it places, `views::record` the games this client has played, and `views::Views` the egui plumbing.

`views::words` holds **every string the client puts on screen**, for the same reason `sim::rule` holds every number: what the game says is a decision, and decisions are easier to get right side by side than scattered through the code that draws them. Log lines are not there — those are for whoever is running it, and belong where the thing they describe happens.

`views::game::Ui` is **what the interface holds between frames** — which screen, and what is half-typed or half-drawn on it. Grouped because building a frame needs `&mut` on all of it while `views` is borrowed: `ui` and `views` are different fields, so borrowing one says nothing about the other and the frame holds one thing. Each field used to be taken out with `mem::take` and put back afterwards, and a field taken out is silently *lost* if anything in between returns early — which one of the arms does. The world, the link and the purse are not interface state and stay on the app; see [planned.md](planned.md#the-session-comes-out-of-the-game-view) for the other half of that split, which is not done.

Every view answers with a **`Shown<T>`**: what it covered, so a click on it does not also reach the world, and what it was told. They answered those two questions five ways once — a bare `bool`, a bare `Option<Rect>`, `(Rect, Did)`, `(Did, Rect)`, and one struct of its own — and the two that differ only in order are the ones that get swapped without anything noticing. The `did` stays each view's own enum, because what a hotbar can be told and what a lobby can be told have nothing in common.

The two are **not two `App`s.** The event loop calls one, and the world, the pipeline and the atlas belong to the game whether or not it is being looked at — so the menu is a `Screen` the game app is in rather than a second app with its own copy of the GPU. What that costs is one question asked at the top of every input handler: a click that lands beside the menu panel must not draw on the world behind it.

`views::menu` opens no sockets and sends no messages. It holds what the player has typed and what the server has said, and returns what was chosen; the client acts on it. That is what keeps "what a menu looks like" and "what connecting means" two separate things.

`client::session` is **what this client is to a server**: the link, the seat, the purse, the subscription set, and the machinery that keeps one world in step with another. It came out of `views::game`, which was a view by where it lived and was not one by what it did — it held the world, the link and the GPU pipeline on one struct with about fifty fields, and folded server messages into the world from inside a frame. The session takes messages in and produces two things: mutations of a world it does not own, and a `session::Effect` per thing only an interface can do — move the camera, put a screen up, say something in a corner. It needs no wgpu and no egui, so it can be tested, and none of it could be.

The world is a **parameter** rather than a field on it. It belongs beside the chunk store that draws it, and passing it in is what lets a test step a session against a world with no window near either. `client::desync` and `client::record` were the first two pieces of this living outside, and they are the shape the rest took.

`views::game::camera` is split out of the game view because it is the one part of it that is pure arithmetic — a position, a scale, and the mapping between the screen and the world. That mapping was written out at each of its four call sites, and none of it could be tested without a window to put it in. It knows nothing about what is drawn or what the pointer means: the view decides that a middle drag pans, the camera decides what panning is.

`Views` translates winit events into `egui::RawInput` by hand rather than using `egui-winit`, which does not compile for wasm32 — see [gotchas](gotchas.md). That now includes the keyboard: a menu is two text fields, and nothing needed typing until there was something to type into. A key that produces text produces two egui events, `Key` and `Text`, because egui routes shortcuts off the first and content off the second — a field given only text could never be corrected.
