# The game

Conway's rules, with owners and an economy on top.

## Value

Every player has a `value`, on `sim::Player`. It is what they have to spend.

| | |
|---|---|
| place life | −1 each |
| place a mine | −10 each |
| place a turret | −15 each, and four is the smallest one that works |
| place ice | −5 each |
| **anywhere but your own ground** | **×10**, whatever is being placed |
| **a mine of yours is born** | **+1** |
| **a dead mine of yours, each generation it lies there** | **−1**, sixteen times in sixty-four |
| reclaim your own | +1 |
| take another player's | −1, because taking ground should not be free |
| take what is not there | nothing |

Life is cheap because it is drawn by the stroke rather than placed cell by cell: a pencil lays tens of cells in a gesture, and at five a cell that is a gesture nobody can afford. Ice stays dear because a pane is a wall, and a wall that costs what a cell costs is not a decision.

Life at one against reclaiming at one means putting a cell down and taking it back is free, which is deliberate — rearrange your own board as much as you like. **What drains value is the rule.** A cell that dies of its neighbours cannot be reclaimed, so the sink is mortality rather than the act of placing, and the game is about drawing patterns that survive. Starting value is 100.

The taking rule reads the same for life and for ice, because the question it asks is whether the thing being taken is there and whose it is, not which of the two it is.

An action that cannot be afforded is refused. The client prices and refuses locally on the same terms the server would, so a refusal is instant rather than a round trip away, and the two cannot disagree because `net::value_delta` is one function used by both.

Cost is read **before** the action is applied, since it depends on what is there now.

## Mining

A **mine** is a living cell that pays its owner every time one of its kind is born. It is bought once and inherited afterwards: a birth copies its parent, so a mine's children are mines — and because a birth picks one of three parents at random, the kind spreads through a mixed population rather than being handed down whole. What you are paying for is a **lineage**, not a cell.

**A mine's corpse costs once and is then ordinary ground.** The charge falls due at `MINE_UPKEEP`, sixteen in sixty-four, and when it does the square loses its kind — so a mine field is a debt with a bottom to it rather than one you cannot pay off.

It costs `MINE_DRAIN`, which is **two** against a birth's **one**. That is what decides which patterns pay, and the discriminator is *how long your corpses lie about*: a corpse that is reborn before its charge falls due escapes it entirely. A blinker re-uses its own ground every other generation and escapes most of them; a glider abandons its corpses behind it and escapes none. That is what stops mining being the answer to everything: making every cell you own a mine used to be free money, because a settled pattern gives birth constantly and each birth paid, and the wreckage was free.

So income is births minus the upkeep of everything you have let die, and the thing being rewarded is a **compact machine**. `cargo run --no-default-features --example balance` prints the table, in value per generation at steady state:

| drain | block | blinker | glider | r-pentomino |
|---|---|---|---|---|
| 1 | −40 | +434 | +137 | +11552 |
| **2** | **−40** | **+298** | **−276** | **−7754** |
| 3 | −40 | +162 | −689 | −27060 |

Net over three hundred generations, placement included. **Two is the line where a blinker pays and a glider does not**, and that is the rule the number was chosen to satisfy: a machine that stays where you put it earns, and anything that wanders off dragging a trail of corpses costs. At one everything pays and sprawl pays best, which is where this started; at three even a blinker is barely worth building.

A block of mines is the honest edge case: nothing is ever born and nothing ever dies, so it neither earns nor costs. It is simply forty spent on nothing.

A corpse whose ground decays away is never charged at all, because nobody owns it to charge — so a trail far from anything alive fades before the bill arrives.

Three constants, and they are one decision: `MINE_COST` against `MINE_YIELD` is a mine's payback period, and `MINE_UPKEEP` decides how much a mess costs to hold. All of them live in `sim::rule` with the rest of the tunable numbers, which is where anybody balancing the game should be looking.

Value is floored at zero. A cost that comes from an action is refused when it cannot be paid; a drain arrives whether or not there is anything to take it from, and a player in debt would be one who cannot act and has no way to stop owing.

A mine's corpse keeps its kind, as any cell does — so ground a mine died on shows the mine sprite, and life born there from an ordinary parent is ordinary. Placing plain Life over it explicitly sets the kind back to normal, or drawing over a mine's corpse would hand you a free mine.

## Turrets

