# Networking

Both sides hold a copy of the world and run the same deterministic step. The client holds less of it — roughly its viewport and a margin — and advances it locally. The server is authoritative and is consulted only for what a client cannot derive:

1. other players' actions,
2. changes with no local cause,
3. chunks the client does not hold, when its viewport moves.

## Transport

WebSocket, over axum, serving the browser client and the socket from **one origin and one port**. No second static-file server and no cross-origin question.

One task owns the world and the tick; connections reach it through channels. That keeps the simulation single-threaded with a fixed ordering, which is what determinism needs — a mutex shared between connection tasks would make the order actions land in depend on scheduling.

The client's socket runs on its own thread with its own runtime and talks to the app over channels, because making a winit event loop async to accommodate a socket would be the tail wagging the dog. The browser has neither threads nor sockets to give tokio, so `net/link_web.rs` is a second implementation over `web_sys::WebSocket` behind the same `Link` surface: callbacks push into a queue the app drains each frame.

## Frames

Postcard over **binary** websocket messages. Binary because a chunk is raw cell bytes and a UTF-8 round trip would mangle it; postcard because it embeds no field names, so a `ChunkData` frame is barely larger than the chunk. Round-trip tested both directions, and garbage decodes to an error rather than a panic.

## Messages

```rust
enum ClientMessage {
    Join { name, token, room: Option<RoomName> },
    Act(Stamped),                              // an action, with player and tick
    Subscribe { chunks }, Unsubscribe { chunks },
    Checkpoint { tick, chunks: Vec<(ChunkId, u64)> },
    Rooms,                                     // what worlds are here?
}

enum ServerMessage {
    Welcome { you, tick, spawn, token, value, room, world: WorldKind },
    Rejected { reason },
    Step { tick, actions },
    ChunkData { tick, chunk, cells },
    Resync { tick, chunks },
    Rooms { rooms: Vec<RoomInfo> },            // name, players online, shape
}
```

`Rooms` and `Join` are the only two messages a connection with **no seat** may send, and for the same reason: neither names a world. A room is a world, so a player has to see the rooms before picking one, and asking from inside one is asking too late. Everything else from an unjoined connection is dropped rather than answered out of the default room.

A `RoomInfo` is enough to choose by and no more — the name, how many players are connected *now*, and whether the world ends. Not the tick and not the chunk count: neither says anything about what it is like to be in there. The order is the server's, so two players looking at the same menu see the same list in the same order.

A chunk is identified by **where it is** — `type ChunkId = Coord`. There is no id to allocate, keep unique, or reconcile after a reconnect; two peers naming the same coordinate mean the same chunk. On a torus, fold with `World::canonical` before comparing.

Actions are stamped with a player and a tick, so replay lands at the same point in the sequence on every peer. They are not raw keystrokes: input resolves to a world effect before it goes on the wire, so the server validates an intent rather than replaying a keyboard.

## Joining

`Join` names a **room**, or names none and takes the server's default. A room is a separate world, so this decides which cells the player will ever see; see [server.md](server.md#rooms).

`Welcome` names the room back and carries **`world: WorldKind`** — the shape of it, and how big if it wraps. The client builds that world rather than assuming a plane. It is not something a client can derive: nothing it can see says whether the ground ends, and a client that assumed an infinite world against a wrapping server folded no coordinates, so chunks the server called one were several to the client, digests were taken against coordinates the server had never heard of, and the seam showed the moment anything crossed it.

Subscriptions are folded with `World::canonical` before they are sent, for the same reason. On a wrapping world the viewport runs off the edge and comes back, covering one chunk under several global coordinates — and a `Resync` names the folded one, so asking under the unfolded name would subscribe several times to one chunk and then fail to match the name the server used when it said that chunk was wrong.

On `Welcome` the client **adopts the server's tick**. That is not cosmetic — a birth's owner is seeded from the generation, so a client simulating at a different tick would make different choices from identical cells and desync immediately.

It then subscribes to the chunks its viewport covers plus a margin, so life entering from off screen is already held rather than popping in late.

A client always opens with a world to look at and replaces it on `Welcome`. A socket object exists long before it connects and may never connect at all, so its mere existence is no reason to blank the view — a client that never connects would otherwise sit on an empty world and look broken.

## Prediction and desync

The client applies its own actions immediately **and** sends them, rather than sending and awaiting. The rules are deterministic and the server runs the same `net::apply` and charges by the same `net::value_delta`, so acting immediately shows the right answer a round trip early.

`Checkpoint` carries **per-chunk** digests. A whole-world digest could never have worked: a client holds only what its viewport covers, so it would disagree every time. The server answers with just the chunks that differ, and silence means agreement.

It digests **the chunks the client has asked for**, which is not the same as the chunks its world contains. A torus is allocated whole, so every chunk exists from the moment the world is built, and digesting all of them meant claiming to hold hundreds that had never been sent. They read as empty, the server disagreed with every one, and answered with a `Resync` naming the lot — every checkpoint interval, until the whole world had been dragged across. An infinite world hid it, because there a stored chunk is one that was fetched or grown.

