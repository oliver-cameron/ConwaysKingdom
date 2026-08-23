# The game

Conway's rules, with owners and an economy on top.

## Value

Every player has a `value`, on `sim::Player`. It is what they have to spend.

| | |
|---|---|
| place life | −1 each |
| place ice | −5 each |
| reclaim your own | +1 |
| take another player's | −1, because taking ground should not be free |
| take what is not there | nothing |

Life is cheap because it is drawn by the stroke rather than placed cell by cell: a pencil lays tens of cells in a gesture, and at five a cell that is a gesture nobody can afford. Ice stays dear because a pane is a wall, and a wall that costs what a cell costs is not a decision.

Life at one against reclaiming at one means putting a cell down and taking it back is free, which is deliberate — rearrange your own board as much as you like. **What drains value is the rule.** A cell that dies of its neighbours cannot be reclaimed, so the sink is mortality rather than the act of placing, and the game is about drawing patterns that survive. Starting value is 100.

The taking rule reads the same for life and for ice, because the question it asks is whether the thing being taken is there and whose it is, not which of the two it is.

An action that cannot be afforded is refused. The client prices and refuses locally on the same terms the server would, so a refusal is instant rather than a round trip away, and the two cannot disagree because `net::value_delta` is one function used by both.

Cost is read **before** the action is applied, since it depends on what is there now.

## Placing and taking

One button, and the cell under it decides which — for whatever the hotbar is holding:

- what you are holding is already there → take it back
- it is not → put it down

Keyed on what is held rather than on whether the cell is occupied at all, because **life and ice are independent**. Holding Life and clicking a living cell under a pane kills the life and leaves the pane standing; clearing the square outright would destroy a pane the player never aimed at, at five a cell. Holding Ice and clicking that same pane lifts it and leaves the life, which is the only way a misplaced pane comes back.

`Action::Erase` carries what to remove for the same reason `Paint` carries what to lay: the server judges an intent, and "kill the life on this square" is a different intent from "clear this square".

**A drag lays the shape the held slot lays**, and the two slots lay different shapes. Life is a **pencil**: every cell the pointer crosses, so you draw a pattern and watch the line appear under your hand. Ice is a **rectangle**: two corners, because a pane is a shape you place and dragging one out says how big before it exists. Which one a drag is drawing is fixed when the button goes down, so changing slot midway does not change a line already half drawn.

**A stroke is laid as it is drawn**, not when the button comes up. Holding it back meant every line appeared a moment after the hand that drew it, which reads as lag however fast everything else is — and it is a pencil, so it should behave like one. Each frame's new cells are priced and sent as a batch, so a stroke that runs out of value stops where the money did rather than being refused whole. The cells are their own preview; only a rectangle, which does not exist until it is released, still shows one.

The pencil marks every cell between one pointer position and the next, not just the ones it was reported at. Events arrive far apart when the hand moves quickly — a fast stroke crosses twenty cells between two of them — so marking only the reported positions would draw a dotted line. A stroke that crosses itself lists each cell once: the pricing compares every cell against the world rather than against the cells before it, so a repeat would be charged for twice and laid once.

A drag always places, never takes: a sweep across occupied ground is far more likely to be building over it than a request to clear it cell by cell, and an accidental sweep that wiped a structure would be unforgiving. Taking stays a deliberate single click.

What will be laid is drawn while you draw it, with its size and its price beside it, and a drag that cannot be paid for is drawn as refused before the button comes up. **A drag is all or nothing.** One laid as far as the value stretched would stop somewhere the hand did not, and the player would be left working out where it ran out and why.

Only the cells a placement actually changes are charged for. Extending a rectangle means sweeping the whole of it again, and paying twice for the part that was already there made the natural gesture the expensive one. This is why `value_delta` reads the world for a paint as well as for an erase.

One drag lays at most 4096 cells. A rectangle at one pixel per cell can cover millions, each to be listed, priced, applied and put on the wire. A stroke stops growing when it reaches the cap and says so in its label, rather than being trimmed at the end where nobody would see what was lost.

More than one cell is what makes a drag a drag. A press that travelled but stayed inside one cell would place where a click would take, so which of the two happens must not turn on a few pixels of hand shake at high zoom.

## Territory

Every dead cell carries an owner, and that is territory. The rule spreads it: a dead cell next to living ones takes one of their owners most generations, so ground is claimed by the life that grows over it. It stays dead — this sets the owner and nothing else. Ice is exempt while it stands, since a pane's cover is not claimed out from under it.

**A player may only place inside their own territory.** Ground nobody has reached belongs to nobody and is closed to everyone, so reach grows where life goes and nowhere else. `net::may_place` is the whole rule, and the client refuses on the same terms the server does — instantly, and with the same answer.

