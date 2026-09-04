# The game

Conway's rules, with owners and an economy on top.

## Value

Every player has a `value`, on `sim::Player`. It is what they have to spend.

| | |
|---|---|
| place life | −1 each |
| place a factory | −10 each |
| place a turret | −15 each, and four is the smallest one that works |
| place ice | −5 each |
| **anywhere but your own ground** | **×10**, whatever is being placed |
| **a factory of yours is born** | **+1** |
| **a dead factory of yours, each generation it lies there** | **−1**, sixteen times in sixty-four |
| reclaim your own | +1 |
| take another player's | −1, because taking ground should not be free |
| take what is not there | nothing |

Life is cheap because it is drawn by the stroke rather than placed cell by cell: a pencil lays tens of cells in a gesture, and at five a cell that is a gesture nobody can afford. Ice stays dear because a pane is a wall, and a wall that costs what a cell costs is not a decision.

Life at one against reclaiming at one means putting a cell down and taking it back is free, which is deliberate — rearrange your own board as much as you like. **What drains value is the rule.** A cell that dies of its neighbours cannot be reclaimed, so the sink is mortality rather than the act of placing, and the game is about drawing patterns that survive. Starting value is 100.

The taking rule reads the same for life and for ice, because the question it asks is whether the thing being taken is there and whose it is, not which of the two it is.

An action that cannot be afforded is refused. The client prices and refuses locally on the same terms the server would, so a refusal is instant rather than a round trip away, and the two cannot disagree because `net::value_delta` is one function used by both.

Cost is read **before** the action is applied, since it depends on what is there now.

## Manufacture

A **factory** is a living cell that pays its owner every time one of its kind is born. It is bought once and inherited afterwards: a birth copies its parent, so a factory's children are factories — and because a birth picks one of three parents at random, the kind spreads through a mixed population rather than being handed down whole. What you are paying for is a **lineage**, not a cell.

**A factory's corpse costs once and is then ordinary ground.** The charge falls due at `MINE_UPKEEP`, sixteen in sixty-four, and when it does the square loses its kind — so a factory field is a debt with a bottom to it rather than one you cannot pay off.

It costs `MINE_DRAIN`, which is **two** against a birth's **one**. That is what decides which patterns pay, and the discriminator is *how long your corpses lie about*: a corpse that is reborn before its charge falls due escapes it entirely. A blinker re-uses its own ground every other generation and escapes most of them; a glider abandons its corpses behind it and escapes none. That is what stops manufacture being the answer to everything: making every cell you own a factory used to be free money, because a settled pattern gives birth constantly and each birth paid, and the wreckage was free.

So income is births minus the upkeep of everything you have let die, and the thing being rewarded is a **compact machine**. `cargo run --no-default-features --example balance` prints the table, in value per generation at steady state:

| drain | block | blinker | glider | r-pentomino |
|---|---|---|---|---|
| 1 | −40 | +434 | +137 | +11552 |
| **2** | **−40** | **+298** | **−276** | **−7754** |
| 3 | −40 | +162 | −689 | −27060 |

Net over three hundred generations, placement included. **Two is the line where a blinker pays and a glider does not**, and that is the rule the number was chosen to satisfy: a machine that stays where you put it earns, and anything that wanders off dragging a trail of corpses costs. At one everything pays and sprawl pays best, which is where this started; at three even a blinker is barely worth building.

A block of factories is the honest edge case: nothing is ever born and nothing ever dies, so it neither earns nor costs. It is simply forty spent on nothing.

A corpse whose ground decays away is never charged at all, because nobody owns it to charge — so a trail far from anything alive fades before the bill arrives.

Three constants, and they are one decision: `MINE_COST` against `MINE_YIELD` is a factory's payback period, and `MINE_UPKEEP` decides how much a mess costs to hold. All of them live in `sim::rule` with the rest of the tunable numbers, which is where anybody balancing the game should be looking.