`cargo run --example join -- ws://…` joins, takes what the server sends, checkpoints it back, and prints MATCH or the chunks that disagree.

## The server is the clock

`ServerMessage::Step { tick, actions }` goes out once a generation, even a quiet one. A connected client applies the actions and then advances to `tick` — it does **not** step on its own clock.

That is not a refinement, it is the whole of whether multiplayer works. A step is a pure function of state and tick, and every seed is derived from the generation, so two peers stay identical only while they step at the same ticks. A client that kept its own timer drifted immediately: same nominal rate, different phase, nothing correcting it. Measured at four generations apart within a minute, and growing. Births then chose different owners on each side and territory spread differently, so the two worlds separated while both looked plausible. Late joining still worked, because that is a snapshot — everything after it was one world each.

A client that finds itself somewhere other than one step behind says so and takes the server's number. Papering over it quietly would hide exactly the case worth knowing about, since by then the worlds have already diverged.

## Predicting, and finding out when it was wrong

A client applies its own action straight away, connected or not, so what you draw appears under your hand. The rules are deterministic and the server runs the same `net::apply`, so acting immediately shows the right answer a round trip early.

Usually. The server applies it whenever the message lands — this generation if it arrives before the next step, the one after if it arrives later — so a click is a coin flip, and on the losing side that client has evolved those cells a generation earlier than everyone else. Waiting for the server instead would remove it, at the cost of a quarter of a second before you see your own cells, which is a poor trade for something rare.

So it is found rather than prevented. Every few seconds a client sends a **`Checkpoint`**: one FNV-1a digest per chunk it holds, stamped with the generation they were taken at. A chunk is 512 bytes and its digest is eight, so a whole world's worth of state fits in a message that costs nothing — which is what lets agreement be checked constantly, with only the chunks that actually disagree ever sent back.

The server compares against its own chunks and answers `Resync` with the ones that differ; the client asks for those again at once rather than waiting for the viewport to notice, because a wrong chunk off screen is still wrong. A checkpoint from any tick but the server's current one is ignored rather than answered wrongly — comparing against the wrong generation would disagree for a reason that is not a bug — and the next checkpoint is only seconds away.

Measured with `examples/two.rs`, which runs two peers over the real protocol and compares digests every shared generation. Plain, they agree on all of about four hundred. With `LIE=1` one peer invents a block nobody sent it — a still life, since a lone cell dies of loneliness and heals the lie before anyone notices — and the disagreement is found and put right within a checkpoint interval, nine to eleven generations.

## Coming back

`Welcome` carries the player's **value** as well as their number and ground. A returning player has a value already and the client cannot know it; assuming the starting figure left the two disagreeing from the first frame, with the client offering to spend money the server knew was gone and the server refusing the difference silently.

`Welcome` hands out a **token**: a random 128-bit secret the client keeps, in `localStorage` in a browser and under `$XDG_DATA_HOME/conwayskingdom/tokens/` natively. Present it on a later `Join` and you get your player back — the same number, the same value, the same ground.

It is filed **under the room**. A room is a separate world with its own player numbers, so one secret for the whole server would offer a token minted in one world to a server that keeps its players in another, where it matches nobody, joins you as somebody new, and overwrites the token that would have got you back. A token that returns you to the wrong room is worse than no token at all.

That leaves the case of a client naming no room: it cannot look up the right secret before it has been told where it is going, and by the time it is told it has already joined. So it offers **the last room's**, which is nearly always where it is about to be put, and a wrong guess costs nothing that was not already lost — an unrecognised token joins you as somebody new, exactly as having none would.

Not keyed by *server*, though, and that is a gap rather than a decision: two servers both running a room called `main` share one secret, and visiting the second costs you your player on the first. The address a client typed is not remembered anywhere yet, and when it is, it belongs beside this.

It is not authentication. It proves nothing to anybody else, and whoever holds it *is* that player. That is the right strength for a game with no accounts: what it buys is that a dropped connection is not a new life. A name would not do, since two players may pick the same one and anybody could claim yours. An IP address would be worse — two people in a house share one, and a phone changes its own between reconnects.

**A player number is never reused.** It used to fill the gap a departing player left, which was harmless when a number only meant some live cells. It is not harmless now: territory *is* the owner field, so handing a number on hands over everything that player claimed, and the ground outlives the connection. A player who leaves is marked gone and kept. Thirty-one numbers is therefore a limit on players a world has ever seen rather than on players connected at once, and coming back is what the token is for.

A token nobody holds is not an error — it joins you as somebody new. Anything else would lock a player out over a stale file. **Nor does a token in use bring you back**: two clients on one machine share a token store and two browser tabs share one storage, so a token whose player is already connected also joins you as somebody new. Nobody may be two people at once, and nobody may be one person twice — without that rule the second player to arrive simply becomes the first, which is not a multiplayer game but one player with two windows.

Running two clients on one machine as two people therefore wants `--token DIR` on each — a directory, one file per room — so they keep their secrets apart and both can come back.
