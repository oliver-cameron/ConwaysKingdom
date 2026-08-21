# Server

Holds the whole world, owns the tick, and assigns player numbers. Links `sim` and `net` and nothing else.

```
cargo run --no-default-features --features server --bin server -- --serve .
```

`Server::handle` takes a decoded `ClientMessage` and returns the replies, so whatever carries the bytes is somebody else's problem.

## Players

The lowest unused number, from 1. Zero is reserved for unowned cells, and the cell has five bits, so **31 players** is the capacity — a full server refuses rather than truncating a number into a cell.

Joins and departures are logged:

```
connection opened
join: PlayerId(1) "late" at tick 73 (1 online)
subscribe: Some(PlayerId(1)) asked for 81 chunks, sending 4 that hold life
leave: PlayerId(1) "late" after 10 ticks (0 online)
```

The subscribe line is the useful one when a client sees nothing: it says whether the viewport is even pointed somewhere the server has data.

## Saving

Every 30 seconds and on a clean shutdown. Writes go to a temporary beside the target and are renamed into place, so a crash mid-write cannot leave a half-written world where the real one was.

A player record carries their token, so a restart does not hand every returning player a new number and leave their ground standing there, theirs and unreachable.

A missing file starts fresh. A corrupt or mismatched one is an **error**, not a silent reset — discarding a world is the worst possible response to a bad read.

### The `.ckw` format

Hand-rolled and little-endian. A chunk is already a flat byte array, so the file is mostly a memcpy, and owning the format means a version byte can gate a migration.

```
"CKW\0"                    magic
u8                         version (currently 2)
u8 u8                      chunk edge, cell width
u8                         world kind: 0 infinite, 1 toroidal
[i32 i32]                  rows, cols — toroidal only
u64                        tick
u32                        chunk count
  i32 i32                  coordinate            } repeated, sorted,
  [u8; N*N*cell]           cells, verbatim       } only chunks holding something
u32                        player count
  u8                       id                    }
  u16 + bytes              name                  } repeated
  u64                      last seen             }
  i32                      value                 }
```

The header records chunk size and cell width, and a file written by a build with different ones is **refused** rather than read as garbage — the shapes are exactly what would otherwise decode into plausible nonsense.

Empty chunks are not written; they are implied.