A **turret** claims ground at range. Every generation it takes the nearest square that is not yours and makes it yours — wherever that is, out to six cells. It is the opposite of a mine in every way that matters: a mine earns on turnover and a turret works by standing still, a mine is bought once per lineage and a turret once per cell, and a mine wants a machine that keeps giving birth where a turret wants one that never has to.

**A turret is placed in fours.** One turret is one live cell with no live neighbours and is gone in a generation, so the smallest turret that works is the 2×2 block — the cheapest thing in Conway that never dies and never gives birth. Sixty against a starting hundred, which is an opening you can afford exactly one of.

That the block gives no births is the point rather than a cost. A block of mines is the honest edge case above, forty spent on nothing; a block of turrets is the best thing a turret can be. **The still life is a mine's worst shape and a turret's best.**

How much it takes is `rule::TURRET_POWER`, and that number decides what a turret is *for*. Low, and a claim is taken straight back by the ordinary spread of territory wherever anything is alive, so a turret is a way of reaching past your own frontier into empty land. High, and it holds ground against a living neighbour and becomes a way of pushing on one. A dead turret gives back whatever a live one would take.

A turret takes **ground**, not the life standing on it. It will not claim a square something is alive on, whoever owns it, and it will not touch what is under a pane. What it does take is dead ground, including ground nobody has ever held — which is what makes it a way to reach past your own frontier rather than a way to fight over somebody's colony. Against a living neighbour it barely works at all: their life takes the square straight back through the ordinary spread of territory, and a turret only claims one square a generation.

**A turret inside your own ground does nothing.** Everything within reach is already yours, so it idles. Put them on your frontier, which is where you would have put them anyway — the rule enforces its own placement without a rule about placement.

### When one dies

A dead turret runs the whole thing backwards. It takes the nearest square that **is** yours and gives it up, and since a live cell must have an owner, doing that to a square you have something living on kills it. That is not a separate rule about killing — it is the same rule read in the other direction, and taking a square away from its owner is the only thing it does.

So a failed emplacement fires on the ground behind it, including the other three cells of its own block, and a block that loses a cell dismantles itself. A dead turret decays back to ordinary ground four times in sixty-four, so it does that for about sixteen generations before it stops.

Granted ground is exempt, for the same reason it never decays: it is the ground you can always build on at the ordinary price, and a machine of yours that failed must not be what takes that away.

A turret is not inherited. A mine's children are mines, but a birth beside a turret is ordinary life owned by the turret's owner — the ground changes hands and the machine does not copy itself. Without that a gun would be a turret factory, and whoever built one first would own the map.

## Placing and taking

One button, and the cell under it decides which — for whatever the hotbar is holding:

- what you are holding is already there → take it back
- it is not → put it down

Keyed on what is held rather than on whether the cell is occupied at all, because **life and ice are independent**. Holding Life and clicking a living cell under a pane kills the life and leaves the pane standing; clearing the square outright would destroy a pane the player never aimed at, at five a cell. Holding Ice and clicking that same pane lifts it and leaves the life, which is the only way a misplaced pane comes back.

**Life and a mine are different things to hold, and clicking one over the other replaces it.** Holding Mine and clicking your own living cell makes that cell a mine, at what a mine costs; holding Life and clicking a mine makes it ordinary life again, at what a cell costs. It used to kill the cell instead, because the question being asked was whether taking the held thing away would change anything — and life and a mine are both taken away by clearing the same bit, so a mine held over life read as already being there. What a player holding Mine over their own life means is *make this a mine*, and the only click that should take a mine back is one holding a mine. `net::Placement::is_on` is the question now, and it asks whether the square holds *this*.

That is a different relationship from the one life has with ice. Life and ice are **independent** — a square may carry both, so holding one and clicking the other leaves the other standing. Life and a mine are **exclusive**: they are the same cell in two states, so holding one and clicking the other converts it. A corpse is neither, whatever kind it kept, so a click over a dead mine places rather than takes — which is what stops drawing over one handing out a free mine.

The owner is no part of it. Somebody else's life is still life, so a click holding Life takes it, at the reclaim price, which is what lets you clear a glider that has flown onto your ground.

`Action::Erase` carries what to remove for the same reason `Paint` carries what to lay: the server judges an intent, and "kill the life on this square" is a different intent from "clear this square".

