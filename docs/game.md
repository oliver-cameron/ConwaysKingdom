# The game

Conway's rules, with owners and an economy on top.

## Value

Every player has a `value`, on `sim::Player`. It is what they have to spend.

| | |
|---|---|
| place life or ice | −5 each |
| reclaim your own | +1 |
| take another player's | −1, because taking ground should not be free |
| take what is not there | nothing |

Placing at five against reclaiming at one is what sets the pace: anything you place and later take back is a net loss, so building commits you. Starting value is 100 — twenty cells, enough to build something before the mining loop takes over. It is a number to tune; the ratio matters more than the starting figure.

The rule reads the same for life and for ice, because the question it asks is whether the thing being taken is there and whose it is, not which of the two it is.

An action that cannot be afforded is refused. The client prices and refuses locally on the same terms the server would, so a refusal is instant rather than a round trip away, and the two cannot disagree because `net::value_delta` is one function used by both.

Cost is read **before** the action is applied, since it depends on what is there now.

## Placing and taking

One button, and the cell under it decides which — for whatever the hotbar is holding:

- what you are holding is already there → take it back
- it is not → put it down

Keyed on what is held rather than on whether the cell is occupied at all, because **life and ice are independent**. Holding Life and clicking a living cell under a pane kills the life and leaves the pane standing; clearing the square outright would destroy a pane the player never aimed at, at five a cell. Holding Ice and clicking that same pane lifts it and leaves the life, which is the only way a misplaced pane comes back.

`Action::Erase` carries what to remove for the same reason `Paint` carries what to lay: the server judges an intent, and "kill the life on this square" is a different intent from "clear this square".

A **drag fills a rectangle**, at five per cell. Filling always places, never takes: a sweep across occupied ground is far more likely to be building over it than a request to clear it cell by cell, and an accidental sweep that wiped a structure would be unforgiving. Taking stays a deliberate single click.

The rectangle is drawn while you sweep it, with its size and its price beside it, and a drag that cannot be paid for is drawn as refused before the button comes up. **A fill is all or nothing.** One laid as far as the value stretched would be a different shape from the one that was drawn, and the player would be left working out where it stopped and why.

Only the cells a placement actually changes are charged for. Extending a rectangle means sweeping the whole of it again, and paying twice for the part that was already there made the natural gesture the expensive one. This is why `value_delta` reads the world for a paint as well as for an erase.

One drag may cover at most 4096 cells. At one pixel per cell a sweep across the screen is millions, and every one of them would be listed, priced, applied and put on the wire.

A press that travels but stays inside one cell is still a click. A one-cell fill would place where a click would take, so which of the two happens must not turn on a few pixels of hand shake at high zoom.

## Ice

A pane freezes what it covers. It is a flag, not a kind, so a cell may be alive, iced, both or neither.

A pane belongs to whoever laid it. There is **one owner field per cell**, so icing another player's living cell takes the cell with it — deliberate, and part of why a pane costs what it does. If ice should never transfer a cell, that needs a second owner field, which the cell has no spare bits for at sixteen.

Life touching a pane shatters the whole connected run. See [simulation](simulation.md#ice) for exactly what counts as touching, and for the emergent behaviour that a pane laid over a dense pattern is short-lived.

## Controls

| | |
|---|---|
| left click | act on one cell |
| left drag | fill the rectangle |
| middle drag, right drag, space + left drag | pan |
| arrows / WASD, shift to hurry | pan |
| mouse wheel | zoom |
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

Picks what a click acts on — both what it places and, on ground that already has it, what it takes back. Slots are data in `client::views::hotbar::SLOTS`, so adding one is a row. The two are `Life` and `Ice`: the placement is named for what is put down, since a cell is the square and life is one of the two things that can be on it.

What is being placed travels in the action as a named `Placement`, not as cell bits: the server has to judge whether a placement is allowed, and it can only do that against a vocabulary it understands. A client that could send arbitrary bits could place anything.

## The HUD

Player and their colour, value, generation, chunks held and drawn, zoom, connection state, and why the last action was refused.

It also reports the cell under the cursor, whether the pointer is over the panel or the world, and what the last click did. That is deliberate: a click on empty ground that takes nothing looks exactly like a click that never arrived, so the client says which of the two happened.
