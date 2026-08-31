# Not built yet

**One file.** There used to be two — a roadmap for directions decided on and not costed, and this for designs costed and not built — and the split did not survive contact. Entries only ever moved one way, nothing moved back, and both files went stale in the same way: by describing things that had since been built as though they had not. Status is a label on an entry now, not a file it lives in.

| status | means |
|---|---|
| **Built** | in, and kept here only for what is *left* — the design itself is in [the rest of docs/](README.md) |
| **Being built** | on a branch now |
| **Designed** | worked out, including what it costs; not started |
| **Decided** | a direction agreed and not costed. What used to be the roadmap |
| **Open** | a fault that is known and not fixed, with what has been ruled out |

The system as it actually stands is [the rest of docs/](README.md). Everything here is an intention. Where an idea was borrowed from somebody, [inspiration.md](inspiration.md) says whom and for what.

## Contents

| | status | |
|---|---|---|
| [What to do next](#what-to-do-next) | — | a reading of this list, in order |
| [Player profiles](#player-profiles) | Designed | a person, rather than a seat in one room |
| [Icons on the bar](#icons-on-the-bar) | Decided | a picture where a word is now |
| [Zooming out without lying](#zooming-out-without-lying) | Part built | antialiasing is in; the level of detail is what low zoom actually needs |
| [A torus repeats, so its textures can](#a-torus-repeats-so-its-textures-can) | Built | one copy of a wrapping world, drawn many times |
| [Payloads](#payloads) | Decided | a cell that explodes after a while |
| [Overclockers](#overclockers) | Decided | a cell that steps more than once a generation |
| [Depleted mines](#depleted-mines) | Decided | a mine that stops paying, so income does not scale with size |
| [The simulation on the GPU](#the-simulation-on-the-gpu) | Costed | a compute shader, and the one thing that makes it hard |
| [Making rooms from the client](#making-rooms-from-the-client) | Built | a world, a match or a private game, from the menu |
| [Spectating](#spectating) | Built | a room with no seat in it |
| [Games and matches by code](#games-and-matches-by-code) | Built | private rooms, and what is left of the idea |
| [Ice anywhere, at a price](#ice-anywhere-at-a-price) | Decided | a wall you can build on somebody else's doorstep |
| [The mercy rule](#the-mercy-rule) | Designed | a player who cannot act becomes a spectator |
| [Teams](#teams) | Built | one seat, one platform and one purse to a side |
| [Rating](#rating) | Built | per server, on the home screen; a leaderboard is not |
| [Many servers](#many-servers-and-what-must-not-be-decentralised) | Being built | a person exists; a *safe* one does not, and discovery is not started |
| [The menu draws nothing on some machines](#the-menu-draws-nothing-on-some-machines) | **Open** | a bug, not reproduced; what is ruled out and what is not |
| [Experiments](#experiments) | Decided | a laboratory rather than a match: pause, step, save, reset |
| [Better interfaces](#better-interfaces) | Decided | the menu had two passes; everything else had none |
| [Bots](#bots) | Decided | a player the server plays, and no protocol change |
| [Predicting a match](#predicting-a-match-and-what-it-shares-with-bots-and-experiments) | Decided | run the world forward and look; one derive away, and shared with bots |
| [A leaderboard](#a-leaderboard) | Decided | the second half of rating, waiting on the same thing |
| [The session comes out of the game view](#the-session-comes-out-of-the-game-view) | Designed | the one place the architecture does not hold |
| [Rooms per server](#rooms-per-server) | Built | what is left is lifetime |
| [Auto-mining](#auto-mining) | Built | |
| [Turrets](#turrets) | Built | |
| [Stamps](#stamps) | Built | a shape; what it is made of is the hotbar's other axis |
| [Territory as a level, not a flag](#territory-as-a-level-not-a-flag) | Built | what is left is drawing it |
| [Fifteen slots, and more than fifteen clients](#fifteen-slots-and-more-than-fifteen-clients) | **Open** | a room that has met fifteen people is full for ever |
| [Matches](#matches) | Built | |
| [Type, and the numbers that jitter](#type-and-the-numbers-that-jitter) | Decided | |
| [A minimap](#a-minimap) | Decided | |
| [Mobile](#mobile) | Designed | |
| [Known, and left alone](#known-and-left-alone) | — | |

## Payloads

Payload cells will utilise the new age flag in the cell. While it is alive, a payload will have a chance to increase it's timer, with this chance at 100% in the state right before detonation.

### Detonation
The goal of detonation is to make the surrounding area look like random noise. On detonation, a payload can do a certain number of actions, in order of precedence. These actions are given a score, weighted by distance, and selecting the greatest scoring action, recalculating each time.
1. Setting nearby cells to detonate the next generation:
> Payloads with a fuse of less than 14 will be set to 14, with a score of 20.
2. Reducing "randomness cost"
> The randomness cost of a cell is a sum of the life of cells in a kernel around it, where alive is 1 and dead is zero, balanced by a density constant, squared. The score given to a particular cell to flip is the total cost decrease over the kernels that that cell lives in. This can be negative.

*Closeness* is of the form $\frac{1}{\delta x ^ 2 | \delta y ^ 2 + D}$, where `D` is some constant. Score is multiplied by this.

This makes the surrounding area more random.
## What to do next

A reading of the rest of this file, in the order the things depend on each other rather than in the order they were thought of. Nothing here is new; what it adds is which one unblocks the most.

**1. [An identity a server cannot take](#identity-is-a-keypair-and-today-it-is-not).** `people.tsv` holds a plaintext secret per person, and a secret is a bearer credential — so every server that has met you can be you on every other server that has met you, and that file is the thing on the machine worth stealing. `net::auth::person` says so itself and says it has to change before there are two servers. It is first not because anything is broken today but because it is the only item here that gets **worse** the more the project succeeds, and because everything social — a directory, friends, inviting somebody in particular, a leaderboard that means anything — is built on top of who somebody is.

**2. [A level of detail](#zooming-out-without-lying).** Low zoom does not work and it is not the sampling: one chunk is one texture array layer against a guaranteed floor of 256, so a 1080p screen is mostly backdrop below about zoom five and an island in a sea of nothing at the floor. The answer is the cell without its art — one texel a cell, one quad, no sheet lookup — and it is **one piece of work with four uses**: zooming out, a minimap, a world overview, and a spectator following a player. Three of those are separate entries in this file.

**3. `World: Clone`.** One derive, and it is what [a match prediction](#predicting-a-match-and-what-it-shares-with-bots-and-experiments), a bot that chooses rather than follows a book, and an [experiment's](#experiments) pause-step-reset are all waiting on. The step is already a pure function of state and tick, so a copy diverges cleanly; there is simply no way to step a world that is not the world. Nothing else on this list has a ratio like it.

**4. [The session comes out of the game view](#the-session-comes-out-of-the-game-view).** `client::views::game` is 3,400 lines, 51 fields and about 70 methods on one struct, and none of the part that talks to a server is testable because it shares that struct with the sprite atlas. `Ui` came out of it recently and that was the easy half. Everything else here that touches the client — [better interfaces](#better-interfaces), [mobile](#mobile), a minimap — is harder for as long as this is undone.

**5. [Depleted mines](#depleted-mines).** The economy has no ceiling that is not the blunt one, `Player::MAX_VALUE`, which stops a purse growing and does nothing about why it was growing. Small, needs nothing first, and `Kind::inherits` already makes it a row in a table rather than a rule.

Worth saying what is **closer than this file implies**. [A leaderboard](#a-leaderboard) says it waits on a person being a keypair; per server it does not, because `server::ratings` is already keyed by `PersonId` and already saved — what is left is a message answerable without a seat and a screen. Only a leaderboard that spans servers needs item 1.

And what is not next. [The simulation on the GPU](#the-simulation-on-the-gpu) is a large piece of work whose benefit begins at a world size nobody has run, and the level of detail above removes the one argument that was pulling it forward. [Mobile](#mobile) is a layout problem that wants the interface to stop moving first.

## Player profiles

**A person, rather than a seat in one room.** Everything a player accumulates is currently filed against something that does not survive what it should: a `PlayerId` is a seat in one world, a rejoin token is per room, a rating is per server, and a stamp library is per browser. Somebody who plays two games on two machines is four different people to this code.

The identity exists already, in the weak form. `net::auth` mints a `Secret`, the client keeps it — **at startup now, not at the first join**, because a record and a stamp library both exist before a server has been reached and both want an owner — and `Join` carries it. A server can already say *which person* is asking. Two things are missing: everything that should be filed against the answer, and a way of asking that does not hand the server a credential it could reuse elsewhere — see [identity is a keypair, and today it is not](#identity-is-a-keypair-and-today-it-is-not).

### It subtracts before it adds

**A profile deletes the rejoin token.** The token exists so that a dropped connection comes back to the same seat, and [networking.md](networking.md#coming-back) is honest about what it costs: it is filed per room rather than per server, so two servers both running `main` share one secret; whoever holds it *is* you; and a token whose player is already connected joins you as somebody new. A key does the same job strictly better — the server maps person to seat per room, the claim is signed rather than presented, and there is one key for everywhere rather than one secret per room per server.

So the first version of this is not a feature on top. It is `Join` carrying a person and nothing else, `Server::join_with` looking a seat up by `PersonId`, and the token machinery going.

### What is yours and what is the server's

The line worth drawing early, because everything else follows from it: **anything another player is shown has to be the server's**. Client-side state is self-asserted, so a rating you keep is a rating you can type.

| | where | why |
|---|---|---|
| the key | yours, portable | it *is* the identity; the server only ever sees the public half |
| your name | yours, sent on join | you choose it; nobody else is shown it as a fact |
| stamps | yours | nobody else sees them, so nobody can be misled by them |
| rating | the server's | shown to others, and the whole point is that it is not self-reported |
| games played, largest held | the server's, per server | same argument; `client::record` stays as your own diary |
| when first seen | the server's | it is a fact about a visit, not about a person |

`client::record` does not have to move. It is a client's own history and it is worth keeping as one — what it must not do is be *shown to anybody else* while it lives there. The server keeps its own count of what happened on it, and those two numbers disagreeing is fine and readable: one is "what I have played" and the other is "what I have played here".

### Names, and the two Alices

Two players may pick the same name and nothing stops them. A key fixes this without an account system: show the name beside a **short fingerprint of the public key** — `alice·3f2a` — which is derived rather than assigned, so neither Alice has to accept being Alice2 and neither can take the other's name.

Four hex characters is enough for a room of fifteen and is not meant to be enough for the world; it disambiguates the people you can see, and the full key is what identifies anybody absolutely.

### What a profile screen shows

**Yours.** Name, editable. Fingerprint beside it. Your devices, and the control that authorises another — which is the most important control in the client, because losing every device is losing the person and there is no recovery. Not an *export*: see [an identity is a set of devices](#so-an-identity-is-a-set-of-devices), which is what replaces copying a key about. Rating, marked *provisional* until enough games. Your record. Your stamps.

**Somebody else's**, from the lobby or the standings: name and fingerprint, their colour, their rating, and what they have done **on this server**. Not their key file, not their stamps, not their record from elsewhere — a server can only vouch for what happened on it.

### Ratings need a provisional state

A new profile has no games, and an Elo from a fixed start is a number that means nothing until it has moved. The usual answer is a high K for the first *n* results and a mark on the figure until then, so a leaderboard is not topped by somebody who won once. `server::rating` already computes deltas; this is a count and a threshold.

### What it must not become

**An account system.** No password, no email, no recovery. The key is the whole of it, which is the right strength for a game with no accounts and is the same argument the rejoin token was already making — it is just a better claim ticket.

**Required.** A client with no key still plays and is nobody the server remembers. That has to stay true: a browser with storage switched off is a real browser.

**Part of a cell.** The `PlayerId` in the owner byte is four bits and a seat in one world, and it must stay that. A profile is looked up *from* a seat, never stored in one — the reason territory works at all is that a cell's owner fits in half a byte.

### What it runs into

**One person, two clients.** A key can be on a laptop and a phone. `net` already says nobody may be two people at once and nobody may be one person twice, and that rule is enforced per token today; keyed by person it becomes exactly the same rule with a better key. A person holding a seat in *several rooms* is allowed and should stay allowed. Two connections as one person in **one** room is what must still be refused.

**Importing a key that is in use.** Paste your key onto a second machine while the first is connected, and the second is refused the seat rather than stealing it — which is the rule above, and is worth saying out loud because the refusal will look like a bug to whoever pastes.

**Scope.** Per server, keyed by person, and stop there. That fixes the rating and the record and deletes the token. Across servers it is a distributed identity problem and [many servers](#many-servers-and-what-must-not-be-decentralised) already says what must not be attempted; the stamp library can stay per client until there is an answer.

**Migration.** Records filed under a room's display name cannot be re-keyed, because the thing that would key them was never written down. They stay as they are and the server's counts start at zero, which is the honest outcome and is one line in the release note rather than a migration.

## Icons on the bar

The four figures on the hotbar are labelled with a word each — `purse`, `ground`, `tick`, `elo`. A picture would be better: the words are read once and then ignored, they take a line of height that a picture would not, and three of the four are already drawn somewhere in the game.

`views::icons` already tints the sprite sheet on the CPU the way the shader tints it on the GPU, so a cell can be drawn on a button; a purse and a clock are not cells and would be the first art in the client that is not a cell. That is the decision this is waiting on — a second sheet, or a handful of shapes painted with `egui::Painter` the way `icons::back` is.

`icons::back` is the precedent and the argument for painting them: the arrow it replaced was a character in a font this client does not load, and the one control whose job is to be recognised at a glance was drawn as a square until somebody painted it.

## Overclockers

**A cell that steps more than once a generation.** Twice, or more. Not designed yet; this is here to say what it will run into, because that is the part that is already decidable.

**A generation is the unit everything else is keyed to.** The seeded dice are derived from it, so a birth's owner is a function of the generation — see [simulation.md](simulation.md#determinism). The server broadcasts one `Step` per generation and a client that finds itself at a different tick throws its world away and asks again. Mining pays per birth and the standings go out every eight generations. So "twice a generation" is not a rule about a cell so much as a question about what a generation *is*, and the answer has to be one of two shapes:

**Sub-steps inside one generation.** The generation stays the unit on the wire and in the save; inside `World::step`, a region containing an overclocker runs the rule twice before the generation is called done. Everything outside the sim is untouched — one `Step`, one tick, one digest. What it needs is a rule for what a sub-step reads: the second pass sees the first pass's output, which means overclocked regions and ordinary ones are being stepped against different states at their border, and the border is where it will look wrong.

**A faster tick with slower cells.** The opposite: the generation gets shorter and ordinary cells step every *n*th one. Nothing about the rule changes and everything about the cost does — the server steps four times as often for the same simulation, and the tick is already the thing the [latency work](networking.md#messages) was about.

The first is almost certainly right and the second is worth writing down because it is the one that does not need a new concept.

**What it must not become** is a cell whose speed depends on anything a client can see and the server cannot, or two peers stepping the same region a different number of times. Whatever the design turns out to be, the test is the one `examples/two` already runs: two peers, the real protocol, digests compared every shared generation.

## Zooming out without lying

**Antialiasing is built. The level of detail is not, and it is the one that matters.**

### What was wrong, and what is fixed

A point sample is exact at one pixel per *texel* and a lie below it. That is zoom **sixteen**, not zoom one — a cell is sixteen texels of art — so the aliasing started three quarters of the way up the zoom range rather than at the bottom of it. At zoom four one pixel stood for a 4x4 block of a hand-drawn sprite and picked one texel of it, so a pattern shimmered whenever the camera moved and thin structures winked in and out at some zooms and not others.

`grid.wgsl` now averages a `k` by `k` grid over the pixel's own footprint, `k` from the zoom and capped at four, measured in **texels** — which makes one rule cover both halves of the problem: inside a tile it antialiases the art, past one cell it averages over the cells a pixel covers, and nothing has to know which case it is in.

Three things about it are load-bearing:

**Averaged in shaded colour, never in cell bytes.** Half of one kind and half of another is a third kind with art of its own, and a player number between two players is a third player. `Uint` and `textureLoad` stay exactly as they were.

**Each sample recomputes its own cell and its own tile.** Averaging within one tile's sheet coordinates would blend a cell's art into its neighbour's, because the sheet is an atlas and adjacent tiles are unrelated pictures. That is also why the sheet cannot simply be given mipmaps, and why its sampler cannot be `Linear`: a bilinear tap near a tile edge reaches into the next tile.

**An explicit level, not an implicit one.** `textureSample` needs derivatives and derivatives may not be taken in non-uniform control flow, which a loop and a branch on a quad's kind both are. The sheet has one mip and a `Nearest` sampler, so `textureSampleLevel(..., 0.0)` is what the implicit path chose anyway.

The [brightness difference between a loaded chunk and the backdrop](known-bugs.md#loaded-chunks-read-differently-from-the-backdrop) should be much reduced by this and is not proved gone: both routes now converge on the area average of their footprint, which is the same number, but they still reach it by different arithmetic. It wants looking at rather than reasoning about.

### The real bound on low zoom is residency, not sampling

Worth stating plainly, because the whole entry used to assume otherwise. **One chunk is one texture array layer, and the guaranteed floor for `max_texture_array_layers` is 256.** A 1920x1080 screen covers about `8100 / zoom²` chunk positions, so:

| zoom | chunks on screen | fits in 256 layers | fits in 1024 quads |
|---|---|---|---|
| 16 (opening) | 40 | yes | yes |
| 8 | 160 | yes | yes |
| 5 | 336 | **no** | yes |
| 3 | 875 | no | yes |
| 1 (the floor) | 8100 | no | **no** |

So a screen is already mostly backdrop below about zoom five, and at the floor it is a 16x16-chunk island of real cells in a sea of empty ground. That is `render::chunks::covered`, which is pure and has the arithmetic pinned in a test. Better sampling makes a mostly-empty screen smoother; it does not put the world back on it.

### So the level of detail is the cell without its art

The old plan here was a reduction chain over the cell texture, which wanted a compute pass, which wanted the [format change](#the-simulation-on-the-gpu) first. That is the wrong shape twice over. The problem is not that the cell texture is too detailed — it is that there are only 256 of them and a quad each. And a *reduction* answers a question nobody asked: at low zoom a player does not want a summary of sixteen cells, they want the cells.

What they stop wanting is the **art**. A cell is 16x16 texels of sprite, and that is the entire reason residency is one array layer per chunk: sixteen texels a side is what a sheet lookup needs. Below about four pixels per cell the sprite is not legible and is costing 256 texels per cell to draw as a blur.

So the coarse level is **one texel per cell**, in a plain 2D texture over a window of the world, drawn as **one quad**, with no sheet lookup at all. What each texel holds is exactly what stays legible at that size, and it is not a new format — it is the cell:

| | | |
|---|---|---|
| R | the owner byte, unchanged | `>> PLAYER_SHIFT` is the player, so the hue table is read exactly as it is today |
| G | the tile byte, unchanged | bit 0 is alive, bit 1 is ice; kind and age ride along free and are ignored |

Which is `Rg8Uint`, the cell's own two bytes, cast straight out of a `Chunk` the way the fine texture used to be before the neighbour mask joined it. Nothing is derived, nothing is summarised, and nothing has to agree with anything.

**What it draws** is a flat colour a cell: the player's hue for whose it is, light or dark for alive or dead, and a tint for ice. That is the whole shading path — three reads off two bytes and one hue lookup, against a sheet sample and an edge mask on the fine path. A world at one pixel per cell reads as a map of who holds what, with life as texture on it, which is what somebody zoomed out is looking for.

### What it costs, and where it stops

**Coverage is the interesting number.** A 2048x2048 texture is 8 MB and covers 2048 cells a side, against 256 chunks — 4096 cells' worth spread over whatever is on screen — for the array. On a 1920-pixel screen that is the full width at one pixel per cell with room to spare, and it runs out around a quarter of a pixel per cell, where the screen wants 7680. So one coarse level takes the floor from 1 down to about 0.25 and no further, and a second level below it is the same trick again rather than a new idea.

**The window has to move.** A 2D texture over a window is re-centred when the camera leaves its middle, and re-uploading 8 MB per frame while panning is not free. The standard answer applies: address it with a wrap, and upload only the rows and columns newly exposed — the same scrolling window a clipmap uses. On a torus the wrap is `World::canonical` and the window can simply *be* the world once the world is smaller than the window.

**The swap wants hysteresis.** Two paths with one threshold flicker between them when the zoom sits on it. Swap out at four pixels per cell and back in at five, so a scroll wheel resting on the boundary picks one.

**The antialiasing already written carries over unchanged.** Its footprint is measured in texels, and a coarse texel is a cell, so below one pixel per cell it averages over cells exactly as it averages over sprite texels above — same loop, same `k`, and the continuity across the swap is free.

### And it needs no compute shader

Which is the other thing the old plan got wrong. A max-or-count reduction on the GPU is elegant and would want `Rgba8Uint` storage bindings and a compute pass that WebGL2 cannot run at all. Copying two bytes a cell into a second texture is a memcpy the client already affords, it works on every backend, and it does not block on [the simulation on the GPU](#the-simulation-on-the-gpu).

**This is also what a minimap is**, and what a world overview is, and what a spectator following a player wants — one piece of work with four uses, which is the argument for doing it before any of them.

## A torus repeats, so its textures can

**Built, and it was built before this entry was written.** `ChunkStore::sync` keys residency by `World::canonical`, so a chunk that appears at nine places on screen is one texture layer and nine quads, and the resident set is bounded by the *world* rather than by how far anybody has panned. It landed in "A wrapping world that wraps, and a player who can play in it"; this entry was added later from a misreading of the code and claimed the opposite.

The decision it records is still worth keeping, because it is the one that makes the arithmetic above work: **a wrapping world is drawn by folding, not by tiling.** Every position the viewport covers is asked which chunk fills it, which on a torus is many-to-one. The version before it drew a fixed number of copies either side of the original, so panning off the third copy fell into blank space for ever and a large torus paid for nine copies of every chunk whether or not any were on screen.

`render::chunks::covered` is that arithmetic, pure and out of `sync` so it can be checked without a device. Two tests hold it: a 4x4 torus under a 12x12-chunk viewport is 144 quads over 16 layers, and panning a thousand worlds along finds no new chunks.

## Depleted mines

**The problem is that mine income scales faster than size.** A mine pays when one of its kind is *born*, and births scale with the perimeter of a growing pattern — so a player with four times the territory does not earn four times as much, they earn more than that, and they can spend it on more territory. Nothing in the rules pushes back.

A **depleted** mine is the push-back: past some point it stops paying and is an ordinary cell that happens to have cost more. What that buys is a ceiling on what any one lineage is worth, so income comes from *building new things* rather than from having built a big one.

### Where the bit comes from

Byte 1 is full — alive, ice, kind, age; see [simulation.md](simulation.md#the-cell). There is no spare bit, so this is a choice between three, and they are not equally good.

**A kind.** `Kind::DEPLETED_MINE` beside `Kind::MINE`, costing one of eight kind indices and no bits at all. It gets art of its own for free, which a flag would not — a depleted mine has to *look* spent or nobody can tell which of their cells still earns. `Kind::inherits` already decides whether a birth copies a kind, so "a depleted mine's children are ordinary" or "are also depleted" is a row in the table rather than a rule. This is the one to do.

**The age field.** A mine's age *is* its depletion: `net::earnings` scales down with it and a mine at [`bits::MAX_AGE`] pays nothing. No new state anywhere, and the eight steps are a fade rather than a cliff, which is likely to play better. What it costs is that mines can no longer use age for anything else, and it collides with payloads if a payload is ever also a mine.

**A bit off age.** Three bits become two, four ages instead of eight. Cheapest to write and the worst of the three: it takes resolution away from the one field that has a use lined up, to buy a flag that a kind gives away.

### What is not decided

How much is "past some point", and whether depletion is a count of births or of generations. A count of births is the honest one — it is what a mine is paid for — but it needs somewhere to keep the count, which is the age field again.

## The simulation on the GPU

**Costed, not started.** The full working is in [design-notes/05-compute-feasibility.md](../design-notes/05-compute-feasibility.md); the parts that decide anything are here.

`Rg8Uint` **cannot be a compute shader's output.** wgpu's guaranteed format features give it `msaa | attachment` and no `STORAGE_BINDING`, and a compute shader can only write to a storage texture. So moving the simulation onto the GPU means changing the cell's texture format first: `Rgba8Uint` is the natural one — storage-capable, and four independent `u8`s where the cell already wants fields — or `R32Uint`, which is fully read-write and has atomics at the cost of packing by hand.

`Rgba8Uint` grants read-only and write-only storage but not read-write, which suits Conway anyway: bind one generation read-only and the next write-only, and swap each tick.

**WebGL2 cannot do it at all.** `Limits::downlevel_webgl2_defaults` zeroes every compute limit, and the browser client falls back to WebGL2 whenever WebGPU is unavailable — a blocklisted driver, a VM, a headless browser; see [gotchas.md](gotchas.md). So this is never the only simulation. There has to be a CPU path regardless.

### The hard part is not the shader

It is that **two simulations must agree exactly.** The server steps on the CPU and the client predicts against it, and a `Checkpoint` compares them chunk by chunk — so a GPU step that differs from the CPU step by one cell is not slower or uglier, it is a client that resyncs every few seconds forever. Everything the rule does is integer work on bytes, which is reproducible on a GPU in a way floating point would not be, but "reproducible" has to be *established* rather than assumed: the seeded dice in `sim::seed`, the order births resolve in, and the tie-breaks in the territory rule all have to come out the same.

Which suggests the shape: `examples/headless` already runs the simulation with no GPU, so the test is a world stepped both ways for a few hundred generations with the digests compared every step — the same comparison `examples/two` already makes between two peers.

### What it buys, and when

Nothing yet. The world steps four times a second and a chunk is 256 cells; the server's cost is linear in *resident* chunks and the client only holds its viewport. This is worth doing when a room holds a world big enough that a quarter-second is not enough to step it, and that is a size nobody has run.

The cheap thing to do now is not to close the door: `Rgba8Uint` is a format constant and a fourth byte on the cell, and it is the difference between swapping a constant and rewriting the storage layer.

## Making rooms from the client

**Built** — see [game.md](game.md#the-menu) for the form and [server.md](server.md#made-by-a-client) for the wire, the cap and the owner. A world, a match or a private game, from the menu, on a phone.

What is left:

**No way to close a room from the client.** The owner is recorded in `Rooms::made` and nothing reads it. `Rooms::delete` exists and is at the console; putting it on the wire wants the owner check to be real, and wants an answer for a room somebody is standing in.

**Nothing starts a match but `match dispatch`.** A client can make a match and cannot blow the whistle on it, so a client-made match still needs the operator for its one remaining verb. `ClientMessage::Start` is the same shape as `Create` and wants the same owner check.

**Auto-sleep is the fix the cap only backstops.** Every room steps four times a second for as long as the process lives, whether or not anybody is in it. Half the answer is already built and unused: `Server::set_asleep` exists, `Server::step` returns nothing for a sleeping room, and `world sleep` / `world wake` are at the console. What is missing is the trigger — a room whose last player leaves sleeps after a grace period, and the `Join` that resolves to it wakes it.

Waking is indistinguishable from never having slept, for a reason worth stating rather than assuming: the tick **is** the generation, and nothing else advances while a world is not stepping. There is no second clock to drift. The one thing to watch is the save, which records that tick — a room saved asleep records the generation it stopped at, which is the right number under the only meaning the field has ever had. A match must not sleep and does not: `set_asleep` refuses on anything but `Phase::Open`.

## Spectating

**Built** — see [server.md](server.md#watching). `ClientMessage::Watch` takes a room and no seat; `ServerMessage::Watching` answers with the world and its clock and no player, purse or spawn. A watcher reads and cannot act, which is enforced by an action now belonging to the **connection that sent it** rather than to the `PlayerId` it names.

Admitted at any generation, and that is the point rather than an oversight: **no late joining is a rule about players.** A `Join` to a running match is still refused, and the refusal is what the client turns into an offer to watch — keeping "you cannot play in this" and "would you like to watch it" two separate answers, which is what they are.

What is left: a watcher cannot follow a particular player's ground, which is what a spectator actually wants once a world is larger than a screen. That wants the camera to take a target, which is `views::camera`'s business and not the protocol's.

## Games and matches by code

**Built** — see [server.md](server.md#made-by-a-client). A private room is kept out of the listing and reached by a six-character code. The code is a **credential and not an identity**: separate from the room's id, which never changes, and separate from its name, so a private game can still be called something its owner chose and a code could be rotated later.

The alphabet leaves out `0`, `o`, `1`, `i` and `l` — 31⁶ is 887 million codes, or 29.7 bits, against 36⁶ and 31.0 bits for the full alphanumeric set. That trade is deliberate: those five characters are the whole of why a code gets mistyped when it is read off one screen and typed into another, and the keyspace is not what protects a private room anyway. With the room cap where it is a random guess finds one in about twenty-eight million, so the defence is that guessing is not worth anybody's time — and if it ever became worth somebody's time the answer is a limit on how fast a connection may guess, not a longer code.

What is left: **`?code=` in a URL**, which is the whole point of a code being short. `?room=` already skips the menu and `resolve` already takes a code wherever it takes a name, so this is a query-string parameter and nothing else.

## Ice anywhere, at a price

**Decided, not costed.** Ice may be placed outside your own territory, for a great deal more money, and laying it takes no ground.

Everything else is confined to your own reach, and that rule is what makes the map mean anything: territory is the resource, spreading is how you get it, and a player who could build anywhere would have no reason to hold anything. Ice is the one placement where the confinement is doing something different, and worse. **A wall you can only build inside your own country is a wall against nothing.** What ice is for is stopping something — a glider run, a spread you cannot outpace, a corridor between two of somebody's holdings — and every one of those is a thing happening *outside* your border by the time it is worth stopping. So the one defensive tool in the game can only be used where you are already safe.

Two conditions, and the second is the one that keeps it honest.

**It costs a great deal more.** Not a little more: enough that walling somebody in is a decision about the whole of your purse rather than a thing you do while doing something else. The number is the part that is not costed — it wants playing with, and it is a multiple of the ordinary price rather than a separate figure, so a change to what ice costs at home moves both.

**It takes no ground.** Placing ordinarily claims the square, which is how territory grows; ice placed abroad must not, or the price becomes irrelevant and the rule becomes "buy land anywhere", which is precisely the rule this game does not have. A pane laid on somebody else's ground is somebody else's ground with a pane on it — it freezes what is under it, which is what ice does, and changes nothing about whose it is.

What it runs into is that `net::may_place` asks one question for every placement and would have to ask two, and that `net::price` is per-cell and per-placement and would need to know *whose ground it is on* rather than only what is being put there. Both are small; the second is the one to be careful with, because the client prices a drag before it sends it and the two must agree to the penny or every wall becomes a resync.

It also wants a word on the screen. A price that changes depending on where the pointer is, with no explanation, reads as a bug — the drag preview already says what a gesture costs, so the label is where this is explained rather than in a help screen.

## The mercy rule

**Designed.** A player can reach a state where nothing they do has any effect, and the game does not notice.

Two ways in. **No money and nothing alive** — value floors at zero, so a player who spent everything and then lost their pattern has nothing to place and no way to earn, because income comes from mine births and they have no mines. And **no territory**, which sounds impossible because a granted patch never decays, but is not: an opponent who grows over your home keeps it as theirs, mark and all. Either way the player is sitting in front of a world they cannot touch, clicking, with the client saying only that the placement was refused.

What should happen is that they become a **spectator** — which is now a state that exists, so this is a rule rather than a feature. They keep watching the room they were in, they are told why, and their seat goes back into the pool. That last part is the reason to do it rather than a nicety: a seat is one of fifteen, and one held by somebody who cannot play is a seat a player who could is being refused.

Three things it needs.

**A per-player message.** `Step` and `Standing` are broadcast to a whole room, and this is addressed to one player. The cheapest shape that fits the existing model is to broadcast it and let the client it names act on it — `Ousted { player, reason }` beside `Standing` — since every client already drops what is not about it.

**A `Player` that can be out.** The condition has to be *remembered*, or a player who is ousted and then has ground spread back onto them flickers in and out of their seat. That is a field on `Player`, which is in the save, so it wants a thought about what an older save means when it comes back without one. Absent should read as "not out", which is the reading that costs nothing.

**A cadence.** Both halves of the condition need a pass over the world, which `Server::territory` already does for `Standing` — so this belongs on `STANDING_EVERY` rather than every generation, and can read the count that pass already produced.

The one thing to be careful of: **do not oust somebody who has just arrived.** A player is granted their patch on joining, so they always have territory from their first generation — but a match that has not started grants nothing until the whistle, so during `Gathering` every player would qualify. Gate it on `phase.stepping()`.

## Teams

**Built for matches** — see [game.md](game.md#teams) and [server.md](server.md#teams). Solo or teams is chosen on the creation form; how many teams is chosen there too; who is on which, and what each is called, is settled in the lobby.

It turned out smaller than the design said, and then smaller again. The first version widened one comparison: the cell already carries an owner and the rules already read it, so what a team changed was not what a cell *is* but **what counts as yours** — `net::reach` with `==` widened to `Sides::allied`, and `net::value_delta` where an ally's cell reclaims at your own rate. That was the right shape and one abstraction too many.

**A team is a player.** It has a number, a purse and a patch of granted ground like anybody else; joining one takes its controls, and `Player::plays_as` is the whole of what the client and the server have to know. Every comparison went back to `==`, because two allies are not two players who may build on each other's ground — they are one player with several people at the keyboard. `Sides`, `TeamId`, `seat_number`, `leader_of` and the family-of-hue machinery all went with it, and two bugs went with them: see [server.md](server.md#teams).

Three decisions worth keeping:

**Scoring needs no special case.** A team's cells carry its number, so `Server::territory` counts them under it and `matches::leader` is the answer. The version that summed each side by hand is what a separate team concept cost.

**The balance check is at the whistle, not in the lobby.** A lobby that refuses to let you join your friend because the teams would be uneven makes people argue about the order they clicked in; one that refuses to *start* until everybody has picked and no team is empty is one where they sort it out and press it again. Sizes beyond that are not checked: three against two is a match people arrange on purpose.

**Teams are settled once it starts.** Changing them mid-match would hand your ground to the people you were fighting, which the scoring could not sensibly explain.

### What is left

**The colour needed nothing** — see [game.md](game.md#teams). A team is a player, so its cells carry one number and are drawn in one colour, and the hue table went back to what it was before teams existed: a player's number stepped around the wheel by the golden ratio, a constant the client hands to the shader in the camera uniform.

There was a real design here and it is worth recording what it cost, because the measurement it was waiting on is exactly the measurement that says the design was unnecessary. A team took a golden-ratio step and its **members** spread over a narrow arc around it, a twelfth of the circle, so that allies read as one colour across a screen of cells and were still told apart when looked at. The arc was fixed rather than widening with the team, on the reasoning that mistaking your own two colours costs nothing and mistaking an enemy for an ally costs the game. All of it was 165 lines of arithmetic keeping two numbers *look* like one number — and the thing it never established, whether two allies a twelfth apart are distinguishable at four pixels a cell, stopped mattering the moment they were one number.

**Friendly fire is on**, and that is the honest first answer rather than a decision. A glider is a weapon whoever built it, and a rule making allied life pass through allied life would be a rule in `sim` — which is what this design exists to avoid. Teams are about scoring and building, not immunity.

**A world may have them too**, which reversed a decision. Teams were a match feature on the reasoning that a team is a way of deciding a result and a world has none — but that is only half of what a team is, and the other half is people playing as one player, which needs nothing to win. A world with two teams is two shared kingdoms rather than fifteen small ones. What stays a match's alone is the balance check, because a world has no moment to make it at.

**The lobby cannot lock a team**, so anybody may join any team including one that is already full. That is deliberate — see the balance check above — and it does mean a five-player match can end up four against one if people are careless. The whistle allows it; whether it should is a playtest question.

## Rating

**Built.** A number that says how good somebody is, updated by results, in the shape of Elo. It is on the home screen, above the record and deliberately not inside it: what `views::record` shows is what this *client* has done out of its own store, and a rating is what a *server* thinks of you against everybody else there. Folding one into the other would suggest the client had worked it out, which it must never look like it can.

`server::ratings` is the table, keyed by `PersonId`, saved to `ratings.tsv` beside `people.tsv`. `Rooms::step` settles a match on the generation it is decided — not the room that ended, because a rating outlives every world here and a match's world is about to stop existing — and broadcasts `ServerMessage::Rated` to everybody who was in it, so the number moves on the screen somebody is looking at rather than on their next join. A `Welcome` carries it too, for arriving.

What is **not** built is [a leaderboard](#a-leaderboard), and it is not an oversight: a table of who is best is a reason to cheat, and this game has never had one. Per server the only lever is who you play and how often, which is a question about what results count rather than about who recorded them.

The arithmetic is in, as [`server::rating`](../src/server/rating.rs): expected score from a rating difference, a K-factor times the surprise, and the reduction that turns a match of up to fifteen into something a two-player formula can eat. It reads and writes nothing and is keyed by nobody — it takes numbers and returns numbers — which is the half that can be correct before the question below is answered.

The reduction is the part that was a choice rather than a formula, and it is written up at `deltas`: **every pairwise outcome**, so a fifteen-player match is a hundred and five little games and coming second in a field of experts is not the same result as coming second in a field of beginners. The surprise is divided by the number of opponents, so K stays the most a *match* can move a rating rather than the most a pairing can — otherwise entering a crowded game would be worth more than being good at one. Allies are never rated against each other, since there is no result between two people who won the same match, which is what makes a team result fall out as one pairwise outcome per opposing pair.

What is left is the sentence below, and it is all of what is left.

Most of the rest exists. A match already has a winner, `Victory` already says how it was decided, and `client::record` already keeps what this client has played — so the *client* half of showing a rating is nearly free.

What it ran into was that a rating is a fact about a person and this game had no people. **That is answered**: a person is something the server can name across rooms, `sim::Player` records who is sitting in each seat, and `server::people` is the table to key by — see [identity](#identity-is-a-keypair-and-today-it-is-not), which is about making that naming safe rather than about whether it exists. What is left is the table itself and the call, which is one `rating::deltas` at `MatchPhase::Over`.

Two more, both real:

**It cannot live on the client.** `client::record` is a browser's `localStorage` — a player who wants a better number can edit it, and one who clears their cache loses it. A rating that anybody can set is a rating nobody reads. So this is a **server** table keyed by that identity, which is the first persistent thing the server would keep that is not a world.

**Elo is for two players.** A match here is up to fifteen, and multiplayer Elo is a genuine choice rather than a formula: treat the result as every pairwise outcome (everybody you beat, everybody who beat you), or score against the field average, or rate only the winner. The pairwise reading is the usual answer and is what a free-for-all wants, and it falls out naturally once [teams](#teams) exist, because a team result is one pairwise outcome per opposing pair.

The order that makes sense is identity, then teams, then this. Doing it before identity means building a rating on a number that gets handed to somebody else next week — and the identity in question is the one in [many servers](#many-servers-and-what-must-not-be-decentralised), which is the same missing piece seen from a different side.

[A leaderboard](#a-leaderboard) is the other half of this and waits on exactly the same thing; the tiers, the placement matches and the decay are written up there rather than here, because they are what a rating is *shown* as and this is what it is.

## Many servers, and what must not be decentralised

**Decided, and partly costed.** A client that knows several servers rather than one, and a way to find them that is not a list somebody maintains.

Start with what does **not** move, because it is the constraint everything else is arranged around. **A world has exactly one authority, and that is not a limitation to be engineered away — it is what makes the simulation deterministic.** The tick is the unit of lockstep: an action is applied *at* a generation, a birth's owner is seeded from the generation, and two peers stepping the same cells at different ticks produce different worlds within a few seconds. Splitting one world across two authorities means agreeing on a tick across a network, which is precisely the problem this design exists to avoid — see [simulation.md](simulation.md) and [networking.md](networking.md#the-server-is-the-clock). A federated world is not a hard version of this feature; it is a different game.

So what decentralises is **discovery and identity**. Three pieces, in the order they depend on each other.

### Identity is a keypair, and today it is not

**Open, and this entry said Built.** Correcting the record first, because the claim that was here is the kind that matters: it said the client mints a keypair, never sends it, and signs a challenge, and that `server::people` "holds no secrets at all". None of that is true of the code.

What is true is in [`net::auth::person`](../src/net/auth/person.rs) and [`server::people`](../src/server/people.rs), both of which say so plainly. The ed25519 scheme was **removed**: a `Secret` is now sixteen random bytes that the client sends on every join, and the server stores it beside the id it issued, in `people.tsv`, in plaintext. So today:

- a secret is a **bearer credential** — whoever holds it is you, and the server it is presented to holds it;
- `people.tsv` is the file on the machine worth stealing, because every line in it is a player somebody can be;
- and a server that has met you can be you **on every other server that has met you**.

`person.rs` calls that a single-server design and says it has to change before there are two. That is exactly right, and it is the first thing in this entry rather than a footnote to it: everything below assumes an identity a server cannot take.

### What has to be true

**Three properties, and the third is the one that is easy to lose.**

1. **A server verifies rather than looks up.** A join is a signature it checks by arithmetic, so there is nothing in a server's files worth stealing and nothing to leak.
2. **The private half never leaves the device it was made on.** Not "is not sent" — cannot be read, by the page or by anything on it.
3. **The central service cannot be you either.** A directory that could impersonate its users is a server with a bigger blast radius, not a solution.

### The scheme, and the one detail worth getting right

ed25519, back where it was: the server offers a nonce on the socket's first word and the client signs. `PersonId` becomes a **fingerprint of the public key** rather than something a server issues — derived, so every server calls you the same thing, which is the whole point and is also what makes `people.tsv` a table with nothing secret in it.

**Sign more than the nonce.** A signature over a bare challenge is replayable sideways: server A, which you are joining honestly, hands your signature to server B and is you there. So the signed message names **the server and the room** as well as the nonce — a signature is then evidence about one join to one place, and a relay has nothing to relay. This is the bug the previous scheme would have had and nobody would have found until there were two servers, which is to say until it mattered.

A server's identity is its own keypair, so "which server" is a public key rather than a hostname somebody could take. That also gives the client something to pin, which is what stops a room list from sending you to an impostor.

Migration is a version 4 line in `people.tsv` holding a public key. A person whose version 3 line holds a secret cannot be re-keyed, because the thing that would key them was never on that machine — their rating starts again. One line in a release note, which is the same answer this file already gives for records filed under a room's display name.

### Non-extractable keys, which is what "never leaves the device" needs

A secret in `net::keep` is hex in `localStorage` on the web, and any script on the page can read it — including one that got there by accident. The settings screen prints it on purpose. That is the sense in which the key is too easy to reach: it is not that it is exposed, it is that nothing prevents it.

**WebCrypto has the answer and it is not a library.** `crypto.subtle.generateKey({name: "Ed25519"}, false, ["sign"])` returns a `CryptoKey` whose private half JavaScript never holds — the `false` is `extractable`, and the browser enforces it. Store the handle in IndexedDB and the page can **use** the key while it is open and can never **take** it. That is a large difference and it costs one API.

Ed25519 in WebCrypto is recent enough to need a fallback; ECDSA P-256 has been there for a decade and is the obvious one, with an algorithm tag on the wire so a server knows which it is verifying. Natively this is a file at `0600` and the same discipline.

**What it costs is export**, and that is the trade rather than a detail. A key that cannot be read cannot be carried to another machine, and carrying it is how somebody is the same person on their phone and their laptop today. The answer is not to make it extractable.

### So an identity is a set of devices

**One identity key, and a device key per machine.**

The identity key **is** you. It signs, and otherwise sits still — written down once as a recovery phrase, or kept non-extractable on the first device with the phrase as the only copy. A device key is made on the machine that will use it, is never extractable, and is authorised by a signature from the identity key.

**Adding a device moves no key material.** The new one shows its public key as a short code or a QR; a device already authorised signs it; the pair is published. Nothing secret crosses the gap, so it does not matter what the gap is — a photograph of a screen is fine.

**Removing one is a revocation** signed by the identity key. A server checks three things on a join: the signature is by a device key, that key chains to an identity key, and it is not revoked.

**And that is what transferring ownership is.** You do not hand somebody a key. You authorise theirs and revoke yours, which is the same mechanism as getting a new laptop and has the property that the moment of handover is a signed statement rather than a copied file. It is also the only version that is honest, because a copied key means two people are permanently the same person and neither can undo it.

### Where the big server fits, and where it must not

**A directory, not an authority.** This is the load-bearing claim of the whole design: once identity is a keypair, a game server verifies you by arithmetic, so a central service is **never in the authentication path**. It can be down and you can still play. It can be malicious and it still cannot be you, cannot forge a result, and cannot refuse you a game — because everything it serves is signed by the key it is about, so the worst it can do is **withhold**.

That is what makes it acceptable to have one at all, and it is worth stating as a rule rather than an outcome: *anything the directory holds must be either public, or signed by the person it is about, or worthless if it is wrong.*

What it holds:

| | why it needs a centre |
|---|---|
| name → public key | uniqueness is inherently central, and nothing else here is |
| a key's device set and revocations | publishing, not vouching: each entry is signed by the identity key |
| friends | a list of public keys, signed by you, so an edited list is detectable |
| presence and invites | routing between two people who are not connected to each other |
| the server list | the tracker this entry already describes |

What it must never hold: a private key, or a rating. A rating that travels between servers needs servers to trust each other's arithmetic, which is a much larger thing than a keypair — see [rating](#rating).

**Names are a claim, not a fact.** A registered name is a statement signed by the key and timestamped by the directory. Show `alice` where a directory the client trusts vouches for it, and `alice·3f2a` where nothing does — which is what the client does for everybody today, so a player who trusts no directory sees exactly what they see now and nothing stops working.

### Friends, searching, and inviting somebody in particular

**Friends** are a list of public keys, signed by you and stored by the directory. One-way is enough and is simpler than mutual: what the list is for is "where is Alice playing", and that wants following rather than a handshake.

**Search** is a name prefix against the directory's table. It is also the entry's one real abuse surface — enumeration, and unwanted contact — and the answer is the directory's rather than the game's: rate limits, and a person who has not registered a name is not in it.

**Presence is opt-in**, and that is the only line in this entry that is about something other than engineering. A game server telling a directory "this key is in this room" is the one piece of the design that is genuinely about being watched rather than about being found, and it should be off until somebody turns it on.

**An invite names a key, and that is what makes it better than a code.** A private room today is reached by a six-character code, which is a *bearer* credential: whoever it is forwarded to gets in, and the room cannot tell. An invite is a signed statement — *this key admits that key to this room on this server until this time* — so forwarding it achieves nothing, because the far end signs as somebody else.

The room side is small: `Rooms` gains a set of admitted keys per private room, and `Join` already carries a signature, so the check is a set lookup. Delivery is through the directory when both are connected and a link when they are not — and the link is then not a bearer token, because it names you.

**Codes stay.** They are good at the thing they are good at, which is reading six characters out loud to somebody sitting next to you, and that case wants no directory and no account.

### Room ownership should be keyed by person

`Rooms::owner` is a `PlayerId`, which is deliberate and survives a reconnect — a seat is per room and comes back. What it does not survive is a restart, and it cannot mean anything on a second server. Keyed by `PersonId` it is one fact everywhere, which is what "close the room you opened" and "hand this room to somebody else" both need, and it falls out of the identity work rather than being separate from it.

### Multi-homing the client

`GameApp` holds one `Link`. Knowing several servers does not mean holding several sockets — you are in one world at a time, so one socket is right — it means the **store** holds a list of servers rather than the last one, and the Play screen lists rooms from more than one.

Small, and mostly already there: `net::keep` is a string store that would gain a list, the room list is already a message rather than a guess, and `RoomId` is already distinct from a room's name, so a room from one server and a room from another never collide. What is genuinely new is that a `RoomInfo` in a merged listing has to carry **which server it is on**, and the client has to hold that beside it — which is one field and one column in the list.

### Discovery, where the two answers actually differ

**A tracker.** Servers announce themselves to a small service; clients read the list. This is the Minecraft server-list model, it is half a day's work, and it is centralised — but *replaceable* centralised: the address of the tracker is a setting, anybody may run one, and a client may read several. That last property is what makes it an acceptable first answer rather than a betrayal of the idea.

**Gossip.** Each server holds a list of peers and exchanges room listings with them periodically, so a client connected to one server sees the mesh. Genuinely decentralised, and the interesting part is the failure modes rather than the protocol:

- **Stale listings.** A room that has gone still appears for as long as it takes the news to travel, so a client must be able to find out on the join rather than only from the list — which it already can, because `Rooms::resolve` refuses a name that is not here and the refusal is already shown.
- **A server advertising what it does not have**, honestly by lag or dishonestly on purpose. The join is the check either way, which is the same answer as above and is why it is worth having the refusal be good.
- **Fan-out.** Every server carrying every room's listing is fine at a dozen servers and is not at a thousand. That is a real limit and the point at which this wants a proper answer rather than a periodic exchange.

The order that makes sense is identity, then multi-homing, then a tracker, and gossip only if a tracker turns out to be the thing people object to. Doing discovery first gets you a list of servers you are a different person on every time you visit.

### What this leaves alone

**Rating stays server-side and per-server**, at least at first. A rating that travels between servers needs signed results, which needs servers to trust each other's arithmetic, which is a much larger thing than a keypair — and a per-server ladder is a perfectly good ladder. See [rating](#rating).

**Anti-cheat does not get easier.** The server is authoritative over its own world and always was; nothing here weakens that, and nothing here helps with a server that lies about its own results. That is the wall a cross-server rating runs into and is why it is not in this entry.

## The menu draws nothing on some machines

**Open, and not reproduced.** On one laptop the client starts, the canvas is created and the background colour is painted — and no interface is ever drawn. On the web the loading panel stays up, which means `init()` has not resolved; on native the window is simply empty. Joining a room directly with `?room=main` works, and from there the back arrow reaches a menu that draws perfectly.

What is known, all of it checked rather than assumed:

- **It is not the code.** A fresh clone of the same commit is fine. Whatever differs is not in git.
- **It is not the GPU.** The `?room=main` path renders the world — the shader, the sheet, the chunk texture — so the adapter works. `render::context` falls back to WebGL2 anyway.
- **It is not the module path.** `/` and `/?room=main` are both at the root, where relative and absolute resolve identically, so it cannot be what separates them.
- **It is not the toolchain**, by the owner's account, though `rust-version` is declared now so an old one says so by name.
- **It is not a debug assertion in the offline simulation**, which is the one thing the menu runs that a joined client does not — the local world only steps when there is no link. Four thousand generations of a granted world with assertions live trips nothing.
- **It is not a small or zero-size canvas, nor a populated record**, both of which now have tests.

What is still worth suspecting, in order:

**The pending GPU is collected only when something else happens.** `resumed` puts the window and an async `GpuState` aside; `take_pending` collects it on the next event. `about_to_wait` used to request a redraw *only if the app was already running*, so before that there was nothing scheduled and the loop slept under `ControlFlow::Wait` — whether it ever woke came down to an unrelated event arriving. That is fixed here: it polls until the GPU is in hand. It matches the symptom exactly and it is unconfirmed, because it cannot be reproduced on a machine where it does not happen.

**The menu's own geometry.** It is the one panel that places and sizes itself by hand — an `Area` at `ctx.content_rect().min` sized by `set_min_size`, at `Order::Background` — where every panel that *does* appear for them lets egui place it. A panel at the wrong place is a panel that is not there, and that is precisely a background colour with nothing on it. Replacing it was attempted and abandoned: `CentralPanel::show` takes a `&mut Ui` rather than a `Context` in egui 0.36, and the client only has a `Context`; the intermediate arrangement clipped the form's action button behind a scroll region. **The next attempt should start by measuring `ctx.content_rect()` on the machine that fails**, because every hypothesis here turns on whether it is the screen.

What would end it in one step is the browser console on a failing load, or the same for native with `RUST_LOG=debug`.

## Experiments

**Decided, not costed.** A mode for using this as a laboratory rather than playing it: pause, step one generation, save what is on screen, reset it, and more than one world side by side. Somewhat replacing Golly.

The argument for it is that **the simulation is already the hard part and it is already done.** `sim` is a deterministic cellular automaton with a rule table, chunked storage that only holds what life has reached, and a step that is a pure function of state and tick. What Golly is for — draw a pattern, watch it, step it a generation at a time, save it, come back to it — is that plus an interface, and everything in the way of it today is a *game* decision rather than a simulation one: the server is the clock, a player may only build where their influence reaches, and placing costs money.

So the shape of the work is mostly subtraction, and it lands in three places.

**The clock becomes the player's.** [networking.md](networking.md#the-server-is-the-clock) is emphatic that a connected client advances when told and never on its own, and that is right for a shared world and exactly wrong here. An experiment is offline by construction — one authority, which is you — so pausing and stepping is `World::step` behind a button rather than anything on a wire. The existing offline path already does this; what it lacks is a pause and a single step.

**The rules come off.** Placing is confined to a player's own territory and is priced, both of which are the game. A laboratory wants neither, and the honest way to get there is a flag on the world rather than a second `sim` — the moment there are two simulations they diverge, and the whole value of experimenting here is that what you see is what a match would do. `net::price` and `net::grant` are the two places that would have to ask.

**A pattern becomes a file.** `client::views::stamp` is most of this already — a captured rectangle of cells, kept between visits — and what it is missing is a *format*. Golly reads and writes RLE and `.cells`, which is how every pattern anybody has ever published is written down, and reading RLE is an afternoon. That is the single highest-value piece here and it is worth doing whether or not the rest happens: it turns the stamp library from a scratchpad into a way in to fifty years of other people's work.

Two things it is not. It is not a second renderer: split panes are several viewports onto several worlds, and `render::app` holds one surface and one camera, so the cost is a camera and a viewport per pane rather than a second pipeline. And it is not multiplayer — a shared laboratory is a shared world with the rules off, which is a room anybody can edit anywhere, and that is a different feature with a different argument behind it.

**Pause, step and reset are one derive between them.** Stepping on a button is what the offline path already does, and what it lacks is a way *back* — which is a kept copy of the world put in place again. That is `World: Clone`, and it is the same line [a match prediction](#predicting-a-match-and-what-it-shares-with-bots-and-experiments) and a searching bot are both waiting on. Reset is then restore, and "save what is on screen" is a clone with a name on it, which is most of the way to a scratch library that is not the stamp library.

The order that makes sense is RLE first, since it stands alone; then pause, step and reset, which is a derive and three buttons; then the rules flag; then panes, which is the only part that is real work.

## Better interfaces

**Decided.** The menu has had two passes and everything else has had none, so the client now reads as two different products depending on which screen you are on.

What is actually wrong, in the order it bites:

**The HUD is a desktop panel.** It covers a third of a phone screen, and its hint lines name a left button, a right button, WASD and escape — none of which a phone has. It also has no hierarchy: every line is the same weight, so nothing on it says what matters, where the menu now has one accent per column and says exactly that.

**There is no help a phone can open.** `?` shows the key list, and a phone has no `?` and nothing to do with a list of keys once it has one. What a touch client needs is not that list; it is the four or five gestures, shown once, dismissible.

**The hotbar is reachable and small.** It was sized against a mouse. Ten stamps and four tools on a phone want either bigger targets or fewer of them on screen at once.

**Numbers still shuffle.** [Type, and the numbers that jitter](#type-and-the-numbers-that-jitter) is the entry for that, and the record panel is the only place it has been fixed — everything else still sets a changing figure in a proportional face.

None of this is a rewrite. The pieces the menu needed already exist: `theme::Metrics` holds the sizes, `words` holds the strings, and `hue` holds the colours. What is missing is somebody applying them to the other four views.

## Bots

**Decided, and smaller than it sounds.** A player the server plays.

The reason it is small is that **nothing about the protocol changes**. A bot is a `Player` with no connection: the server generates a `Stamped` action for it, pushes it into `pending`, and it goes out in the next `Step` like anybody else's. Every client already applies actions from players it has never heard from, because that is what a `Step` is. No new message, and no client work at all beyond the lobby saying which players are bots.

Three things it does need.

**A seat.** A bot occupies one of fifteen, and the cap is per room — so a match with four bots is a match four people cannot join. That is the right behaviour and it wants saying in the lobby, next to the count.

**Somewhere to run.** In `Server::step`, before the world steps, on whatever cadence its difficulty says. It must not run on the *client*, and that is not a policy preference: a client-run bot would need a connection and could be edited into a cheat, and the server is the only thing that knows the whole world anyway — a client holds the chunks it subscribed to.

**Something to play.** This is the interesting part and the reason it is worth doing. `examples/balance.rs` already measured what the economy rewards: a blinker pays, a glider bleeds, a sprawl bleeds badly. So a competent bot is not a search — it is a small book of shapes and a rule about where to put them. Compact oscillators inside its own ground to earn, life at the frontier where territory is contested, and ice on anything it wants to keep. Difficulty is then how often it acts and how well it chooses, which are two dials rather than an algorithm.

**The book is the right first version, and a search is the second.** A bot that places from a small book of shapes needs nothing that does not exist. A bot that *chooses* — try a placement, step a copy of the world, score what happened — needs a world it can step without stepping the real one, which is the same `World: Clone` [a prediction wants](#predicting-a-match-and-what-it-shares-with-bots-and-experiments). Doing them in that order means the second is a better evaluator behind the same interface rather than a rewrite, and it means difficulty stops being two dials and becomes how deep the search goes.

What it runs into: **a bot that plays well makes a match unwinnable, and one that plays badly is a candidate for [the mercy rule](#the-mercy-rule)** — which would oust it, freeing its seat mid-match, which is either exactly right or very confusing and has not been played enough to say.

Determinism is not a problem, which is worth stating because it looks like one. A bot's choices are made once, on the server, and reach every client as ordinary actions at a stated tick — so two clients never disagree about what it did, and a bot may use whatever randomness it likes without touching `sim::seed`.

## Predicting a match, and what it shares with bots and experiments

**Decided, not costed.** A live estimate of who is going to win.

### Why it is cheap here and expensive everywhere else

Games estimate a result with a model fitted to past games, because they cannot run the game forward. **This one can.** `sim` is a deterministic cellular automaton, a step is a pure function of state and tick, and `examples/headless` already runs it with no GPU — so the honest way to say who is winning is to step a copy of the world forward and look. No model, no training data, and right by construction for the assumption it makes.

One rollout per victory condition, and both read off machinery that exists. **Timer:** step a copy to the deadline and read `net::standings`. **Territory:** step until somebody crosses the line, or a bound is reached.

### What it assumes, which is the interesting part

A rollout with nobody acting answers *who wins if everybody stops playing*, and that is a **bad predictor in a game where income compounds**. A player with mines running and money in hand is exactly the one whose position keeps improving, and a no-input rollout scores them as though they had already spent everything they were going to.

So there are two versions and they differ by one thing.

**Nobody acts.** Cheap, and honest if it is labelled as what it is — *if nothing more is placed*. Worth having on its own, because it answers a question a player actually has, which is whether they are ahead or whether it merely looks that way while a shape of theirs is about to die.

**Everybody keeps playing**, which needs somebody to play them. That is a bot. So the good predictor **is** a bot run against a copy of the world, and the two stop being separate pieces of work.

### The missing object all three want

Every one of these needs to step a world without stepping *the* world, and nothing can today.

`World` is not `Clone`. That is one derive — `Storage` is a `HashMap<Coord, Chunk>` or a `Box<[Chunk]>`, and `scratch` and `active` are working space — and the step is already pure in state and tick, so a copy diverges cleanly and cannot reach back. `Server::step` owns the only world there is, and a rollout must touch neither the pending actions, nor the purses, nor the tick.

So the whole of the machinery is: derive `Clone`, and a rollout is a clone stepped *n* times with `net::standings` read off the end. What that one derive buys:

| | |
|---|---|
| a prediction | a clone stepped to the deadline |
| a bot that searches rather than follows a book | a clone per candidate placement, scored |
| an experiment's **reset** | keep the clone, put it back |
| an experiment's **step one generation** | the offline path already does this; the clone is what makes it undoable |

Four things this file lists separately, waiting on one line.

### Where it runs

**The server**, for the same reason `Standing` is the server's: a client holds the chunks its viewport covers, so a client-side rollout predicts its own screen rather than the world. On the `STANDING_EVERY` cadence, since it is a clone and *n* steps, and a figure that moved four times a second would be unreadable anyway.

Cost is the honest question and it has an honest answer. A rollout to the end of a two-thousand-generation match, four times a second, is not affordable — and does not have to be, because the useful claim is not the final score. A hundred generations says whether the leader is **pulling away**, which is what somebody watching wants to know and is twenty times cheaper.

### What it changes about the game

**A prediction tells you when to give up**, and that is not a neutral thing to add. It meets [the mercy rule](#the-mercy-rule) — the server deciding somebody cannot act — and forfeiting, which is a player deciding it. A figure saying four per cent makes conceding a reasonable act rather than a rude one, and it therefore makes it happen more often. Whether that is wanted is a playtest question, and it is much easier to ask before it is on everybody's screen than after.

There is a way to have it without asking yet: **show it to a spectator and not to a player.** A spectator wants exactly this figure and has no decision for it to distort, and [spectating](#spectating) is already built.

## A leaderboard

**Decided.** Who is best on this server, and a screen that says so.

It is the second half of [rating](#rating) and it waits on the same thing: a rating is a fact about a person, and until a person is a keypair rather than a seat there is nothing to key a table by — see [many servers](#many-servers-and-what-must-not-be-decentralised). Building it before that means a table of numbers that get handed to somebody else next week.

Once there is an identity, the work is ordinary: a table on the server keyed by it, a `ClientMessage::Leaderboard` answerable without a seat the way `Rooms` is, and a screen. It is the **first thing the server would keep that is not a world**, which is the part worth thinking about rather than the ranking — a save format, a place for it to live, and an answer for what happens when it cannot be written.

Three decisions taken from [MCSR Ranked](inspiration.md#the-dashboard-and-a-rating) and worth taking together:

**Named tiers over a bare number.** Six ranks at thresholds, so a rating is something to reach rather than a figure to read. A raw number tells a player nothing about where they stand.

**Placement matches before a rating is shown**, so one bad first game does not define somebody.

**Decay only at the top, and only on inactivity.** It keeps the top of a table honest without punishing anybody who plays occasionally.

What it runs into is that **a leaderboard is a reason to cheat**, and this game has never had one before. Per server it is manageable: the server is authoritative over its own world, so the only lever is who you play and how often. Across servers it is not — a server can say whatever it likes about its own results — which is why [rating](#rating) stays per-server until there is a reason to solve that properly.

## The session comes out of the game view

**Designed.** `client::views::game` is the one place the architecture in [architecture.md](architecture.md) does not hold, and the largest file in the crate by a factor of two.

It is a folder now — `start` is where the client is told where to go and what to look at until it gets there, and `input` is the gesture arithmetic, which was already tested without a window and now says so by living apart from everything that needs one. Neither of those is the thing below, and splitting them out has not made it smaller: `mod.rs` is still two and a half thousand lines and still holds the world, the link and the pipeline in one struct.

It is a *view* by where it lives, and it is not one by what it does. It holds the world, the link and the GPU pipeline, and it executes logic: `pump_link` folds server messages into the world, `lay` and `click` price and send actions, `advance_to` steps the simulation. Data, logic and interface in one struct with forty fields — which is exactly the arrangement the [Data / Logic / Interface](inspiration.md#the-architecture) rule names, and which every other view already avoids through the `Chose`/`Picked` return-value convention.

The symptom is visible in the code rather than inferred: `update` takes its own fields out with `mem::replace` and `mem::take` and clones two more every frame, because `self.views.borrow_mut()` holds a borrow of `self` across the whole interface closure. Each of those has a comment explaining the borrow it is dodging, which is the tell — the code documents a design problem rather than fixing it.

What comes out is a **`client::session`**: the link, the subscription set, the room id and name, `me`, the purse, the lobby, the standing, and the `pump_link` / `advance_to` / `send_checkpoint` / `subscribe_to_view` machinery. It takes messages in and produces world mutations and outbound messages, and it needs no GPU and no egui — so it is testable, which none of it is today. `client::desync` and `client::record` are the first two pieces of that already living outside, and they are the shape the rest should take.

Two things it is not. It is not a rewrite of the gesture machine, which is already pure arithmetic tested without a window at the bottom of the same file. And it is not the camera, which came out for this exact reason and is the precedent.

## Rooms per server

**Built** — see [server.md](server.md#rooms). A room is a whole `Server`: one world, one player table, one tick, one file. Rooms are listed, joined, made and left from the client, and a room is identified by an **id** rather than by its name, so renaming one keeps every seat and every rejoin token valid.

What is left is lifetime — [auto-sleep](#making-rooms-from-the-client), above — and one gap in the store:

**The token is keyed by room but not by server.** Two servers both holding a room whose id is `main` share one secret, and visiting the second costs you your player on the first. `client::record` has the same hole from the other end: a game is filed under a room's display name, so two servers' `arena` are one line of history. Both stop being bugs rather than being fixed if the token becomes a key the client owns — see [many servers](#many-servers-and-what-must-not-be-decentralised).

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

Two things fell out rather than being designed, and both are better than what was planned here. **A live cell must have an owner**, so taking a square away from its owner kills whatever stands on it — the dead turret's killing is that invariant rather than a rule about killing. And a turret needed no rule about where it may be placed: its first choice is always ground that is not its owner's, so it reaches past a frontier from anywhere behind one.

The inheritance problem was answered by splitting kinds into those a birth inherits and those it does not, which is now `kinds!` in `sim::cell` — one list writing `Kind::ALL`, the count and `Kind::inherits`, the way `rules!` writes the rule chain and its names. A kind that does not inherit passes over ownership alone, so a birth beside a turret is ordinary life owned by the turret's owner and a gun is not a turret factory. That made the rest of what was planned here unnecessary: a turret never spreads, so it needs no bill to stop it sprawling, and its balance is its purchase price and its claim rate and nothing emergent.

What is left is numbers rather than mechanism.

**The balance is argued, not measured.** `TURRET_COST` at fifteen, `TURRET_REACH` at six and `TURRET_DECAY` at four in sixty-four were reasoned off the decay arithmetic — a claim a generation against `DECAY` settles at about thirty squares, so a block of four holds about a hundred and thirty — and nothing has run to check it. `examples/balance.rs` is the harness that answered this for mines and prints nothing about turrets. It should, and the shapes to put in it are the block against a lone turret against a turret dropped into a glider, since those are the three things a player will try.

**Half of this landed with territory levels.** A turret plants influence rather than flipping a flag: `rule::TURRET_PUSH` is what it puts on a square it takes, and it plants at full rather than nudging, because the rule assigns a square the strongest claim reaching it rather than adding to what is there — a push of three would be wiped the next time that square worked itself out. What did *not* change is `TURRET_POWER`, which is still a count of squares. Making it a quantity of level instead is the version that contests properly with everything else pushing on the same ground, and it is still worth doing.

**Whether a turret should press on a living neighbour is a number rather than a rewrite.** `rule::TURRET_POWER` is how many squares it takes a generation and sits at **one**, which makes it the reaching tool rather than the weapon. The arithmetic that used to be here was written against `SPREAD`, a constant the level rule deleted; what a turret now holds against a living colony is whatever `LEVEL_SPREAD` and `LEVEL_EBB` give back, and that has not been measured. `examples/balance.rs` is where the answer should be printed rather than argued about.

**A turret under ice** is the same open question as a mine under ice, and sharper. A frozen turret does not fire, so a pane is a cheap way to switch off somebody's territory engine without taking any ground from them. Whether that is a feature or a hole is for whoever sets the rate.

**The remedy for a corpse gets dearer the longer it is left.** A dead turret is cleared by building on it — placing life sets the kind back to ordinary, as it does over a dead mine — and what the corpse is doing is taking your ground away a square at a time, so the square you need to build on stops being yours and the fix goes from one to ten. That may be exactly the right shape and it has not been played enough to say.

**A turret should not also kill, and the reason is that a claim is contested and a kill is not.** Ground a turret takes is taken straight back by `SPREAD` at forty in sixty-four, which is why it cannot touch ground anything is alive on and why one square a generation is nearly nothing. Nothing does that to a kill: a dead cell stays dead unless Conway hands it back. So the same "one a generation, forever" that is almost nothing for claiming is decisive for killing — and a turret is a **still life**, four cells, immortal, free after purchase and unreachable without flying something into it. A block of them killing four cells a generation forever is not a territory tool, it is area denial with no answer.

It would also cost the two things that make a turret readable. The dead turret's kill is not a rule about killing — it is the `Cell::alive` invariant showing through, since unowning a live square kills what stands on it — and that reads as a mirror only while the live turret does not kill. And a turret that finds no frontier in reach falls back to reinforcing its own thin ground, which is a slow indirect push; one that kills always has something to shoot at, wherever it stands, and that distinction goes.

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

**Outliving the tab.** A stamp lives as long as the client does — capture a glider gun, close the tab, and it is gone, which makes the library a scratchpad rather than a collection. `net::keep` is already where a client keeps what it has, and `client::record` is now a worked example of putting a versioned, line-per-item format there. What makes it more than a `serde` derive is that a stamp is **cells and their kinds**, so the stored shape has to survive a change to `Placement` or to `Kind`: a library written before turrets existed should not come back as a library of nothing. A version on the file, and a bad line skipped rather than fatal, which is what `record` does.

## Territory as a level, not a flag

**Built.** Ownership on a dead square is a level rather than a flag, `sim::rule::territory` is one rule where there were three, and granted ground is a source rather than a carve-out. The design, the measurements behind every constant, and why a sum needs a cap are in [simulation.md](simulation.md#territory) — this section used to hold all of it as a proposal and said "nothing built" for weeks after it shipped.

The owner byte is player four bits, level three, `HOME` one. That cost a bit off the player field, which is what the entry below is about. The save format went to version 5 with no migration: a flag carries no level.

Two things are left.

**The level is not drawn.** A gradient nobody can see is a gradient nobody can play against, and this was named as probably the reason to do the whole thing. Ground reads as claimed or not, so what is on screen is where a border ended up rather than where the pressure is. Shading it also brings back the graded price — placing costing more where your influence is thin — which was abandoned for exactly this reason: a cost the player cannot see is a cost they cannot play around. The two stand or fall together and neither should be tried alone.

### Fifteen slots, and more than fifteen clients

**Open.** A `PlayerId` is four bits, so a room can tell fifteen players apart, and a number is never reused because it is written into every cell its owner claimed. Thirty-one was comfortable as "players a room has ever seen". Fifteen is not: a room that has met fifteen people is full for ever.

A person is a keypair now and `Server::join_with` looks a seat up by it, so the mapping from person to seat exists and the token is gone. What does not exist is **reclaiming** a seat. One is free when its player is offline and their number appears nowhere — no life, no ice, no ground — and `Server::territory` already counts per player in one pass, so widening it to count life and panes answers the question with a scan the world is nearly doing anyway.

Nothing about this reaches the cell, which is the point: the person lives in the server's table and the seat stays four bits.

## Matches

**The server half is built** — see [server.md](server.md#matches). Phases, both win conditions, scoring that ignores granted ground, no late joining, and `match new` / `match start` / `match dispatch` at the console. A match is a room with a phase and it needed nothing in `sim`, as expected; gathering does not step, so the opening is drawn into a frozen world rather than raced, and the deadline is a tick so it needs no clock synchronisation.

**Most of it reaches the client now.** `ServerMessage::Match` carries the phase, the win condition and who is here, and there is a lobby panel over the board while a match gathers and a result panel when it is decided; `ServerMessage::Standing` carries who holds most ground, drawn as bars in the HUD; and `RoomInfo` carries the phase, so the room list says which rooms are matches and which have started.

**The clock is in**, along the top: generations left and the same figure as a clock for a timer match, the leader's distance from the target for a territory one, with a bar under either. What is left of a match is the smaller things — no countdown into the start, so `match dispatch` is the only thing that begins one, and no way for a player to leave a lobby they have joined.

**A starting value of zero is not in.** It was in the original description and it is held back on purpose: life costs one, zero buys none of it, and the granted block never gives birth, so a match starting everybody at zero under today's rules is one where nobody can ever act. The income question below has to be settled first.

### The opening

**Settled: nothing happens before the whistle.** Gathering neither steps nor takes actions — players join, get their patch, and wait. The build phase below was tried and is wrong, and the reason is worth keeping: freezing the world makes an opening fair in *generations* and leaves it unfair in **time**, because holding the tick still does not hold a clock still and whoever joined ten minutes early has had ten minutes to think. What is left is a race, which is a better opening than a draw — everybody looks at the same thing when the clock starts, and hesitating costs generations rather than nothing.

That also answers the clicker worry differently from the way this section did. The first thing anybody does in a match is spend, under time pressure, at the same moment as everybody else. The question is only what they are spending, which is below.

### What was rejected, and why it is written down

**A 2×2 block and an income is a clicker.** A still life is the one shape in Conway that does nothing at all — it does not breed, it does not move, and it cannot die — so a player granted one and paid a trickle has exactly the clicker loop in front of them: wait, tap, wait. Whatever the income turns out to be, it does not fix that, because a stationary pattern's footprint is fixed and so its income is flat. Anything that pays by the generation pays a block the same amount forever.

Note what the block was solving, because it is easy to throw out with it. `game.md`: four cells that hold their shape forever, the same for everyone, so nobody begins ahead — and the block is also what *keeps* the ground, since territory spreads from living cells and a bare patch would never grow. So the grant has to be **immortal**, or an unlucky opening eliminates somebody before they have acted, and **identical**, or the draw decides the match. In Conway those two pull hard against "and it should do something": the patterns that do something either wander off or grow without bound.

**A build phase** — the world frozen, a fixed budget of cells, then the clock — was the recommendation here and is rejected. It is what ice already is, promoted to a rule, and Conway is a game about the initial condition, so a competition about the initial condition is a competition about the right thing. What it cannot fix is that drawing takes wall-clock time, which a frozen tick does nothing about: the first player into the lobby draws at leisure and the last draws in a hurry. A phase that is fair only if everybody arrives at once is not a phase, it is a request.

It could be rescued by a countdown — freeze, then everyone gets the same sixty seconds — and that is a real design worth trying later. It is a bigger thing than it looks: a countdown is a wall clock, and a wall clock is the one thing the tick was chosen to avoid needing.

### What happens during the run

Three readings, and they are different games.

- **Nothing.** No placing at all once the clock starts. The purest, and the least to do for however long the match lasts.
- **Territory pays, and you may intervene.** Value per generation in proportion to ground held, spent on repairs and raids. The win condition and the economy become the same thing, which falls the right way at the end — a player losing ground earns less and falls further behind. Reaching into somebody else's half is not a matter of price at all: you may only place where your own influence reaches, so it means growing there first.
- **A second build phase**, partway through. Freeze, everybody draws, unfreeze. Keeps the front-loaded decision and gives the match a second act.

The middle one is the recommendation, with the first worth trying as a variant because it costs nothing to offer.

**And if the grant is to be a machine after all**, an oscillator is the only thing that is immortal, stationary and gives births: a blinker of mines is the smallest, three cells earning every other generation forever. It is a fallback rather than an answer — it pays a flat rate, which is the clicker again with a better animation.

### Scoring

Nothing counts ground per player. The `territory` example's `survey` does it for one player by walking every stored chunk, and a scoreboard is that for all fifteen: one pass over the world, the same cost as `ice_cells` or `turrets`, of which there are already two a generation. Fine once a second, not fine every step.

It must also be the **same number for everyone**, and a client cannot compute it — it only holds the chunks it subscribed to, so it can count its own screen and nothing else. So the server counts and broadcasts, which is a new message and the first thing in the protocol that is about a match rather than about a world.

### What a scored match does to `HOME`

Granted ground never decays, so a player whose life is wiped out still holds their patch and still scores for it at the whistle. In a sandbox that is a floor that keeps them playing; in a match it is points for having turned up. Either home stops being exempt once a match is running, or it stops counting toward the score — the second is the smaller change and keeps the floor doing its job.

### What the lobby actually buys

More than it looks. Grants are laid out on a fixed grid at a fixed pitch sized for every number the cell can hold, whether two turn up or twelve, and `spawn_for` derives a position from a player number alone. With the roster known before the world starts, the grid can be packed to the players actually in it — everybody the same distance from their neighbours, and no advantage in having joined early or late. That is a change to `spawn_for`, and it is the one place a match touches something that already works.

### What is left over

A finished match should stop stepping, which is the same machinery as the sleeping rooms above and has the same trap: the tick is what a returning client adopts, so a room that stops must not look like one that never ran.

Reconnecting matters more here than in a sandbox — a refresh in the middle of a match has to put you back in your seat, which the rejoin token now does across a server restart as well.

And a player who cannot afford anything is a player watching. A build phase puts the spending before the clock rather than after it, which is the point: what a zero start must not mean is a minute of nothing while the money arrives.

## Type, and the numbers that jitter

**The defect is real and the pairing is not decided.** The generation counter, chunk counts, zoom, value, the desync rate and the match clock all redraw every frame, and egui's bundled Ubuntu-Light has proportional digits — so those columns shuffle sideways as the numbers change. A readout that moves while you read it is harder to trust than one that does not.

What fixes it is **tabular figures**, which is not the same thing as a monospace font. Monospace gives every glyph one advance width; tabular figures give it only to the digits, and plenty of proportional faces have them. The reason the two get conflated here is egui: it exposes no OpenType feature toggle, so `tnum` cannot be switched on at runtime. That leaves three routes — send numbers to the `Monospace` family, which works today and costs nothing; ship a proportional face with `tnum` frozen in by fonttools so its default figures are tabular, which is invisible and costs a build step; or allocate each digit column by hand, which is exact and costs work at every readout.

The split is the part worth deciding first, and it is not "mono or not". **A number that is compared against itself over time belongs in mono; a number read once inside a sentence does not.** The generation counter and the chunk counts are a readout sitting in a column, and mono's register is correct there rather than a compromise — that is what an instrument looks like. "3 players", "12×12 chunks, wrapping" and "first to 500 squares" are prose, and mono makes them look like a mistake and makes them wider, in a HUD already competing for the screen.

Which leaves only the proportional face as an open question, and it is a preference rather than a defect. Inter is the most legible at the sizes the HUD uses and the most neutral, which is what [theme.rs] asks for — an instrument beside the simulation rather than a frame around it. IBM Plex Sans is the same argument with a voice, drawn for technical documentation. Space Grotesk is the one with character, and is styled enough to risk becoming the frame. All three are OFL and about 180–250 KB subset to Latin, against a wasm bundle already 7.5 MB after `wasm-opt`.

Worth doing after the level shading lands rather than before, because that changes what the HUD is competing with.

[theme.rs]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/src/client/views/theme.rs

## A minimap

**Not yet**, and the reason is not effort. A client holds the chunks it subscribed to, which is its own screen and a margin — so a minimap drawn from what the client has is a picture of where you already are, which is the one place you do not need a map for.

A real one needs a **coarse summary from the server**: something like a byte per chunk, saying which player holds most of it and how strongly, broadcast on a cadence the way `Standing` is. That is a small message and a straightforward pass over the world.

What it runs into is the boundless world. "The whole map" has no edge, so a minimap of one is either a window around the action or the bounding box of everything anybody holds, and both change size as people play. On a **torus** there is no such question: the world is a fixed rectangle and a minimap is exactly that rectangle.

Which suggests the order. Do it for wrapping worlds first, where it is nearly free, and let matches be where it lands — a match wants a fixed arena anyway, and a match is where knowing who holds what without panning across the world actually decides something.

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

**Touch reaches the interface now**, which it did not: `Views` translated winit's mouse events by hand and never translated a touch, so egui received no press at all and every button on a phone was dead — see [gotchas](gotchas.md#a-finger-is-not-a-pointer-unless-somebody-says-so). The world always worked, because the client reads `App::on_touch` itself, which is why it went unnoticed.

What is left is the layout rather than the plumbing. The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And the key list behind `?` is a list of keys, which is a screen a phone has no way to open and nothing to do with once it is open.

## Known, and left alone

- **Fifteen players is a ceiling on players a room has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. It was thirty-one until the level took a bit off the owner byte. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. See [fifteen slots](#fifteen-slots-and-more-than-fifteen-clients).
- **Territory creeps and decays now**, so ground is traded and lost as well as won, with granted ground exempt as the floor. What is unsettled is the floor: "your home patch is permanent" is a strong promise, and it also means an opponent who grows over it keeps a square that will never decay for them either.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::game` is 1900 lines doing five jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, `chose`, `to_menu`, and the `me`, `room`, `value`, `screen` and `subscribed` fields — is everything about talking to a server and nothing about drawing. The menu made that worse rather than better: the screen the client is on and the connection it is holding are the same state machine, and it now lives in the same struct as the sprite atlas.

