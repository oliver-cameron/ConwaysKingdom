# Known bugs

Things that are wrong, or are probably wrong, and are not fixed. Written down because a bug nobody has recorded is a bug somebody rediscovers.

Each entry says **what it is**, **what you would see**, and **why it is still here**. A few were found by reading and never reproduced; those say so, because "I think this is broken" and "I made this break" are different claims and only one of them is evidence.

For bugs that *were* fixed, the reasoning lives in [gotchas.md](gotchas.md) — that file is the record of what a symptom turned out to mean, and it is the one to read when something looks familiar. [Fixed](#fixed) at the bottom holds the few that were closed by a decision rather than by a chase, where there is a number to explain and no symptom to recognise.

## Confirmed

### A player who was away at the whistle plays a team match on no team

`teams_are_fair` refuses to start a match with anybody unplaced, but it only looks at players who are **online**. Somebody who joined the lobby, dropped, and comes back during the match is admitted — the returning-player gate in `Server::handle` is deliberate and right — and they are on no team, so they play as themselves against the teams.

Reproduced: three players in a two-team match, one offline at the whistle, rejoining after it. They are welcomed, granted their own patch, and `plays_as` is their own number.

*You would see:* a lobby listing two teams and one player under neither, and a scoreboard with three competitors in a two-team match.

*Still here because* it is not obviously a bug rather than a missing rule, and the two candidate rules disagree: put them on the smallest team, or make them a spectator. The second is what [planned.md](planned.md) already wants for the mercy rule, and doing it once for both is better than doing it twice.

*Not* dangerous any more. It used to seat them on top of a team's opening and take it — `seat_number` drew team ids and player numbers out of one 1..15 space — which is fixed.

### The help screen's monospace column misaligns on a wide glyph

`help.rs` pads each keycap with `format!("{key:widest$}")`, which counts **characters**. A learned label containing a full-width glyph counts as one character and draws two columns wide.

*You would see:* the two columns of the key list stepping out of line by a character on the rows whose labels were learned from a keyboard with wide glyphs.

*Still here because* it needs a width table rather than `chars().count()`, and the layouts that produce wide keycaps are the input-method ones this client does not otherwise support.

### Loaded chunks read differently from the backdrop

Ground the client holds is very slightly brighter or darker than ground it does not, so the edge of what has arrived is visible as a faint patchwork — on empty ground, where the two are drawing the same thing.

*You would see:* a rectangular seam that moves as chunks arrive, most obvious when zoomed out and over ground nobody owns.

*Still here because* the fix is not small, though **the mechanism written down here has since gone** and what is left is not measured. Both paths draw the same dead cells and arrive at "which cell is this pixel" by different arithmetic: a loaded chunk interpolates `local` across its own quad, and the backdrop derives it by wrapping the world position onto one shared layer. At one pixel per cell the two agree. Below that, one pixel covers several cells and `textureLoad` picks exactly one of them, so the two routes round differently.

What made that rounding a *brightness* difference was a faint ring drawn round every chunk's outer cells: the two routes landed on a different proportion of ring pixels. The ring is gone — it was making a grid at chunk pitch over the whole board, which was the worse of the two — so the only thing left to differ over is the transparent texel between sprites. Whether the seam is still visible has not been checked since.

It is one symptom of point-sampling at low zoom rather than a fault of its own, so the fix is [zooming out without lying](planned.md#zooming-out-without-lying) — or the zoom floor that made the case not arise. Reported from reading the shader rather than from measuring it, which is why the ring going took the explanation with it and left the entry: what is confirmed is that the two paths compute the cell differently and that the sheet sampler is `Nearest`, so it is not the sprite atlas bleeding. **Somebody should look at a screen before this is written up again.**

## Likely, from reading

These have a mechanism and a code path and were not made to happen.

### Shift can stick down if it is released into a text field

`GameApp` tracks shift from `KeyCode::ShiftLeft | ShiftRight` in `on_key`, and `render::app` only calls `on_key` for events egui did not consume — `Views::on_window_event` returns `wants_keyboard()` for every key press while a field has focus.

So: hold shift over the world, open the menu, release shift into a field. The press was seen and the release was not, and `self.shift` stays true until the next shift press-and-release outside a field. `Focused(false)` clears it, but an in-app focus change is not that.

*You would see:* the digit keys picking tools instead of stamps, and panning at the hurried speed, after visiting the menu.

*Not confirmed* because it needs a window and a focused field, which is exactly the state the tests cannot reach. It is a good argument for the modifier state living in `Views`, which sees every event, rather than in the app, which sees the ones egui did not want.

### A dead socket's outbox grows without limit

`net::link_web::Link::send` queues into `outbox` whenever the socket is not open, and `open` is never set back to false when it closes. A client that keeps sending after a close — a checkpoint every few seconds, a subscribe per camera move — accumulates encoded messages nobody will ever send.

In practice `pump_link` sets `self.link = None` on the frame it notices `is_closed`, so the window is short. The one case that is not short is a socket that never opens and never errors, which is what a hung connection through a proxy looks like.

*Still here because* the deep-link timeout added in the same pass bounds it: eight seconds, then the link is dropped. It should still be a bounded queue.

### An encoding failure is reported as a successful send

`server::ws::send` returns `true` — meaning "the connection is fine" — when `encode_server` fails. That is probably right, since a message the server could not serialise is not the connection's fault and killing it would be worse. It is the one branch in that function with no comment saying so, which is how a deliberate decision becomes an accident later.

### The native client polls its socket every 8 ms

`net::link::pump` drains a synchronous channel, then `select!`s the socket against an 8 ms sleep, so it wakes 125 times a second whether or not anything is happening. It also means an outbound message waits up to 8 ms.

Not a bug so much as a shape: the outbound half wants a `tokio::sync::mpsc` the select can await, and then there is no timer at all.

### A table written in place is a table a crash can shorten

`people.jsonl`, `profiles.jsonl`, `stamps.jsonl` and `games.jsonl` are saved with `std::fs::write`, which truncates and then writes. A world is not — `persist::save` writes beside the file and renames it into place, so a crash leaves the old world or the new one and never half of one — and the four tables beside it never learned the trick.

What it costs is bounded by the format rather than by the write, which is [what the format was chosen for](server.md#the-four-tables-beside-the-rooms): one object a line means a crash mid-write leaves the rows that got there and loses the rest, and a half-written last line is skipped on the way back in. So it is the tail of a table rather than the table.

*You would see* nothing until somebody who had played here arrived as a stranger — a lost row in `people.jsonl` is a person whose profile, patterns and record are filed under an id nobody holds any more.

*Not confirmed,* and confirming it would take a power cut in a window of milliseconds a few times an hour. The fix is the trick `persist::save` already knows, applied to the four writers that do not.

### The tunnel is not in this repository

`agent.py` and `relay.py` are what make a server on a home connection reachable, and they sit beside this repository rather than in it — so they are not in the tests, not in `cargo fmt`, and not in any history. The pool bug in [gotchas.md](gotchas.md#one-browser-is-six-connections-and-the-tunnel-counted-players) was a bug in the browser client as far as anybody debugging it could tell, and there was nowhere to record the fix.

Either they belong in `tunnel/` here or they belong in a repository of their own. What they should not be is untracked files next to a tracked project that documents them. `design-notes/` had the same problem and is now in the repository, which is the shape of the answer.

**Superseded, and the answer turned out to be neither.** `cloudflared` is what makes the server reachable now — one outbound connection to Cloudflare, TLS at the edge, no port forwarded and nothing listening on the open internet — so the two scripts have no job left. What is in the repository is the ingress rule that replaces them, [deploy/cloudflared.yml](../deploy/cloudflared.yml), and [server.md](server.md#deploying) says how it is run. The pool bug is still worth reading: **anything that pools connections for this game has to be sized in connections and not in people**, and an edge in front of a home connection has not changed that.

## Not bugs, but the next thing to go wrong

### A match is never saved, and now the teams go with it

`Rooms::save` skips any room that is not `Phase::Open`, deliberately — "a half-finished match restored into a server that has forgotten it was a match would run on forever with nobody able to win it". That was already true of the phase and the victory condition. It is now also true of `Player::plays_as`, which is not in the save format.

So this is consistent rather than broken: matches do not survive a restart, and nothing about a team pretends to. If matches ever *are* persisted, `plays_as` has to go in the file with the phase, and a save from before that has to read as "plays as itself", which is what a world does.

### Fifteen numbers, and a team spends one

`PlayerId` is four bits in the cell, so a world can tell fifteen players apart, and teams come out of the same pool. A match with `n` teams seats `15 - n` people, and `MAX_TEAMS` is seven because a team nobody can sit on is not a team — seven teams and eight seats is exactly fifteen.

That is the price of a team being a player, and it is the right price: what it buys is that a team and a seat can never be the same number, which is what the old scheme got wrong. But it is a real ceiling and [planned.md](planned.md#fifteen-slots-and-more-than-fifteen-clients) already wants to lift it by making a seat something other than a cell's owner byte.

### A world too small to seat everybody still admits everybody

`too_cramped_for_grants` now asks the grid whether it holds every number, which is the question its name asks. Both call sites still only **log** the answer and carry on, so a world that cannot seat fifteen players seats them overlapping anyway.

That is better than refusing somebody a world with visible space in it, and it is worth saying out loud rather than leaving as a warning nobody reads: the honest fix is for the server to refuse a `Create` whose shape cannot seat a full room, which is a decision about what a small world is *for*.

## Fixed

Closed by a decision rather than by a chase, which is why they are here and not in [gotchas.md](gotchas.md): nobody debugged either of them, and what happened is that a number was picked. Both are about what a connection may do before it is anybody, which is the half of this server that a public address makes interesting — see [server.md](server.md#deploying).

### A connection that never joins is never reaped

*Was:* neither side sends a ping, and a connection that has not joined is in no room, hears nothing, and is written to only when it asks something — so a peer that went away without closing its socket was never found. *You would have seen:* nothing, until a server up for weeks was holding a few hundred sockets belonging to browser tabs closed long ago, each with a task and a broadcast receiver.

A connection in no room is now closed after **two minutes** of saying nothing. One with a seat, or a room it is watching, is exempt: it is written to four times a second and a dead one is found by the write. `server::unjoined::deadline` is the decision, and `MOST_UNJOINED_SILENCE` beside `MOST_BYTES_AT_ONCE` is the number — **chosen, not measured**, being far longer than any exchange the menu makes and short enough that the leak is bounded by the minute rather than by the week.

What it costs is the menu. The room list refreshes itself every three seconds while it is on screen, so browsing rooms holds the socket open; a player who leaves the menu on another page for two minutes has the socket closed under them, and sees the menu say the server did not answer. Connecting again works, and the periodic `Ping` this entry used to ask for is what would remove the fault — along with the message the server owes a lagging client, which is still owed.

### Nothing rate-limits an unjoined connection

*Was:* `Rooms`, `Join` and `Create` are answerable without a seat, which they have to be, and nothing bounded how many connections one address could hold open asking them. *You would have seen:* a server holding as many tasks as somebody cared to open sockets, and the game still working for whoever got in first, until it did not.

One address may now hold **eight** connections that have joined nothing. `server::unjoined::PerAddress` is the table and `MOST_UNJOINED_PER_ADDRESS` is the number; a ninth is refused with a 429 before the upgrade, so it costs a handshake rather than a task. A connection gives its place back the moment it is welcomed or starts watching and never takes one again, because a socket that has been somebody is not the stranger the cap is for. **Chosen, not measured**, and counted in connections rather than in people for the reason the pool bug above gives.

What is bounded is connections and not messages: one socket may still ask `Rooms` as fast as it can write, and the answer is small but not free. The frame cap and `--max-rooms` are the whole of the rest, and a rate per connection is the next piece if a server ever needs one.
