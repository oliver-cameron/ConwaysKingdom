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

## Turrets

**Built** — see [game.md](game.md#turrets) and [simulation.md](simulation.md#turrets). A turret claims ground at range: every generation it takes the nearest square that is not its owner's, out to `rule::TURRET_REACH`, and a dead one runs that backwards over the ground behind it. It is a pass after the rules in absolute coordinates, beside `break_ice_from`, because every rule in `sim::rule` sees one cell and its eight neighbours and no halo can answer "the nearest square that is not mine".

Two things fell out rather than being designed, and both are better than what was planned here. **A live cell must have an owner**, so taking a square away from its owner kills whatever stands on it — the dead turret's killing is that invariant rather than a rule about killing. And a turret inside its owner's ground finds everything within reach already theirs and idles, so **it only works from a frontier** and needed no rule about where it may be placed.

The inheritance problem was answered by splitting kinds into those a birth inherits and those it does not, which is now `kinds!` in `sim::cell` — one list writing `Kind::ALL`, the count and `Kind::inherits`, the way `rules!` writes the rule chain and its names. A kind that does not inherit passes over ownership alone, so a birth beside a turret is ordinary life owned by the turret's owner and a gun is not a turret factory. That made the rest of what was planned here unnecessary: a turret never spreads, so it needs no bill to stop it sprawling, and its balance is its purchase price and its claim rate and nothing emergent.

What is left is numbers rather than mechanism.

**The balance is argued, not measured.** `TURRET_COST` at fifteen, `TURRET_REACH` at six and `TURRET_DECAY` at four in sixty-four were reasoned off the decay arithmetic — a claim a generation against `DECAY` settles at about thirty squares, so a block of four holds about a hundred and thirty — and nothing has run to check it. `examples/balance.rs` is the harness that answered this for mines and prints nothing about turrets. It should, and the shapes to put in it are the block against a lone turret against a turret dropped into a glider, since those are the three things a player will try.

**Whether a turret should press on a living neighbour is a number now rather than a rewrite.** `rule::TURRET_POWER` is how many squares it flips a generation, and against a living colony `SPREAD` hands them back at forty in sixty-four, so what it holds of contested ground is about `TURRET_POWER × 64 / SPREAD` — one and a half squares at one, six at four. It sits at **one**, which is the reaching tool rather than the weapon. Moving it is the experiment, and `examples/balance.rs` is where the answer should be printed rather than argued about.

**A turret under ice** is the same open question as a mine under ice, and sharper. A frozen turret does not fire, so a pane is a cheap way to switch off somebody's territory engine without taking any ground from them. Whether that is a feature or a hole is for whoever sets the rate.

**The remedy for a corpse gets dearer the longer it is left.** A dead turret is cleared by building on it — placing life sets the kind back to ordinary, as it does over a dead mine — and what the corpse is doing is taking your ground away a square at a time, so the square you need to build on stops being yours and the fix goes from one to ten. That may be exactly the right shape and it has not been played enough to say.

**A turret should not also kill, and the reason is that a claim is contested and a kill is not.** Ground a turret takes is taken straight back by `SPREAD` at forty in sixty-four, which is why it cannot touch ground anything is alive on and why one square a generation is nearly nothing. Nothing does that to a kill: a dead cell stays dead unless Conway hands it back. So the same "one a generation, forever" that is almost nothing for claiming is decisive for killing — and a turret is a **still life**, four cells, immortal, free after purchase and unreachable without flying something into it. A block of them killing four cells a generation forever is not a territory tool, it is area denial with no answer.

It would also cost the two things that make a turret readable. The dead turret's kill is not a rule about killing — it is the `Cell::alive` invariant showing through, since unowning a live square kills what stands on it — and that reads as a mirror only while the live turret does not kill. And a turret inside its owner's ground idles, which is what makes it a frontier piece placed without a rule about placement; one that kills always has something to shoot at, and that goes.

So: **another kind**, and the interesting question is what powers it, because "stands there and fires" is exactly what the turret does and exactly what should not have a kill attached. The shape that fits this game is a kind that spends a **birth** — a cell that, when one of its kind is born, kills the nearest enemy life. Its rate is then your pattern's birth rate, which is what the game already rewards building; a gun feeds it and a block does not; and it is counted where `Halo::step_into` already counts a mine's births, holding each cell before and after in one breath. That makes killing something you run a machine for rather than something you park.

Worth saying out loud first: **Conway already has a weapon.** A glider is five cells and one gesture and it kills what it hits. Whatever this kind turns out to be has to be worth more than that, or it is a button for a thing players can already do.

**The art is a stand-in** like the rest of the sheet: the ordinary cell with a solid plus stamped into it, generated rather than drawn, in `assets/sprites/art.png` at tiles 8–11 with the mine's own mark colours so the two read as siblings. It is legible against all four states and in any player's hue, and it is not what anybody would draw on purpose.

## Stamps

**Started** — see [game.md](game.md#stamps). Capture with Grab, place with a click, ten on the bar and the rest behind a key. Captured as live cells and their kind rather than as a rectangle of ground, trimmed to what was caught, and only ever your own life.

The old plan wanted a **separate hotbar**, and it turned out not to need one: segmenting the single bar does the same job, and the gesture the second bar existed to disambiguate — is this drag a pane or a capture? — is answered by what you are holding.

What is left:

**The double cost.** It was decided that a stamp costs twice what drawing it would, and it does not yet. That needs the action to say on the wire that it is a stamp: the doubling has to be a server-side check as well as a client-side price, or the client charges two and the server charges one and `Purse` quietly hands the difference back. An `Action::Stamp` beside `Paint` is the obvious shape.

**A file.** A stamp lives as long as the client does. The plan wanted a file holding several at once, so a library can be shared as one thing — which is `net::keep`'s business now that it is where a client keeps what it has.

**Rotation and mirroring**, and what to do when a stamp will not fit inside your territory: refuse it whole, as it does now, or place the part that fits.

**Naming.** A stamp is called `3x4` because nothing has asked what it is. The library is where a name would be typed, and the shape beside it already does most of the work a name would.

## Matches

**Nothing built.** A lobby, everyone starting at once, a timer, and the most territory at the whistle. What follows is the shape it takes on what is already here and the three things that have to be decided before any of it is written.

**A match is a room with a lifecycle**, and that is the whole of the architecture. Rooms are already separate worlds with their own players, tick, value and file; what a match adds is a phase — gathering, running, finished — and a deadline. It needs nothing in `sim`. The simulation does not know what a match is, the same way it does not know what money is, and that boundary is worth more than any convenience of breaking it.

The deadline is a **tick**, not a wall clock. The tick is the generation and it is already what a client adopts from `Welcome`, so a match that ends at tick N needs no clock synchronisation, cannot be lengthened by a client that pauses, and is the same instant for everybody by construction.

### The opening is the problem, not the income

**A 2×2 block and an income is a clicker.** A still life is the one shape in Conway that does nothing at all — it does not breed, it does not move, and it cannot die — so a player granted one and paid a trickle has exactly the clicker loop in front of them: wait, tap, wait. Whatever the income turns out to be, it does not fix that, because a stationary pattern's footprint is fixed and so its income is flat. Anything that pays by the generation pays a block the same amount forever.

Note what the block was solving, because it is easy to throw out with it. `game.md`: four cells that hold their shape forever, the same for everyone, so nobody begins ahead — and the block is also what *keeps* the ground, since territory spreads from living cells and a bare patch would never grow. So the grant has to be **immortal**, or an unlucky opening eliminates somebody before they have acted, and **identical**, or the draw decides the match. In Conway those two pull hard against "and it should do something": the patterns that do something either wander off or grow without bound.

**A build phase resolves it, and it is what ice already is.** Lobby, then a phase with the world frozen and a fixed budget of cells to lay, then the clock starts and the world runs. `game.md` on ice: *a schematic — freezing a region lets a large pattern be laid out over many generations without the rule eating the half-built work*. A match's opening is that idea promoted to a rule, with the whole world under the pane and everybody drawing at once.

What it buys is that **the interesting decision moves to the front**. Conway is a game about the initial condition, and a match where everyone lays a hundred cells into a frozen world and then watches them compete for two thousand generations is a competition about the thing the game is actually about. It also makes the grant question moot: nobody is handed a machine, everybody builds one, and what you are handed is a budget and a patch to spend it on.

It leaves one question rather than three: **what happens during the run.** Three readings, and they are different games.

- **Nothing.** No placing at all once the clock starts. The purest, and the least to do for however long the match lasts.
- **Territory pays, and you may intervene.** Value per generation in proportion to ground held, spent on repairs and raids. The win condition and the economy become the same thing, which falls the right way at the end — a player losing ground earns less and falls further behind. Placing outside your own ground already costs ten times, so reaching into somebody else's half is expensive and deliberate rather than a click.
- **A second build phase**, partway through. Freeze, everybody draws, unfreeze. Keeps the front-loaded decision and gives the match a second act.

The middle one is the recommendation, with the first worth trying as a variant because it costs nothing to offer.

**And if the grant is to be a machine after all**, an oscillator is the only thing that is immortal, stationary and gives births: a blinker of mines is the smallest, three cells earning every other generation forever. It is a fallback rather than an answer — it pays a flat rate, which is the clicker again with a better animation.

### Scoring

Nothing counts ground per player. The `territory` example's `survey` does it for one player by walking every stored chunk, and a scoreboard is that for all thirty-one: one pass over the world, the same cost as `ice_cells` or `turrets`, of which there are already two a generation. Fine once a second, not fine every step.

It must also be the **same number for everyone**, and a client cannot compute it — it only holds the chunks it subscribed to, so it can count its own screen and nothing else. So the server counts and broadcasts, which is a new message and the first thing in the protocol that is about a match rather than about a world.

### What a scored match does to `HOME`

Granted ground never decays, so a player whose life is wiped out still holds their patch and still scores for it at the whistle. In a sandbox that is a floor that keeps them playing; in a match it is points for having turned up. Either home stops being exempt once a match is running, or it stops counting toward the score — the second is the smaller change and keeps the floor doing its job.

### What the lobby actually buys

More than it looks. Grants are laid out on a fixed grid at a fixed pitch sized for thirty-one players, whether two turn up or twenty, and `spawn_for` derives a position from a player number alone. With the roster known before the world starts, the grid can be packed to the players actually in it — everybody the same distance from their neighbours, and no advantage in having joined early or late. That is a change to `spawn_for`, and it is the one place a match touches something that already works.

### What is left over

A finished match should stop stepping, which is the same machinery as the sleeping rooms above and has the same trap: the tick is what a returning client adopts, so a room that stops must not look like one that never ran.

Reconnecting matters more here than in a sandbox — a refresh in the middle of a match has to put you back in your seat, which the rejoin token now does across a server restart as well.

And a player who cannot afford anything is a player watching. A build phase puts the spending before the clock rather than after it, which is the point: what a zero start must not mean is a minute of nothing while the money arrives.

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And there is no way to reach a server without a command line, which is the menu above.

## Known, and left alone

- **Thirty-one players is a ceiling on players a world has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. Rooms make this less pressing than it was — thirty-one is per room now — and no easier to fix.
- **Territory creeps and decays now**, so ground is traded and lost as well as won, with granted ground exempt as the floor. What is unsettled is the floor: "your home patch is permanent" is a strong promise, and it also means an opponent who grows over it keeps a square that will never decay for them either.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::battle` is 1900 lines doing five jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, `chose`, `to_menu`, and the `me`, `room`, `value`, `screen` and `subscribed` fields — is everything about talking to a server and nothing about drawing. The menu made that worse rather than better: the screen the client is on and the connection it is holding are the same state machine, and it now lives in the same struct as the sprite atlas.
