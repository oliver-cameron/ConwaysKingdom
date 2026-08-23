# Server

Holds the worlds, owns the tick, and assigns player numbers. Links `sim` and `net` and nothing else.

```
cargo run --no-default-features --features server --bin server -- --serve .
```

`Server::handle` takes a decoded `ClientMessage` and returns the replies, so whatever carries the bytes is somebody else's problem.

## Rooms

One `Server` is one **room**: one world, one player table, one tick. A process runs several side by side in a `server::rooms::Rooms`, so "the server" in the sense of the address you connect to is the `Rooms`, and a `Server` is one of the worlds behind it.

A room is a separate world rather than a view of a shared one, which is the simpler of the two things it could have meant and what "rooms" usually means. Nothing in `sim` had to learn that a world might be one of many, because a world never is. What it costs is that **territory, value, player numbers and the rejoin token are all per room**, so a player in two rooms is two players and their number in one says nothing about the other. `rooms::Seat` is the pair that does identify somebody: which room, and who they are in it.

**Rooms are declared, not conjured.** Joining a name nobody declared is refused, and the refusal names the rooms that do exist:

```
REFUSED — no room "loby" here; this server has arena, lobby
```

The alternative — creating a room for whoever asks — turns a typo into a world: `loby` gets you an empty plane where you own nothing, know nobody, and cannot tell that you are the only one who will ever be there.

The menu asks `ClientMessage::Rooms` and shows what comes back, so a name is normally clicked rather than typed. The rejection still carries the names, because a name can still arrive typed — `--room` on a command line, `?room=` in a link — and a client refused that way falls back to the menu with **both** the reason and the live list on screen. The two are halves of one answer, and showing only the list reads as the click having done nothing.

`Join` carries `room: Option<RoomName>`; `None` takes the server's default, so a client with nothing to say about rooms still lands somewhere. `Welcome` names the room back, because the client may have asked for none and because the token it is about to keep is filed under that name.

A room name is lowercase letters, digits, `-` and `_`, at most 24 characters, and is folded to lowercase. Narrow because the name is also the save file's name: a path separator in it would escape the rooms directory, and on a case-insensitive filesystem `Lobby` and `lobby` would be two rooms on one machine and one on another. `net::room_name` is the whole rule, and the client checks `--room` against it before connecting so a bad name is a message about the argument rather than a connection that opens and is turned away.

Every room runs on **one clock**. Separate worlds, but one generation span: a room with its own rate would be a second thing for a client to be told and a second way for the two to disagree about what a tick is.

There is one broadcast channel, and every message on it carries the room it came from; each connection drops what is not its own. One channel per room would save that comparison and cost a shared map of senders that connections and the simulation task would both have to lock — a lock, to avoid a string compare, on the one path that must not have one.

## Players

The lowest unused number, from 1. Zero is reserved for unowned cells, and the cell has five bits, so **31 players** is the capacity — a full server refuses rather than truncating a number into a cell.

Joins and departures are logged:

```
connection opened
join: PlayerId(1) "late" in room lobby at tick 73 (1 online)
subscribe: Some(PlayerId(1)) asked for 81 chunks, sending 4 that hold life
leave: PlayerId(1) "late" from room lobby after 10 ticks (0 still on)
```

The subscribe line is the useful one when a client sees nothing: it says whether the viewport is even pointed somewhere the server has data.

## Saving

Every 30 seconds and on a clean shutdown, every room at once. Writes go to a temporary beside the target and are renamed into place, so a crash mid-write cannot leave a half-written world where the real one was.

**One file per room**, `<name>.ckw` in the rooms directory. The format holds one world and its players, which is exactly one room, so the file is unchanged and the directory is what grew. The room's name is the file's name and is not written inside it: two places to keep one fact is one too many, and the one a person can rename is the one that has to win. Renaming `lobby.ckw` renames the room.

A player record carries their token, so a restart does not hand every returning player a new number and leave their ground standing there, theirs and unreachable.

A missing file starts fresh. A corrupt or mismatched one is an **error** naming the room that failed, not a silent reset — discarding a world is the worst possible response to a bad read, and with several of them "cannot read the world" no longer says which. A file in the directory that is not a room name is skipped with a warning rather than opened: refusing to start over one stray name would take every other world down with it. One room failing to save does not stop the others, for the same reason.

The shutdown save is **waited for**, not aborted. The simulation task saves after its loop, and the loop only ends once every sender is gone; `sim.abort()` cancelled it at its next await point, which is inside the loop, so the save never ran and a clean exit quietly lost up to thirty seconds of every room. The wait is bounded at ten seconds, because a shutdown that does not shut down is worse than one that loses a save it warned about.

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

Empty chunks are not written; they are implied — which is why a world is rebuilt **empty** and then filled from the file. Building a torus with `World::toroidal`, which seeds a glider, meant every chunk the file did not mention kept it, so loading a saved wrapping world invented five cells nobody had placed, in chunk zero, on every restart. The infinite arm was already `infinite_empty`, and that asymmetry is how it hid.