**A drag lays the shape the held slot lays**, and the two slots lay different shapes. Life is a **pencil**: every cell the pointer crosses, so you draw a pattern and watch the line appear under your hand. Ice is a **rectangle**: two corners, because a pane is a shape you place and dragging one out says how big before it exists. Which one a drag is drawing is fixed when the button goes down, so changing slot midway does not change a line already half drawn.

**Nothing is laid until the button comes up** — a stroke as much as a pane. What a drag would place is drawn as a preview under the hand, with its size and its price beside it, and the world does not change until you let go.

It was the other way round for a while: a stroke laid itself cell by cell as it was drawn, on the reasoning that a pencil should behave like one and that holding it back reads as lag. What that cost was everything a preview buys. Escape could not abandon a stroke, because most of it was already down. A refusal arrived partway through, so a sweep that left your territory laid the cells before the boundary and stopped. And the pricing had to be per batch, so a stroke that ran out of value stopped where the money did — somewhere the hand had not — instead of being refused whole. Committing on release makes the pencil obey the same rule as everything else: **a drag is all or nothing**, and you can see what it will do before it does it.

The pencil marks the cells between one pointer position and the next, not just the ones it was reported at. Events arrive far apart when the hand moves quickly — a fast stroke crosses twenty cells between two of them — so marking only the reported positions would draw a dotted line.

But not *every* cell it passes over, and **the gaps are the point**. A cell counts only if the pointer went through the middle of it, `CELL_COLLIDER` wide. A solid line is not what you want to draw here: the patterns worth placing have holes in them, and a stroke that fills everything it touches can only make walls. What the rule buys is that a stroke passing diagonally between two cells does not catch the two beside the corner — which is what makes a diagonal a diagonal, and **a glider one motion of the hand rather than five clicks**.

That is what the width was measured against, with a hand that wobbles a quarter of a cell and cuts its corners:

| tolerance | lands a glider exactly | cells missed | cells extra |
|---|---|---|---|
| 0.35 | 2% | 2.4 | — |
| 0.55 | 57% | 0.4 | — |
| **0.70** | **96%** | none | none |
| 0.80 | 64% | none | 0.35 |

Below 0.7 the misses are the problem: you have to pass nearer the centre of every cell than a hand reliably can. Above it the extras are, because the band grows wide enough to catch the cells beside a corner and the gaps close up.

A 45° stroke is one cell thick and unbroken at every value in that range, so it is not what the number is for. Angled strokes **do** break, and that is wanted rather than tolerated — an unbroken angled line is a thing you can draw with two strokes, and a shape with holes is not.

Not every pattern is one motion. A lightweight spaceship has nine cells, one of them touching none of the other eight and three of them with a single neighbour, so no tolerance makes it a single stroke: a stroke is a path, and a path has two ends.

A stroke that crosses itself lists each cell once: the pricing compares every cell against the world rather than against the cells before it, so a repeat would be charged for twice and laid once.

A drag always places, never takes: a sweep across occupied ground is far more likely to be building over it than a request to clear it cell by cell, and an accidental sweep that wiped a structure would be unforgiving. Taking stays a deliberate single click.

A drag that cannot be paid for is drawn as refused *while the button is still down*, so the answer arrives before the commitment rather than after it — and since a drag is all or nothing, a refusal means no cells at all rather than as many as could be paid for.

Only the cells a placement actually changes are charged for. Extending a rectangle means sweeping the whole of it again, and paying twice for the part that was already there made the natural gesture the expensive one. This is why `value_delta` reads the world for a paint as well as for an erase.

One drag lays at most 4096 cells. A rectangle at one pixel per cell can cover millions, each to be listed, priced, applied and put on the wire. A stroke stops growing when it reaches the cap and says so in its label, rather than being trimmed at the end where nobody would see what was lost.

More than one cell is what makes a drag a drag. A press that travelled but stayed inside one cell would place where a click would take, so which of the two happens must not turn on a few pixels of hand shake at high zoom.

## Territory

Every dead cell carries an owner **and a level** — how much of that owner's influence reaches it, nought to seven — and that is territory. The level is not drawn: ground fading with it made every claim look like a different kind of cell, and a strength you cannot act on separately is one the map does not need to spell out. What it decides is where the border ends up, which the map shows by having one. The rule spreads it: a dead cell next to living ones takes one of their owners most generations, so ground is claimed by the life that grows over it. It stays dead — this sets the owner and nothing else. Ice is exempt while it stands, since a pane's cover is not claimed out from under it.

**A player may only place where their own influence reaches**, and it costs the same wherever that is.