That makes a grant necessary. A player who owned nothing could place nothing and so could never come to own anything, so joining claims a 12×12 patch with a **2×2 block** standing in the middle of it. Grants are laid out in a **square**, not a line: a line puts the last player thirty patches from the first, so the two could never reach each other and the map is a corridor. A square keeps everyone within a few patches of several others, which is the only arrangement in which territory meeting territory is something that happens.

The world decides the spacing. An infinite one has room, so the grid sits at a fixed pitch centred on the origin and the world grows in every direction rather than off into one quadrant. A torus does not — its ground is finite and has to be shared out — so the same grid is spread over whatever there is and **every player still gets their square**, on a small world as much as a large one. A world too small even for that says so at startup; the earlier players keep theirs and the later ones get what is left.

Which means the client cannot work out where its own ground is: that depends on the shape of the world, and it does not know the shape until it is told. `Welcome` carries the spawn: four cells that hold their shape forever, the same for everyone, so nobody begins ahead. The block is also what keeps the ground, since territory spreads from living cells and a bare patch would never grow. An offline client grants itself the same thing, or a game of one would have no opening move.

Territory has no die-off yet, so it only ever spreads. A glider therefore leaves a permanent trail of claimed ground, and the world grows with it — deliberately, since territory that vanished the moment life moved on would be no territory at all.

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

A wheel and a trackpad arrive as the same winit event, and the only thing separating them is the unit: a wheel reports discrete lines, a trackpad continuous pixels. Splitting on that is what makes the gestures consistent — treating every scroll as zoom made a two-finger swipe lurch the zoom where every other application pans.

Drawing and moving the view are never the same gesture, so neither has to guess which was meant. Three ways to pan rather than one because the middle button does not exist on a laptop trackpad and the right button is not always reachable either; space and drag is what a drawing tool does, and works everywhere.

On a touchscreen **one finger draws and two move the view**. Two fingers pan and zoom as one gesture — spreading while travelling does both — because they are one motion of the hand, and anchoring the zoom on the fingers' midpoint is what keeps the world under them instead of sliding it out. The split is the same one the mouse makes: the primary pointer draws, a second gesture moves the view. The alternative, one finger panning as a map does, leaves nothing to draw with, and the hotbar has already promised the player is holding something.

Letting go of a pan while still moving lets the view coast and settle. A press, a key, a scroll or a pinch stops it: they are all aiming at something, and a view still sliding would take the target away.

A gesture that began on the world keeps the pointer until it ends, even if it strays over a panel. Otherwise a drag released over the hotbar is swallowed, the rectangle is never filled, and the gesture stays open with nothing to close it.

## The hotbar

Picks what a click acts on — both what it places and, on ground that already has it, what it takes back — and what a drag with it held lays. Slots are data in `client::views::hotbar::SLOTS`, so adding one is a row: a name, a `Placement`, and a `Stroke`. The two are `Life` and `Ice`: the placement is named for what is put down, since a cell is the square and life is one of the two things that can be on it.

What is being placed travels in the action as a named `Placement`, not as cell bits: the server has to judge whether a placement is allowed, and it can only do that against a vocabulary it understands. A client that could send arbitrary bits could place anything.

## The menu

The screen before the game. A name, a server, and the rooms that server has, or "play alone".

The room list is **asked for, not guessed**. A room is a whole separate world, so a name that does not exist is not a mistyped filter, it is nowhere — and a client cannot know what a server has without asking. So the menu shows nothing under Rooms until `ServerMessage::Rooms` comes back, rather than offering a name that might be there. A server that never answers becomes a message naming the address after eight seconds, because a menu that says "asking" forever is indistinguishable from one that is broken, and the two likeliest causes — a wrong address, and a server that is not running — both look exactly like it.

The world is still drawn behind the panel, and still running when the client is offline. A menu over a dead grey rectangle says the game has not started; a menu over a world says it is waiting for you.

On the web the server is shown but not editable: the socket is derived from the page's origin, so a typed address would be a promise the client cannot keep. Natively it is a field, because there is no page to have come from.

## The HUD

Player and their colour, value, generation, chunks held and drawn, zoom, connection state, which room, whether the world wraps, and why the last action was refused.

The room and the world's shape are there because both are invisible otherwise. A room is a whole separate world, so two players who cannot find each other are far more likely to be in different rooms than at different ends of one; and nothing on the board says whether walking east far enough brings you back.

It also reports the cell under the cursor, whether the pointer is over the panel or the world, and what the last click did. That is deliberate: a click on empty ground that takes nothing looks exactly like a click that never arrived, so the client says which of the two happened.
