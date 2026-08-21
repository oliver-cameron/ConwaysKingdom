# Documentation

| | |
|---|---|
| [architecture.md](architecture.md) | The module split, who depends on whom, and how the build enforces it |
| [simulation.md](simulation.md) | Cells, chunks, worlds, the rules, and the determinism contract |
| [rendering.md](rendering.md) | The pipeline, sprites, colour, and the camera |
| [game.md](game.md) | Value, placement, ice, and the controls |
| [networking.md](networking.md) | The protocol, prediction, and desync detection |
| [server.md](server.md) | Running it, and the save format |
| [gotchas.md](gotchas.md) | Things that cost a day, so they only cost one |

## Running it

The server serves the browser client and the websocket from one origin, so there is no second static-file server and no cross-origin question.

```
cargo run --no-default-features --features server --bin server -- --serve .
```

| flag | meaning | default |
|---|---|---|
| `--addr ADDR` | listen address | `0.0.0.0:8080` |
| `--world PATH` | save file | `world.ckw` |
| `--serve DIR` | static files at `/` | none, so `/` 404s |
| `--span MS` | milliseconds per generation | 250 |
| `--fresh` | ignore an existing save | off |
| `--torus RxC` | a world that wraps, sized in chunks | infinite |

A save is authoritative, so the shape a `--torus` asks for only applies when there is no file — the shape of a world is not something a flag can change after cells have been written into it. Restarting against an old world is the usual reason a change seems not to have taken; `--fresh` skips it. The startup log says what is actually there and where.

A world opens empty either way. There is no seeded pattern: the first life arrives with the first player, who is granted ground and a block on joining.

The native client takes `--ws URL`, `--name NAME`, and `--torus RxC` for an offline world that wraps. Without `--ws` it runs offline; connected, the server's world is the world and `--torus` is ignored with a note saying so. The browser client needs neither: it derives its socket from the page's own origin.

Rebuild the browser client with `wasm-pack build --target web`.

## Testing

```
cargo test                                             # everything
cargo test --no-default-features                       # without the renderer
cargo build --target wasm32-unknown-unknown --lib      # the browser client
wasm-pack test --headless --firefox                    # GPU setup, in a browser
cargo run --example headless -- 400 infinite           # the simulation, no GPU
```
