# Not built yet

What has been decided, what has not, and what each one runs into. Everything here is an intention rather than a description — [the rest of docs/](README.md) is the system as it actually stands.

## A menu

A screen before the game: play locally, or type an address and join. Today the only way to reach a server is `--ws` on a command line, which a phone does not have and a browser only gets by being served from the server it will talk to.

Most of what it needs already exists. `client::set_connection` and `set_world` are one-shots read once at startup; a menu would set them from a form instead of from `main`. The browser client derives its socket from the page's origin, so the menu's address field only matters when the page and the server are not the same machine.

Two decisions worth making before it is built. Whether a local game and a joined game are the same screen with a different answer, or two screens. And whether the address field remembers what you typed — which, given the rejoin token is kept per room in the same store, wants to sit beside it rather than invent a second place to keep things.

Rooms have arrived since this was written, so the menu now has a third field and a new job: **listing what a server has**. A client cannot ask. See below.

## Rooms per server

**Started.** Several worlds behind one address, joined by name — see [server.md](server.md#rooms). A room is a whole `Server`: one world, one player table, one tick, one file. `Join` carries the room, `Welcome` names it back and says what shape that room's world is, player numbers and value and territory are per room, and the rejoin token is filed under the room so that coming back returns you to the world you left.

What is left is discovery, movement and lifetime.

**A client cannot ask what rooms exist.** Rooms are declared on the command line and joining an undeclared one is refused with a message naming the ones that are there, which is deliberate — the alternative, creating a room for whoever asks, turns a typo into an empty world you cannot tell is empty. But a rejection is a poor menu. The wire needs a `Rooms` request and a reply carrying, per room, its name, how many players are on and probably its shape; the menu is what would send it. That message is not on the wire yet, on purpose: a message nobody sends is scaffolding, and the thing that would send it does not exist.

**Nothing on the client can ask to change rooms.** The server side is done: a second `Join` on a live connection leaves the old seat and welcomes you into the new room, and `Welcome` already carries everything a client needs to start over — room, tick, shape, spawn, value. What is missing is anything that would send it, which is the menu again. Until then a room change means dropping the socket, which throws away the subscription set and the world for no reason.

**Every room steps every generation, whether or not anyone is in it.** An empty world is cheap, because `compute_active` finds nothing to do, but a room somebody built in and abandoned is not: it costs its full simulation four times a second for nobody. Sleeping a room with no players online is the obvious answer and is not free — a room that stops stepping stops at a tick, and the tick is what a returning client adopts, so waking it has to be indistinguishable from its never having slept. That is easier than it sounds, because the tick *is* the generation and nothing else moves while a world is not stepping. The thing to be careful of is the save, which records a tick that would then mean "when it went to sleep" rather than "now".

**A room's shape is a server-wide flag.** `--torus` applies to every room created in a run, so a server cannot offer a wrapping world and a boundless one side by side, which is exactly the sort of thing rooms are for. The shape belongs per room, which means `--room` needs to carry it — `--room arena:18x18` — or rooms need a config file, which is a bigger thing than this deserves yet.

**The token is keyed by room but not by server.** Two servers both running a room called `main` share one secret, and visiting the second costs you your player on the first. The address is not remembered anywhere yet; when the menu remembers it, that is the second half of this key.

## Auto-mining

A way to gather value without clicking for it — and it should be **a new kind of cell**, not a rule applied to life in general.

Value has exactly one source today: reclaiming your own living cells, one apiece. That makes the only way to earn a click, and it is the click you least want to be making. The obvious fix — living cells you own earn a trickle simply by being alive — is worse than it looks, because it prices territory by area. Whoever holds the most cells earns the most, sprawl is the strategy, and every cell becomes an accountant.

A kind localises it. Only mines earn; a mine is placed deliberately, at a price; and what a mine does to the ground around it is what that price buys. `Cell::update` already matches on `Cell::kind` and falls through to Conway for every value but one, so this is an arm rather than a rewrite, and the kind field is six bits with one of sixty-four used.

### A mine is a property of the square, not of the cell

Like ice, and for the same reason: a mine wants patterns to run over it. If a mine had to be alive it would need Conway's support to survive — a lone one dies of loneliness — and it would count as a neighbour, so placing one would change the pattern it was meant to measure.

As a kind on dead ground it disturbs nothing. Life is born on it and dies on it and the kind survives both, because a birth already keeps whatever metadata the dead cell carried and only takes the owner. Which also gives the mine its capture rule for free: **a birth on your mine hands it to whoever's parent won the roll**, and territory spread claims a dead one the same way it claims any other dead cell. A mine you cannot defend becomes somebody else's income, by the rule that already exists, with nothing added.

That is the strongest argument for doing it this way, and it is worth stating as a test the design has to pass: *whatever a mine does, an opponent must be able to take it with life.*

### The two rules it could follow

**It pays on death.** A cell that was alive at generation G and dead at G+1, on a mine, pays the mine's owner. Income is then proportional to **turnover** rather than to holdings: a still life beside a mine earns nothing, a gun firing into a crash site earns steadily, an oscillator that kills and rebirths earns forever. The economy becomes "build a machine that churns, and route it over your mines", which is what Conway is actually interesting for.

It also closes the loop rather than adding one. Mortality is the only sink today — a cell that dies of its neighbours cannot be reclaimed — so making it the source as well means the same event is a loss or a gain depending on where you paid to make it one. And it is nearly free to compute: `Halo::step_into` has the before and the after cell in hand at the moment it writes one, so the tally costs a comparison per cell and no second pass.

**It prevents births nearby.** No birth happens in a mine's eight neighbours, and the mine earns a fixed amount per generation instead. The suppression is the price: ground under mines grows nothing, so mining and building compete for the same territory rather than stacking.

The trouble is that it fails the test above. A glider needs births to move, so a glider entering the suppressed halo stops and dies — which makes a mine field an uncapturable glider trap that pays its owner forever, and a cheaper, better wall than ice. Suppressing births removes the only means of taking a mine back, and it is the same mechanic ice already provides, minus the counterplay.

**The two do not compose.** Suppressing births suppresses the deaths that pay, so a mine that did both would sterilise its own income. They are opposites rather than flavours, which is worth knowing before anyone tries to have both.

So: pay on death. The suppressing mine is written down here because it was the other candidate and because the reason it loses — that a rule must leave the opponent a move — is the reason worth keeping.

### What has to be settled

**The rate against the cost.** These are one decision, not two: together they are a mine's payback period, and everything about whether mining is worth doing is in that number. A mine dear enough to matter and a rate slow enough to require a working machine is the shape to aim at; the precise pair wants play rather than argument. Reclaiming pays one, so a mine at *N* placed in the wrong spot costs *N−1* to undo, which is the commitment a mine should carry and the reason it should be reclaimable at all rather than permanent like ice.

**Whose deaths pay.** The mine's owner, or the dying cell's owner. The mine's owner is the more interesting answer by some way: it makes a mine something you place *at a border*, to harvest the churn of whoever is fighting you, and it gives a reason to care where somebody else's machine is running. The dying cell's owner makes mines private infrastructure and nothing more.

**Whether a client can predict its own income.** This is the real engineering problem and it is not small. A client holds its viewport and a margin, so income from mines off screen is income it cannot compute — its predicted value would drift below the server's every generation and never correct, because everything a client predicts today is its own action, which is by definition on screen. Three answers:

1. *Send it.* `Step` goes out every generation already, but it is one broadcast encoded once for everybody, and a per-player number cannot ride on it without making it per-connection.
2. *Correct it.* `Checkpoint` and `Resync` exist because the world is predicted and predictions are sometimes wrong; value is the same problem, and an authoritative figure in the reply would bound the drift to one checkpoint interval. This is the answer — the machinery is there, it works, and a second mechanism for one class of problem is the thing to avoid.
3. *Confine it.* Define income over the region a peer holds. Ruled out, and written down so nobody re-derives it: the step would stop being a pure function of state and tick, which is the contract everything else rests on.

**Where the tally lives.** `sim` has no wallet — `Player` is defined there but no world holds a table of them — so `World::step` has to *produce* earnings rather than apply them, and the server and client each fold the result into the number they keep. It must be ordered by player rather than by iteration, for the same reason the active-chunk list is sorted.

**The art.** A kind has four consecutive tiles — dead, alive, iced, alive and iced — and `render::atlas` asserts every one of them is drawn. So a mine costs four sprites before it compiles, and one of the four is "a mine under ice", which is a state with a meaning worth deciding: ice freezes what it covers, so **a pane over a mine turns it off without capturing it**. Whether that is a feature — a cheap way to deny income without taking ground — or a hole is a question for whoever sets the rate.

## Stamps

A pattern you capture once and place again, at **double cost** — the convenience is the thing being paid for.

Decided: a **separate hotbar** rather than slots in the existing one, a **library** of them rather than a single clipboard, and a file that can hold **several stamps at once** so they can be shared as one thing. Its own branch.

It needs nothing new on the wire. A stamp is a `Paint` of the cells it covers, judged against territory and value like anything else — the doubling is a client-side price and a server-side check on the same action. The capture gesture is the interesting part: dragging a rectangle over your own cells is the obvious way to take one, and it is also how ice is placed, so the second hotbar has to make clear which of the two a drag means.

The open questions are all interface. Where a second bar sits on a phone, whether a stamp can be rotated or mirrored when placed, and what happens when a stamp will not fit inside your territory — refuse it whole, as a drag does, or place the part that fits.

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And there is no way to reach a server without a command line, which is the menu above.

## Known, and left alone

- **Thirty-one players is a ceiling on players a world has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. Rooms make this less pressing than it was — thirty-one is per room now — and no easier to fix.
- **Territory only ever spreads.** There is no die-off, so a glider leaves a permanent trail and an infinite world grows with it. Deliberate for now; the die-off is what would bound it.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::battle` is 1700 lines doing four jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, and the `me`, `room`, `value` and `subscribed` fields — is everything about talking to a server and nothing about drawing.
