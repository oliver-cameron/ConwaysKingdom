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
    Join { name, room: Option<RoomName>, person: Option<Secret> },
    Act(Stamped),                              // an action: tick, player, seat
    Subscribe { chunks },                      // send me these; a fetch, not a standing order
    Checkpoint { tick, chunks: Vec<(ChunkId, u64)> },
    Rooms,                                     // what worlds are here?
    Create { name, shape, victory, teams, private, laboratory, party: Option<PartyId> },
    JoinTeam { team }, NameTeam { team, name },
    AddBot { team, level }, RemoveBot { seat },   // a seat the server plays; see server.md
    Hello { name, person: Secret },            // who I am, before any room
    Close { room },                            // a room I made, once it is empty
    Invite { who, room },                      // hold this private room's door open for them
    Parties, MakeParty { name }, InviteToParty { party, who }, JoinParty { party }, LeaveParty { party },
}

enum ServerMessage {
    Welcome { you, person, tick, spawn, value, room, world: WorldKind },
    Rejected { reason },
    Step { tick, actions },
    ChunkData { tick, chunk, cells },
    Resync { tick, chunks },
    Rooms { rooms: Vec<RoomInfo> },            // name, players online, shape, owner
    Purse { value },                           // what you actually have
    You(Profile),                              // who this server says you are
    Closed(Result<RoomId, String>),
    Invited { from: Profile, room, name },
    NotDone { reason },                        // it would not, and you are where you were
    Parties { parties: Vec<PartyInfo> },       // yours: people, who is online, its worlds
    PartyInvite { from: Profile, party, name },
}
```

`Stamped` carries a **`seat`** beside its `player`: who sent it, as against what number its cells carry. Those were one question until a team became a player and several clients started sharing that number — and a client skips its own actions when they come back, so with only `player` to go on it skipped its *teammates'* actions too and never applied them at all. The server checks both, because a client that could name any seat could act as a teammate, and one that could name any player could put its cells under another team's number.

`EndMatch` and `Forfeit` are the two ways a match ends that are not its own condition; see [server.md](server.md#ending-one).

**`Acted` is one action the moment the server takes it**, rather than at the end of the generation it belongs to. The client that *made* it never waited — it predicts — and everybody else waited half a tick on average for news the server already had: 129 ms on loopback with a 250 ms tick, of which 4 ms was the link. Other clients apply it at once, which is the same prediction the actor already made.

The `Step` still carries it, because a broadcast can be dropped — `server::ws` logs `connection lagged` and carries on — so this is a shortcut and not a replacement. A client records what it applied early and skips those in the `Step`, or a `Paint` lands twice: idempotent on the generation it was meant for and not on the one after.

The condition for applying one is **being caught up**, not the tick the action names. `stamped.tick` is the actor's guess; the server applies whatever is pending on the step it happens to be on, which is the generation every caught-up client is already on. Guarding on the tick was the first attempt and cost a whole generation whenever an action was made near a boundary — measured at a median of 129 ms before, 9.7 ms with the tick guard, and 8.8 ms without it, flat across tick spans where it used to track them.

`Purse` rides on every `Checkpoint` reply. Value used to be predictable from a client's own actions alone, so the number on screen was the number the server would agree with. Manufacture broke that: earnings depend on births *anywhere* in the world and a client holds a viewport, so its guess is always low and always getting lower, and nothing else would ever correct it. The machinery for "your copy is wrong, here is mine" already exists and runs every few seconds, so value uses it rather than growing a second one. The cost is that an action sent for the current tick and not yet applied shows for a moment as money still in hand.

**`Subscribe` is a fetch.** The server answers with the chunks it holds and forgets the request: a chunk *change* reaches a client as the `Step` for the generation it happened in, broadcast to everybody in the room, so there is no push for a subscription to select from. There was an `Unsubscribe` beside it and a per-player list on the server for it to remove from; nothing ever read the list and no client ever sent the message. The list was unbounded, undeduplicated and grown by every resync, and removing from it was an `O(n*m)` scan over attacker-sized input on the one task that owns every room. One `Subscribe` is capped at 4096 chunks, because the reply is unbounded and goes into an unbounded channel — on a torus, where every chunk exists, a few kilobytes of request was half a gigabyte of `ChunkData`.

**A shape off the wire is checked before anything is built out of it.** `Create` carries a `WorldKind`, and a torus is allocated whole — so `rows: 0` reached an `assert!` and `100000x100000` overflowed the `i32` multiply that sizes the allocation, either one killing the simulation task and with it every room in the process, from one message on a connection that had not joined anything. `WorldKind::checked` is the single answer to what a torus may be, and every path from a client to `build` goes through it, including `net::sane_world` for a client being told the shape by a server it should trust about the shape and not about the size.

`Rooms`, `Join` and `Create` are the messages a connection with **no seat** may send, and for the same reason: neither names a world. A room is a world, so a player has to see the rooms before picking one, and asking from inside one is asking too late. So are `Profile`, `People`, `Challenge`, `Close` and everything about a party, which are about people rather than worlds. Everything else from an unjoined connection is dropped rather than answered out of the default room.

A `RoomInfo` is enough to choose by and no more — the name, how many players are connected *now*, whether the world ends, and whose it is by key when a keyed player made it. Not the tick and not the chunk count: neither says anything about what it is like to be in there. The owner is there so a menu can offer to close your own rooms and nobody else's, and it is as public as the fingerprint a lobby already shows beside a name. The order is the server's, so two players looking at the same menu see the same list in the same order.

## Before a seat

**A client on the menu is somebody.** `Join` carries the secret, so a seat has always come with a person; nothing else did, and a connection that had opened the page and joined nothing was nobody to the server — a challenge queued for it sat in the outbox until it joined a room, and nothing filed against a person could be answered to it. `Hello { name, person }` is the meeting a `Join` does with the room left off. It is answered with `You(Profile)`, which is the one reply the **socket reads as well as the client**: it tells the connection which person it carries before a `Welcome` has, and a `Profile` somebody looked up must not be taken for it. Whatever was waiting for that person rides out with the answer rather than with the next room list. The client says it on every connect, before its first `Rooms`, and on a link straight into a room, so a spectator is somebody too.

The name rides with it for the reason it rides on a `Join`: it is the one thing a profile takes a client's word for, and a person met by hello and nowhere else would otherwise have none. This is the pre-seat state the keypair handshake will need — see [planned.md](planned.md#what-doing-it-actually-costs-in-order) — arrived at without a signature: a `Hello` is where the signed presentation goes when there is one.

`NotDone { reason }` is the refusal for an invitation and for the party verbs. `Rejected` closes a door on a connection — the client shows the menu — and an invitation refused from inside a room has to leave you in it, the way `NotStarted` leaves you in a lobby. A challenge's refusals are still `Rejected`, as they were before there was a `NotDone`; a challenge is asked of a person too, and moving it is a client change as much as a server one.

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

A client that finds itself somewhere other than one step behind **throws away its world and asks for it again.** It used to step forward to close a gap of up to thirty-two generations, which reads as recovery and is the opposite: a `Step` carries the actions applied at its tick, so a gap is not "we are behind by n", it is "n generations happened that we were never told the contents of" — and stepping to close it runs those generations empty. The world that comes out is one nobody else has, and Life turns a handful of missing cells into a different pattern within a minute.

The gap is real and not hypothetical. A websocket does not lose or reorder, which is what made catching up look safe, but the broadcast channel in front of it does: `server::ws` logs `connection lagged n messages` and carries on, and a client whose socket is slow to drain — a backgrounded tab throttles exactly that — is the ordinary case rather than an exotic one. So the whole world goes, not the chunks that look wrong: every chunk was stepped alongside every other, one that missed an action has been feeding wrong cells across its edges ever since, and a chunk outside the viewport is never checkpointed at all, so "the ones we know are wrong" is a set the client cannot compute.

It self-limits — the generation is the server's afterwards, so the next `Step` is one past it — and it looks like a join, because it is one.

What is still missing is the other half: the server knows a connection lagged and does not tell it, so the client only finds out on the next `Step` it does receive.

## Predicting, and finding out when it was wrong

A client applies its own action straight away, connected or not, so what you draw appears under your hand. The rules are deterministic and the server runs the same `net::apply`, so acting immediately shows the right answer a round trip early.

Usually. The server applies it whenever the message lands — this generation if it arrives before the next step, the one after if it arrives later — so a click is a coin flip, and on the losing side that client has evolved those cells a generation earlier than everyone else. Waiting for the server instead would remove it, at the cost of a quarter of a second before you see your own cells, which is a poor trade for something rare.

**A client does not apply its own actions when they come back** — its own by `seat`, not by `player`. It predicted them; applying them again is not a no-op, because a `Paint` is idempotent on the generation it was meant for and not one generation later. By then the cells it named have moved, and laying them again stamps the original pattern back on top of where it went.

The symptom is unmistakable once you have seen it: draw a glider, watch it thicken into a blob and settle into a honey farm, and watch it snap back to a glider a few seconds later when the resync lands. It needs latency to happen at all — the action has to miss one server step, which on a loopback socket it never does — so it is a browser and a real network problem and invisible locally.

Skipping them leaves the phase error prediction has always had, the same cells a generation out, which the checkpoint puts right. `a_paint_applied_late_is_not_the_paint_you_asked_for` pins the difference: five cells either way when the client skips, and more than five when it does not.

So it is found rather than prevented. Every few seconds a client sends a **`Checkpoint`**: one FNV-1a digest per chunk it holds, stamped with the generation they were taken at. A chunk is 8192 bytes and its digest is eight, so a whole world's worth of state fits in a message that costs nothing — which is what lets agreement be checked constantly, with only the chunks that actually disagree ever sent back.

The server compares against its own chunks and answers `Resync` with the ones that differ; the client asks for those again at once rather than waiting for the viewport to notice, because a wrong chunk off screen is still wrong. A checkpoint from any tick but the server's current one is ignored rather than answered wrongly — comparing against the wrong generation would disagree for a reason that is not a bug — and the next checkpoint is only seconds away.

Measured with `examples/two.rs`, which runs two peers over the real protocol and compares digests every shared generation. Plain, they agree on all of about four hundred. With `LIE=1` one peer invents a block nobody sent it — a still life, since a lone cell dies of loneliness and heals the lie before anyone notices — and the disagreement is found and put right within a checkpoint interval, nine to eleven generations. With `OVERCLOCK=1` the peer that paints also stands a block of overclockers over its blinker, so the second pass runs on both sides of the socket and the same comparison covers it.

## The reading, not the bang

A `Resync` is an event: a log line, a refetch, silence. That says nothing about whether the last one was an isolated hiccup or the fourth this minute, and the difference is the whole diagnosis — prediction makes a generation of disagreement normal *by design*, so "did we disagree" has never been the useful question. "How often, and is it settling" is.

So the client keeps a **geiger counter**, `client::desync::Geiger`. Every chunk a `Resync` names is one click, clicks decay with a half-life of twelve seconds, and what the HUD shows is the rate. Per chunk rather than per message, because a resync naming forty chunks is a world being rebuilt and one naming a single chunk is one prediction that missed, and counting messages would make those the same event.

Twelve seconds is about three checkpoint intervals at four generations a second: a burst that stops is visibly falling by the next checkpoint and gone by the third, which is fast enough to read as "settled" and slow enough that two hiccups a few seconds apart still add up. Decay is a function of **elapsed time and not of how often it is looked at**, which matters more than it sounds — a client in trouble is usually a client dropping frames, and a rate that fell per frame would read lowest exactly when it should read highest. There is a test for that.

It decays every frame rather than only when something arrives, or it would sit at its peak until the next resync. It is cleared on `Welcome`, because a different room is a different world and a different argument about it.

What reaches the screen is one word beside "connected", which is the claim it qualifies: a link that is open and a link that is keeping up are two facts, and only the first was ever on screen. Silence until there has been something to be silent about — a link that has never slipped says nothing at all, and one that has slipped and settled says so, because a rate back at nought and a link that was never in trouble look identical and are not the same thing.

## Which vocabulary these bytes are in

One byte on the front of every frame — `codec::PROTOCOL` — and it is there because nothing detected a mismatch.

Postcard writes an enum variant as its **index**, so a message inserted in the middle of `ClientMessage` renumbers every one after it, and a field added to a struct that rides on one changes that message's shape. Both are ordinary changes. What made them dangerous is that the browser client is a generated `pkg/` that **a pull does not update** — see [gotchas.md](gotchas.md) — so a page from last week talks to a new server and the frames decode to *something*. A join half works, a profile comes back empty, and the only sign is a warning in a log nobody is reading.

Now a mismatch is a `Rejected` with the reason on it, which is the one thing here that already reaches the screen. Bump `PROTOCOL` whenever the vocabulary moves. Appending a variant is still the safe change, and every message added since — `AddBot` and `RemoveBot`, then `Hello` and everything under it in `ClientMessage`, and `You` and everything under it in `ServerMessage` — was appended.

**It is 2, moved once for all of them, and not because of the messages.** Three structs that ride on frames changed shape, and any one of them would have needed the bump: `Seat` gained `bot`, so a page from before it reads the flag as the start of the next seat and every lobby is misread; `RoomInfo` gained `owner`, which does the same to a room list; and `Create` gained `party`. A bot's own actions need nothing new — they go out in the `Step` for the generation they were taken in, from a seat like anybody's, which is what makes a bot cost the protocol one bit. See [server.md](server.md#bots).

## Coming back

`Welcome` carries the player's **value** as well as their number and ground. A returning player has a value already and the client cannot know it; assuming the starting figure left the two disagreeing from the first frame, with the client offering to spend money the server knew was gone and the server refusing the difference silently.

**Coming back is coming back as yourself.** A client keeps a [`Secret`](server.md#who-somebody-is) — sixteen random bytes, made at startup — and sends it on a `Join`. The server exchanges it for a `PersonId` and finds the seat that person already holds in that room: the same number, the same value, the same ground.

There was a **rejoin token** here until recently, and its going is worth recording because the reasons it was hard are the reasons a person is better. A token said *which seat in which room*, so it had to be filed per room — one secret for the whole server would have offered a token minted in one world to a server keeping its players in another, where it matched nobody, joined you as somebody new, and overwrote the one that would have got you back. It was also not keyed by *server*, so two servers running a room called `main` shared one and visiting the second cost you your player on the first. And a client naming no room had to guess which token to offer, because it could not know where it was going until it had already arrived.

A secret says *who*, and the seat follows from that. One secret for everything, no room in the key, nothing to guess, and the per-server hole closed by there being nothing per-room to collide.

**A person is not two players**, and this is the one place the new rule is stricter. A token whose player was already connected quietly handed out a *new* player — honestly, since a token named a seat and two tabs sharing one were two seats. A second join from a person who is already in the room is **refused**, with a reason: somebody who has carried their secret to another machine wants to be told, and being handed a stranger's seat four hundred generations into a match is not being told.

It is still not authentication. Whoever holds the secret is you, which is the right strength for a game with no accounts — a name would not do, since two players may pick the same one, and an IP address would be worse, since a house shares one and a phone changes its own between reconnects.

A person this room has never seated is a **new player**, not an error. Anything else would lock somebody out of a room they have not been in.

A client with **no** secret still plays, as somebody new every time. That is the honest outcome for a browser with storage switched off rather than a reason to refuse to let anybody in.

Running two clients on one machine as two people therefore wants `--keep DIR` on each, so they keep different secrets — otherwise they are one person, and the second is told so.

**A player number is never reused.** It used to fill the gap a departing player left, which was harmless when a number only meant some live cells. It is not harmless now: territory *is* the owner field, so handing a number on hands over everything that player claimed, and the ground outlives the connection. A player who leaves is marked gone and kept. Thirty-one numbers is therefore a limit on players a world has ever seen rather than on players connected at once, and coming back is what a person is for.

