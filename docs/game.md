# The game

Conway's rules, with owners and an economy on top.

## Value

Every player has a `value`, on `sim::Player`. It is what they have to spend.

| | |
|---|---|
| place a cell | −5 each |
| reclaim your own living cell | +1 |
| destroy another player's cell | −1, because taking ground should not be free |
| erase empty ground | nothing |

Placing at five against reclaiming at one is what sets the pace: a cell you place and later take back is a net loss, so building commits you. Starting value is 100 — twenty cells, enough to build something before the mining loop takes over. It is a number to tune; the ratio matters more than the starting figure.

An action that cannot be afforded is refused. The client prices and refuses locally on the same terms the server would, so a refusal is instant rather than a round trip away, and the two cannot disagree because `net::value_delta` is one function used by both.

Cost is read **before** the action is applied, since it depends on what is there now.

## Placing and taking

One button. The cell under it decides which:

- something there → take it
- empty ground → put down whatever the hotbar has selected

A **drag fills a rectangle**, at five per cell. Filling always places, never takes: a sweep across occupied ground is far more likely to be building over it than a request to clear it cell by cell, and an accidental sweep that wiped a structure would be unforgiving. Taking stays a deliberate single click.

## Glass

A pane freezes what it covers. It is a flag, not a kind, so a cell may be alive, glassed, both or neither.

A pane belongs to whoever laid it. There is **one owner field per cell**, so glassing another player's living cell takes the cell with it — deliberate, and part of why a pane costs what it does. If glass should never transfer a cell, that needs a second owner field, which the cell has no spare bits for at sixteen.

Life touching a pane shatters the whole connected run. See [simulation](simulation.md#glass) for exactly what counts as touching, and for the emergent behaviour that a pane laid over a dense pattern is short-lived.

## Controls

| | |
|---|---|
| left click | act on one cell |
| left drag | fill the rectangle |
| middle drag | pan |
| arrows / WASD | pan |
| mouse wheel | zoom |
| trackpad scroll | pan |
| ctrl + scroll, trackpad pinch | zoom |
| one finger | pan |
| two fingers | pinch zoom |
| 1–9 | choose a hotbar slot |

A wheel and a trackpad arrive as the same winit event, and the only thing separating them is the unit: a wheel reports discrete lines, a trackpad continuous pixels. Splitting on that is what makes the gestures consistent — treating every scroll as zoom made a two-finger swipe lurch the zoom where every other application pans.

Panning with the mouse is the middle button so that drawing a rectangle and moving the view are never the same gesture, and neither has to guess which was meant.

## The hotbar

Picks what a click places. Slots are data in `client::views::hotbar::SLOTS`, so adding one is a row.

What is being placed travels in the action as a named `Placement`, not as cell bits: the server has to judge whether a placement is allowed, and it can only do that against a vocabulary it understands. A client that could send arbitrary bits could place anything.

## The HUD

Player and their colour, value, generation, chunks held and drawn, zoom, connection state, and why the last action was refused.

It also reports the cell under the cursor, whether the pointer is over the panel or the world, and what the last click did. That is deliberate: a click on empty ground that takes nothing looks exactly like a click that never arrived, so the client says which of the two happened.
