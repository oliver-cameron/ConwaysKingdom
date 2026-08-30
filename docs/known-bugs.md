# Known bugs

Things that are wrong, or are probably wrong, and are not fixed. Written down because a bug nobody has recorded is a bug somebody rediscovers.

Each entry says **what it is**, **what you would see**, and **why it is still here**. A few were found by reading and never reproduced; those say so, because "I think this is broken" and "I made this break" are different claims and only one of them is evidence.

For bugs that *were* fixed, the reasoning lives in [gotchas.md](gotchas.md) — that file is the record of what a symptom turned out to mean, and it is the one to read when something looks familiar.

## Confirmed

### A connection that never joins is never reaped

Neither side sends a ping. A joined client is written to four times a second by the `Step` broadcast, so a peer that has gone away is discovered when a write eventually fails — but a connection that has **not** joined is in no room, hears nothing, and is written to only when it asks something. The server never finds out it is gone.

*You would see:* nothing, until a server that has been up for weeks is holding a few hundred sockets belonging to browser tabs closed long ago. Each holds a task and a broadcast receiver.

*Still here because* the fix is a periodic `Ping` and an idle deadline, and both want a decision about how long is too long for a client sitting on the menu. It is also the natural place to add the message the server owes a lagging client — `server::ws` logs `connection lagged n messages` and does not tell it, so the client only finds out on the next `Step` it manages to receive. See [networking.md](networking.md#the-server-is-the-clock).

### A player who was away at the whistle plays a team match on no team

`teams_are_fair` refuses to start a match with anybody unplaced, but it only looks at players who are **online**. Somebody who joined the lobby, dropped, and comes back with their token during the match is admitted — the returning-player gate in `Server::handle` is deliberate and right — and they are on no team, so they play as themselves against the teams.

Reproduced: three players in a two-team match, one offline at the whistle, rejoining after it. They are welcomed, granted their own patch, and `plays_as` is their own number.

*You would see:* a lobby listing two teams and one player under neither, and a scoreboard with three competitors in a two-team match.

*Still here because* it is not obviously a bug rather than a missing rule, and the two candidate rules disagree: put them on the smallest team, or make them a spectator. The second is what [planned.md](planned.md) already wants for the mercy rule, and doing it once for both is better than doing it twice.

*Not* dangerous any more. It used to seat them on top of a team's opening and take it — `seat_number` drew team ids and player numbers out of one 1..15 space — which is fixed.

### The hotbar labels its stamp squares with keys a keyboard may not have

`hotbar::tool_hint` asks the keyboard what shift and a digit type here, and falls back to `S1`–`S4` until somebody has pressed one. `hotbar::stamp_hint` beside it hard-codes `1`–`9` and `0`.

*You would see:* on a French keyboard, ten stamp squares labelled `1`–`0` whose keys type ``&é"'(-è_çà``. The help screen is right about this and the bar is not, which is worse than both being wrong.

*Still here because* the machinery is all there — `Views::label(code, false)` answers it, and the help screen was taught to ask in the same pass that found this — and it is a small change to a file that was not otherwise being touched.

### The help screen's monospace column misaligns on a wide glyph

`help.rs` pads each keycap with `format!("{key:widest$}")`, which counts **characters**. A learned label containing a full-width glyph counts as one character and draws two columns wide.

*You would see:* the two columns of the key list stepping out of line by a character on the rows whose labels were learned from a keyboard with wide glyphs.

*Still here because* it needs a width table rather than `chars().count()`, and the layouts that produce wide keycaps are the input-method ones this client does not otherwise support.

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

## Not bugs, but the next thing to go wrong

### A match is never saved, and now the teams go with it

`Rooms::save` skips any room that is not `Phase::Open`, deliberately — "a half-finished match restored into a server that has forgotten it was a match would run on forever with nobody able to win it". That was already true of the phase and the victory condition. It is now also true of `Player::plays_as`, which is not in the save format.

So this is consistent rather than broken: matches do not survive a restart, and nothing about a team pretends to. If matches ever *are* persisted, `plays_as` has to go in the file with the phase, and a save from before that has to read as "plays as itself", which is what a world does.

### Fifteen numbers, and a team spends one

`PlayerId` is four bits in the cell, so a world can tell fifteen players apart, and teams come out of the same pool. A match with `n` teams seats `15 - n` people, and `MAX_TEAMS` is seven because a team nobody can sit on is not a team — seven teams and eight seats is exactly fifteen.

That is the price of a team being a player, and it is the right price: what it buys is that a team and a seat can never be the same number, which is what the old scheme got wrong. But it is a real ceiling and [planned.md](planned.md#a-seat-is-not-a-person) already wants to lift it by making a seat something other than a cell's owner byte.

### A world too small to seat everybody still admits everybody

`too_cramped_for_grants` now asks the grid whether it holds every number, which is the question its name asks. Both call sites still only **log** the answer and carry on, so a world that cannot seat fifteen players seats them overlapping anyway.

That is better than refusing somebody a world with visible space in it, and it is worth saying out loud rather than leaving as a warning nobody reads: the honest fix is for the server to refuse a `Create` whose shape cannot seat a full room, which is a decision about what a small world is *for*.

### Nothing rate-limits an unjoined connection

`Rooms`, `Join` and `Create` are answerable without a seat, which they have to be. `Create` is capped at `--max-rooms` and the shape is checked; `Rooms` is a small message answered with a small message. Neither is limited in how often it may be asked, and a connection costs a task.

The defence today is that the game is served to people who were sent a link. That is a fine defence for what this is and a bad one to forget about.
