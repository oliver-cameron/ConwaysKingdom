# Documentation

| | |
|---|---|
| [architecture.md](architecture.md) | The module split, who depends on whom, and how the build enforces it |
| [simulation.md](simulation.md) | Cells, chunks, worlds, the rules, and the determinism contract |
| [rendering.md](rendering.md) | The pipeline, sprites, colour, and the camera |
| [game.md](game.md) | Value, placement, ice, and the controls |
| [networking.md](networking.md) | The protocol, prediction, and desync detection |
| [server.md](server.md) | Running it, and the save format |
| [planned.md](planned.md) | What is not built yet, and what is left of what is — a status per entry |
| [inspiration.md](inspiration.md) | Where the design is borrowed from, and which problem each source solved |
| [gotchas.md](gotchas.md) | Things that cost a day, so they only cost one |
| [simplifying.md](simplifying.md) | How to cut this tree back, and what not to cut |
| [../design-notes/](../design-notes/) | The working behind the decisions: cell layout, residency, topology, compute |
| [known-bugs.md](known-bugs.md) | What is wrong and not fixed, with what you would see |
| [../deploy/](../deploy/) | Bringing a host up: the build, the unit, the tunnel, and what to back up |

## Running it

The server serves the browser client and the websocket from one origin, so there is no second static-file server and no cross-origin question.

```
cargo run --no-default-features --features server --bin server -- --serve .
```

| flag | meaning | default |
|---|---|---|
| `--addr ADDR` | listen address | `[::]:8080` |
| `--rooms DIR` | where rooms are saved, one file each | `rooms` |
| `--room NAME` | declare a room; repeatable | one called `main` |
| `--serve DIR` | the browser client: `index.html`, `pkg/`, `assets/` | none, so `/` 404s |
| `--bpm N` | generations a minute | 240 |
| `--fresh` | ignore every existing save | off |
| `--max-rooms N` | how many rooms players may make | 32 |
| `--torus RxC` | a world that wraps, sized in chunks | infinite |
| `--hide NAME` | a screen clients are asked not to offer; repeatable | none |
| `--api-token TOKEN` | mount the HTTP API at `/api`, for whoever sends this as a bearer token; `CK_API_TOKEN` in the environment is the same setting | not mounted |