Value is capped at **`Player::MAX_VALUE`**, six figures, and floored at zero. The ceiling is a rule rather than a display: manufacture pays on birth and births scale with a growing pattern, so income runs away from a big player and nothing in the rules pushes back — see [depleted factories](planned.md#depleted-factories), which is the shape of a proper answer. This is the blunt half of it, and it does the second job of making the figure a fixed six columns on the bar.

Value is floored at zero. A cost that comes from an action is refused when it cannot be paid; a drain arrives whether or not there is anything to take it from, and a player in debt would be one who cannot act and has no way to stop owing.

A factory's corpse keeps its kind, as any cell does — so ground a factory died on shows the factory sprite, and life born there from an ordinary parent is ordinary. Placing plain Life over it explicitly sets the kind back to normal, or drawing over a factory's corpse would hand you a free factory.

## Turrets

A **turret** claims ground at range. Every generation it takes the nearest square that is not yours and makes it yours — wherever that is, out to six cells. It is the opposite of a factory in every way that matters: a factory earns on turnover and a turret works by standing still, a factory is bought once per lineage and a turret once per cell, and a factory wants a machine that keeps giving birth where a turret wants one that never has to.

**A turret is placed in fours.** One turret is one live cell with no live neighbours and is gone in a generation, so the smallest turret that works is the 2×2 block — the cheapest thing in Conway that never dies and never gives birth. Sixty against a starting hundred, which is an opening you can afford exactly one of.

That the block gives no births is the point rather than a cost. A block of factories is the honest edge case above, forty spent on nothing; a block of turrets is the best thing a turret can be. **The still life is a factory's worst shape and a turret's best.**

How much it takes is `rule::TURRET_POWER`, and that number decides what a turret is *for*. Low, and a claim is taken straight back by the ordinary spread of territory wherever anything is alive, so a turret is a way of reaching past your own frontier into empty land. High, and it holds ground against a living neighbour and becomes a way of pushing on one. A dead turret gives back whatever a live one would take.

A turret takes **ground**, not the life standing on it. It will not claim a square something is alive on, whoever owns it, and it will not touch what is under a pane. What it does take is dead ground, including ground nobody has ever held — which is what makes it a way to reach past your own frontier rather than a way to fight over somebody's colony. Against a living neighbour it barely works at all: their life takes the square straight back through the ordinary spread of territory, and a turret only claims one square a generation.

**A turret works from anywhere in your country.** Its first choice is always ground that is not yours, so one on your frontier reaches past it exactly as before. When everything within reach is already yours there is nothing to take, and rather than idle it **reinforces**: it plants your thinnest square back up to full, which feeds the border through the ordinary spread of territory instead of at it. So a turret in the middle pushes slowly and indirectly, one on the edge pushes directly, and neither needs a rule about where it may be placed.

### When one dies

A dead turret runs the whole thing backwards. It takes the nearest square that **is** yours and gives it up, and since a live cell must have an owner, doing that to a square you have something living on kills it. That is not a separate rule about killing — it is the same rule read in the other direction, and taking a square away from its owner is the only thing it does.

So a failed emplacement fires on the ground behind it, including the other three cells of its own block, and a block that loses a cell dismantles itself. A dead turret decays back to ordinary ground four times in sixty-four, so it does that for about sixteen generations before it stops.

Granted ground is exempt, for the same reason it never decays: it is the ground you can always build on at the ordinary price, and a machine of yours that failed must not be what takes that away.

A turret is not inherited. A factory's children are factories, but a birth beside a turret is ordinary life owned by the turret's owner — the ground changes hands and the machine does not copy itself. Without that a gun would be a turret factory, and whoever built one first would own the map.

## Placing and taking

One button, and the cell under it decides which — for whatever the hotbar is holding:

- what you are holding is already there → take it back
- it is not → put it down

Keyed on what is held rather than on whether the cell is occupied at all, because **life and ice are independent**. Holding Life and clicking a living cell under a pane kills the life and leaves the pane standing; clearing the square outright would destroy a pane the player never aimed at, at five a cell. Holding Ice and clicking that same pane lifts it and leaves the life, which is the only way a misplaced pane comes back.

**Life and a factory are different things to hold, and clicking one over the other replaces it.** Holding Factory and clicking your own living cell makes that cell a factory, at what a factory costs; holding Life and clicking a factory makes it ordinary life again, at what a cell costs. It used to kill the cell instead, because the question being asked was whether taking the held thing away would change anything — and life and a factory are both taken away by clearing the same bit, so a factory held over life read as already being there. What a player holding Factory over their own life means is *make this a factory*, and the only click that should take a factory back is one holding a factory. `net::Placement::is_on` is the question now, and it asks whether the square holds *this*.

That is a different relationship from the one life has with ice. Life and ice are **independent** — a square may carry both, so holding one and clicking the other leaves the other standing. Life and a factory are **exclusive**: they are the same cell in two states, so holding one and clicking the other converts it. A corpse is neither, whatever kind it kept, so a click over a dead factory places rather than takes — which is what stops drawing over one handing out a free factory.

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

Both the other arrangements are out and they went together. Placing anywhere for ten times the price was too weak — no obstacle at all to anybody with a factory running — and it made the map somewhere you bought your way into rather than somewhere you grew into. Grading the price by how thin your influence was went with it: a cost the player cannot see is a cost they cannot play around, and once ground stopped being shaded by its level there was nothing on screen to read it off.

A wall was tried before levels and was worse than either, because a player whose life went out was finished — they could not place, so could not grow ground, so could not place. The wall is safe now for a reason that has nothing to do with price: **granted ground is a source**, so everybody always has a patch with a live gradient to build on.

`net::may_place` is the whole rule, and the client refuses on the same terms the server does — instantly, and with the same answer. A drag is all or nothing, so one cell out of reach refuses the stroke rather than trimming it.

Both sides ask it through `net::may_place_under`, which reads the rule and the room's `Rules` together — a laboratory can take it off, and the rule and the switch that takes it off have to be read in one place or a client predicts a placement the server refuses. `net::price_under` is the same arrangement for what an action costs. See [planned.md](planned.md#experiments).

That is what a grant is for. A player who owned nothing could do nothing at all, since there would be nowhere their influence reached — so joining claims a 12×12 patch with a **2×2 block** standing in the middle of it, which is somewhere to build from the moment you arrive.

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

Your **granted patch is the exception** and never decays, which is exactly what makes the wall above safe to have: a player who has lost everything else still has a patch with a live gradient around it, and so still has somewhere to build. It is a mark on the ground rather than on the cells, so it survives the ground changing hands, and an opponent who grows over your home keeps it as theirs.

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
| middle drag, right drag | pan |
| arrows / WASD, shift to hurry | pan |
| mouse wheel | zoom |
| ctrl + wheel, trackpad pinch | zoom, on any device |
| trackpad scroll | pan |
| ctrl + scroll, trackpad pinch | zoom |
| one finger | draw |
| two fingers | pan and zoom, together |
| escape | abandon the drag in progress |
| 1–9 | choose a hotbar slot |

On the web the page prevents only the defaults that would fight the game: the arrows and space, which scroll a document and are the walk cluster and the pause key here; the context menu, because right-drag pans; middle-click autoscroll, for the same reason; and **every wheel over the canvas**, which is always the game's — ctrl and a wheel is a trackpad pinch the page would take as its own zoom, and a sideways wheel is a two-finger swipe Chrome takes as going back a page. That listener is on the canvas rather than on the window, and that is the whole reason it works: browsers make wheel listeners on `window`, `document` and `body` passive by default, and a passive listener cannot prevent anything. `overscroll-behavior: none` is the other half, for the gestures a browser decides on before anybody gets an event. Everything else reaches the browser — winit prevents everything it handles by default, and that took `F12` and `ctrl+shift+I` with it, so the page ate the key that opens the inspector and a build misbehaving in a browser could not be looked at. Touch is settled in the page's CSS instead, with `touch-action: none`, because a browser decides what a gesture means before it delivers an event anybody can cancel.

A wheel and a trackpad arrive as the same winit event, and the only thing separating them is the unit: a wheel reports discrete lines, a trackpad continuous pixels. Splitting on that is what makes the gestures consistent — treating every scroll as zoom made a two-finger swipe lurch the zoom where every other application pans.

Drawing and moving the view are never the same gesture, so neither has to guess which was meant. Two buttons pan rather than one, because the middle button does not exist on a laptop trackpad and a right drag does. Space held used to pan as well — the convention in a drawing tool — and it is **play and pause** now, which is the stronger claim on it: panning also has the walk cluster and the arrows, and a pause key has nowhere else obvious to be.

On a touchscreen **one finger draws and two move the view**. Two fingers pan and zoom as one gesture — spreading while travelling does both — because they are one motion of the hand, and anchoring the zoom on the fingers' midpoint is what keeps the world under them instead of sliding it out. The split is the same one the mouse makes: the primary pointer draws, a second gesture moves the view. The alternative, one finger panning as a map does, leaves nothing to draw with, and the hotbar has already promised the player is holding something.

Letting go of a pan while still moving lets the view coast and settle. A press, a key, a scroll or a pinch stops it: they are all aiming at something, and a view still sliding would take the target away.

A gesture that began on the world keeps the pointer until it ends, even if it strays over a panel. Otherwise a drag released over the hotbar is swallowed, the rectangle is never filled, and the gesture stays open with nothing to close it.

## The hotbar

Two segments, and one thing selected across both:

```
    [ Life  Factory  Turret  Ice ]   [ Draw │ Grab  stamps … +7 ]
```

**Two axes: what a cell is, and how the cells are chosen.** It was one — a row
of tools and stamps where picking any of them replaced everything about the
last — so a factory was always a pencil, ice was always a pane, and a stamp was
always whatever it had been captured as. A line of ice and a pane of factories were
not unimplemented, they were *unsayable*, because the stroke came attached to
the material. The left segment is the material and the right is the shape, and
picking one never disturbs the other.

Draw and Pane share **one square**, which says which is current rather than offering both: they are one choice with two answers, and two squares spent twice the room saying the same thing while making the unselected one look like a third thing you could be doing. A click takes the other; while a stamp is held the square says so, since the shape axis is where a pattern lives too, and clicking it is the way back to drawing.

Ice is among the kinds now rather than behind a rule. It used to sit apart
because it was the one that walls people off and because it came with a
different stroke; the stroke is the other axis, so what is left is a material
like the others.

**Every square shows a picture rather than a word.** Life, Factory, Turret and Ice are drawn from the same sprite sheet the world is drawn from, tinted with the same hue, so what you are choosing is what will be on the board — which is where you are looking. Grab is a camera, painted rather than sampled, because capturing is not a cell and the sheet has no picture of one. A stamp shows **the pattern it holds, drawn from the same sheet**: `2x2` said nothing about what was about to be placed, and at button size a glider is a glider and a block is a block. It is drawn in whatever the kind axis is holding, because a stamp is a shape and not a material — the same glider reads as life, as factories or as ice depending on what would come out of it, which is more useful than a fixed picture of how it happened to be captured. The names are still there, on hover.

The kinds are the game's own vocabulary and never change; the stamps are whatever you happened to capture, and there may be none or thirty. Run together, the Ice key would move every time you saved a pattern, which is why they are two segments.

**The digits are the stamps** — `1` to `9` then `0`, which is ten and is why the bar holds ten — and **shift and a digit is a kind**. The stamps get the bare keys because they are what you hold ten of and swap between without looking; the kinds are the game's own vocabulary and grow only when the game does, so they can afford a modifier. Adding the turret moved Ice from shift-3 to shift-4, which is the cost of that arrangement and is paid once per new kind rather than every time a pattern is captured.

**A pattern and the same pattern turned are one pattern.** `R` turns what you are holding a quarter clockwise, shift and `R` the other way, and `F` mirrors it — because a rotation cannot produce a reflection, so a glider has four turns and four more that are its mirror image and half of them would otherwise be unreachable. Without this the library fills up with its own reflections: four gliders, and the same again for every spaceship and every corner of a wall. The turn is **held rather than saved**, so it is part of what you are about to place and changes nothing in the library — there is nothing to store, migrate or forget — and the square on the bar shows the pattern as it would be laid, turn and all, because a thumbnail that stayed upright while the preview under the pointer rotated would be two answers to what is about to happen.

**The shape square is on the shifted row with everything else on the bar.** Pencil and pane are one choice with two answers, so they are one square that says which is current, and shift and its digit does what clicking it does — a toggle. It used to be the backtick, doing something *different* from the square it was drawn on: the key put the shape back to the kind's usual and the click toggled. It was also the least reachable key in the game on one of its most ordinary actions, since `~` is a dead key on the Spanish, Portuguese and Nordic layouts and produces no text at all. Now the whole bar reads left to right as shift and the digits, with no control outside the row. Flipping clears the turn with it, because a pattern left rotated after a flip is a reset somebody has to reset.

**Writing on the bar has a shadow under it.** Every square has a picture behind its text — a sprite, a captured pattern, the world through a gap — and thin light glyphs over a busy one are a smear rather than a word. A shadow rather than a panel, which would cover the picture the square exists to show, and rather than an outline, which at this size turns a glyph into a blob.

Binding is by *physical* key, so it is the same key on every layout and only the **label** is ever in question. The label starts as what the common layout types — `!` `@` `#` `$` — and is corrected the moment a key says otherwise, so the great majority see the right thing on the first frame and somebody on Programmer Dvorak, where the digits are shifted to begin with, is only shown the wrong one until they use it.

Guessed rather than asked because there is no portable way to ask: `navigator.keyboard.getLayoutMap()` would answer properly on the web and is Chrome-only and asynchronous, and natively there is nothing.

**The help screen learns too.** It is the one list that exists to be read by somebody who does not know the keys, so a row that says the wrong ones is worse than no row — and it said `WASD` to everybody, which is a name for a shape on the board and only spells itself on one layout. It shows what those four keys actually print, learned as they are pressed, and says `arrows` alone until it has anything to report rather than guessing. Nothing else on the list asks: every other row is either a key bound by *character*, which is the same label everywhere by construction, or a key with a name rather than a print.

And `?` has a square. A key nobody knows about is not a key, and `?` was discoverable only by pressing `?` — while the one place a player already looks to find out what something does is the bar, where every square teaches its own keystroke in the corner.

## Stamps

A pattern captured once and placed again. Nothing on the wire is a stamp: placing one is a `Paint` over the cells it covers, judged against territory and value like anything else.

**Grab** is a square of its own, at the front of the stamps segment. Hold it, drag a box round your own life, and what was inside becomes a stamp. It needs its own square because otherwise there is nowhere to start — capturing was "drag with a stamp held", which is fine once you have one and impossible before. Holding an existing stamp and dragging captures too, since by then you are already thinking about stamps.

A stamp is **the live cells and their kind**, not the rectangle you swept, and the thumbnail draws each of them as the cell it will become.

**Or draw one.** The library has a pad on it: pick a kind, click cells or drag a run of them, and keep what you drew. Capturing needs something already alive and standing where you can reach it, which makes the first stamp of a session the hardest one to get and makes trying a pattern out mean building it first — a pad needs nothing. What is kept is trimmed to what was drawn, so which corner of the pad you used is not part of the pattern, and a drawn stamp is a stamp like any other: the same `Paint` on the wire, priced and judged the same way.

The pad asks the same questions the board does. A click lays a cell, or lifts it if what is there is already what you are holding; a drag only ever lays, because a sweep across cells already drawn is far more likely to be drawing over them than asking for them back. What the pad holds is its own choice and not the hotbar's — a pad that changed what your next click on the world would do would be a trap. The dead ones are gaps, and a stamp carrying them would wipe whatever it was placed over; the kind travels because a gun built of factories is a different thing from one built of life. It trims to what it caught, so a sloppy box round a glider still gives you a glider — and it takes only *your own* life, because somebody else's pattern is a thing they built.

Placing one puts its middle under the pointer, and goes as one action per placement it holds, priced whole: half a pattern is not the pattern.

**The library survives the session.** It is written to the client's own store after every change — after, not on the way out, because a browser gives no reliable moment to save at. So a stamp is worth naming, and a name is a click on the one it already has.

**Which ten are on the bar is yours to choose.** Pin any and the bar is exactly what is pinned; pin none and it is the newest ten, which is what it always was. Not "your pins, then the newest of the rest", which would reshuffle the bar under your fingers every time you captured something. The bar has ten squares, so an eleventh pin is refused rather than silently doing nothing.

**Editing** opens a stamp on the pad, centred, and keeping puts it back where it was with its pin intact — a stamp that jumped to the top of the library and off the bar every time you corrected a cell is one nobody corrects twice. The pad says which of the two it is about to do.

The library is behind a key that is **always** there, not only when something has overflowed, because it is where a stamp is named, pinned, edited and thrown away as well as where the extras live. It shows each pattern at a size you can recognise, beside its size and cell count.

**Not yet: the double cost.** `planned.md` decided a stamp should cost twice what drawing it would, and it does not — it costs the same. Doubling needs the action to say on the wire that it is a stamp, or the client charges double and the server charges single and the two disagree about money.

Picks what a click acts on — both what it places and, on ground that already has it, what it takes back — and what a drag with it held lays. Slots are data in `client::views::hotbar::SLOTS`, so adding one is a row: a name, a `Placement`, and a `Stroke`. The placement is named for what is put down, since a cell is the square and life is one of the things that can be on it.

Factory is a pencil rather than a rectangle, because a factory is placed a few at a time and *into* a pattern — what it is worth depends on what it is next to — where a pane is a wall you lay out. Turret is a pencil for the same reason and a shorter stroke: four cells in a block is a gesture, and a gesture is what a pencil is for.

What is being placed travels in the action as a named `Placement`, not as cell bits: the server has to judge whether a placement is allowed, and it can only do that against a vocabulary it understands. A client that could send arbitrary bits could place anything.

## Matches

A **match** is a room that starts together, ends, and has a winner. It gathers first: you can join it, and until it starts **nothing happens at all** — the world does not step and nothing you do is taken. That is not an honour system, it is a screen. A still world with a hotbar that does nothing is indistinguishable from a broken game, so a match that has not started shows a panel over the board saying so, listing who else is here and how it will be won.

**A match's world does not exist until it starts.** Nobody is granted anything while it gathers — no patch, no block, and no value at all. Granting on arrival would put the first player's block on a world the last player has not seen yet, and would hand out ground in the order people happened to click; so a gathering match is an empty world and a list of names, and the whistle lays every seat at once, in player order.

Starting with **nothing to spend** is the other half of the same rule. An ordinary room hands out a hundred so you can build the moment you arrive; in a match that would be an opening bought rather than played, and whatever you did with it before the whistle would be a head start measured in wall-clock time.

Letting people build while gathering was tried and is wrong. It is fair in *generations* — the world is frozen, so nobody gains any — and unfair in **time**, because holding the tick still does not hold a clock still and whoever joined ten minutes early has had ten minutes to think. So the opening is a race: everybody is looking at the same thing when the clock starts, and hesitating costs generations rather than nothing.

**No late joining.** Somebody arriving at generation four hundred is not in the same race — everybody else has four hundred generations of ground and they have a block — so the join is refused with a reason rather than allowed and hopeless. Coming back to a match you are already in is a different question, and being the same person still gets you your own seat.

**A running match says how much of itself is left**, along the top of the screen. A board in a match otherwise looks like any other world, and a player cannot tell whether there are ten generations left or ten thousand — which is the whole of the difference between a match and a sandbox, since everything you decide in one depends on how much is left to decide in. Along the top because it is the one thing on screen that is about the *room* rather than about you: the HUD's corner is a player's own business.

A timer says the generations left and the same figure as minutes and seconds, from the rate the world is stepped at. A territory match says how close the **leader** is to the target instead, because the question a target asks is how close anybody is to ending it. Both draw a bar underneath, which is the part a number does badly — "1240 left" means nothing without "of what" — and it turns colour in the last tenth, so the end of a match is visible without reading anything.

It is won either on a **timer**, which is most ground when the generations run out, or on **territory**, which is first to a number of squares. Granted ground counts for neither: your home patch never decays, so scoring it would be points for having turned up.

## The menu

The screen before the game. A name, a server, and the rooms that server has, or "play alone".

The list **refreshes itself** every few seconds while it is on screen, and there is a button for when you have just made a room on another screen and do not want to wait out the interval. A list is a photograph of the moment it was asked for — rooms come and go, matches start — and one that is only right when you remember to refresh it is one you cannot trust. Each room says whether it is a match and what it is doing, because a room and a match are the same thing to everything else, and finding out by clicking into one that has already started and being refused is a worse way to learn it.

The room list is **asked for, not guessed**. A room is a whole separate world, so a name that does not exist is not a mistyped filter, it is nowhere — and a client cannot know what a server has without asking. So the menu shows nothing under Rooms until `ServerMessage::Rooms` comes back, rather than offering a name that might be there. A server that never answers becomes a message naming the address after eight seconds, because a menu that says "asking" forever is indistinguishable from one that is broken, and the two likeliest causes — a wrong address, and a server that is not running — both look exactly like it.

**The menu is the screen.** It fills the window and is drawn as the window: one surface, no border, with the content laid out as a column inside it. It used to be a card floating in a much larger dark field, which reads as cramped however wide the card is — the eye takes the border as the edge of the thing and everything outside it as room the game is refusing to use. There is no stroke for the same reason: a border says where one surface ends and another begins, and at the edge of the window there is nothing on the other side.

The content is still a column rather than the whole width. A room list stretched across two thousand points is one nobody can follow from a name to the count beside it, and prose that wide is unreadable; the screen is not cramped, so the content does not have to sprawl to prove it. It scrolls, because filling the screen does not make the screen taller.

**The world is not drawn behind it**, on the reasoning that a menu over a dead grey rectangle says the game has not started where a menu over a world says it is waiting for you. That was true while the menu was a small panel in a corner, and stopped being true once it was not: a world sliding about behind a full-height panel is motion nobody asked for beside the thing they are reading. What the extra room bought is buttons the size of things you click rather than the size of a HUD row.

A **gathering match** gets a screen of its own for a plainer reason — its world does not exist yet, so there is nothing to draw behind it. A **decided** one keeps its board, because the result is what is on it and covering that to say who won would hide the reason why.

**The play screen is two columns**, and the split says something true: on the left is what already exists — a list the server owns, which refreshes itself every few seconds whether or not you touch it — and on the right is what does not exist yet, which is a form, and yours, and stays where you left it. They are not two panels of the same kind, so they are not drawn as two: the list sits on the panel's ground and the form is a card. Below a screen width where two columns of form would be two columns of nothing, they stack.

One accent per **column** rather than per screen. Each column has exactly one thing you would do next in it — join the world you picked, or make the one you described — and they are in different places, so neither is competing to be the one thing.

**One refresh, beside the address, on both clients.** It briefly lived inside the branch that draws the address as a *field* — and a browser has a label there, because its socket comes from the page it was served by — so the web client had no button, nothing else able to ask, and no way to reach anything at all. The address is what differs between the two; asking is not.

Reaching a server for the first time and asking it again are the same act from where a player stands — tell me what is on there, now — so they are one control whose meaning follows the state rather than two that have to be told apart. The rooms column used to carry its own; that was the same button in a second place.

The client also reaches on its own when the typing settles: on enter, on leaving the field, or after a pause. Debounced rather than fired per keystroke, because `ws://127.0.0.1:8080/ws` passes through twenty addresses on its way to being one. That was briefly the *only* way in, and it was wrong: an address that had already been asked about was never asked again, so a server that refused once left a screen with nothing to press and retyping the same address did nothing at all. Pressing the button and pressing enter are **deliberate**, so they always ask; only the settle is guarded, and only against asking twice about one address with nobody having done anything. A refusal is retried on a slow cadence besides, because the usual reason is a server that is not running yet.

**Both columns are there whatever the server has said.** Gating them on a room list having arrived meant a server that refused once left no way to make a world either, which is not a consequence anybody would choose. The list says it is waiting; the form is a form either way and says what it needs at the point of pressing rather than by being absent.

The address field is **never blank**: it opens on what was used last, or on an example that works — filled in once on the way into the screen, not while the field is drawn, because a field that refills itself is a field you cannot clear to type your own. A hint is a shape; an example is a thing you can press enter on, and somebody who has never seen the game should be editing a number rather than inventing a URL. On the web it is not a field at all — the socket comes from the page's own origin.

**A new world is made in the right-hand column.** It asks one thing per row: a name, a shape, a size if the shape wraps, whether it ends, and the target if it does. A row appears only when the decision it belongs to is live — there is no size to give a boundless world and no target for one that never ends — which is borrowed from [Infinite Chess](https://www.infinitechess.org/), where the difficulty appears only once the opponent is a computer.

"Ends: Never" is what makes a world rather than a match, which is the honest way round: a room with no end is the ordinary case and a match is the one with a condition on it, so the form never has to ask "world or match?" as a question of its own. Making one does not put you in it — the client joins the name that comes back, which is the same join a room in the list sends, so there is one way into a world rather than two. What a server will hold is capped and set by `--max-rooms`; see [server.md](server.md#made-by-a-client).

### Teams

A match is played **solo or in teams**, chosen when it is made, along with how many teams there are. Who is on which team, and what each is called, is settled in the lobby: anybody may join any team, anybody may rename one, and joining the team you are already on leaves it.

**A team is a player, and the people on it are at its controls.** Everything a team does follows from that and needs no rule of its own. Its cells carry one number, so allies build on one patch of ground, spend one purse, are scored as one total, and are drawn in **one colour** — not a family of colours, one. A team is a competitor in the world exactly as a lone player is; what differs is how many hands are on it.

That is the second version of teams. The first made allies two players who were allowed to build on each other's ground: a `Sides` array said who was on whose side, every rule that read a cell's owner asked it, and the colour was a *family* of hue per team with each member on a narrow arc inside it so that allies read as allies without being identical. All of it was machinery for keeping two numbers behaving like one, and the two bugs it produced were both about the seam — see [server.md](server.md#teams).

The shader looks a hue up rather than computing one, and the table is a constant: a player's colour is their number stepped around the wheel by the golden ratio, which is what it was before teams existed. Nothing else about a cell changes with the player — the sprite, its lightness and its coverage all come from the sheet, and the player contributes a hue and a saturation tier.

What a team costs is a **number**. There are fifteen, and a team takes one, so a match with three teams seats twelve people. The simulation knows nothing about teams at all, the same way it knows nothing about matches or money — and now neither does anything else.

**Friendly fire is on.** A glider is a weapon whoever built it, and making allied life pass through allied life would be a rule the world has to honour rather than an arrangement of who is playing. Teams are about scoring and building.

The **evenness is checked at the whistle**, not in the lobby: a match will not start while somebody has not picked a team or while a team is empty, and the lobby says who. Sizes beyond that are left alone, because three against two is something people arrange on purpose. Teams are settled once it starts — changing them mid-match would hand your ground to the people you were fighting.

**A private match shows its code in the lobby**, where somebody waiting for their friends can read it off and send it. It appears once in the menu when the room is made and is gone the moment they leave that screen, which is a minute before they want it.

**A match is started by whoever made it.** A gathering match has a Start on its lobby panel, and only for the player whose room it is — anybody may join a gathering match, and if anybody could also start it the person who set it up could not wait for their friends. Everybody else is told what it is waiting for. Ownership is a `PlayerId` and not the connection that asked, so it survives a refresh: coming back as the same person brings you back to the same number, which is exactly when losing your own match would be most annoying. A match the server made stays the server's and starts at the console. Who blew the whistle is remembered and shown with the result.

**The room list is a selection.** Clicking a room picks it out and its actions appear inside it — Join, and Watch. Beside every row they made the list twice as tall and twice as busy to read, and most of those buttons belonged to rooms nobody was looking at. Arrow keys walk the list and wrap at both ends, enter joins what is picked, and tab moves between the controls on the screen; a focused control wears the accent, like a selected one.

**Escape leaves whatever you are in**, one level at a time and innermost first: a text field lets go of the keyboard **and of its selection**, then the page goes back, then a half-drawn rectangle is abandoned. Surrendering focus alone left the highlight painted where it was, so the field looked as though it still had the keyboard and there was nothing left to press. The innermost rung is the one that was missing and the one that was most annoying — egui takes the keyboard while a field has focus, so escape never reached the game, and a highlighted selection in a field you could not leave had no way out at all. Every screen having a key out of it is the habit taken from [chess-tui](https://github.com/thomas-mauran/chess-tui). There is a way back by pointer too — an arrow in the corner of the HUD, and a button in the lobby, return to the menu from wherever you are. The socket is kept and the room list asked for again, so going back is a step rather than a disconnection.

**Going back gives the seat up**, with `ClientMessage::Leave`. It used to send nothing, on the reasoning that the seat is held until another join takes its place — which is true of somebody rejoining the same room and false of everything else: the player stayed marked online, so the room went on counting them, and the way back — which only returns you to a player who is **not** online — found them online and made a new player instead. Leave and come back three times and a room with one person in it said three. Nothing is given up but the seat, so coming back is still coming back to the same player, the same value and the same ground.

On the web the server is shown but not editable: the socket is derived from the page's origin, so a typed address would be a promise the client cannot keep. Natively it is a field, because there is no page to have come from.

## Where you are, in the address bar

On the web, **each screen has its own address**, so a link means something more than "the front door", a refresh puts you back where you were, and a bug report can arrive as a URL.

| address | screen |
|---|---|
| `/home` | your name and your record |
| `/play` | a server and what is on it |
| `/room/ID` | in a world |
| `/lobby/ID` | waiting in a match, before the whistle |
| `/watch/ID` | watching a room without a seat in it |

The page is **one document served at several paths**, and the client never navigates between them. Moving from home to play to a room is `history.replaceState` — the address changes and nothing is fetched. The path is read exactly once, at startup, which is what makes a reload or a pasted link land where it says.

That is also why the page loads its module from an **absolute** `/pkg/…`. A relative `./pkg/…` resolves against the path the document was served at, so at `/room/arena` it asked for `/room/pkg/…`, got a 404, and showed a blank screen — invisible in normal use, because in-app movement never re-fetches anything, and immediate on a reload.

Paths, because that is what an address looks like. The objection to them was that the client is one file served out of `--serve`, so `/play` would be a request with nothing behind it and would 404 on a refresh — which is a fact about the server, and the server answers each of these paths with the page now. By name: an unknown path is still a 404, so `/src/main.rs` says no rather than quietly returning the client. See [server.md](server.md#what-is-served).

`?room=`, `?lobby=`, `?watch=` and `?screen=` are still **read**, because `?room=` was the link this game had before it had any others and links do not stop existing when a scheme changes. The path wins where both say something.

**A lobby and a room are one request read and two screens written.** Following either does the same thing — join that room — and what you get is whichever screen the phase calls for; what differs is what the address says while you are there, which is the point of having one per screen. It follows that a link can go stale: send somebody `?lobby=` and they open it after the whistle, and they are refused for being late rather than shown a lobby. That is the honest outcome, and the refusal says so.

Watching is its own address rather than a flag on a room, because "come and play" and "come and watch" are two different invitations and are answered by two different messages.

The address is **replaced, not pushed**. A client that pushed every screen change would fill the history with the six presses it took to get into a game, and the back button would walk them backwards rather than leaving — which is not what a back button means to somebody who wants out.

## The HUD, and the bar

Two places, split by whether you **watch** it.

The four figures that change while you play — your purse, the ground you hold, the tick and your rating — are a segment at the left end of the **hotbar**, beside a stripe in the colour the shader gives your cells. They were in the HUD, in the opposite corner from the squares and the pointer, which put the numbers you read most furthest from where you play. Each is the figure first and bigger with a quiet word under it: at a glance the number is what is being read and the word is what makes it mean something.

**Monospaced, and padded to their width.** These change every generation, and a proportional digit is a different width from its neighbour — so the figure grew and shrank, the label under it slid about, and the eye re-found both every time. Leading zeroes rather than spaces, because a leading space reads as the number having moved. A word each for now; [icons](planned.md#icons-on-the-bar) would be better and are what they are waiting for.

The **HUD** keeps what is about the connection and the room: whether the link is up and how well it is keeping up, which room, whether the world wraps, what the last match did to your rating, **who is winning**, and why the last action was refused. None of that is watched, which is what makes a corner the right place for it.

Who is winning is a column of **bars**, one per player in their own colour — the same one the shader gives their cells, so a bar and the ground it counts cannot disagree about whose it is. Bars rather than figures because the question is who is ahead and by how much, which is a comparison: six numbers in a column have to be read and subtracted, where six bars are one glance. The numbers sit beside them for when it is close. Scaled to the leader rather than to the world, since what is being asked is how the players compare with each other and against a boundless world every bar would be a sliver. Six of them, most first, because fifteen people can have been through a world and a column of fifteen bars is most of a panel.

The counts come **from the server**. A client holds the chunks it subscribed to, which is its own screen, so counting locally would score the view rather than the world. They arrive every eight generations — a pass over the world to work out, and a bar that moved four times a second would be harder to read than one that moves every couple of seconds — and again the moment a match is decided, whatever the cadence says, because the last one is the result. Granted ground is not counted: `HOME` never decays, so scoring it would be points for having turned up.

"Chunks held" is what the client has **asked the server for**, not what its world has room for. A torus is allocated whole, so the second number there would be the size of the world and would say nothing about what has arrived.

The room and the world's shape are there because both are invisible otherwise. A room is a whole separate world, so two players who cannot find each other are far more likely to be in different rooms than at different ends of one; and nothing on the board says whether walking east far enough brings you back.

It can also report the cell under the cursor, whether the pointer is over the panel or the world, what the last click did, chunks held and drawn, the zoom, and the list of keys — but **not by default**. Every one of those earned its place while something was being built and none of them is what somebody playing wants a third of their screen taken by, so they are behind `hud::DEBUG`, off. Off rather than deleted, because each is the fastest way back to a whole class of bug: a stuck pointer-over-panel silently eats every click, and a click on empty ground that takes nothing looks exactly like a click that never arrived.