Both the other arrangements are out and they went together. Placing anywhere for ten times the price was too weak — no obstacle at all to anybody with a mine running — and it made the map somewhere you bought your way into rather than somewhere you grew into. Grading the price by how thin your influence was went with it: a cost the player cannot see is a cost they cannot play around, and once ground stopped being shaded by its level there was nothing on screen to read it off.

A wall was tried before levels and was worse than either, because a player whose life went out was finished — they could not place, so could not grow ground, so could not place. The wall is safe now for a reason that has nothing to do with price: **granted ground is a source**, so everybody always has a patch with a live gradient to build on.

`net::may_place` is the whole rule, and the client refuses on the same terms the server does — instantly, and with the same answer. A drag is all or nothing, so one cell out of reach refuses the stroke rather than trimming it.

That is what a grant is for. A player who owned nothing could still act, at ten times the price, but a hundred of value buys ten cells of life and nothing else — so joining claims a 12×12 patch with a **2×2 block** standing in the middle of it, which is somewhere to build at the ordinary rate.

A grant claims **dead ground whoever held it**. It used to claim only cells nobody held, on the principle that territory is taken by life reaching it rather than handed out over what is already held — and that principle costs a player the game. Territory only ever spreads, so a world with an edge eventually belongs to whoever got there first, and a player joining after that was granted nothing: no ground, and therefore no block, since the block only stands on ground they own. They could place nothing, could never come to own anything, and were locked out of a world they were looking at. On a torus that is not an edge case; it is what happens to the second player to arrive at a world that has been running.

Living cells and panes are still untouched — a grant takes ground, never anybody's life. And dead ground is what the rule hands around freely anyway, since a corpse's owner flips to whoever grows over it. The block is then placed on the nearest 2×2 of free ground to the middle rather than blindly in it, because the middle four may be somebody's life, and a block with a cell missing is not a still life — it is three cells that die. Grants are laid out in a **square**, not a line: a line puts the last player thirty patches from the first, so the two could never reach each other and the map is a corridor. A square keeps everyone within a few patches of several others, which is the only arrangement in which territory meeting territory is something that happens.

**And the square grows with the roster.** Seats are a spiral out from the origin, so four players fill a 2×2, nine fill a 3×3 and sixteen a 4×4 — filled, not merely bounded. A fixed grid six across, filled in reading order, put the first six players in a line, which is the very thing the square arrangement exists to avoid; on a world with three or four people on it that was the usual case rather than the edge one. A spiral rather than laying out a grid for whoever is present, because a seat must never move: a position is a function of the player's number alone, so your ground stays where it was put when somebody else joins.

A seat that has ended up **inside somebody's country** is given up, and the player is put out at the nearest one that is nobody's. What counts as a country is a bar rather than any foreign cell at all: a grant claims dead ground whoever held it and steps its block around anything alive, so a few of a neighbour's squares cost nothing, and it takes a real border to move anybody. A patch already granted keeps its `HOME` marks and is recognised as its owner's however the world around it changes — a grant runs again on every rejoin, and a seat that wandered would hand a returning player a second patch every time.

The world decides the spacing. An infinite one has room, so the grid sits at a fixed pitch centred on the origin and the world grows in every direction rather than off into one quadrant — a patch, plus **three chunks of no-man's-land** between one player's edge and the next's. Measured in chunks because that is the unit the world is built and drawn in, and because how far away your neighbour is, is a question about the map rather than about the size of a patch. It was three patches' worth, thirty-six cells, and that read as close: two grants nearly touching at chunk scale, with the halo each block holds already a third of the way across. What the gap buys is the time before anyone's territory meets, and it wants to be enough to build a machine in. A torus does not — its ground is finite and has to be shared out — so the same grid is spread over whatever there is and **every player still gets their square**, on a small world as much as a large one. A world too small even for that says so at startup; the earlier players keep theirs and the later ones get what is left.

Which means the client cannot work out where its own ground is: that depends on the shape of the world, and it does not know the shape until it is told. `Welcome` carries the spawn: four cells that hold their shape forever, the same for everyone, so nobody begins ahead. The block is also what keeps the ground, since territory spreads from living cells and a bare patch would never grow. An offline client grants itself the same thing, or a game of one would have no opening move.

