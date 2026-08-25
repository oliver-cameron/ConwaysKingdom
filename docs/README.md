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
| `--serve DIR` | static files at `/` | none, so `/` 404s |
| `--span MS` | milliseconds per generation | 250 |
| `--fresh` | ignore every existing save | off |
| `--max-rooms N` | how many rooms players may make | 32 |
| `--torus RxC` | a world that wraps, sized in chunks | infinite |

A room is a whole separate world — see [server.md](server.md#rooms). `--room` declares one, and every `<name>.ckw` already in the rooms directory is one too, so a restart keeps what a previous run was asked for. The first `--room` is where a client that names no room is put; with no `--room` at all that is `main`, which is created if it is not there.

A save is authoritative, so the shape a `--torus` asks for only applies to rooms that do not exist yet — the shape of a world is not something a flag can change after cells have been written into it. Restarting against an old world is the usual reason a change seems not to have taken; `--fresh` skips it, for every room at once. The startup log lists the rooms, their shapes and what is in them.

A room opens empty. There is no seeded pattern: the first life arrives with the first player, who is granted ground and a block on joining.

The server also reads its own terminal — `help` for the list, `new NAME [ROWSxCOLS]` to make a room without restarting, `stop` to save every room and shut down. So does SIGINT, and so does **SIGTERM**, which is what `kill`, `systemctl stop` and `docker stop` send. See [server.md](server.md#the-console).

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

Rebuild the browser client with `wasm-pack build --target web` — but **not while iterating**, because most of that wall clock is `wasm-opt` rather than the compiler. wasm-pack runs it over the whole module after wasm-bindgen, single-threaded and whole-program, and this module is large: wgpu, naga, winit and egui are all linked into it. It earns its ninety seconds for a build that ships, taking 12.1 MB down to 7.5 MB, and earns nothing for one served from localhost.

```
wasm-pack build --profiling --target web   # iterating: same codegen, no wasm-opt
wasm-pack build --target web               # shipping
```

The `--profiling` profile is release codegen with the second optimiser switched off, in `[package.metadata.wasm-pack.profile.profiling]`.

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

## Testing

```
cargo test                                             # everything
cargo test --no-default-features                       # without the renderer
cargo build --target wasm32-unknown-unknown --lib      # the browser client
wasm-pack test --headless --firefox                    # GPU setup, in a browser
cargo run --example headless -- 400 infinite           # the simulation, no GPU
cargo run --no-default-features --example balance      # what mining pays, per pattern
cargo run --no-default-features --example territory    # what ground does, in numbers and shapes
```

[planned.md](planned.md) holds everything not built yet, with a status on each entry — built, being built, designed, or decided and not costed. [inspiration.md](inspiration.md) says where a design was borrowed from and for what.
