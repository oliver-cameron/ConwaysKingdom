# Not built yet

What has been decided, what has not, and what each one runs into. Everything here is an intention rather than a description — [the rest of docs/](README.md) is the system as it actually stands.

## A menu

**Built** — see [game.md](game.md#the-menu). One screen with a different answer rather than two, because a local game and a joined game differ only in whether there is a socket. The address and the name are remembered in the same store as the rejoin token, which is what that section said they should be.

What it does not do yet is **reconnect**. A link that closes mid-game drops the client to offline with a line in the log, and there is no way back but restarting — the menu is only reachable before a world, not from inside one. It should be reachable from inside one: the machinery is all there, since a second `Join` on a live connection is already a room change, and `Screen::Menu` is a state the app can be put back into from anywhere.

## Rooms per server

**Started.** Several worlds behind one address, joined by name — see [server.md](server.md#rooms). A room is a whole `Server`: one world, one player table, one tick, one file. `Join` carries the room, `Welcome` names it back and says what shape that room's world is, player numbers and value and territory are per room, and the rejoin token is filed under the room so that coming back returns you to the world you left.

Rooms are listed too: `ClientMessage::Rooms` is answerable without a seat, and the menu shows what comes back. What is left is movement and lifetime.

**Nothing on the client can ask to change rooms.** The server side is done — a second `Join` on a live connection leaves the old seat and welcomes you into the new room, and `Welcome` carries everything a client needs to start over: room, tick, shape, spawn, value. The menu can send exactly that message; what is missing is a way to get back to the menu without restarting. Same gap as the reconnect above, and the same fix.

**Every room steps every generation, whether or not anyone is in it.** An empty world is cheap, because `compute_active` finds nothing to do, but a room somebody built in and abandoned is not: it costs its full simulation four times a second for nobody. Sleeping a room with no players online is the obvious answer and is not free — a room that stops stepping stops at a tick, and the tick is what a returning client adopts, so waking it has to be indistinguishable from its never having slept. That is easier than it sounds, because the tick *is* the generation and nothing else moves while a world is not stepping. The thing to be careful of is the save, which records a tick that would then mean "when it went to sleep" rather than "now".

**A room's shape is a server-wide flag.** `--torus` applies to every room created in a run, so a server cannot offer a wrapping world and a boundless one side by side, which is exactly the sort of thing rooms are for. The shape belongs per room, which means `--room` needs to carry it — `--room arena:18x18` — or rooms need a config file, which is a bigger thing than this deserves yet.

**The token is keyed by room but not by server.** Two servers both running a room called `main` share one secret, and visiting the second costs you your player on the first. The address is not remembered anywhere yet; when the menu remembers it, that is the second half of this key.

## Auto-mining

**Built** — see [game.md](game.md#mining). A mine is a living cell that pays its owner every time one of its kind is born, and the mechanism is **inheritance**: a birth copies its parent, kind and all, so a mine's children are mines and the kind spreads through a mixed population because the parent is picked at random.

That is a better idea than what was written here before, which was a mine as a marker on the ground paying out on deaths. Inheritance makes a mine an investment in a *lineage* rather than a square, needs no per-square bookkeeping, and the payout is counted where the rule already holds a cell before and after — so it costs a comparison and no second pass.

Two of the three open questions answered themselves. The rule counts births and `net` prices them, so the tally never taught the simulation about money. And the prediction problem went the way this section said it should: `Purse` rides on every `Checkpoint` reply, reusing the machinery that already exists for "your copy is wrong, here is mine".

A mine's corpse now costs while it lies there, sixteen generations in sixty-four, so income is births minus the upkeep of everything you have let die. What that rewards is a machine that stays where you put it: a blinker pays, and a glider dragging twenty corpses behind it bleeds. `cargo run --no-default-features --example balance` prints the table, and the rate was picked off it rather than argued about.

What is left is a hole rather than a number: **there is no way to clear a mine's corpse.** A dead cell cannot be reclaimed, so the only remedy for a mine field you regret is to let the life on it go out and wait for territory decay to take the ground. That is a long punishment for a misclick, and value floors at zero so a bad enough mess simply stops you playing. Reclaiming a corpse to clear its kind — for a price, or for nothing — is the obvious fix and needs a decision about what it should cost.

The art is a stand-in like the rest of the sheet: the ordinary cell with a diamond and a pip stamped into it, generated rather than drawn, in `assets/sprites/art.png` at tiles 4–7. It reads clearly against all four states and in any player's hue, and it is not what anybody would draw on purpose.

Also unsettled: **a mine under ice**. A pane freezes what it covers, so a frozen mine gives no births and earns nothing — a cheap way to switch off somebody's income without taking their ground. Whether that is a feature or a hole is a question for whoever sets the rate.

## Stamps

**Started** — see [game.md](game.md#stamps). Capture with Grab, place with a click, ten on the bar and the rest behind a key. Captured as live cells and their kind rather than as a rectangle of ground, trimmed to what was caught, and only ever your own life.

The old plan wanted a **separate hotbar**, and it turned out not to need one: segmenting the single bar does the same job, and the gesture the second bar existed to disambiguate — is this drag a pane or a capture? — is answered by what you are holding.

What is left:

**The double cost.** It was decided that a stamp costs twice what drawing it would, and it does not yet. That needs the action to say on the wire that it is a stamp: the doubling has to be a server-side check as well as a client-side price, or the client charges two and the server charges one and `Purse` quietly hands the difference back. An `Action::Stamp` beside `Paint` is the obvious shape.

**A file.** A stamp lives as long as the client does. The plan wanted a file holding several at once, so a library can be shared as one thing — which is `net::keep`'s business now that it is where a client keeps what it has.

**Rotation and mirroring**, and what to do when a stamp will not fit inside your territory: refuse it whole, as it does now, or place the part that fits.

**Naming.** A stamp is called `3x4` because nothing has asked what it is. The library is where a name would be typed, and the shape beside it already does most of the work a name would.

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And there is no way to reach a server without a command line, which is the menu above.

## Known, and left alone

- **Thirty-one players is a ceiling on players a world has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. Rooms make this less pressing than it was — thirty-one is per room now — and no easier to fix.
- **Territory creeps and decays now**, so ground is traded and lost as well as won, with granted ground exempt as the floor. What is unsettled is the floor: "your home patch is permanent" is a strong promise, and it also means an opponent who grows over it keeps a square that will never decay for them either.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::battle` is 1900 lines doing five jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, `chose`, `to_menu`, and the `me`, `room`, `value`, `screen` and `subscribed` fields — is everything about talking to a server and nothing about drawing. The menu made that worse rather than better: the screen the client is on and the connection it is holding are the same state machine, and it now lives in the same struct as the sprite atlas.