**Ground is traded and lost as well as won.** Where nothing alive is touching a dead cell, it takes the owner of a neighbouring cell — whoever that is, **including nobody**. So the same rule spreads your ground and eats it: inside your territory every neighbour agrees and nothing moves, at the edge it is a coin weighted by how much of the ground around it is yours, and a thin trail is nearly all empty neighbours and goes quickly. A slow decay on top means ground nothing lives on eventually goes rather than settling into a shape and staying.

Territory used to only ever spread, so a glider left a permanent trail and the world grew for as long as anything moved; a map that only fills up is not one anybody competes over.

Life holds the ground around it, because a square touching something alive is claimed before creep or decay ever see it. So your territory is a **halo around your life** whose edge moves both ways, and holding ground means keeping something alive on it. `cargo run --no-default-features --example territory` draws it.

Every chance in the rules is out of **sixty-four**, per cell, per generation — `SPREAD` 40, `CREEP` 8, `DECAY` 2, `MINE_UPKEEP` 16. One denominator, so a constant is a chance and nothing has to say which way round it reads.

Your **granted patch is the exception** and never decays. It is no longer the difference between playing and not — placing outside your own ground is a price now rather than a refusal — but it is the ground the cheap rate applies on, and a player who has lost everything else still has somewhere to build at one a cell. It is a mark on the ground rather than on the cells, so it survives the ground changing hands, and an opponent who grows over your home keeps it as theirs.

## Ice

Ice is a schematic. Freezing a region lets a large pattern be laid out over many generations without the rule eating the half-built work, and **shattering clears the ice flag and nothing else** — so what was drawn underneath, alive cells and deliberate gaps alike, starts living exactly as it was drawn. That it also walls other players off is the same mechanic pointed outward.

**A pane cannot be taken back.** It stops time over whatever it covers, and being able to lift one at will would make it cheap to undo as well as strong to place. What removes ice is life reaching it, which an opponent can arrange with a glider and the owner cannot simply click away — so laying one is a decision you are committed to. Holding Ice and clicking a pane says so rather than doing nothing, and the server refuses the action as well, because a client that sends whatever it likes is the case that check exists for.

A pane freezes what it covers. It is a flag, not a kind, so a cell may be alive, iced, both or neither.

A pane belongs to whoever laid it. There is **one owner field per cell**, so icing another player's living cell takes the cell with it — deliberate, and part of why a pane costs what it does. If ice should never transfer a cell, that needs a second owner field, which the cell has no spare bits for at sixteen.

