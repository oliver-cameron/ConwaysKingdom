# Roadmap

Directions, not designs. [planned.md](planned.md) holds the things that have been thought through and not built; this holds the things that have been decided on and not thought through. Something moves from here to there when somebody works out what it actually costs.

## Spectating

Watching a world without a seat in it, and especially watching a **match** — which is the case that wants it, since a match is a thing with a result and an audience.

Most of what a spectator needs already goes over the wire: `Match` carries the lobby and the phase, `Standing` carries who is winning, `Step` and `ChunkData` carry the world. What is missing is a connection that has a **room but no player**. Everything keys off `Seat`, which is a room and a `PlayerId` together, and `Rooms::handle` drops any message from a connection without one — so a spectator is not a player with the actions taken away, it is a state that does not exist yet.

Two things make it worth doing properly rather than as a flag on a player. A seat is one of **fifteen** now, and burning one on somebody who is only watching is a real cost. And **no late joining** is a rule about players: a spectator arriving at generation four hundred is exactly what spectating is for, so the refusal has to know the difference.

## Games and matches by code

A short code instead of a room name — the thing you send somebody rather than the thing you type.

Room names are typed, and typed names collide, are guessed, and have to be told over the phone. `?room=lobby` in the URL already skips the menu, so `?code=` is the same machinery with a generated name behind it: a code to room map on the server, made when the room is.

The second reason is the more interesting one. `Rooms` lists everything it has, so every world is public; a coded room could be **unlisted**, which is what somebody wants when they make a match for four friends rather than for whoever is on the server. That is a change to the listing rather than to the code.

## Stamps that outlive the tab

A stamp lives as long as the client does. Capture a glider gun, close the tab, and it is gone — which makes the library a scratchpad rather than a collection, and makes drawing one by hand something you do again every session.

`net::keep` is already where a client keeps what it has, across a browser's `localStorage` and a native filesystem both, so there is somewhere obvious to put them. What makes it more than a `serde` derive is that a stamp is **cells and their kinds**, so the stored shape has to survive a change to `Placement` or to `Kind` — a library written before turrets existed should not come back as a library of nothing. That means a version on the file and a decision about what to do with a stamp naming a kind this build does not have.

The other half, from [planned.md](planned.md#stamps): a library is the natural thing to **share as one file**, which is a different feature wearing the same clothes and wants the format to be worth handing to somebody else.

## A minimap

**Not yet**, and the reason is not effort. A client holds the chunks it subscribed to, which is its own screen and a margin — so a minimap drawn from what the client has is a picture of where you already are, which is the one place you do not need a map for.

A real one needs a **coarse summary from the server**: something like a byte per chunk, saying which player holds most of it and how strongly, broadcast on a cadence the way `Standing` is. That is a small message and a straightforward pass over the world.

What it runs into is the boundless world. "The whole map" has no edge, so a minimap of one is either a window around the action or the bounding box of everything anybody holds, and both change size as people play. On a **torus** there is no such question: the world is a fixed rectangle and a minimap is exactly that rectangle.

Which suggests the order. Do it for wrapping worlds first, where it is nearly free, and let matches be where it lands — a match wants a fixed arena anyway, and a match is where knowing who holds what without panning across the world actually decides something.