That is the command for a machine somebody is sitting at. A host answering on a domain is one origin behind a Cloudflare tunnel, a systemd unit and a state directory to back up: [deploy/README.md](../deploy/README.md) brings one up from nothing, and [server.md](server.md#deploying) says what the server does differently there — `/healthz`, the cache headers, the token in the environment, and the header it reads to find out who a connection is from.

A room is a whole separate world — see [server.md](server.md#rooms). `--room` declares one, and every `<name>.ckw` already in the rooms directory is one too, so a restart keeps what a previous run was asked for. The first `--room` is where a client that names no room is put; with no `--room` at all that is `main`, which is created if it is not there.

A torus is at most **128 chunks a side and 1024 in total**. It is allocated whole, so the ceiling is what a server can step four times a second rather than what it can hold — about four million cells, which `examples/frametime` measures at roughly 41 nanoseconds each, and a chunk is 64 cells a side — and the same limit answers a room a *client* asks for, which is the path that used to take the process down: a shape arrives over the wire and `rows: 0` reached an `assert!` while `100000x100000` overflowed the multiply that sizes the allocation. `WorldKind::checked` is the one answer, and the command line and the socket both go through it. The release profile is `panic = "abort"`, so neither crash even unwound.

The two figures are **one budget divided two ways**, which is why they move when a chunk's size does: at sixteen cells to a chunk edge the total was 16384 chunks, and at sixty-four it is 1024, because a chunk holds sixteen times as many cells and a quarter of a second did not get longer. The per-side cap is the same division doing a second job — refusing a 1x1024 world that fits the budget and is a corridor nobody can play in.

A save is authoritative, so the shape a `--torus` asks for only applies to rooms that do not exist yet — the shape of a world is not something a flag can change after cells have been written into it. Restarting against an old world is the usual reason a change seems not to have taken; `--fresh` skips it, for every room at once. The startup log lists the rooms, their shapes and what is in them.

A room opens empty. There is no seeded pattern: the first life arrives with the first player, who is granted ground and a block on joining.

The server also reads its own terminal — `help` for the list, `new NAME [ROWSxCOLS]` to make a room without restarting, `bot add ROOM [LEVEL] [TEAM]` to seat a player the server plays, `stop` to save every room and shut down. So does SIGINT, and so does **SIGTERM**, which is what `kill`, `systemctl stop` and `docker stop` send. See [server.md](server.md#the-console).

A bot is a seat the server plays from a small book of shapes, and an outside program can play a seat too, through the HTTP API `--api-token` mounts. Both are in [server.md](server.md#bots).

`--span MS` is gone with it. A world's speed is **generations a minute** now — 250 milliseconds is four a second is 240 a minute, which is a number people can halve and double meaningfully, and passing the old flag says what to pass instead. It is also a *room's* rate rather than the server's: it rides on `net::Rules` beside `paused`, a laboratory's rules panel has a slider for it, and the server ticks on a fine grain while each room banks time against its own. Safe to change while a world runs, which almost nothing here is — the dice are seeded by the generation *number*, never a clock, so how fast generations arrive changes nothing any peer computes.

`--world PATH` is gone. A world is now one room among several, saved under its room's name, so the flag says a thing that no longer has a meaning; passing it is an error that says what to do instead. The file format is unchanged, so an old `world.ckw` becomes a room by moving it to `rooms/main.ckw`.

### The client

Both clients open on a **menu**: a name, a server, and — once that server has been asked — its rooms, with a "play alone" underneath. That is the only way to reach a server or choose a room without a command line, which is what a phone and a browser have.

| flag | meaning |
|---|---|
| `--ws URL` | go straight to this server, skipping the menu |
| `--room NAME` | which room on it; needs `--ws` |
| `--name NAME` | who to play as |
| `--torus RxC` | the shape of the offline world, for "play alone" |
| `--keep DIR` | where this client remembers things |

`--ws` skips the menu because an address on a command line is a choice already made. Connected, the server's room is the world and `--torus` is ignored with a note saying so; a `--room` the server does not have is refused, and the client falls back to the menu with the reason and that server's real room list on screen.

`--keep` is the store: a rejoin secret per room, plus the room, server and name last used, so the menu opens on what you used last. Two clients on one machine otherwise share it and try to be one player — give them a directory each.

The browser client needs no address; it derives its socket from the page's own origin. `?room=lobby` in the URL skips the menu and goes straight to that room, which is how a link takes somebody to a world.

Building needs **Rust 1.87 or newer** — `rust-version` in `Cargo.toml` says so, so an older toolchain is refused by name rather than by a parse error somewhere in the middle of the crate. Edition 2024 sets the floor at 1.85 and `is_multiple_of` on integers raises it to 1.87.

That matters more for the browser client than it looks, and the two are worth reading together:

`pkg/` is **generated and not committed** — it is in `.gitignore` — so a pull never updates it. A working copy keeps whatever `wasm-pack` last wrote there while `index.html` and the Rust move on, and nothing detects the mismatch. If a copy that used to work stops, and a fresh clone of the same commit does not, rebuild it before looking anywhere else — and read the output, because **a build that fails leaves the old `pkg/` in place** and the page then runs an old module against a new page:

```
rm -rf pkg && wasm-pack build --target web
```

The page shows a **loading bar** while the module arrives, and it is not decoration: the module is megabytes, and until this the page was black and empty for the whole of that. A blank screen is indistinguishable from a broken one, so the wait read as a client that could not reach the server rather than one that had not started. The bar needs the module's length to show a percentage and falls back to an indeterminate sweep without one. The server sends `Content-Length`, and sends the same number again as `X-Content-Length`, because an edge that compresses the module on the way through — Cloudflare does — takes the first and leaves the second; see [server.md](server.md#deploying).

Rebuild the browser client with `wasm-pack build --target web` — but **not while iterating**, because most of that wall clock is `wasm-opt` rather than the compiler. wasm-pack runs it over the whole module after wasm-bindgen, single-threaded and whole-program, and this module is large: wgpu, naga, winit and egui are all linked into it. It earns its ninety seconds for a build that ships, taking 12.1 MB down to 7.5 MB, and earns nothing for one served from localhost.

```
wasm-pack build --profiling --target web   # iterating: same codegen, no wasm-opt
wasm-pack build --target web               # shipping
```

The `--profiling` profile is release codegen with the second optimiser switched off, in `[package.metadata.wasm-pack.profile.profiling]`.

## Looking at it

`tools/typefaces.html` is the type bench: the hotbar and the HUD rebuilt at the sizes `views::theme` gives them, with a switcher for seven typeface pairings and the fifteen player colours with their closest pairs measured in OKLab. Open it in a browser — it builds nothing and talks to nothing. The numbers in it are copied out of the tree, so it goes stale if the tree moves.

## Formatting

`rustfmt.toml` exists because the tree was written to a style rustfmt's defaults disagreed with in about four hundred places, so anybody running `cargo fmt` — or an editor set to format on save — silently rewrote half the crate. Three settings recover what was actually being written by hand:

| setting | why |
|---|---|
| `max_width = 100` | what the code was already wrapped to |
| `use_small_heuristics = "Max"` | a call that fits on the line stays on it |
| `style_edition = "2021"` | keeps `{Cell, …, CHUNK_N}` import order; 2024 sorts uppercase first |

```
cargo fmt --check
```

Keep comments concise: replace five lines of comment with well-labelled code and a link to the documentation. [simplifying.md](simplifying.md) is the test to apply to one comment, where the cut material goes instead of the bin, and how to find the worst of it.

## Testing

```
cargo test                                             # every test there is
cargo check --features server --bins                   # and the one file they miss
cargo test --features server                           # the one test in it, over a real socket
cargo test --no-default-features                       # without the renderer
cargo build --target wasm32-unknown-unknown --lib      # the browser client
wasm-pack test --headless --firefox                    # GPU setup, in a browser
cargo run --example headless -- 400 infinite           # the simulation, no GPU
cargo run --no-default-features --example balance      # what manufacture pays, per pattern
cargo run --no-default-features --example territory    # what ground does, in numbers and shapes
cargo run --no-default-features --example blast        # what a stick turns over, and what that is worth against turrets and life
cargo run --example locker -- ws://127.0.0.1:8080/ws    # a library surviving the socket, over a real one
cargo run --example two -- ws://127.0.0.1:8080/ws       # two peers agreeing over a real one; LIE=1, OVERCLOCK=1
```

### Where the time goes

`cargo test` is about a minute and a half and two thirds of that is in two places, measured with `--test-threads=1`:

| | |
|---|---|
| `client::views::menu::tests` | 29 tests, over half the total |
| `sim::world::tests::a_torus_wraps_in_both_axes` | the single slowest test there is |

Both are **expensive for a reason that is written down where they are**, so neither is a sweep to trim. The menu tests go through `probe`, which presses a lane every 24 points across the whole screen rather than a fixed number of them; the comment on it names the three separate times a fixed count silently stopped finding the button it was meant to press. And a glider laps a torus in `4 * lcm(height, width)` generations, so a test that a glider *does* lap one has to run that many — the four shapes it sweeps are the degenerate one-chunk case, two square ones and a rectangular one, and only the rectangle proves the two axes wrap independently.

Next after those are `stepping_is_deterministic`, `a_peer_built_from_steps_agrees_with_the_server_with_a_bot_in_the_room` and `territory_creeps_across_a_chunk_boundary`, each a few seconds and each stepping a world a few hundred times in a debug build.

`server::ws` is the whole of what `cargo test` cannot reach: it is the only module behind `#[cfg(feature = "server")]`, and everything else under `src/server/` compiles by default. So a green run says nothing about that one file, and a rename that broke it once sat in the tree behind four hundred passing tests. The `cargo check` line above is the cheapest thing that would have caught it. The one test the file does have — `/healthz` answering over a real socket, with and without a page to serve — runs only under `cargo test --features server`, because there is no `tower` in the tree to drive a router without a socket.

[planned.md](planned.md) holds everything not built yet, with a status on each entry — built, being built, designed, or decided and not costed. [inspiration.md](inspiration.md) says where a design was borrowed from and for what.