Any live cell in the eight neighbours shatters the whole connected run, **even if it is the generation that cell dies in**. It is alive and it is touching, and that is the whole of what breaking means: a cell about to die has still crashed into the pane. So the seeds are taken before the rule runs and acted on after it — taken after, a cell that died on the way would already be gone and the pane would stand, which reads as ice ignoring something that plainly hit it. Acted on before, the pane would come off in time for this generation's rule and a pattern drawn under ice would take its first step in the same breath as being uncovered. One consequence: a cell *born* beside a pane breaks it a generation later, since it was not there when the seeds were taken — placed or born, yours or anyone's. The one exception is a cell that is itself under ice, which is frozen and cannot break what covers it. A pane laid tightly over a pattern is short-lived, because life is born just outside it and breaks it at once; give it a cell of margin and the pattern cannot break its own cover. That protects it from itself and from nothing else: **a glider flown into a pane shatters it on contact**, however well sealed it is. See [simulation](simulation.md#ice).

## Controls

| | |
|---|---|
| left click | act on one cell |
| left drag | draw with what you hold: a stroke of life, a pane of ice |
| middle drag, right drag, space + left drag | pan |
| arrows / WASD, shift to hurry | pan |
| mouse wheel | zoom |
| ctrl + wheel, trackpad pinch | zoom, on any device |
| trackpad scroll | pan |
| ctrl + scroll, trackpad pinch | zoom |
| one finger | draw |
| two fingers | pan and zoom, together |
| escape | abandon the drag in progress |
| 1–9 | choose a hotbar slot |

On the web the page prevents only the defaults that would fight the game: the arrows and space, which scroll a document; the context menu, because right-drag pans; middle-click autoscroll, for the same reason; and **every wheel over the canvas**, which is always the game's — ctrl and a wheel is a trackpad pinch the page would take as its own zoom, and a sideways wheel is a two-finger swipe Chrome takes as going back a page. That listener is on the canvas rather than on the window, and that is the whole reason it works: browsers make wheel listeners on `window`, `document` and `body` passive by default, and a passive listener cannot prevent anything. `overscroll-behavior: none` is the other half, for the gestures a browser decides on before anybody gets an event. Everything else reaches the browser — winit prevents everything it handles by default, and that took `F12` and `ctrl+shift+I` with it, so the page ate the key that opens the inspector and a build misbehaving in a browser could not be looked at. Touch is settled in the page's CSS instead, with `touch-action: none`, because a browser decides what a gesture means before it delivers an event anybody can cancel.

A wheel and a trackpad arrive as the same winit event, and the only thing separating them is the unit: a wheel reports discrete lines, a trackpad continuous pixels. Splitting on that is what makes the gestures consistent — treating every scroll as zoom made a two-finger swipe lurch the zoom where every other application pans.

Drawing and moving the view are never the same gesture, so neither has to guess which was meant. Three ways to pan rather than one because the middle button does not exist on a laptop trackpad and the right button is not always reachable either; space and drag is what a drawing tool does, and works everywhere.

On a touchscreen **one finger draws and two move the view**. Two fingers pan and zoom as one gesture — spreading while travelling does both — because they are one motion of the hand, and anchoring the zoom on the fingers' midpoint is what keeps the world under them instead of sliding it out. The split is the same one the mouse makes: the primary pointer draws, a second gesture moves the view. The alternative, one finger panning as a map does, leaves nothing to draw with, and the hotbar has already promised the player is holding something.

Letting go of a pan while still moving lets the view coast and settle. A press, a key, a scroll or a pinch stops it: they are all aiming at something, and a view still sliding would take the target away.

A gesture that began on the world keeps the pointer until it ends, even if it strays over a panel. Otherwise a drag released over the hotbar is swallowed, the rectangle is never filled, and the gesture stays open with nothing to close it.

## The hotbar

Two segments, and one thing selected across both:

```
    [ Life  Mine  Turret │ Ice ]   [ Grab  stamps … +7 ]
```

**Every square shows a picture rather than a word.** Life, Mine, Turret and Ice are drawn from the same sprite sheet the world is drawn from, tinted with the same hue, so what you are choosing is what will be on the board — which is where you are looking. Grab is a camera, painted rather than sampled, because capturing is not a cell and the sheet has no picture of one. A stamp shows **the pattern it holds, drawn from the same sheet**: `2x2` said nothing about what was about to be placed, and at button size a glider is a glider and a block is a block. It shows what the pattern is *made of* as well as its shape, because a stamp carries the kind of every cell in it — a gun built of mines is a different thing from one built of life, and a turret is a third, so a thumbnail with only the shape was hiding the half that decides what it costs and what it does. The names are still there, on hover.

The tools are the game's own vocabulary and never change; the stamps are whatever you happened to capture, and there may be none or thirty. Run together, the Ice key would move every time you saved a pattern. Ice sits with the tools but behind a rule, because it is the one that walls people off and should not be a neighbour of the one you draw with.

**The digits are the stamps** — `1` to `9` then `0`, which is ten and is why the bar holds ten — and **shift and a digit is a tool**. The stamps get the bare keys because they are what you hold ten of and swap between without looking; the tools are the game's own vocabulary and grow only when the game does, so they can afford a modifier. Adding the turret moved Ice from shift-3 to shift-4, which is the cost of that arrangement and is paid once per new tool rather than every time a pattern is captured.

Binding is by *physical* key, so it is the same key on every layout and only the **label** is ever in question. The label starts as what the common layout types — `!` `@` `#` `$` — and is corrected the moment a key says otherwise, so the great majority see the right thing on the first frame and somebody on Programmer Dvorak, where the digits are shifted to begin with, is only shown the wrong one until they use it.

Guessed rather than asked because there is no portable way to ask: `navigator.keyboard.getLayoutMap()` would answer properly on the web and is Chrome-only and asynchronous, and natively there is nothing.

## Stamps

A pattern captured once and placed again. Nothing on the wire is a stamp: placing one is a `Paint` over the cells it covers, judged against territory and value like anything else.

**Grab** is a square of its own, at the front of the stamps segment. Hold it, drag a box round your own life, and what was inside becomes a stamp. It needs its own square because otherwise there is nowhere to start — capturing was "drag with a stamp held", which is fine once you have one and impossible before. Holding an existing stamp and dragging captures too, since by then you are already thinking about stamps.

A stamp is **the live cells and their kind**, not the rectangle you swept, and the thumbnail draws each of them as the cell it will become.

**Or draw one.** The library has a pad on it: pick a kind, click cells or drag a run of them, and keep what you drew. Capturing needs something already alive and standing where you can reach it, which makes the first stamp of a session the hardest one to get and makes trying a pattern out mean building it first — a pad needs nothing. What is kept is trimmed to what was drawn, so which corner of the pad you used is not part of the pattern, and a drawn stamp is a stamp like any other: the same `Paint` on the wire, priced and judged the same way.

The pad asks the same questions the board does. A click lays a cell, or lifts it if what is there is already what you are holding; a drag only ever lays, because a sweep across cells already drawn is far more likely to be drawing over them than asking for them back. What the pad holds is its own choice and not the hotbar's — a pad that changed what your next click on the world would do would be a trap. The dead ones are gaps, and a stamp carrying them would wipe whatever it was placed over; the kind travels because a gun built of mines is a different thing from one built of life. It trims to what it caught, so a sloppy box round a glider still gives you a glider — and it takes only *your own* life, because somebody else's pattern is a thing they built.

Placing one puts its middle under the pointer, and goes as one action per placement it holds, priced whole: half a pattern is not the pattern. Past ten, the rest are behind the library key — which is **always** there, not only when something has overflowed, because the library is where a stamp is looked at and thrown away as well as where the extras live. It shows each pattern at a size you can recognise, beside its size and cell count.

**Not yet: the double cost.** `planned.md` decided a stamp should cost twice what drawing it would, and it does not — it costs the same. Doubling needs the action to say on the wire that it is a stamp, or the client charges double and the server charges single and the two disagree about money.

Picks what a click acts on — both what it places and, on ground that already has it, what it takes back — and what a drag with it held lays. Slots are data in `client::views::hotbar::SLOTS`, so adding one is a row: a name, a `Placement`, and a `Stroke`. The placement is named for what is put down, since a cell is the square and life is one of the things that can be on it.

Mine is a pencil rather than a rectangle, because a mine is placed a few at a time and *into* a pattern — what it is worth depends on what it is next to — where a pane is a wall you lay out. Turret is a pencil for the same reason and a shorter stroke: four cells in a block is a gesture, and a gesture is what a pencil is for.

What is being placed travels in the action as a named `Placement`, not as cell bits: the server has to judge whether a placement is allowed, and it can only do that against a vocabulary it understands. A client that could send arbitrary bits could place anything.

## Matches

A **match** is a room that starts together, ends, and has a winner. It gathers first: you can join it, and until it starts **nothing happens at all** — the world does not step and nothing you do is taken. That is not an honour system, it is a screen. A still world with a hotbar that does nothing is indistinguishable from a broken game, so a match that has not started shows a panel over the board saying so, listing who else is here and how it will be won.

**A match's world does not exist until it starts.** Nobody is granted anything while it gathers — no patch, no block, and no value at all. Granting on arrival would put the first player's block on a world the last player has not seen yet, and would hand out ground in the order people happened to click; so a gathering match is an empty world and a list of names, and the whistle lays every seat at once, in player order.

Starting with **nothing to spend** is the other half of the same rule. An ordinary room hands out a hundred so you can build the moment you arrive; in a match that would be an opening bought rather than played, and whatever you did with it before the whistle would be a head start measured in wall-clock time.

Letting people build while gathering was tried and is wrong. It is fair in *generations* — the world is frozen, so nobody gains any — and unfair in **time**, because holding the tick still does not hold a clock still and whoever joined ten minutes early has had ten minutes to think. So the opening is a race: everybody is looking at the same thing when the clock starts, and hesitating costs generations rather than nothing.

**No late joining.** Somebody arriving at generation four hundred is not in the same race — everybody else has four hundred generations of ground and they have a block — so the join is refused with a reason rather than allowed and hopeless. Coming back to a match you are already in is a different question, and your token still gets you your own seat.

**A running match says how much of itself is left**, along the top of the screen. A board in a match otherwise looks like any other world, and a player cannot tell whether there are ten generations left or ten thousand — which is the whole of the difference between a match and a sandbox, since everything you decide in one depends on how much is left to decide in. Along the top because it is the one thing on screen that is about the *room* rather than about you: the HUD's corner is a player's own business.

A timer says the generations left and the same figure as minutes and seconds, from the rate the world is stepped at. A territory match says how close the **leader** is to the target instead, because the question a target asks is how close anybody is to ending it. Both draw a bar underneath, which is the part a number does badly — "1240 left" means nothing without "of what" — and it turns colour in the last tenth, so the end of a match is visible without reading anything.

It is won either on a **timer**, which is most ground when the generations run out, or on **territory**, which is first to a number of squares. Granted ground counts for neither: your home patch never decays, so scoring it would be points for having turned up.

## The menu

The screen before the game. A name, a server, and the rooms that server has, or "play alone".

The list **refreshes itself** every few seconds while it is on screen, and there is a button for when you have just made a room on another screen and do not want to wait out the interval. A list is a photograph of the moment it was asked for — rooms come and go, matches start — and one that is only right when you remember to refresh it is one you cannot trust. Each room says whether it is a match and what it is doing, because a room and a match are the same thing to everything else, and finding out by clicking into one that has already started and being refused is a worse way to learn it.

The room list is **asked for, not guessed**. A room is a whole separate world, so a name that does not exist is not a mistyped filter, it is nowhere — and a client cannot know what a server has without asking. So the menu shows nothing under Rooms until `ServerMessage::Rooms` comes back, rather than offering a name that might be there. A server that never answers becomes a message naming the address after eight seconds, because a menu that says "asking" forever is indistinguishable from one that is broken, and the two likeliest causes — a wrong address, and a server that is not running — both look exactly like it.

**The menu has the screen to itself.** The world used to be drawn behind it, on the reasoning that a menu over a dead grey rectangle says the game has not started where a menu over a world says it is waiting for you. That was true while the menu was a small panel in a corner, and stopped being true once it was not: a world sliding about behind a full-height panel is motion nobody asked for beside the thing they are reading. What the extra room bought is buttons the size of things you click rather than the size of a HUD row.

A **gathering match** gets a screen of its own for a plainer reason — its world does not exist yet, so there is nothing to draw behind it. A **decided** one keeps its board, because the result is what is on it and covering that to say who won would hide the reason why.

**There is a way back.** An arrow in the corner of the HUD, and a button in the lobby, return to the menu from wherever you are. The socket is kept and the room list asked for again, so going back is a step rather than a disconnection: your seat is held until another join takes its place, which is what the server treats a second join as anyway.

On the web the server is shown but not editable: the socket is derived from the page's origin, so a typed address would be a promise the client cannot keep. Natively it is a field, because there is no page to have come from.

## The HUD

Player and their colour, value, generation, **who is winning**, connection state, which room, whether the world wraps, and why the last action was refused.

Who is winning is a column of **bars**, one per player in their own colour — the same one the shader gives their cells, so a bar and the ground it counts cannot disagree about whose it is. Bars rather than figures because the question is who is ahead and by how much, which is a comparison: six numbers in a column have to be read and subtracted, where six bars are one glance. The numbers sit beside them for when it is close. Scaled to the leader rather than to the world, since what is being asked is how the players compare with each other and against a boundless world every bar would be a sliver. Six of them, most first, because thirty-one people can have been through a world and a column of thirty-one bars is a screen of its own.

The counts come **from the server**. A client holds the chunks it subscribed to, which is its own screen, so counting locally would score the view rather than the world. They arrive every eight generations — a pass over the world to work out, and a bar that moved four times a second would be harder to read than one that moves every couple of seconds — and again the moment a match is decided, whatever the cadence says, because the last one is the result. Granted ground is not counted: `HOME` never decays, so scoring it would be points for having turned up.

"Chunks held" is what the client has **asked the server for**, not what its world has room for. A torus is allocated whole, so the second number there would be the size of the world and would say nothing about what has arrived.

The room and the world's shape are there because both are invisible otherwise. A room is a whole separate world, so two players who cannot find each other are far more likely to be in different rooms than at different ends of one; and nothing on the board says whether walking east far enough brings you back.

It can also report the cell under the cursor, whether the pointer is over the panel or the world, what the last click did, chunks held and drawn, the zoom, and the list of keys — but **not by default**. Every one of those earned its place while something was being built and none of them is what somebody playing wants a third of their screen taken by, so they are behind `hud::DEBUG`, off. Off rather than deleted, because each is the fastest way back to a whole class of bug: a stuck pointer-over-panel silently eats every click, and a click on empty ground that takes nothing looks exactly like a click that never arrived.
