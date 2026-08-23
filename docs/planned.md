# Not built yet

What has been decided, what has not, and what each one runs into. Everything here is an intention rather than a description — [the rest of docs/](README.md) is the system as it actually stands.

## A menu

A screen before the game: play locally, or type an address and join. Today the only way to reach a server is `--ws` on a command line, which a phone does not have and a browser only gets by being served from the server it will talk to.

Most of what it needs already exists. `client::set_connection` and `set_world` are one-shots read once at startup; a menu would set them from a form instead of from `main`. The browser client derives its socket from the page's origin, so the menu's address field only matters when the page and the server are not the same machine.

Two decisions worth making before it is built. Whether a local game and a joined game are the same screen with a different answer, or two screens. And whether the address field remembers what you typed — which, given the rejoin token is already kept per device, wants to sit beside it rather than invent a second place to keep things.

Everything else here needs somewhere to live, so this comes first.

## Rooms per server

Several worlds behind one address, joined by name.

The largest of these, because it reaches furthest. `Server` holds one `World`, one player table and one tick; a room is all three. The save format holds one world and its players, so it grows a room list or becomes a file per room. `ClientMessage::Join` names a player but not a room, so the protocol grows a field. Player numbers are per world — five bits, thirty-one of them — so they become per room too, and a player in two rooms is two players.

The thing to decide first is whether a room is a world or a *view* of a world. Separate worlds is simpler and is what "rooms" usually means. It also means territory, value and the rejoin token are per room, and a token that returns you to the wrong room is worse than no token.

## Auto-mining

A way to gather value without clicking for it.

This is a rule rather than an interface, which is what makes it independent of the rest. Value has exactly one source today: reclaiming your own living cells, one apiece. That makes the only way to earn a click, and it is the click you least want to be making.

The shape it probably wants: living cells you own earn value over time simply by being alive, so building something that survives is the income, and Conway's rules become the economy rather than a tax on it. That is already half-true — a cell that dies takes its value with it — so this completes the loop rather than adding a new one.

What has to be settled: the rate, and whether it scales with how much you hold (which rewards sprawl) or with how long a pattern has survived (which rewards structures that work). Both are one line in `Server::step`; neither is one line to choose.

## Stamps

A pattern you capture once and place again, at **double cost** — the convenience is the thing being paid for.

Decided: a **separate hotbar** rather than slots in the existing one, a **library** of them rather than a single clipboard, and a file that can hold **several stamps at once** so they can be shared as one thing. Its own branch.

It needs nothing new on the wire. A stamp is a `Paint` of the cells it covers, judged against territory and value like anything else — the doubling is a client-side price and a server-side check on the same action. The capture gesture is the interesting part: dragging a rectangle over your own cells is the obvious way to take one, and it is also how ice is placed, so the second hotbar has to make clear which of the two a drag means.

The open questions are all interface. Where a second bar sits on a phone, whether a stamp can be rotated or mirrored when placed, and what happens when a stamp will not fit inside your territory — refuse it whole, as a drag does, or place the part that fits.

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And there is no way to reach a server without a command line, which is the menu above.

## Known, and left alone

- **A client is never told the world wraps.** `--torus` is a server flag and `Welcome` does not carry it, so a client joining a toroidal server builds an infinite world locally. The seam will show as soon as a wrapping world has more than one player.
- **Thirty-one players is a ceiling on players a world has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind.
- **Territory only ever spreads.** There is no die-off, so a glider leaves a permanent trail and an infinite world grows with it. Deliberate for now; the die-off is what would bound it.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
