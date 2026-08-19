# ConwaysKingdom

Conway's game of life, weaponised. An unbounded shared world where players own the cells they place, mine them back for value, and wall each other off with ice.

Runs natively and in a browser, against a server or alone. The simulation is deterministic, so an unconnected client is a complete game rather than a broken one.

```
cargo run --no-default-features --features server --bin server -- --serve .   # server + page
cargo run --bin native -- --ws ws://127.0.0.1:8080/ws                         # native client
```

Then open <http://localhost:8080/>. The browser client connects back to whatever served it, so there is nothing to configure.

Documentation is in [docs/](docs/) — start with [docs/README.md](docs/README.md).
