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
| [Cloudflare, and which half of this fits](#cloudflare-and-which-half-of-this-fits) | Thought about | the page fits Pages; the server is not a Worker |
| [A minimap](#a-minimap) | Noted | a picture of where the territory is, drawn on the server, not from a client's own screen |
| [Parties](#parties) | Built | a private set of worlds for one group of players, keyed by today's person; what is left is signed invitations |
| [Buttons on a narrow screen](#buttons-on-a-screen-narrower-than-the-hotbar) | Thought about | shrink, wrap, or stop being full width |
| [Better interfaces](#better-interfaces) | Part built | the home screen is three buttons; the in-game views are still a desktop |
| [Player profiles](#player-profiles) | Part built | a person rather than a seat; what is left is devices and the name field |
| [Icons on the bar](#icons-on-the-bar) | Decided | a picture where a word is now |
| [Zooming out without lying](#zooming-out-without-lying) | Built | antialiasing, a coarse level, and a floor low enough to use them |
| [A torus repeats, so its textures can](#a-torus-repeats-so-its-textures-can) | Built | one copy of a wrapping world, drawn many times |
| [Dynamite](#dynamite) | Built | the art is a placeholder; the numbers are measured, and stand |
| [Overclockers](#overclockers) | Built | the art is a placeholder; the price is a turret's until somebody measures |
| [Depleted factories](#depleted-factories) | Built | a factory's age is its wear; the numbers have not been through `balance` |
| [The simulation on the GPU](#the-simulation-on-the-gpu) | Costed | a compute shader, and the one thing that makes it hard |
| [Making rooms from the client](#making-rooms-from-the-client) | Built | a world, a match or a private game, from the menu, started and closed from it; what is left is auto-sleep |
| [Spectating](#spectating) | Built | a room with no seat in it |
| [Games and matches by code](#games-and-matches-by-code) | Built | private rooms with a door that knows who was let in; what is left is `?code=` |
| [Ice anywhere, at a price](#ice-anywhere-at-a-price) | Decided | a wall you can build on somebody else's doorstep |
| [The mercy rule](#the-mercy-rule) | Designed | a player who cannot act becomes a spectator |
| [Teams](#teams) | Built | one seat, one platform and one purse to a side |
| [Rating](#rating) | Built | per server, on the home screen; a leaderboard is not |
| [Many servers](#many-servers-and-what-must-not-be-decentralised) | Being built | a person exists; a *safe* one does not, and discovery is not started |
| [The menu draws nothing on some machines](#the-menu-draws-nothing-on-some-machines) | **Open** | a bug, not reproduced; what is ruled out and what is not |
| [Experiments](#experiments) | Part built | a kind of **room**: the clock and the placing rules are the room's, and shared; RLE and reset are not |
| [Keys the player chooses](#keys-the-player-chooses) | Decided | defaults cannot be right; three faults have come out of that |
| [How to play](#how-to-play) | Designed | the rules nobody can infer, the four tips that matter, and who wrote the rule underneath |
| [A profile screen worth visiting](#a-profile-screen-worth-visiting) | Designed | stamps edited out of play, a face, and finding somebody |
| [Antialias always](#antialias-always) | Built | one rule at every zoom, a box filter one pixel wide |
| [Texels nothing samples](#texels-nothing-samples) | **Open** | between zoom 5 and 16 the art is drawn from a subset of itself |
| [Something to see when it goes off](#something-to-see-when-it-goes-off) | Designed | a blast is a frame of noise and nothing else says it happened |
| [Bots](#bots) | Built | a player the server plays, from a book, and an API an engine plays through; a search on `World: Clone` is what is left |
| [Predicting a match](#predicting-a-match-and-what-it-shares-with-bots-and-experiments) | Decided | run the world forward and look; one derive away, and shared with bots |
| [A leaderboard](#a-leaderboard) | Part built | per server it is built; across servers it waits on identity |
| [The session comes out of the game view](#the-session-comes-out-of-the-game-view) | Built | what is left is the gesture-to-cells half |
| [Rooms per server](#rooms-per-server) | Built | what is left is lifetime |
| [Auto-manufacture](#auto-manufacture) | Built | |
| [Turrets](#turrets) | Built | |
| [Stamps](#stamps) | Built | a shape; what it is made of is the hotbar's other axis |
| [Territory as a level, not a flag](#territory-as-a-level-not-a-flag) | Built | what is left is drawing it |
| [Fifteen slots, and more than fifteen clients](#fifteen-slots-and-more-than-fifteen-clients) | **Open** | a room that has met fifteen people is full for ever |
| [Matches](#matches) | Built | |
| [Type, and the numbers that jitter](#type-and-the-numbers-that-jitter) | Decided | |
| [Mobile](#mobile) | Designed | |
| [Known, and left alone](#known-and-left-alone) | — | |

## Dynamite

**Built**, and the age field it was designed around finally does something. `Kind::DYNAMITE` counts down while it lives, `World::detonate` runs at the top of the generation, and the blast walks outward to somewhere worth hitting. What is left is the **art** — the sprites in the sheet are a generated placeholder, a casing that fills as the fuse burns. The numbers are measured now, in `examples/blast.rs`, and [left where they were](#the-numbers-which-are-measured).

Three things came out of building it that the design did not say.

**A kind's rules live on the kind.** `Kind::ages` is a table: `Ages::Never`, `Ages::Fuse(chance)` while it lives, `Ages::Depletes` for a factory's wear. A dynamite's fuse and a factory's wear are the same field and the same step, and saying so once is what stops them being two spellings of one thing.

**A factory's age is not for this, and the table is where that is written down.** A dead factory still clears on `FACTORY_UPKEEP`, a roll of sixteen in sixty-four a generation, and it stays a roll for two reasons. The scatter does work: a corpse reborn before the charge falls due escapes it, so a chance means *some* of a pattern's corpses escape rather than all or none, which is what grades the cost by how much a pattern leaves lying about. And the field is spent on [depleted factories](#depleted-factories) — `Ages::Depletes` on that row is the wear on the square a factory was born on, a fade where a flag would be a cliff — so nothing else may count on it.

**The detonating dynamite takes its own blast's roll**, and goes through the same `World::blasted` as every other square in the disc. Left alive it is a cell standing in the middle of noise nothing else could have produced, which reads as a survivor rather than as a crater.

### The fuse

The **age** field is the fuse: three bits, so nought to seven. While a dynamite is alive and not under ice it has a chance each generation to advance, and at six it always advances — so it goes off on the generation after it reaches seven.

Two reasons for a chance rather than a count, and the second is the one that earns it. A chance **scatters** dynamite placed together, so four laid in one gesture do not go off in lockstep. And the certain last step makes the warning **reliable**: the sprite for "about to go" is on screen for exactly one generation, always, so it is a tell somebody can act on rather than a maybe. That is worth a rule of its own, because a weapon with a random warning is a weapon with no warning.

Ages are rows on the sheet — eight of them under a kind's four states, which is [why age sits where it does](simulation.md#the-cell) — so a dynamite visibly counts down and needs no interface at all to be readable.

### Detonation, simplified

The goal was *make the surrounding area look like random noise*, and the design for it was a scoring function: a squared density cost over a kernel, a closeness weight of `1/(dx² + dy² + D)`, and a greedy pass picking the highest-scoring cell to flip and recomputing after each one.

**Every peer already agrees about randomness.** `sim::seed` is a seeded stream per cell per generation, it exists precisely so that two peers make the same random choice without exchanging anything, and it is what a birth already uses to pick a parent. So the scoring function is a way of *manufacturing* randomness with a deterministic optimiser inside a codebase that has deterministic randomness. One `mix` per square gives the same result for nothing.

So detonation is: **every square within `DYNAMITE_REACH` takes its own roll, and comes up alive at `DYNAMITE_DENSITY` out of sixty-four.** One constant, and it is the same constant the cost function was reaching for — the density term was the only thing in that design doing work that a probability does not do directly.

Three more reasons to drop the search, beyond not needing it:

**It is the wrong shape for this simulation.** Every rule in `sim::rule` is a pure function of a cell and its eight neighbours, which is what lets a generation run out of a `Halo` with no bounds checks. A region-wide optimiser cannot be a rule; it has to be a pass, like `fire_turrets` and `break_ice_from`. Detonation is a pass either way — "every square within reach" is not a question a halo can answer — but a pass that is one roll per square is nothing, and a pass that is *actions × area × kernel* with a recomputation between each is the most expensive thing in the game happening at the least predictable moment.

**Cost that varies with the board is the enemy of a prediction.** The same step runs on the server and on every client, and [a rollout](#predicting-a-match-and-what-it-shares-with-bots-and-experiments) runs it hundreds of times over. A detonation whose cost depends on how crowded the neighbourhood is makes all of that unschedulable.

**And the numbers do not fit.** "Dynamite with a fuse of less than 14 will be set to 14" — the age field is three bits, so a fuse is nought to seven and fourteen is not a number it can hold. That is what a design written beside the code rather than in it eventually does.

### Where it goes off, which is not always where it is

**A blast wasted on its owner's own ground is a blast wasted.** A square the
blast rolls up is already yours afterwards, so a detonation inside your own
country buys you what you had and destroys your own patterns to do it. A player
standing a dynamite deep in their own territory is making that mistake by
accident rather than on purpose.

So the blast **walks outward** until it is worth something: search rings at
increasing distance from the dynamite for a centre whose disc is at least
`DYNAMITE_FOREIGN` in sixty-four not its owner's, take the nearest, and break a
tie with a seeded roll. Which is `turret_target` again, in shape and in code —
the nearest square that answers a question, with the tie broken so a volley
does not always favour one direction.

**Not its owner's, which includes nobody's.** A blast claims what it reaches,
so open country is worth hitting, and a count of somebody *else's* ground —
which is what `worth_hitting` used to take — would leave a dynamite unable to
do the thing it was given. What that re-admits is the crater loop: the debris
of a blast is mostly unowned, so one can be aimed at the last one's hole. It is
priced out rather than ruled out, since a third of a disc for `DYNAMITE_COST`
is a worse rate than any ordinary way of holding ground. And the quarter is
lower than it reads: a disc centred on a frontier is half somebody else's at
best, and anything further in is reached by walking past ground that
qualifies less.

Nobody's includes **ground nobody has loaded**. An infinite world holds only
the chunks something has touched, and both the count and the scramble read an
absent chunk as dead and nobody's, the way a turret does. They did not, and
the same stick did half as much at a chunk corner as in the middle of one —
see [gotchas](gotchas.md#a-pass-that-reads-a-disc-reads-the-void).

Two things make the search cheap enough to be a pass rather than a search.

**What it is looking for is a count, not a cost.** How many squares in the
candidate disc are already this player's — the same question
[`crowding`](../src/net/mod.rs) asks when it seats a latecomer, and one pass
over a disc rather than a scored optimisation over every cell in it.

**And it is bounded.** `DYNAMITE_THROW` is the furthest a centre may be
displaced, so a dynamite in the middle of a large country lobs itself at the
nearest frontier and not across the map. Unbounded, it would be a homing weapon
with a range of the whole world, which is a different thing entirely and a much
worse one.

What that buys, and it is worth stating because it makes the piece playable
rather than merely correct: **a dynamite does not have to be placed exactly.**
Placing is confined to your own influence, so without this the only useful
dynamite is one laid on the exact square of your border nearest something worth
hitting — which is a precision the interface does not really support and a
frontier that moves every generation anyway. Walking outward means "somewhere
near my edge" is good enough, and the rule finds the rest.

### A blob is one bomb, and each charge is worth an area

**A hundred dynamite reach ten times as far as one, not a hundred times.**
`blast_reach` is `DYNAMITE_REACH * sqrt(n)`, so each one going off adds a
constant *area* of blast — which is the only scaling that makes a cluster
worth building without making it the only thing in the game. Below it, a blob
does less than the same dynamite laid apart and nobody clusters; above it,
nothing else matters.

It is also the honest reading of what a cluster is. Dynamite that stand in
each other's disc — within `DYNAMITE_REACH` of one another, which is closer
than discs merely overlapping — are grouped, transitively, so a line of them
is one long bomb rather than a chain of pairs, and the blast is centred on the
middle of the blob, because that is where a bomb made of all of them is.
`DYNAMITE_MOST_REACH` bounds it: the pass is one roll per square, which is nothing until somebody
works out that a thousand of them would rewrite a quarter of a large world in
one generation.

### The chain, which is the best idea in it

The one part of the scoring design worth keeping outright, and it needs no score: **on detonation, every dynamite within reach has its fuse set to full**, so it goes off the next generation. A line of them is then a fuse, and a cluster is one blast a generation wide rather than one big one.

It cannot recurse, and that is worth saying because it reads as though it might: setting a fuse to full means the cell detonates on the **next** generation, not this one, so a chain is one ring per generation and the pass never re-enters itself.

### Whose noise is it

**The blast decides**, and one roll decides both halves: a square that comes up alive is the bomber's, and a square that does not is nobody's. `World::blasted` is the whole rule.

It used to be the other way round — the ground decided, a square brought to life kept the owner already on it, and a square nobody held could not come alive at all. That read well and was wrong in play, because most of anybody's territory is ground with nothing standing on it. A dynamite thrown at an empty frontier filled a third of it with live cells that were **still the defender's**, on ground they still held. The bomb was a gift, and the more of their country was empty the bigger the gift. `a_blast_leaves_no_life_belonging_to_anybody_else` is the test that says so.

So, per square in the disc:

- **Alive: yours**, at full strength, because `Cell::alive` is — level and influence have to agree on a source, and a corpse owned at level nought is a state the rule says cannot exist.
- **Dead: nobody's**, at level nought, which is that same impossible state from the other side. So the two move together and a crater is genuine no-man's-land.

What that buys is that a bomb **breaks a country apart and leaves you a third of the pieces**, rather than merely animating what was there. Their factories' corpses still cost them upkeep and their shapes still stop being shapes; what is new is that you hold what is left.

It is now a land grab, deliberately, and that is the thing `examples/blast.rs` watches: at `DYNAMITE_COST` a dynamite buys about a third of a disc of `DYNAMITE_REACH`, and if that rate ever beat growing life outward the turret would stop being the tool that takes ground. It does not, by a wide margin — see [the numbers](#the-numbers-which-are-measured).

**Two squares are exempt.** Ice, because a pane stops time over whatever it covers and that is every rule. And **granted ground**, which is subtler and matters more: `rule::territory` returns before a home square, so nothing else in the game moves one, and `net::already_granted` reads exactly that to know a returning player still has a seat. A blast that converted one would evict somebody from their spawn permanently and hand them a second patch on their next join. And because the owner there cannot move, a home square that came up *alive* would be alive **for them** — the gift bug again, in the one place somebody would aim to exploit it. So a blast may take life off a granted patch and may never put it there. `a_blast_clears_a_granted_patch_without_taking_or_feeding_it`.

It needs no new placement rule. `net::may_place` confines you to your own influence, so a dynamite is laid on your own frontier and its blast reaches across the border — which is exactly the range question `DYNAMITE_REACH` is for.

### What it is made of

`Kind::DYNAMITE`, which is a row in `kinds!` and costs one of eight kind indices — five are used, and three are spare, since [depleted factories](#depleted-factories) went into the age field rather than a kind and the [overclocker](#overclockers) took the fifth. Sprites at tiles 12–15, which is the last group in the sheet's first row, so the art that exists does not move.

**It inherits**, which is the decision that makes it a weapon rather than a factory you cannot eat. A birth copies its parent, so a glider that picks one up carries it — a pattern that crosses a border and goes off inside somebody's country, which is the piece the rest of this entry was missing. The cost is real and worth stating: a gun that catches one is a factory. What limits it is that the **fuse travels too**, so a factory's output goes off near the factory, and that a dynamite is a live cell like any other — kill the pattern and there is nothing left to inherit.

And it leaves **no corpse**. `Kind::leaves_a_corpse` is the row that says so: a factory's corpse costs its owner and a dead turret fires backwards over the ground behind it, so both go on being what they were, and a fuse that has gone out is ordinary dead ground. An armed corpse would take away the one answer that does not need ice, which is that a dynamite has to be kept alive to be worth anything.

The dynamite is **consumed** — it becomes ordinary dead ground, the way a spent factory does.

### The counterplay is already in the rules

Worth checking rather than assuming, because a weapon that deletes a screen of somebody's work with no answer is not a mechanic. Three answers, none of them new:

**You can see it coming.** Eight ages are eight sprites, and the last one lasts exactly one generation.

**Ice stops it.** A pane freezes what it covers and that is every rule, so an iced dynamite's fuse does not advance. Ice is the counter, and it is the one defensive tool in the game — which is an argument for [ice anywhere, at a price](#ice-anywhere-at-a-price), since a dynamite is precisely the thing you want to wall off on somebody else's doorstep.

**It has to survive to go off.** A dynamite is a live cell, so one on its own dies of loneliness in a generation, exactly like a turret. Keeping it alive means building something around it, and that is the real cost rather than the purchase price.

### The numbers, which are measured

`DYNAMITE_COST`, `DYNAMITE_REACH`, `DYNAMITE_DENSITY`, `DYNAMITE_THROW`, `DYNAMITE_FOREIGN` and the fuse chance. `examples/blast.rs` is where they are argued out, the way [turrets](#turrets) says its own numbers should be, and it measures **value rather than area** now: a stick laid armed on the frontier of somebody's field and stepped, beside a four-turret emplacement and beside a hundred and fifty-three cells of plain life laid in the same place, on three fields — held ground with nothing standing on it, a still life of blocks on a four-square pitch, and a soup at the blast's own density. Held is every square the bomber owns; lost is what the field's owner holds when left alone, less what they hold now. The mean of four seeds, and the stick is laid armed, so the twenty-five generations a fresh fuse takes to burn — and has to be kept alive through — are not in it. What it printed on the soup, at the numbers as they stand:

| | cost | gen 1: held, lost | gen 25: held, lost | gen 100: held, lost | a square held, at 25 |
|---|---|---|---|---|---|
| a stick | 153 | 75, 52 | 230, 136 | 434, 62 | 0.7 |
| four turrets | 60 | 11, 0 | 34, 4 | 0, −109 | 1.8 |
| life | 153 | 297, 4 | 658, 100 | 641, −74 | 0.2 |

**A stick is the only one of the three that takes ground somebody is standing on.** On the generation it goes off it takes about fifty squares off the field whatever is on it — turrets take none and life two to four — and on the soup it has taken a hundred and thirty-six by the twenty-fifth generation while the turrets, which cannot take a square with life on it, have taken four and are dying; by the hundredth they are corpses handing ground back. On ground nothing stands on the turrets take fifty to sixty squares for sixty by the twenty-fifth generation, which is cheaper a square than the stick's hundred and one to a hundred and thirty-six for a hundred and fifty-three. So the price sits where it should: **dearer a square than a turret where a turret works, and the only tool that works where one does not.**

**It never holds ground cheaper than life**, which is what the last version of this entry said to watch. A cell of life costs one, holds itself and a halo from the first generation, and a hundred and fifty-three of them hold about three hundred squares at once and six to seven hundred by the twenty-fifth generation on every field — a quarter of what the stick's ground costs a square at the first generation and a third at the twenty-fifth. The turret stays the tool that takes ground and life the tool that holds it.

**A still life is woken by a blast, and what wakes is theirs.** On the field of blocks the stick's lost column goes negative by the fiftieth generation — sixty-eight, then seventy-eight squares the field's owner holds *more* than if left alone — and life laid against it does worse, at a hundred and eighty-nine and two hundred and forty. The noise runs into the blocks, the chaos it starts is mostly theirs because most of the life in it is, and a still life that held its own area holds a country afterwards. A field of blocks is, over fifty generations, a defence against being blasted. Nobody designed that, and it is worth knowing before anybody balances against it.

**Density: twenty-four stands.** With the reach at six and the price the same, what the bomber holds at the first generation goes as the density — fifty-six, sixty-six, seventy-three and eighty-three at sixteen, twenty-one, twenty-four and thirty-two in sixty-four — and what they hold at the hundredth does not: on the soup five, two hundred and thirteen, four hundred and thirty-four and a hundred and eight; on the empty field two hundred and fifty-six, three hundred and seventy-six, three hundred and thirty-two and two hundred and thirty. A sixth does not catch, a half burns down as the constant's comment said it would, and twenty-one and twenty-four are within four seeds' noise of each other at the top.

**Reach: six stands, and pricing by area is right.** What a stick takes on the generation it goes off goes as its disc — thirty-six, fifty-one and ninety-four squares lost for discs of eighty-one, a hundred and thirteen and a hundred and ninety-seven, which is forty-four to forty-eight hundredths of a square each — so a stick is worth its area and `DYNAMITE_COST` at forty plus the disc is the right shape. Nothing in the sweep prefers five or eight to six: at a hundred generations on the soup a reach of eight holds less than six does, for a disc three quarters bigger.

**The throw and the quarter are not swept**, and nothing here argues for moving either; `a_blast_is_thrown_toward_the_frontier_and_no_further_than_the_throw` and `a_disc_one_square_under_the_foreign_threshold_is_not_worth_hitting` pin what they do. The fuse is the one number with a case against it that the tables cannot show: twenty-five generations is six seconds at two hundred and forty a minute, all of them spent keeping a live cell alive on a frontier, and that is the stick's real price on top of the hundred and fifty-three. Whether it is too long is a question for play rather than for a table.

So: **no constant moved.** Leave all six where they are, re-run the example if any of them or the territory rule under them changes, and watch the still-life column, which is the one result nobody predicted.

## What to do next

A reading of the rest of this file, in the order the things depend on each other rather than in the order they were thought of. Nothing here is new; what it adds is which one unblocks the most.

**1. [An identity a server cannot take](#identity-is-a-keypair-and-today-it-is-not).** `people.jsonl` holds a plaintext secret per person, and a secret is a bearer credential — so every server that has met you can be you on every other server that has met you, and that file is the thing on the machine worth stealing. `net::auth::person` says so itself and says it has to change before there are two servers. It is first not because anything is broken today but because it is the only item here that gets **worse** the more the project succeeds, and because everything social — a directory, friends, inviting somebody in particular, a leaderboard that means anything — is built on top of who somebody is.

**2. ~~[A level of detail](#zooming-out-without-lying).~~** Done — `render::chunks::CoarseTexture` is the cell without its art, one texel a cell, and the entry says what it changed. What is left of it is the two things the entry lists: the coarse window re-fills rather than scrolls on a boundless world, and there is no second coarse level below a quarter of a pixel a cell. The other three uses it was meant to serve — a minimap, a world overview, a spectator following a player — are still their own entries.

**3. ~~`World: Clone`.~~** Done — `World` and its `Storage` derive `Clone`, and `a_clone_steps_without_moving_the_original` in `sim::world` pins the two things that has to mean: a copy steps without moving the original, and it is the original's own future. What the derive was for is still to build: [a match prediction](#predicting-a-match-and-what-it-shares-with-bots-and-experiments), a bot that chooses rather than follows a book, and an [experiment's](#experiments) pause-step-reset.

**4. ~~The session comes out of the game view.~~** Done — `client::session` holds the link, the seat and the purse, and nothing in it needs a GPU. What is left of that entry is `lay`, `click` and `stamp_at`, which is smaller and is about wording rather than structure.

**5. ~~[Depleted factories](#depleted-factories).~~** Done — a factory's age is its wear, `Ages::Depletes` on the kind's row and `rule::factory_chance` a parabola over it, so a lineage pays best in its prime and almost nothing when spent. What is left is the numbers: `FACTORY_PRIME`, `FACTORY_BEST` and `FACTORY_SPENT` have not been through `examples/balance.rs`, which does not yet run a pattern long enough to show the fall.

Worth saying what is **closer than this file implies**, and what has since arrived. [A leaderboard](#a-leaderboard) per server is built — `ClientMessage::People` with an empty search is the leaderboard, answered without a seat, and the home screen shows it — because `server::ratings` was already keyed by `PersonId` and saved. Only a leaderboard that spans servers needs item 1. The same reading was applied to [parties](#parties), which are built: a membership keyed by today's per-server person, on the pattern a challenge already used, at the price that the rows reset when a person becomes a key, which is the price the leaderboard already pays.

And what is not next. [The simulation on the GPU](#the-simulation-on-the-gpu) is a large piece of work whose benefit begins at a world size nobody has run, and the level of detail above removes the one argument that was pulling it forward. [Mobile](#mobile) is a layout problem that wants the interface to stop moving first.

## Player profiles

**Part built.** The server keeps a profile per person — `server::profiles`, which is what `server::rating` became when a row stopped being a number. It holds the name last joined under, the rating and the ratings behind it, how many matches have been settled and the most ground ever held, and `net::Profile` is that on the wire. `ClientMessage::Profile` asks about anybody and is answerable **without a seat**, because a profile is looked at from a lobby, from a standings bar and from a menu and only one of those is inside a room.

**Ratings are provisional until ten matches.** `rating::PROVISIONAL_AFTER` is the threshold and `K_PROVISIONAL` is twice the ordinary K, so somebody new reaches their level in a handful of matches rather than thirty. Each entrant moves by **their own** K, which stops a settled player being dragged about by whoever they happened to draw — and makes a match between the two not zero-sum, which is the point rather than a defect: the two are not equally uncertain, so the same result is not equally informative about them. A draw between equals moves nothing and still counts, because it says everything about how much the table now knows.

The mark is shown on the home screen and on a profile and **not on the bar**: it exists so a rating read as a *claim* is not taken for one it is not, and the bar is your own readout of your own number rather than a comparison. It says the count as well as the word, because "provisional" on its own is a label somebody has to already know.

**A name is a way in.** `net::Seat` carries the number, the name and the person, so a lobby row and a standings bar both open `views::social::profile` — a panel over the world rather than a page, because a profile answers a question about the screen underneath it. It has three states and not two: asking, never-met, and an answer, because a slow server and a stranger otherwise look the same.

What is **left**: the name is not editable anywhere but the menu's join field, and devices — the control that authorises a second machine — waits on [identity being a keypair](#identity-is-a-keypair-and-today-it-is-not), which is item 1 on this list. `first_seen` is not kept: it needs a clock threaded through `Rooms::handle`, which nothing else there wants, and it is the least useful of the three facts the table holds.

The rest of this entry is the design, and it still reads.

**A person, rather than a seat in one room.** Everything a player accumulates is currently filed against something that does not survive what it should: a `PlayerId` is a seat in one world, a rejoin token is per room, a rating is per server, and a stamp library is per browser. Somebody who plays two games on two machines is four different people to this code.

The identity exists already, in the weak form. `net::auth` mints a `Secret`, the client keeps it — **at startup now, not at the first join**, because a record and a stamp library both exist before a server has been reached and both want an owner — and `Join` carries it. A server can already say *which person* is asking. Two things are missing: everything that should be filed against the answer, and a way of asking that does not hand the server a credential it could reuse elsewhere — see [identity is a keypair, and today it is not](#identity-is-a-keypair-and-today-it-is-not).

### It subtracts before it adds — done

**A profile deletes the rejoin token**, and it has: a seat is found by the person in it, `persist` is at version 7 for it, and there is no token anywhere in the tree. The token exists so that a dropped connection comes back to the same seat, and [networking.md](networking.md#coming-back) is honest about what it costs: it is filed per room rather than per server, so two servers both running `main` share one secret; whoever holds it *is* you; and a token whose player is already connected joins you as somebody new. A key does the same job strictly better — the server maps person to seat per room, the claim is signed rather than presented, and there is one key for everywhere rather than one secret per room per server.

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

### Ratings need a provisional state — done

A high K for the first ten results and a mark on the figure until then, so a leaderboard is not topped by somebody who won once. See the summary at the top of this entry for what each entrant moving by their own K costs and buys.

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

**Built.** `Kind::OVERCLOCK` is a machine placed in fours, and `World::overclock_pass` runs the rule again over the union of its discs after the whole-world pass and before the generation is called done — so the generation stays the unit on the wire, in the save and in the digest, and the checkpoint covers it with no new message. The design is in [simulation.md](simulation.md#overclockers): why sub-steps rather than a faster tick, what the second pass reads at a disc's edge, and why it rolls dice of its own. The price is in [game.md](game.md#overclockers), and `examples/two` runs it over the real protocol behind `OVERCLOCK=1`.

What is left:

**The art** is a placeholder in the manner of the others — a double chevron over the plain kind's four tiles, at row 8 of the sheet, which is the first art in its bottom half.

**The numbers want playing.** `OVERCLOCK_COST` starts where a turret's does and nothing has measured what an overclocked gun is worth; `examples/balance.rs` wants a row for a blinker and a gun inside a disc, and the price read off it. A factory inside a disc is born twice a generation, so it pays twice as often and depletes twice as fast, and which of those wins is the number to look at.

**A rate other than two** is a loop that is already generic and art and pricing that are not: `OVERCLOCK_RATE` is one constant for every overclocker, and if a kind ever wants its own it belongs as a column on the `kinds!` row rather than a constant beside it.

**Ice seeds between passes.** They are taken once, at the top, so a cell born beside a pane in the second pass breaks it a generation later — the same one-beat lag the first pass has. Retaking them between passes would break it in the same generation, and is a line if that turns out to be wanted.

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

### The real bound on low zoom was residency, not sampling

Worth keeping, because it is why the level of detail is the shape it is. **One chunk is one texture array layer, and the guaranteed floor for `max_texture_array_layers` is 256.** A 1920x1080 screen covers about `8100 / zoom²` chunk positions, so:

| zoom | chunks on screen | fits in 256 layers | fits in 1024 quads |
|---|---|---|---|
| 16 (opening) | 40 | yes | yes |
| 8 | 160 | yes | yes |
| 5 | 336 | **no** | yes |
| 3 | 875 | no | yes |
| 1 (the old floor) | 8100 | no | **no** |

So a screen was already mostly backdrop below about zoom five, and at the floor it was a 16x16-chunk island in a sea of empty ground. Better sampling made a mostly-empty screen smoother; it did not put the world back on it. `render::chunks::covered` is that arithmetic, pure and with the numbers pinned in a test.

**Which is also why a wrapping world never visibly repeated.** The folding has worked since "A wrapping world that wraps" — one layer per distinct chunk, a quad per position — but seeing a torus come round again means seeing more than one world-width at once, and for any torus worth playing that is a zoom at which residency had already collapsed. The repeat was correct and unobservable.

### The level of detail is the cell without its art

**Built.** The problem was never that the cell texture is too detailed — it is that there are only 256 of them and a quad each. And a *reduction*, which the plan here used to call for, answers a question nobody asked: at low zoom a player does not want a summary of sixteen cells, they want the cells. What they stop wanting is the **art**.

A cell is 16x16 texels of sprite, and that is the entire reason residency is one array layer per chunk. Below about four pixels a cell the sprite is not legible and is costing 256 texels a cell to draw as a blur. So `render::chunks::CoarseTexture` is **one texel per cell**, in a plain 2D texture, drawn as **one quad**, with no sheet lookup at all — and what each texel holds is not a new format, it is the cell:

| | | |
|---|---|---|
| R | the owner byte, unchanged | `>> PLAYER_SHIFT` is the player, so the hue table is read exactly as the fine path reads it |
| G | the tile byte, unchanged | bit 0 is alive, bit 1 is ice; kind and age ride along free and are ignored |

`Rg8Uint`, the cell's own two bytes cast straight out of a `Chunk`. Nothing is derived, summarised or averaged, so there is nothing that can disagree with what the fine path would have drawn.

**It draws lightness for state and hue for owner**, which is the division the sprite sheet already makes — a texel there is saturation and lightness and the hue arrives from the player — so a coarse cell is that with the sheet's variation dropped rather than a second colour scheme. Player zero is unowned and `player_saturation` already answers nought for it, so unheld ground comes out grey with no arm saying so.

**A torus that fits is held whole**, which is the case that matters: 1024 cells a side is 64 chunks, and 64 is the largest a client may ask for — `menu::draft::MAX_CHUNKS` — so any world somebody makes from the menu is one texture with no window to recentre and no seam to get wrong. The shader wraps it, and *that* is what makes a wrapping world visibly repeat: zoomed out, the world collapses **into itself** rather than into the backdrop. Anything larger, and any boundless world, gets a window centred on the view.

**A boundless world draws only the chunks it has.** `fill` walks `world.stored()` rather than the window, so an infinite world's coarse texture is exactly its resident set and everywhere else reads as a dead unowned cell — which is what the backdrop is. Unloaded ground draws as unloaded ground for nothing.

**And the backdrop goes flat.** It is one quad wrapping the world onto a single dead chunk, so what is on it is the dead sprite and the transparent gap between sprites — one cell in sixteen. Below a couple of pixels a cell neither is visible and both are moiré, so under `BACKDROP_FLAT` it draws the same grey the coarse path gives unheld ground. The two then agree exactly where they meet, which also takes most of [the brightness difference](known-bugs.md#loaded-chunks-read-differently-from-the-backdrop) with it.

The antialiasing carries across unchanged, which is what its footprint being measured in texels bought: a coarse texel is a cell, so below one pixel per cell it averages over cells exactly as it averages over sprite texels above.

### What is left

**The floor is a quarter of a pixel per cell**, which is where four samples a side stop covering the footprint exactly and where a 1024-cell window stops covering a 1080p screen. Lower wants a second coarse level, which is the same trick again rather than a new idea.

**A window on a boundless world does not scroll yet** — it is re-filled when the view moves it, rather than uploading the rows and columns newly exposed. Two megabytes at four generations a second is the worst case and only while zoomed out, which is the frame with nothing else to do, so this is a real cost that has not yet been worth paying down.

**And it needs no compute shader**, which is the other thing the old plan got wrong: a max-or-count reduction on the GPU would want `Rgba8Uint` storage bindings and a compute pass WebGL2 cannot run at all. Copying two bytes a cell is a memcpy the client already affords, works on every backend, and does not block on [the simulation on the GPU](#the-simulation-on-the-gpu).

**This is also what a minimap is**, and a world overview, and what a spectator following a player wants.

## A torus repeats, so its textures can

**Built, and it was built before this entry was written.** `ChunkStore::sync` keys residency by `World::canonical`, so a chunk that appears at nine places on screen is one texture layer and nine quads, and the resident set is bounded by the *world* rather than by how far anybody has panned. It landed in "A wrapping world that wraps, and a player who can play in it"; this entry was added later from a misreading of the code and claimed the opposite.

The decision it records is still worth keeping, because it is the one that makes the arithmetic above work: **a wrapping world is drawn by folding, not by tiling.** Every position the viewport covers is asked which chunk fills it, which on a torus is many-to-one. The version before it drew a fixed number of copies either side of the original, so panning off the third copy fell into blank space for ever and a large torus paid for nine copies of every chunk whether or not any were on screen.

`render::chunks::covered` is that arithmetic, pure and out of `sync` so it can be checked without a device. Two tests hold it: a 4x4 torus under a 12x12-chunk viewport is 144 quads over 16 layers, and panning a thousand worlds along finds no new chunks.

## Depleted factories

**Built**, as the age field: `Ages::Depletes` on the factory's row in `kinds!`, and `rule::factory_chance` a parabola over it — a lineage pays best in its prime and almost nothing when spent, with `FACTORY_PRIME`, `FACTORY_BEST` and `FACTORY_SPENT` the three numbers. The roll is gated where the birth is counted, in the rule's tally, so `net::earnings` reads nothing new. What is left is the numbers, which have not been through `examples/balance.rs`. The argument that got here is kept below.

**The problem is that factory income scales faster than size.** A factory pays when one of its kind is *born*, and births scale with the perimeter of a growing pattern — so a player with four times the territory does not earn four times as much, they earn more than that, and they can spend it on more territory. Nothing in the rules pushes back.

A **depleted** factory is the push-back: past some point it stops paying and is an ordinary cell that happens to have cost more. What that buys is a ceiling on what any one lineage is worth, so income comes from *building new things* rather than from having built a big one.

### Where the bit comes from

Byte 1 is full — alive, ice, kind, age; see [simulation.md](simulation.md#the-cell). There is no spare bit, so this is a choice between three, and they are not equally good.

**A kind.** `Kind::DEPLETED_FACTORY` beside `Kind::FACTORY`, costing one of eight kind indices and no bits at all — five of the eight are spent now, on normal, factory, turret, dynamite and the overclocker. It gets art of its own for free, which a flag would not — a depleted factory has to *look* spent or nobody can tell which of their cells still earns. `Kind::inherits` already decides whether a birth copies a kind, so "a depleted factory's children are ordinary" or "are also depleted" is a row in the table rather than a rule. This is the one to do.

**The age field.** A factory's age *is* its depletion: `net::earnings` scales down with it and a factory at [`bits::MAX_AGE`] pays nothing. No new state anywhere, and the eight steps are a fade rather than a cliff, which is likely to play better. What it costs is that factories can no longer use age for anything else, and it collides with dynamite if a dynamite is ever also a factory.

**This is the one that was done.** `Kind::ages` said `Ages::Never` for a factory while it was a reservation: a dead factory's clearing was tried as an age count and put back to a roll precisely so nothing else spends the field. What is left is `net::earnings` reading the age, and something to advance it — a count of births is the honest one, since it is what a factory is paid for.

**A bit off age.** Three bits become two, four ages instead of eight. Cheapest to write and the worst of the three: it takes resolution away from the one field that has a use lined up, to buy a flag that a kind gives away.

### What is not decided

How much is "past some point", and whether depletion is a count of births or of generations. A count of births is the honest one — it is what a factory is paid for — but it needs somewhere to keep the count, which is the age field again.

## The simulation on the GPU

**Costed, not started.** The full working is in [design-notes/05-compute-feasibility.md](../design-notes/05-compute-feasibility.md); the parts that decide anything are here.

`Rg8Uint` **cannot be a compute shader's output.** wgpu's guaranteed format features give it `msaa | attachment` and no `STORAGE_BINDING`, and a compute shader can only write to a storage texture. So moving the simulation onto the GPU means changing the cell's texture format first: `Rgba8Uint` is the natural one — storage-capable, and four independent `u8`s where the cell already wants fields — or `R32Uint`, which is fully read-write and has atomics at the cost of packing by hand.

`Rgba8Uint` grants read-only and write-only storage but not read-write, which suits Conway anyway: bind one generation read-only and the next write-only, and swap each tick.

**WebGL2 cannot do it at all.** `Limits::downlevel_webgl2_defaults` zeroes every compute limit, and the browser client falls back to WebGL2 whenever WebGPU is unavailable — a blocklisted driver, a VM, a headless browser; see [gotchas.md](gotchas.md). So this is never the only simulation. There has to be a CPU path regardless.

### The hard part is not the shader

It is that **two simulations must agree exactly.** The server steps on the CPU and the client predicts against it, and a `Checkpoint` compares them chunk by chunk — so a GPU step that differs from the CPU step by one cell is not slower or uglier, it is a client that resyncs every few seconds forever. Everything the rule does is integer work on bytes, which is reproducible on a GPU in a way floating point would not be, but "reproducible" has to be *established* rather than assumed: the seeded dice in `sim::seed`, the order births resolve in, and the tie-breaks in the territory rule all have to come out the same.

Which suggests the shape: `examples/headless` already runs the simulation with no GPU, so the test is a world stepped both ways for a few hundred generations with the digests compared every step — the same comparison `examples/two` already makes between two peers.

### What it buys, and when — the size has now been run

This said the door opens when a room is big enough that a quarter-second is not
enough to step it, "and that is a size nobody has run". `examples/frametime`
runs it:

```
     world   chunks      cells    ms/step  of 250ms
       4x4       16       4096       0.17      0.1%
     12x12      144      36864       1.55      0.6%
     24x24      576     147456       6.25      2.5%
     48x48     2304     589824      25.26     10.1%
```

Linear at about eleven nanoseconds a cell, so the largest torus the server will
allocate — 16384 chunks, 4.2 million cells — is roughly **180 ms against a
250 ms generation**. Seventy percent of the budget on one core, with the
sockets, the standings and the checkpoints still to pay for. The door is
reached rather than hypothetical, and the ceiling `docs/README.md` asserts is
this number.

### The server wants this and the client mostly does not

Worth separating, because they are not one problem.

**The server steps whole worlds.** Every resident chunk, every generation,
whether or not anybody is looking at it — so its cost is the number above and
it is the one that runs out.

**A client steps only what it predicts**, which is its subscription and so its
viewport. That is a few dozen chunks, which is microseconds, and it is not what
makes a client's frame slow: `update` and the coarse texture are. A client also
already has the GPU busy drawing the thing, so moving the simulation onto it
competes with the frame rather than freeing it.

So the case is **server-side**, which collides with
[Cloudflare](#cloudflare-and-which-half-of-this-fits): a Durable Object has no
GPU at all, and neither does the cheapest container anywhere. A GPU step is a
reason to run the server on a machine chosen for it, and that is a deployment
decision rather than a rendering one.

### Which means the CPU path stays canonical

Not a fallback — **canonical**. WebGL2 has no compute at all and a browser
falls back to it whenever WebGPU is unavailable, so a client that cannot
compute must still predict correctly. The GPU step is therefore an
*acceleration that must be bit-identical*, and the test is the one the entry
above already describes: a world stepped both ways with the digests compared
every generation, which is what `examples/two` already does between two peers.

### The storage layer is the part to change first

The halo is the real obstacle and it is already a known one. Every rule is a
function of a cell and its eight neighbours, and the world lives on the GPU as
**one array layer per chunk** — so a cell on a chunk edge cannot reach the
layer its neighbour is in. That is exactly why `render::chunks::neighbours`
computes the outline mask on the CPU on the way to the GPU rather than in the
shader.

A compute step has the same problem and cannot solve it the same way, because
the halo is the input to the rule rather than a decoration on the output. Which
points at the change: for compute, the world wants to be **one large 2D
texture** with chunks tiled into it, so a neighbour is `textureLoad` at an
offset and the workgroup can stage a tile plus its border into shared memory —
the standard shape for a stencil kernel, and the reason it is standard.

That is a bigger change than a format constant, and it is the same change
`CoarseTexture` already makes for its own purposes: one 2D texture holding a
window on the world, addressed by cell. So the cheap thing to do now is still
not to close the door — `Rgba8Uint` rather than `Rg8Uint`, and a fourth byte on
the cell — but the thing to *design* first is the storage, not the shader.

## Making rooms from the client

**Built** — see [game.md](game.md#the-menu) for the form and [server.md](server.md#made-by-a-client) for the wire, the cap and the owner. A world, a match or a private game, from the menu, on a phone.

**Closing a room from the client is built** — see [server.md](server.md#closing-one). `ClientMessage::Close` is `Rooms::delete` behind the owner check, the owner is the maker's key and is saved, and the answer for a room somebody is standing in is a refusal that says so: it closes once everybody has left, from the menu, which is where you are once you have.

**The whistle is built** — see [game.md](game.md#the-menu). `ClientMessage::Start` and `EndMatch` are answered behind `Rooms::owns`, so whoever made a match starts and ends it from whichever seat their key sits in, and a match the console made is still the operator's and still starts at `match dispatch`. This entry said the whistle was left for some time after it was not.

What is left:

**Auto-sleep is the fix the cap only backstops.** Every room steps four times a second for as long as the process lives, whether or not anybody is in it. Half the answer is already built and unused: `Server::set_asleep` exists, `Server::step` returns nothing for a sleeping room, and `world sleep` / `world wake` are at the console. What is missing is the trigger — a room whose last player leaves sleeps after a grace period, and the `Join` that resolves to it wakes it.

Waking is indistinguishable from never having slept, for a reason worth stating rather than assuming: the tick **is** the generation, and nothing else advances while a world is not stepping. There is no second clock to drift. The one thing to watch is the save, which records that tick — a room saved asleep records the generation it stopped at, which is the right number under the only meaning the field has ever had. A match must not sleep and does not: `set_asleep` refuses on anything but `Phase::Open`.

## Spectating

**Built** — see [server.md](server.md#watching). `ClientMessage::Watch` takes a room and no seat; `ServerMessage::Watching` answers with the world and its clock and no player, purse or spawn. A watcher reads and cannot act, which is enforced by an action now belonging to the **connection that sent it** rather than to the `PlayerId` it names.

Admitted at any generation, and that is the point rather than an oversight: **no late joining is a rule about players.** A `Join` to a running match is still refused, and the refusal is what the client turns into an offer to watch — keeping "you cannot play in this" and "would you like to watch it" two separate answers, which is what they are.

What is left: a watcher cannot follow a particular player's ground, which is what a spectator actually wants once a world is larger than a screen. That wants the camera to take a target, which is `views::camera`'s business and not the protocol's.

## Games and matches by code

**Built** — see [server.md](server.md#made-by-a-client). A private room is kept out of the listing and reached by a six-character code. The code is a **credential and not an identity**: separate from the room's id, which never changes, and separate from its name, so a private game can still be called something its owner chose and a code could be rotated later.

The alphabet leaves out `0`, `o`, `1`, `i` and `l` — 31⁶ is 887 million codes, or 29.7 bits, against 36⁶ and 31.0 bits for the full alphanumeric set. That trade is deliberate: those five characters are the whole of why a code gets mistyped when it is read off one screen and typed into another, and the keyspace is not what protects a private room anyway. With the room cap where it is a random guess finds one in about twenty-eight million, so the defence is that guessing is not worth anybody's time — and if it ever became worth somebody's time the answer is a limit on how fast a connection may guess, not a longer code.

**The id alone no longer admits anybody.** `resolve` takes an id, a name or a code, and took a private room's id from whoever had it — which is everybody who has ever been in the room, since the address bar says it. `Rooms::may_enter` is the door now, and an unlisted room opens by its code, for its maker, and for a key that has been let in: invited, challenged, or once in by the code. See [server.md](server.md#a-private-rooms-door).

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

Two ways in. **No money and nothing alive** — value floors at zero, so a player who spent everything and then lost their pattern has nothing to place and no way to earn, because income comes from factory births and they have no factories. And **no territory**, which sounds impossible because a granted patch never decays, but is not: an opponent who grows over your home keeps it as theirs, mark and all. Either way the player is sitting in front of a world they cannot touch, clicking, with the client saying only that the placement was refused.

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

**The colour needed nothing** — see [game.md](game.md#teams). A team is a player, so its cells carry one number and are drawn in one colour, and the colour table went back to one row per number, a constant the client hands to the shader in the camera uniform — a golden-ratio step at the time, and since then a fixed palette chosen for separation; see [rendering.md](rendering.md#colour).

There was a real design here and it is worth recording what it cost, because the measurement it was waiting on is exactly the measurement that says the design was unnecessary. A team took a golden-ratio step and its **members** spread over a narrow arc around it, a twelfth of the circle, so that allies read as one colour across a screen of cells and were still told apart when looked at. The arc was fixed rather than widening with the team, on the reasoning that mistaking your own two colours costs nothing and mistaking an enemy for an ally costs the game. All of it was 165 lines of arithmetic keeping two numbers *look* like one number — and the thing it never established, whether two allies a twelfth apart are distinguishable at four pixels a cell, stopped mattering the moment they were one number.

**Friendly fire is on**, and that is the honest first answer rather than a decision. A glider is a weapon whoever built it, and a rule making allied life pass through allied life would be a rule in `sim` — which is what this design exists to avoid. Teams are about scoring and building, not immunity.

**A world may have them too**, which reversed a decision. Teams were a match feature on the reasoning that a team is a way of deciding a result and a world has none — but that is only half of what a team is, and the other half is people playing as one player, which needs nothing to win. A world with two teams is two shared kingdoms rather than fifteen small ones. What stays a match's alone is the balance check, because a world has no moment to make it at.

**The lobby cannot lock a team**, so anybody may join any team including one that is already full. That is deliberate — see the balance check above — and it does mean a five-player match can end up four against one if people are careless. The whistle allows it; whether it should is a playtest question.

## Rating

**Built.** A number that says how good somebody is, updated by results, in the shape of Elo. It is on the home screen, above the record and deliberately not inside it: what `views::record` shows is what this *client* has done out of its own store, and a rating is what a *server* thinks of you against everybody else there. Folding one into the other would suggest the client had worked it out, which it must never look like it can.

`server::profiles` is the table, keyed by `PersonId`, saved to `profiles.jsonl` beside `people.jsonl`. `Rooms::step` settles a match on the generation it is decided — not the room that ended, because a rating outlives every world here and a match's world is about to stop existing — and broadcasts `ServerMessage::Rated` to everybody who was in it, so the number moves on the screen somebody is looking at rather than on their next join. A `Welcome` carries it too, for arriving.

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

What is true is in [`net::auth::person`](../src/net/auth/person.rs) and [`server::people`](../src/server/people.rs), both of which say so plainly. The ed25519 scheme was **removed**: a `Secret` is now sixteen random bytes that the client sends on every join, and the server stores it beside the id it issued, in `people.jsonl`, in plaintext. So today:

- a secret is a **bearer credential** — whoever holds it is you, and the server it is presented to holds it;
- `people.jsonl` is the file on the machine worth stealing, because every line in it is a player somebody can be;
- and a server that has met you can be you **on every other server that has met you**.

`person.rs` calls that a single-server design and says it has to change before there are two. That is exactly right, and it is the first thing in this entry rather than a footnote to it: everything below assumes an identity a server cannot take.

### What has to be true

**Three properties, and the third is the one that is easy to lose.**

1. **A server verifies rather than looks up.** A join is a signature it checks by arithmetic, so there is nothing in a server's files worth stealing and nothing to leak.
2. **The private half never leaves the device it was made on.** Not "is not sent" — cannot be read, by the page or by anything on it.
3. **The central service cannot be you either.** A directory that could impersonate its users is a server with a bigger blast radius, not a solution.

### The scheme, and the one detail worth getting right

ed25519, back where it was: the server offers a nonce on the socket's first word and the client signs. `PersonId` becomes a **fingerprint of the public key** rather than something a server issues — derived, so every server calls you the same thing, which is the whole point and is also what makes `people.jsonl` a table with nothing secret in it.

**Sign more than the nonce.** A signature over a bare challenge is replayable sideways: server A, which you are joining honestly, hands your signature to server B and is you there. So the signed message names **the server and the room** as well as the nonce — a signature is then evidence about one join to one place, and a relay has nothing to relay. This is the bug the previous scheme would have had and nobody would have found until there were two servers, which is to say until it mattered.

A server's identity is its own keypair, so "which server" is a public key rather than a hostname somebody could take. That also gives the client something to pin, which is what stops a room list from sending you to an impostor.

Migration is a row in `people.jsonl` holding a public key instead of a secret. A person whose row holds a secret cannot be re-keyed, because the thing that would key them was never on that machine — their rating starts again. One line in a release note, which is the same answer this file already gives for records filed under a room's display name.

### Non-extractable keys, which is what "never leaves the device" needs

A secret in `net::keep` is hex in `localStorage` on the web, and any script on the page can read it — including one that got there by accident. The settings screen prints it on purpose. That is the sense in which the key is too easy to reach: it is not that it is exposed, it is that nothing prevents it.

**WebCrypto has the answer and it is not a library.** `crypto.subtle.generateKey({name: "Ed25519"}, false, ["sign"])` returns a `CryptoKey` whose private half JavaScript never holds — the `false` is `extractable`, and the browser enforces it. Store the handle in IndexedDB and the page can **use** the key while it is open and can never **take** it. That is a large difference and it costs one API.

Ed25519 in WebCrypto is recent enough to need a fallback; ECDSA P-256 has been there for a decade and is the obvious one, with an algorithm tag on the wire so a server knows which it is verifying. Natively this is a file at `0600` and the same discipline.

**What it costs is export**, and that is the trade rather than a detail. A key that cannot be read cannot be carried to another machine, and carrying it is how somebody is the same person on their phone and their laptop today. The answer is not to make it extractable.

### What doing it actually costs, in order

Sized against the code rather than guessed: `Secret` is named in **39 places
across eight files**, and all but a handful are in `net::auth::person`,
`server::people`, `net::keep` and `server::rooms`. The mechanical part is small.
The parts that are not mechanical are these, and the first two are decisions
rather than work.

**1. It resets everybody.** A `PersonId` becomes a fingerprint of a public key,
so it is a different string from the one a server issued — which means every
existing rating, every settled match, every "most ground held" and every seat
somebody could return to is attached to a name nobody can prove any more.
`people.jsonl` and the `.ckw` files are full of ids that no longer refer to
anybody. There are two honest answers and they are not close: **wipe** and say
so, or keep the old id beside the new one for a grace period and let a returning
player claim it with their old secret, which is a migration path that must
itself expire or it is the bearer credential all over again. Nothing here should
be built until that is chosen.

**2. It adds a cryptographic dependency**, and this crate is deliberate about
those — see the arguments for `rustyline` over clap and `allsorts` over
`subsetter` in `Cargo.toml`. `ed25519-dalek` is the obvious one: pure Rust,
builds for wasm32, and is the scheme this used to use. Worth stating plainly
that the last time this existed it was removed for costing "a signature scheme,
an OpenSSH key parser, a round trip before every join, and a dependency" — three
of those four come back, and the OpenSSH parser does not.

**3. The handshake gains a round trip, and `Join` stops being first.** A
connection's first word used to be `Join` and everything else was answered
from the seat it made. A challenge means the *server* speaks first, so
`server::rooms::handle` grows a state before "has a seat" — and `Rooms`,
`Profile` and `People` are all answered without a seat today, which is right
and must keep working for a connection that has not signed anything. That is
the part most likely to go subtly wrong. Half of it exists now:
`ClientMessage::Hello` presents the secret with no room named and `Caller`
carries the person before any seat — see
[networking.md](networking.md#before-a-seat) — so what the handshake adds is a
nonce in front of that and a signature on it, not a new state.

**4. Then the mechanical part.** `Secret` becomes a signing key; `PersonId::new`
becomes a fingerprint of the verifying key; `people.jsonl` loses its secret field
and becomes a table with nothing worth stealing in it, which is the whole point;
`net::keep` stores a key rather than sixteen bytes; and `server::people::knows`
becomes a verification rather than a lookup.

**5. What this does not get you**, and it should not be claimed: property 2
above — that the private half *cannot be read* — is not satisfied by an
ed25519 key sitting in `localStorage`, which is what a pure-Rust
implementation in a browser gives. That wants a non-extractable `CryptoKey` and
is the sub-entry below. Doing the ed25519 work first is still right, because it
gets property 1 and property 3 and is what parties and invites are waiting on;
it just must not be described as more than it is.

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

The room side is built, with the server as the verifier rather than a signature: `Rooms::admitted` is the set of people let into each private room, `ClientMessage::Invite` puts a named person in it and queues an `Invited` the way a challenge is queued, and `may_enter` is the set lookup on `Join` — see [server.md](server.md#invitations). What a signed invitation adds when identity is a key is the "until", delivery through a directory when both are connected, and a link that names you rather than a bearer token.

**Codes stay.** They are good at the thing they are good at, which is reading six characters out loud to somebody sitting next to you, and that case wants no directory and no account.

### Room ownership should be keyed by person

**Keyed by person, and saved.** `Rooms::owner` records the maker's `PersonId` at `Create`, now that a `Hello` says who is asking — or their seat at their first join, for a client with no key — and `Start`, `EndMatch` and `Close` answer to the key wherever it is presented from, while the lobby is told the seat that key holds so a client can show the whistle. It used to be the seat alone, which survives a reconnect and is enough for a refresh, but meant a lobby comparing the owner's seat with the viewer's *side* in a team match, and nobody could start one. It survives a restart in `rooms.jsonl` beside the code, the unlisting and who has been let in — see [server.md](server.md#made-by-a-client) — which is what "close the room you opened" needed and now has. What it cannot yet mean anything on is a second server, until identity is a keypair; and "hand this room to somebody else" is still not built.

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

**Decided, not costed.** Golly, with this simulation underneath: draw a pattern, watch it, step it a generation at a time, save it, come back to it. More than one world side by side.

### The premise, which is now measured rather than argued

Everything here rests on one claim: **a pattern written down by somebody else runs here the way it runs anywhere.** If it does not, reading fifty years of other people's work means reading it wrong, and the whole idea is a curiosity.

It holds, and it is not obvious from the code that it should. Three of the four things this simulation adds to Conway do not touch whether a cell lives — territory writes the owner byte of *dead* squares, a factory is a tally, and ice is inert until a pane is laid. That is an argument; `sim::world`'s `liveness_is_exactly_b3_s23` is the measurement, comparing two hundred generations of a 64x64 soup cell for cell against a B3/S23 stepper written out longhand. An R-pentomino also stabilises here at generation 1103 with 116 cells, which is the figure in every book.

**The caveat is two things, and it is worth knowing exactly which.** Turrets and ice are the only rules that touch liveness — a turret because unowning a live square kills what stands on it, ice because it stops time. Neither is on an empty board, so an imported pattern is unaffected by anything this game adds.

That also makes "the rules come off" much smaller than it sounded. There is no need for a second `sim` and no flag on the step: what has to be switchable is **placing**, and that is two questions, `net::price` and `net::may_place`. The simulation is already the one a match runs, which is the whole value of experimenting here rather than in Golly.

### What is built

**A kind of room, which is what it should have been from the start.** `net::RoomKind` is `World`, `Match` or `Experiment`, and the make-a-world form asks that first because it decides which of the other questions are worth asking — a match is the only one with a way to win, an experiment is the only one whose rules are yours. It used to be implied by "ends: never", which told a world and a laboratory apart not at all, because the laboratory was not a room: it was a mode the client went into with no server, reached from a page of its own at `/experiments`. That page is gone; `/experiments` opens the form on `Kind::Experiment`, and `Menu::describe` is the one way in, shared with the back button.

**It is multiplayer, and that is what being a room buys.** Several people in one laboratory see one board, one clock and one set of switches. What made it solitary before was that the switches were client-held flags — and a client that answers "may I place here" and "what does this cost" for itself predicts placements a server refuses, so it could only ever be offline.

**The rules come off, and it is two questions rather than a second `sim`.** `net::may_place` and `net::price` are the whole of what the game adds to placing, so `net::Rules` is three switches the *room* holds — `paused`, `place_anywhere`, `place_free` — plus `laboratory`, which says whether they are anybody's to change. Both sides ask through `net::may_place_under` and `net::price_under`, so the rule and the switch that takes it off are read together or not at all. A world or a match refuses `SetRules` outright: everywhere but a laboratory these *are* the rules of the game.

**And it opens stopped**, which is Golly's habit and the right one: the first thing anybody does here is draw, and a world running while you draw into it is a world eating what you drew. `Server::step` returns nothing while `rules.paused`, the same whole stop `asleep` is, and `Server::step_once` lifts the pause for exactly one call — a client that unpaused, stepped and paused again would run the world for however long the two round trips took, which at four generations a second is not one step.

A laboratory **picks its own shape, sides and everything else**, like any other room. It was briefly forced boundless and free-for-all on the reasoning that a torus and a team are answers to game questions — which was answering a question nobody asked: a bounded universe is what every Life program offers, and a team in a laboratory is who shares the bench.

### What to take from Golly, in order

**RLE, and it stands alone.** This is the single highest-value piece and it is worth doing whether or not the rest happens. RLE and `.cells` are how every pattern anybody has ever published is written down, reading one is an afternoon, and it turns `client::views::game::stamp::Library` from a scratchpad into a way in to fifty years of work. **Writing it matters as much as reading it** — a pattern found here should be able to leave, or this is a place things go into and not a place they come out of.

**Reset.** Pause and step are built — `ClientMessage::SetRules` and `ClientMessage::StepOnce`, answered by the room and broadcast, so one person stepping is not a world only they can see. Reset is what is left, and it is the derive: see [predicting a match](#predicting-a-match-and-what-it-shares-with-bots-and-experiments), which wants the same `World: Clone` and gets reset as restore.

**Speed.** Golly's generation slider, and `World::update` already takes a span, so this is a number the interface owns. `MAX_CATCHUP_STEPS` is already the guard against a slider that asks for more than a frame can do.

**Rule switching**, which is smaller than it looks and is the one place being *not* Conway is the point. `RuleFn` is already "the whole rule, as a function pointer, for swapping it wholesale", and `SURVIVES_ON` and `BORN_ON` are two small arrays in `sim::rule`. Arbitrary `B/S` rulestrings — HighLife's B36/S23, Day and Night, Seeds — is turning two constants into values the world carries. It is also the one thing on this list that a match must never see: a room whose rule is not the game's is a different game, so this belongs to an experiment and not to a `WorldKind`.

### What not to take, and why the obvious one is wrong

**Hashlife.** It is the first thing anybody asks for and it cannot work here. Hashlife memoises quadtree nodes on the assumption that a region's future is a function of that region alone; this simulation has three passes for which that is false — territory is a field with sources, a turret searches a disc that crosses node boundaries, and ice breaks along connected runs of unbounded length. Nothing memoises. It would work on a board with none of those on it, which is a second simulation, which is exactly what this must not become.

Worth stating rather than leaving to be discovered, because "why is this slower than Golly" has a real answer: Golly is fast at *pure* Conway on repetitive patterns, and the trade here is that what you are watching is what a match would do.

**Scripting.** Golly has Python and Lua. That is a different product.

### What is actually left

**Panes**, which is the only part that is real work: several viewports onto several worlds. `render::app` holds one surface and one camera, so the cost is a camera and a viewport per pane rather than a second pipeline — and the [level of detail](#zooming-out-without-lying) is what makes a small pane showing a whole world possible at all.

It **is** multiplayer, which is what changed. A shared laboratory is a shared world with the rules off, and that turned out to be the simplifying answer rather than a separate feature: it is what let three client-held flags, a page, a `Chose` variant and the whole offline-laboratory path come out at once.

The order: RLE first, since it stands alone and is worth the most; then pause, step and reset, which is a derive and three buttons; then speed and rulestrings; then the placing flag; then panes.

## Keys the player chooses

**Decided.** Every binding in the client is the one it was written with, and the whole of what a player can do about it is nothing.

The argument for it is not preference, it is that **defaults cannot be right**. Three separate faults have now come out of the same place, and each was fixed by guessing better rather than by asking:

- a binding by **character** is unreachable on a layout that cannot type the character — `R` types `к` on a Cyrillic keyboard, and `~` is a dead key on three Latin ones;
- a binding by **position** is wrong for anybody who has moved the key — caps lock mapped to escape sends the escape *meaning* from the caps lock *position*, so escape simply did not work;
- and a **label** is a guess about a keyboard nobody here can see, which is why `navigator.keyboard.getLayoutMap` exists and why it is Chromium-only.

Each fix made the defaults better and none of them makes a default right for somebody who wants `hjkl`, or who plays one-handed, or whose muscle memory is another game's.

### What it needs

**A table from action to binding**, which is most of the way there already: `input::Mnemonic` is the vocabulary of actions, `hotbar::Key` is the rest of it, and `views::words::keys` is what each is *called*. What does not exist is the map being data rather than a `match`.

**Kept per person, not per client**, which is the [stamp library's problem](known-bugs.md#a-pinned-stamp-is-pinned-per-client-not-per-person) exactly: `net::keep` is a browser's `localStorage`, so a player rebinding on a laptop rebinds nothing on their phone. It waits on the same identity everything else does.

**And a screen to edit it**, which is the part that is real work: a list of actions, a press to capture, and a refusal when a key is already spoken for. `help` is the list already — it is generated from `groups`, so a rebinding screen is that list with each row pressable.

### What it must not become

**A second vocabulary the key list cannot describe.** `help` exists so that the game's whole vocabulary is one screen, and it is generated from the bindings rather than written beside them — see the note on `groups`, which says a list that drifts out of step with the keys is worse than no list. A rebinding table has to be the thing `help` reads, not a layer over the thing it reads.

**A way to bind a key the client cannot see.** A browser does not deliver every chord — `ctrl+W` closes the tab and never arrives — so capturing a press has to be able to say "that one does not reach here" rather than recording a binding that will never fire.

## Buttons on a screen narrower than the hotbar

**Thought about, not built.** The question is what a column of full-width
controls does when the screen is narrower than the thing it is sitting beside,
and the honest answer is that three of the four candidate answers are wrong for
a different reason each.

`hotbar::fit` already answers this **for the hotbar** and its rule is the one to
copy: *shrink before you wrap*. A shorter row of smaller squares reads better
than two rows of large ones, and a row costs height, which is what is scarce on
a phone held sideways. It shrinks to a floor of 22 pixels and only then wraps.

For the menu's buttons the same three options give different answers:

**Shrink the text.** Wrong here where it is right for the hotbar, because a
square is legible at 22 pixels and a word is not legible at 8 points. The
hotbar's contents are pictures and a menu's are sentences, and the two scale
differently.

**Two lines.** A button whose label wraps is fine and is what should happen —
egui will do it if the label is given a wrap mode and the button a min height.
What it costs is that a column of buttons stops being one height, and the eye
uses a uniform row height to count a list.

**Vertical, or rather: stop being full width.** The menu is already a column of
full-width controls, so there is no horizontal arrangement to give up. The thing
that actually gives is `Metrics::panel_min` at 360 points — below that the panel
is the screen and the margins are gone.

So the answer is probably **wrap the label and let the row grow**, with the
floor being that a button is never shorter than `action_height` — and the real
work is elsewhere: `two_column_min` is consulted in exactly one place today
(`menu::play`), and the four in-game views have no breakpoint at all. See
[better interfaces](#better-interfaces), which is where that is written down.

What is worth measuring first: the narrowest screen anybody will actually use is
about 320 points, `panel_min` is 360, and nothing has ever been drawn at 320.
Until that is tried, every answer here is a guess about a layout nobody has
seen.

## Better interfaces

**Part built.** The menu has had two passes and everything else has had none, so the client now reads as two different products depending on which screen you are on.

What is actually wrong, in the order it bites:

**The home screen is done.** It is three buttons in the middle — Play, your
account, how to play — and nothing else. It held a name field, a rating, a
record, two lookups and a settings drawer, all of which are things you *read*,
with the one control anybody opens the game to use underneath them. What a
player is now lives on `menu::Page::Account`, which is a page to visit
occasionally and read carefully, and solo hangs off Play rather than off Home
because it is the same errand: one form, answering "make it here" or "make it
on that server" depending on whether one replied.

What is left is the four in-game views.

**The HUD is a desktop panel.** It covers a third of a phone screen, and its hint lines name a left button, a right button, WASD and escape — none of which a phone has. It also has no hierarchy: every line is the same weight, so nothing on it says what matters, where the menu now has one accent per column and says exactly that.

**There is no help a phone can open.** `?` shows the key list, and a phone has no `?` and nothing to do with a list of keys once it has one. What a touch client needs is not that list; it is the four or five gestures, shown once, dismissible.

**The hotbar is reachable and small.** It was sized against a mouse. Ten stamps and four tools on a phone want either bigger targets or fewer of them on screen at once.

**Numbers still shuffle.** [Type, and the numbers that jitter](#type-and-the-numbers-that-jitter) is the entry for that, and the record panel is the only place it has been fixed — everything else still sets a changing figure in a proportional face.

None of this is a rewrite. The pieces the menu needed already exist: `theme::Metrics` holds the sizes, `words` holds the strings, and `hue` holds the colours. What is missing is somebody applying them to the other four views.

## How to play

**Built** — `menu::Page::HowToPlay`, reached from the home screen, with the
five rules and the tip in `words::howto`. What follows is the argument for each
and stays here; the page is the shortest form that still explains.

**Designed, and this is the reasoning behind what is on the page.** A page of its own, reached from the home screen, saying the things a player cannot work out by clicking. The key list at `?` is not it: that is a lookup table for somebody who already knows what they are looking for, and this is for somebody who has just arrived and does not know that placing is confined to ground they already hold.

What it has to say, in the order it bites. Each of these is a rule somebody loses to before they learn it, and none of them is visible on the board.

**You can only build where your influence already reaches.** This is the first thing anybody runs into and nothing on screen explains it — a click lands, says "not yours to build on", and the player has no idea what would make it theirs. The answer is that territory is a field with sources: your granted patch is a spring that never runs dry, and live cells feed it, so you grow ground by growing life outward from what you have. See [game.md](game.md#where-you-may-build).

**A factory pays on turnover, not on holdings.** A block of factories is a still life, never gives birth, and earns nothing at all — which is the exact opposite of what "I own a lot of factories" suggests. An oscillator earns every period and a gun earns forever. This is the single most counter-intuitive rule in the game and the one that decides whether somebody's economy works.

**A turret is the other way round, so it is placed in fours.** It works by standing still, and one on its own dies of loneliness in a generation. The block that is a factory's worst shape is a turret's best: four is the cheapest thing in Conway that never dies and never gives birth.

**Ice cannot be taken back.** It stops time over whatever it covers and only life reaching it breaks it, so a misplaced pane is a decision you live with. Worth saying before somebody spends on one.

Then the part that is a *tip* rather than a rule, and it is the one that opens the game up: **other people's patterns work here.** Liveness is exactly B3/S23 — `sim::world`'s `liveness_is_exactly_b3_s23` measures it against a longhand stepper, and an R-pentomino stabilises at generation 1103 with 116 cells, which is the figure in every book. So a glider is a glider, a gun is a gun, and fifty years of published patterns are things you can build here and expect to behave. That is what the [laboratory](#experiments) is for, and it is why [RLE](#what-to-take-from-golly-in-order) is worth more than anything else on that list.

### And a word about Conway, at the end — built

`words::conway`, at the foot of `menu::Page::HowToPlay`: one paragraph and five
links, no explanations. What follows is why it says what it says.

At the foot of the page, after the tips, briefly and without ceremony — because the rule this game is built on is his, and because he would have wanted the sentence after it.

**John Horton Conway did not want to be remembered for the Game of Life.** He was open about finding it a nuisance: it was a Sunday afternoon's play with counters on a Go board in 1970, it went round the world through Martin Gardner's column, and it then stood in front of everything else he did for fifty years. He came to a sort of peace with it late on, but the irritation was real and it is worth being honest about rather than quoting him as its proud father.

So the page should say what he would rather you looked up:

- **The surreal numbers**, which he thought his best work — and the way in is [Hackenbush](https://en.wikipedia.org/wiki/Hackenbush), which is where they came from and is the example the page should use. You rub out coloured edges and whatever is no longer joined to the ground falls off; a position is then *worth* a number. One blue edge is worth 1, a blue edge standing on a red one is worth a half, and carrying on that way reaches every dyadic fraction. Allow infinite drawings and the same construction reaches the reals, the ordinals, numbers smaller than every fraction and larger than every integer. A single construction, out of nothing but games. Knuth wrote a novella about them.
- **The Conway groups**, three sporadic simple groups he pulled out of the Leech lattice — famously in one sitting, having set aside two long slots for it and needing only the first.
- **Monstrous moonshine**, the conjecture he and Simon Norton made connecting the Monster group to modular functions, which Borcherds proved and won a Fields Medal for.
- **Combinatorial game theory**, which he largely founded — *On Numbers and Games*, and *Winning Ways* with Berlekamp and Guy.
- **The doomsday algorithm** for working out the day of the week in your head, which he delighted in and practised daily.

He died in April 2020. The nod should be short, should link out rather than explain, and should not be sentimental — one paragraph and a list. The point is that somebody who enjoyed this enough to read to the bottom of the page is exactly the person who should be told there is far more, and where it is.

## A profile screen worth visiting

**Designed.** The panel exists — name and fingerprint, colour, rating with its
provisional mark and the line it has traced, matches, most ground held, and
your own diary under the server's count. What is designed and not built is
everything that makes it a *place* rather than a readout.

### Stamps are edited here, not in play

**The library is a modal panel over a running world**, which is the wrong room
for it. Renaming a pattern, pinning one to the bar, throwing one away and
drawing a new one are all things you do *between* games, and doing them over a
board that is still stepping means every one of them competes with the game for
the screen and for your attention. Worse, the pad is where a pattern is drawn
by hand, and drawing a shape carefully is the least interruptible thing in the
client.

So the library moves to the profile screen, beside the record — which is where
it belongs by ownership as well as by timing: a stamp library is *yours*,
filed against your key, and it is the one thing on a profile that nobody else
is ever shown. What stays in play is holding one and putting it down, which is
the hotbar and is already right.

What that needs is the pad and the list working outside a `GameApp`: they take
a `Library` and a theme today and nothing else, so this is mostly about where
`show` is called from.

### A face

Not an upload. **An identicon from the key**, which costs no storage, no
moderation and no upload path — and has an answer to hand that no other game
has: a pattern. Take the fingerprint, seed a small soup, step it a few
generations with the game's own rule, and draw what settles. Everybody's face
is a still life or an oscillator that is theirs, derived rather than assigned,
and it is the same arithmetic the rest of the game is made of.

Uploads are the thing to *not* do, and the reason is not effort: a picture
somebody chose is a picture somebody has to moderate, and this is a game with
no accounts, no email and no way to contact anybody about anything.

### Finding somebody — built

`ClientMessage::People { like }` answers `ServerMessage::People { like, found }`,
and an empty `like` answers the best rated, which **is** the leaderboard —
one message rather than two, because two implementations of "who plays here"
would come to disagree. Answered without a seat, like `Rooms` and `Profile`;
sharper here, because this is how you find a person to look up in the first
place and a menu is where you are standing when you do.

It came out one screen rather than two for the same reason: `menu::Page::People`
is a field and a list, and typing in the field turns the board into a search.

The four things to be careful of each have a test.

- A name is **self-chosen**, so the fingerprint is on every row and is what a
  row names when it is pressed — `pressing_a_row_looks_up_that_persons_fingerprint`
  is two alices, which is the case it exists for.
- It must not become a way to enumerate everybody a server has met, so the
  answer is capped at `net::PEOPLE_MOST` either way. A cap and not a page:
  finding somebody and seeing who is on top neither want paging.
- **Provisional players are off the board and still findable.** A rating from
  one game is mostly the starting rating, so an unbounded board is a table of
  luck; somebody searching by name wants that person regardless.
- Sorted by rating, then name, then fingerprint. A `HashMap` has no order and a
  list that reshuffled between two identical questions looks broken.

The query comes back with the answer and the client drops one that no longer
matches what it is asking, because a search box is retyped a character at a
time and replies arrive out of order with respect to the typing.

What is **not** done here is the swatch. A person's colour is hashed from their
fingerprint, which is stable and theirs and is the cheap version of
[a face](#a-face) rather than a substitute for it.

## Antialias always

**Built.** One rule at every zoom: a box filter over the pixel's own
footprint, always, rather than a point sample above zoom sixteen and a `k×k`
supersample below it.

`aa_side` already computes the footprint in texels and clamps it to `1..4`,
and the rule it implements is already a box filter — the `k == 1` branch is
that same rule with one sample rather than a different one. What is wrong is
the claim above it: *"at one pixel per texel a point sample is exact"* is only
true when pixel centres land on texel centres, which they do not once the
camera is anywhere but an integer offset. So a straddling pixel picks one of
the two texels under it and flips between them as you pan.

The fix is to floor `k` at two rather than one and drop the branch, so a pixel
is always averaged over its own footprint. **The blur is exactly one screen
pixel wide**, which is the whole point: at high zoom the footprint is a
fraction of a texel, so the average is the texel it sits in except within one
pixel of a boundary — crisp blocks with a single soft pixel at each edge, which
is what pixel art wants and what `render/atlas.rs`'s "not a blurred blob" was
guarding against being lost.

What it costs is four texture reads a fragment where there was one, at the
zooms people spend most of their time at. That is the thing to measure before
it lands, and the cheaper alternative if it bites is the texel-snapped bilinear
trick — one sample, edges softened over a pixel by `fwidth` — which needs
derivatives and so needs the sampling out of non-uniform control flow, and
needs care at tile boundaries because the sheet is an atlas and a bilinear tap
across one would bleed a neighbouring picture in.

## Texels nothing samples

**Open.** Between about zoom five and sixteen the picture falls apart, and it
gets worse fast rather than gradually. The cause is not the filter, and no
amount of work on the filter will fix it.

**One reading a pixel is not enough below one pixel a texel.** The world pass
samples the world once per screen pixel — deliberately, so that nothing blends
before the resolve. A cell is sixteen texels of art, so at zoom sixteen one
texel gets exactly one pixel, and below that some texels get **none**: no
sample lands in them and nothing that happens later can know they were there.
The resolve filters over one *screen* pixel, which by then is too late; the
information is already gone.

It degrades quickly because the art is high frequency. A factory is a diamond
outline and a turret a plus, both drawn in strokes one and two texels wide, so
the moment the sample rate drops below two a stroke they stop being sampled
consistently and start winking in and out with the camera.

**The coarse path does not cover it.** `COARSE_BELOW` is four pixels a cell,
chosen for residency — one chunk is one array layer and a 1080p screen wants
more than 256 of them below about zoom five. So the fine sprite path is used
from five upward, and everything from five to sixteen is drawn from a subset of
its own art. That band is most of the useful zoom range.

### What would fix it, worst first

**Mipmaps on the sheet, which is the obvious answer and is wrong here.** The
sheet is an atlas: a mip level averages across tile boundaries, so level one of
a tile contains a quarter of each of its neighbours. It could be made to work
with gutters, or by giving each kind its own array layer, but both are a change
to the sheet's layout and to the tile arithmetic that `Cell::sprite` and
`sprite_index` share — and the second costs a texture array binding to save a
problem the next option does not have.

**Draw the world larger and let the resolve read a block, which is the cheap
one to try.** The offscreen target already exists, and it is the whole of the
machinery: render at twice the width and height and have the resolve average
each 2×2 group, on top of the phase weighting it already does. Four times the
fragment cost, no change to the art, no atlas problem, and it composes with
what is there rather than replacing it. This is the first thing to do.

**Reduced tiles — built, one level.** `atlas::HALF_ORIGIN`, generated by
`tools/cnvt.rs` and read by `sheet_at` in `grid.wgsl`. What follows is the
reasoning and what is left.

**Reduced tiles, drawn once, which is the principled one — and they fit in the
sheet that is already there.** Mipmapping by hand, with the atlas problem
solved by construction: each reduced tile is a whole tile, so nothing bleeds
across a boundary the way a real mip level would. It also gives the art a say,
which is the part no amount of filtering buys — a factory at four texels can be
*drawn* as something legible rather than averaged into a grey smudge, which is
what every pixel-art game with a zoom does.

The layout is the neat bit, and it needs no second texture and no second
binding. **Give the next level down a quadrant.** The sheet is 256 texels
square holding tiles of 16; a half-size tile is 8 texels, so all 256 of them
fit in 128×128 — one quadrant of the sheet exactly. The level after that is 4
texels, 256 of them in 64×64, a quadrant of that quadrant. So each level is the
next power of two down, in the corner, and the address is arithmetic on the
level: a shift on the tile size and a fixed offset, which is the same kind of
sum `sprite_index` already is.

What it costs is **kinds**. The bottom-right quadrant is rows 8-15 and columns
8-15, which the tile arithmetic reads as kinds 6 and 7 at every age and state —
so reserving it spends two of the three kind indices still free.
[Depleted factories](#depleted-factories) wants a fourth kind, which leaves one spare
after this rather than three. That is the trade and it should be made
deliberately: a kind is a mechanic and a level is a zoom band, and there are
currently three of one and none of the other.

### Nothing is allowed to be disjoint, so the levels overlap

The switch between two levels is the part that will look wrong if it is a
threshold, and the coarse path already demonstrates it: `COARSE_BELOW` and
`FINE_ABOVE` give the swap hysteresis so it cannot flicker, and hysteresis
stops the flicker without stopping the **pop**. Two levels that meet at a line
are two pictures meeting at a line.

So the rule for every boundary, this one included: **hold the level you are on
and fade the next one in over it.** Near a switch, sample the level above at
twice its rate — two taps, which is what makes it line up with the finer level
rather than sitting a half-texel off it — and blend towards the finer picture
across the band rather than at a point. The band is a range of zoom, not an
instant, so nothing is ever a hard cut between two ways of drawing the same
cell.

That costs two samples in the band and one everywhere else, which is the right
shape: the band is a slice of the zoom range and the rest of it pays nothing.
It applies to the coarse-to-fine swap that exists today as much as to the
reduced tiles that do not, and doing it there first is the cheaper way to find
out whether the blend reads correctly.

**And there is a floor.** `camera::ZOOM_RANGE` stops at one pixel a cell.
Below that a cell is smaller than the thing drawing it and no filter downstream
can put it back — the level of detail has to keep going instead, which is what
this entry is for.

### What is built, and what is left

**Built: one level, and both fades.** The half-size level is in the quadrant,
the crossing from full to half is a band rather than a threshold, and the finer
level is double-sampled while it fades so its shimmer is averaged out rather
than carried into the band at a reducing weight. The coarse swap is faded too,
by a different trick that costs nothing: the coarse colour is lightness from
the two state bits and hue from the owner, and the fine path already holds
both — so rather than blending two pictures, the fine path *fades into the
answer the coarse path would give* and is already drawing it when the swap
happens. No second quad, no blend state, no extra texture read.

So the bands are continuous end to end: full art above 16, blending to half by
9, half art to 8, fading to the art-less cell by 4, coarse below that, and
`camera::ZOOM_RANGE` floors at one pixel a cell.

**Left: a second reduced level.** Half-size art is exact at eight pixels a cell
and is two-to-one undersampled at four, which is the remaining softness in the
band. A quarter-size level is 256 tiles at 4 texels — 64x64, one quadrant of
the half-size region — but that region is exactly full, so it needs either the
reduced levels stored for used tiles only or a larger sheet. A space decision
rather than a rendering one, and worth making when somebody draws the art.

**Left: the art itself.** The reduced tiles are generated two-to-one from the
full ones, with coverage kept binary because `sprites_have_hard_edges` means it
at every level. That is a stand-in like the rest of the sheet. The whole point
of reduced tiles is that a factory at eight texels can be *drawn* legible rather
than averaged into a smudge, and this is the one entry on the list whose real
cost is somebody drawing rather than somebody typing. The layout is what
matters, and the day the art exists it goes in the same place and `cnvt` stops
running over it.

**Or raise the coarse path to meet it.** `COARSE_BELOW` at sixteen removes the
band entirely by never drawing sprites into it. Honest, one line, and it loses
the art far earlier than anybody would want — worth remembering as the fallback
if the others are ever in doubt, and worth measuring against, since a cell
without its art at zoom twelve may read better than a sprite sampled at half
its detail.

## Something to see when it goes off — built

**Built, as a fireball in the overlay.** `World` reports what went off —
`sim::Blast`, where and how big and whose — and that was the actual missing
piece this entry named: the client learned about a detonation only as cells
that had changed. A server broadcasts them after the `Step` they belong to and
a solitary client takes them off its own world, so both paths end in one list
and one piece of drawing code.

It is drawn in `views::game::overlay`, which is the third answer below and the
one this entry recommended: a mesh fan, bright at the core and transparent at
the rim, expanding fast then settling, cooling white → yellow → orange → ember,
and fading late. `FIREBALL` is 0.75s against a generation's 0.25, deliberately:
an effect that lived exactly as long as the event is under the threshold at
which anybody notices *what* happened as against that something did.

What is still open is everything else the message unlocks — a sound, a screen
shake, a notice in the corner — none of which are rendering either.

**The hard part was that this renderer has nowhere to put an effect.** Everything
on screen is a cell: the world pass draws quads out of the chunk texture and the
interface is egui on top, and there is no layer in between for something that is
neither. So the question is not what a detonation should look like, it is where
a thing that is not a cell is allowed to live. Three answers, cheapest first.

**In the cell, on a timer nobody stores.** A blast already leaves a signature —
a disc of noise whose owner just changed — and `World::generation` is on every
client. So the shader could brighten cells whose *age* is low inside a region it
is told about, with the region coming down as one small uniform per recent
blast. No new pass, no new texture, and it dies out on its own. What it cannot
do is anything outside a cell: no shockwave crossing empty ground, no light
spilling onto a neighbour.

**A second instanced quad, in the pass that already exists.** `KIND_COARSE` and
`KIND_BACKDROP` show the shape: one more `kind` on the instance, a rect in world
cells, and a fragment arm that draws a ring by distance from the centre. That
buys a real expanding shockwave over any ground, costs one branch in a shader
that already has two, and needs the blast list on the client — which it does not
have, because a detonation is currently invisible on the wire: the client learns
about it only as cells that changed.

**Which is the actual missing piece.** Whatever it looks like, the client has to
be *told* a blast happened, where, and how big — and that is a `ServerMessage`
and a matching thing in the offline path, not a rendering decision. It is also
what would let the sound, the screen shake and the notice in the corner all
happen, none of which are rendering either. So: **the message first**, then the
cheapest visual that uses it, and the ring is the one to try.

The rest of this entry is the older design for what it should look like.

**Designed.** A detonation is currently a generation in which a disc of ground
quietly becomes different. There is no bang: the cells before and the cells
after are both just cells, so the largest thing a player can do reads as the
board having glitched — and at four generations a second the whole event is
over in a quarter of a second, which is under the threshold at which anybody
notices *what* happened as against *that something did*.

**It goes in the overlay, not in the world.** `views::game::overlay` already
draws over the board every frame in egui, with the camera's own mapping from
cells to screen — it is where the hover box and the drag preview live, and
those are exactly this shape: a thing drawn about the world that is not in it.
Doing it as cells instead would be wrong twice over. It would have to be part
of the simulation, so every peer would have to agree on it and it would ride
the wire and the save; and it would take the age field or a kind, which are the
two scarcest things in the byte.

So the shape is: the client keeps a short list of blasts it has been told
about — centre, radius, the generation it happened at — and the overlay draws
each one for a fixed number of *seconds* of wall clock, fading out. Seconds and
not generations, because this is animation and the generation clock is four a
second and stops in a laboratory.

**Where the list comes from is the one real question**, and there are two
answers. The client already knows: `net::apply` is deterministic and every peer
steps the same world, so a client could notice a dynamite at `MAX_AGE` about to
go and draw the ring itself, with no protocol change at all — and a client that
is a generation behind draws it a generation late, which nobody can see. Or the
server says so, which is a message and is exact. The first is free and is
probably right; the second is what a spectator following somebody would want,
since it is the only one that survives a client not holding those chunks.

What to draw, in rough order of what it buys:

- **A ring that expands**, from nothing to the blast's own radius, over about a
  third of a second. It is the one thing that says how far it reached, which is
  the number the player wants and now has to infer from the wreckage.
- **A flash**, one frame, bright — because a quarter of a second of anything is
  easy to miss entirely if it starts subtle.
- **A shake**, small and brief, scaled by the blast's radius. `views::camera`
  is pure arithmetic and this is an offset added at draw time, so nothing about
  the camera's own state moves.
- And the same machinery, later, for whatever else earns it: a turret firing
  is the obvious second customer, and it is the same "a thing happened at a
  square" shape.

**What it must not become** is a second thing that decides what is on the
board. It draws over what the rules did and never changes it, which is what
keeps it out of the simulation, off the wire, and out of the save.

## Bots

**Built.** A player the server plays, and an API an outside program plays through. The design is in [server.md](server.md#bots) and [the API](server.md#the-api); this is what came of the reasoning here and what is left.

It was as small as it sounded, and for the reason given: a bot's play changed nothing about the protocol. A bot is a `Player` with no connection, the server makes up its mind for it in `Server::step`, and what it chose goes through the same `act` a wire action does and out in the same `Step`. What the wire did gain was **one bit and two messages** — `Seat::bot`, so a lobby can say which rows are bots, and `AddBot` and `RemoveBot`, so anybody seated can add one to balance a side — and the bit is what cost `PROTOCOL` a bump, since a field on a struct that rides on every `Match` changes the shape of every lobby.

The book was the right first version. Easy lays oscillators of factories inside its own ground, normal also holds the frontier with a still life, hard also walls a factory it laid with ice, and the three act every sixteen, eight and four generations. Where to build is sampled round the seat's home rather than scanned, so a big torus costs no more than an empty plane. **An outside engine's seat is a bot whose driver is the API**: one seat type, one way out, one flag in the lobby, and an action it posts is priced the moment it arrives.

**What is left is the search**, and the seam for it is `Bot::choose`: a clone of the world per candidate placement, stepped and scored, behind the same signature. `World: Clone` is derived and `a_clone_steps_without_moving_the_original` pins it, so nothing stands between the book and an evaluator but writing one; difficulty then stops being two dials and becomes how deep the search goes. The book has nothing that travels, on purpose — a glider bleeds corpses as a factory — and a glider *at the frontier* is the obvious shape to add the day there is an evaluator to say when it pays.

Nothing here has been played enough to tune. The cadences and the two dials are constants in `server::bot` and were not balanced against a person; a hard bot on a small torus may well be [the unwinnable match](#the-mercy-rule) the design warned about, and a bot that has stopped being able to act is still a candidate for the mercy rule, which would free its seat mid-match and has not been tried. The frontier is a fixed six cells, the reach a fixed five patches, and a bot whose territory has grown far past its patch builds less often than it could, because half its samples still land at home.

`examples/two` runs beside a hard bot and agrees with it: the peers agree with each other — at most a few generations in four hundred differ, put right within a couple, which is alice's own prediction arriving — and the server corrects neither at any checkpoint. It used to correct both at every one, and **that was the example's doing, not the bot's**. It built its world to the server's shape and never seeded it, where the client goes through `net::sane_world`; so every rule that rolls — a territory level adjusting, a birth choosing its parent — rolled from seed nought on the peers and from the room's seed on the server, and the chunks it refetched differed in `owner` and `level` and never in `alive`. Without a bot the ground round two grants settles and the rolls stop mattering after the first few checkpoints; a hard bot keeps the ground round it moving, so the disagreement never stopped. `a_peer_built_from_steps_agrees_with_the_server_with_a_bot_in_the_room` had shown the bot innocent from the other side — a peer started from the server's world and fed nothing but `Step`s is the server's world for two hundred generations — and seeding the example is what made the two accounts agree. What is left in its count is the `Resync` that `step` broadcasts with a grant when a peer joins, which the example counts as a correction, and a chunk can still arrive a tick ahead of the `Step` it belongs with, because a connection's replies and its room's broadcasts are two channels `ws::connection` selects between in no fixed order; that shows as one correction at a first checkpoint and is not the bot's either.

## Predicting a match, and what it shares with bots and experiments

**Decided, not costed.** A live estimate of who is going to win.

### Why it is cheap here and expensive everywhere else

Games estimate a result with a model fitted to past games, because they cannot run the game forward. **This one can.** `sim` is a deterministic cellular automaton, a step is a pure function of state and tick, and `examples/headless` already runs it with no GPU — so the honest way to say who is winning is to step a copy of the world forward and look. No model, no training data, and right by construction for the assumption it makes.

One rollout per victory condition, and both read off machinery that exists. **Timer:** step a copy to the deadline and read `net::standings`. **Territory:** step until somebody crosses the line, or a bound is reached.

### What it assumes, which is the interesting part

A rollout with nobody acting answers *who wins if everybody stops playing*, and that is a **bad predictor in a game where income compounds**. A player with factories running and money in hand is exactly the one whose position keeps improving, and a no-input rollout scores them as though they had already spent everything they were going to.

So there are two versions and they differ by one thing.

**Nobody acts.** Cheap, and honest if it is labelled as what it is — *if nothing more is placed*. Worth having on its own, because it answers a question a player actually has, which is whether they are ahead or whether it merely looks that way while a shape of theirs is about to die.

**Everybody keeps playing**, which needs somebody to play them. That is a bot. So the good predictor **is** a bot run against a copy of the world, and the two stop being separate pieces of work.

### The missing object all three want

Every one of these needs to step a world without stepping *the* world, and now they can: **`World` is `Clone`**. `Storage` is a `HashMap<Coord, Chunk>` or a `Box<[Chunk]>`, `scratch` and `active` are working space, and the step is already pure in state and tick, so a copy diverges cleanly and cannot reach back — `a_clone_steps_without_moving_the_original` in `sim::world` pins it. `Server::step` owns the only world there is, and a rollout must touch neither the pending actions, nor the purses, nor the tick.

So the whole of the machinery is a clone stepped *n* times with `net::standings` read off the end. What that one derive buys:

| | |
|---|---|
| a prediction | a clone stepped to the deadline |
| a bot that searches rather than follows a book | a clone per candidate placement, scored |
| an experiment's **reset** | keep the clone, put it back |
| an experiment's **step one generation** | built, as `Server::step_once`; the clone is what makes it undoable |

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

**Built.** `client::session` is the link, the seat, the purse, the subscription set, and the `pump` / `advance_to` / `send_checkpoint` / `subscribe` machinery. `views::game` is 2,600 lines from 3,700, and its struct is 31 fields from 55.

What was wrong was not the size. It was a *view* by where it lived and was not one by what it did: it held the world, the link and the GPU pipeline together, and it executed logic — `pump_link` folded server messages into the world, `advance_to` stepped the simulation, `decide_alone` settled a match. Data, logic and interface on one struct, which is exactly the arrangement the [Data / Logic / Interface](inspiration.md#the-architecture) rule names and which every other view avoids through the `Shown`/`Chose` return-value convention.

The session uses that same convention. It takes messages in and hands back a `session::Effect` per thing only an interface can do — `Entered`, `LookAt`, `Rated`, `Refused`, `NotStarted`, `LobbyMoved`, `Made`, `NotMade`, `Rooms`, `Closed` — and `GameApp::act_on` is the other half. Everything else it does itself.

**The world is a parameter, not a field.** It belongs beside the chunk store that draws it, and passing it in is what makes a session testable against a world with no window near either — which is the point, and none of this was reachable by a test before. `advance_alone` is the offline clock, `may_act` and `may_place_at` and `price` are the three questions, and each now has one.

What is left is the other half the notes named: **`lay`, `click` and `stamp_at`**. Those still turn a gesture into cells and an answer into words, which is view work, but the four steps between — may-act, quote, afford, commit — are the same four in three places with different sentences around them. The session has the pieces (`quote`, `spend`, `commit`); folding the sequence into it wants the words sorted out first, and the words are `views::words`' business.

Two things it is not. It is not a rewrite of the gesture machine, which is already pure arithmetic tested without a window in `input`. And it is not the camera, which came out for this exact reason and was the precedent.

## Rooms per server

**Built** — see [server.md](server.md#rooms). A room is a whole `Server`: one world, one player table, one tick, one file. Rooms are listed, joined, made and left from the client, and a room is identified by an **id** rather than by its name, so renaming one keeps every seat and every rejoin token valid.

What is left is lifetime — [auto-sleep](#making-rooms-from-the-client), above — and one gap in the store:

**The token is keyed by room but not by server.** Two servers both holding a room whose id is `main` share one secret, and visiting the second costs you your player on the first. `client::record` has the same hole from the other end: a game is filed under a room's display name, so two servers' `arena` are one line of history. Both stop being bugs rather than being fixed if the token becomes a key the client owns — see [many servers](#many-servers-and-what-must-not-be-decentralised).

## Auto-manufacture

**Built** — see [game.md](game.md#manufacture). A factory is a living cell that pays its owner every time one of its kind is born, and the mechanism is **inheritance**: a birth copies its parent, kind and all, so a factory's children are factories and the kind spreads through a mixed population because the parent is picked at random.

That is a better idea than what was written here before, which was a factory as a marker on the ground paying out on deaths. Inheritance makes a factory an investment in a *lineage* rather than a square, needs no per-square bookkeeping, and the payout is counted where the rule already holds a cell before and after — so it costs a comparison and no second pass.

Two of the three open questions answered themselves. The rule counts births and `net` prices them, so the tally never taught the simulation about money. And the prediction problem went the way this section said it should: `Purse` rides on every `Checkpoint` reply, reusing the machinery that already exists for "your copy is wrong, here is mine".

A factory's corpse now costs while it lies there, sixteen generations in sixty-four, so income is births minus the upkeep of everything you have let die. What that rewards is a machine that stays where you put it: a blinker pays, and a glider dragging twenty corpses behind it bleeds. `cargo run --no-default-features --example balance` prints the table, and the rate was picked off it rather than argued about.

What is left is a hole rather than a number: **there is no way to clear a factory's corpse.** A dead cell cannot be reclaimed, so the only remedy for a factory field you regret is to let the life on it go out and wait for territory decay to take the ground. That is a long punishment for a misclick, and value floors at zero so a bad enough mess simply stops you playing. Reclaiming a corpse to clear its kind — for a price, or for nothing — is the obvious fix and needs a decision about what it should cost.

The art is a stand-in like the rest of the sheet: the ordinary cell with a diamond and a pip stamped into it, generated rather than drawn, in `assets/sprites/art.png` at tiles 4–7. It reads clearly against all four states and in any player's hue, and it is not what anybody would draw on purpose.

Also unsettled: **a factory under ice**. A pane freezes what it covers, so a frozen factory gives no births and earns nothing — a cheap way to switch off somebody's income without taking their ground. Whether that is a feature or a hole is a question for whoever sets the rate.

## Turrets

**Built** — see [game.md](game.md#turrets) and [simulation.md](simulation.md#turrets). A turret claims ground at range: every generation it takes the nearest square that is not its owner's, out to `rule::TURRET_REACH`, and a dead one runs that backwards over the ground behind it. It is a pass after the rules in absolute coordinates, beside `break_ice_from`, because every rule in `sim::rule` sees one cell and its eight neighbours and no halo can answer "the nearest square that is not mine".

Two things fell out rather than being designed, and both are better than what was planned here. **A live cell must have an owner**, so taking a square away from its owner kills whatever stands on it — the dead turret's killing is that invariant rather than a rule about killing. And a turret needed no rule about where it may be placed: its first choice is always ground that is not its owner's, so it reaches past a frontier from anywhere behind one.

The inheritance problem was answered by splitting kinds into those a birth inherits and those it does not, which is now `kinds!` in `sim::cell` — one list writing `Kind::ALL`, the count and `Kind::inherits`, the way `rules!` writes the rule chain and its names. A kind that does not inherit passes over ownership alone, so a birth beside a turret is ordinary life owned by the turret's owner and a gun is not a turret factory. That made the rest of what was planned here unnecessary: a turret never spreads, so it needs no bill to stop it sprawling, and its balance is its purchase price and its claim rate and nothing emergent.

What is left is numbers rather than mechanism.

**The balance is argued, not measured.** `TURRET_COST` at fifteen, `TURRET_REACH` at six and `TURRET_DECAY` at four in sixty-four were reasoned off the decay arithmetic — a claim a generation against `DECAY` settles at about thirty squares, so a block of four holds about a hundred and thirty — and nothing has run to check it. `examples/balance.rs` is the harness that answered this for factories and prints nothing about turrets. It should, and the shapes to put in it are the block against a lone turret against a turret dropped into a glider, since those are the three things a player will try.

**Half of this landed with territory levels.** A turret plants influence rather than flipping a flag: `rule::TURRET_PUSH` is what it puts on a square it takes, and it plants at full rather than nudging, because the rule assigns a square the strongest claim reaching it rather than adding to what is there — a push of three would be wiped the next time that square worked itself out. What did *not* change is `TURRET_POWER`, which is still a count of squares. Making it a quantity of level instead is the version that contests properly with everything else pushing on the same ground, and it is still worth doing.

**Whether a turret should press on a living neighbour is a number rather than a rewrite.** `rule::TURRET_POWER` is how many squares it takes a generation and sits at **one**, which makes it the reaching tool rather than the weapon. The arithmetic that used to be here was written against `SPREAD`, a constant the level rule deleted; what a turret now holds against a living colony is whatever `LEVEL_SPREAD` and `LEVEL_EBB` give back, and that has not been measured. `examples/balance.rs` is where the answer should be printed rather than argued about.

**A turret under ice** is the same open question as a factory under ice, and sharper. A frozen turret does not fire, so a pane is a cheap way to switch off somebody's territory engine without taking any ground from them. Whether that is a feature or a hole is for whoever sets the rate.

**The remedy for a corpse gets dearer the longer it is left.** A dead turret is cleared by building on it — placing life sets the kind back to ordinary, as it does over a dead factory — and what the corpse is doing is taking your ground away a square at a time, so the square you need to build on stops being yours and the fix goes from one to ten. That may be exactly the right shape and it has not been played enough to say.

**A turret should not also kill, and the reason is that a claim is contested and a kill is not.** Ground a turret takes is taken straight back by `SPREAD` at forty in sixty-four, which is why it cannot touch ground anything is alive on and why one square a generation is nearly nothing. Nothing does that to a kill: a dead cell stays dead unless Conway hands it back. So the same "one a generation, forever" that is almost nothing for claiming is decisive for killing — and a turret is a **still life**, four cells, immortal, free after purchase and unreachable without flying something into it. A block of them killing four cells a generation forever is not a territory tool, it is area denial with no answer.

It would also cost the two things that make a turret readable. The dead turret's kill is not a rule about killing — it is the `Cell::alive` invariant showing through, since unowning a live square kills what stands on it — and that reads as a mirror only while the live turret does not kill. And a turret that finds no frontier in reach falls back to reinforcing its own thin ground, which is a slow indirect push; one that kills always has something to shoot at, wherever it stands, and that distinction goes.

So: **another kind**, and the interesting question is what powers it, because "stands there and fires" is exactly what the turret does and exactly what should not have a kill attached. The shape that fits this game is a kind that spends a **birth** — a cell that, when one of its kind is born, kills the nearest enemy life. Its rate is then your pattern's birth rate, which is what the game already rewards building; a gun feeds it and a block does not; and it is counted where `Halo::step_into` already counts a factory's births, holding each cell before and after in one breath. That makes killing something you run a machine for rather than something you park.

Worth saying out loud first: **Conway already has a weapon.** A glider is five cells and one gesture and it kills what it hits. Whatever this kind turns out to be has to be worth more than that, or it is a button for a thing players can already do.

**The art is a stand-in** like the rest of the sheet: the ordinary cell with a solid plus stamped into it, generated rather than drawn, in `assets/sprites/art.png` at tiles 8–11 with the factory's own mark colours so the two read as siblings. It is legible against all four states and in any player's hue, and it is not what anybody would draw on purpose.

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

**And if the grant is to be a machine after all**, an oscillator is the only thing that is immortal, stationary and gives births: a blinker of factories is the smallest, three cells earning every other generation forever. It is a fallback rather than an answer — it pays a flat rate, which is the clicker again with a better animation.

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

## Mobile

The page lays out at device width now and the canvas no longer asks for three device pixels a point, which were the two things making it unusable. What is left is the interface rather than the plumbing.

**Touch reaches the interface now**, which it did not: `Views` translated winit's mouse events by hand and never translated a touch, so egui received no press at all and every button on a phone was dead — see [gotchas](gotchas.md#a-finger-is-not-a-pointer-unless-somebody-says-so). The world always worked, because the client reads `App::on_touch` itself, which is why it went unnoticed.

What is left is the layout rather than the plumbing. The HUD is a desktop panel: it covers a third of a phone screen and its hint lines name a left button, a right button, WASD and escape, none of which a phone has. The hotbar is reachable but small. And the key list behind `?` is a list of keys, which is a screen a phone has no way to open and nothing to do with once it is open.

## Known, and left alone

- **Fifteen players is a ceiling on players a room has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. It was thirty-one until the level took a bit off the owner byte. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. See [fifteen slots](#fifteen-slots-and-more-than-fifteen-clients).
- **Territory creeps and decays now**, so ground is traded and lost as well as won, with granted ground exempt as the floor. What is unsettled is the floor: "your home patch is permanent" is a strong promise, and it also means an opponent who grows over it keeps a square that will never decay for them either.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::game` is 1900 lines doing five jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, `chose`, `to_menu`, and the `me`, `room`, `value`, `screen` and `subscribed` fields — is everything about talking to a server and nothing about drawing. The menu made that worse rather than better: the screen the client is on and the connection it is holding are the same state machine, and it now lives in the same struct as the sprite atlas.



## Cloudflare, and which half of this fits

**Thought about, not costed.** The two halves of what the server does come
apart cleanly, and one of them is a fifteen-minute job while the other is a
port. Worth writing down before anybody starts with the easy half and discovers
the hard one.

### The page fits Pages, and one line of design is in the way

`index.html`, `pkg/` and `assets/` are static files. Pages serves them, sets
`application/wasm` without being asked, and puts the whole thing behind a CDN,
which is a real improvement over one process in one place serving a 7.5 MB
module.

Two things to check rather than assume. The **loading bar** wants a
`Content-Length` and falls back to an indeterminate sweep without one — see
`index.html` — so if Pages answers the wasm with `Transfer-Encoding: chunked`
the bar stops being a percentage. And Pages is **HTTPS**, which is worth having
for a reason unrelated to security: a secure origin is what makes
`navigator.gpu` exist, so the browser gets WebGPU instead of falling back to
WebGL2. That fallback works and is slower and less capable, and today it is
what anybody visiting by IP gets.

What is actually in the way is that **same origin is load-bearing**. The
browser client derives its socket from the page — `link_web::origin_url`, which
already gets `https` to `wss` right — and the README makes a feature of it:
"the browser client connects back to whatever served it, so there is nothing to
configure". Put the page on Pages and the server anywhere else and that
derivation points at Pages, which has no socket. So splitting them means the
client needs a configured default server: a build-time constant, a `meta` tag
the page carries, or the menu's existing server field pre-filled. The menu
already knows how to be told an address; what is missing is a default that is
not the origin.

### The server is not a Worker

It is a long-lived stateful process: worlds in memory, a tick every 250 ms per
room, websockets held open, and `.ckw` files written on the way out. A Worker
has no memory between requests and no timer of its own, so none of that
survives the model.

**Durable Objects are the right shape**, and it is a close fit rather than a
coincidence. One DO per room maps onto `server::rooms::Room` almost exactly:
single-threaded, addressable by name, with hibernatable WebSockets and alarms.
`--room NAME` becomes a DO id and the room list becomes a DO of its own.

Four things to weigh before believing that:

**It is a third transport, and the second server.** The crate would compile to
wasm32 for workerd, so `axum` and `tokio` leave the server path entirely — the
`server` feature already draws that line, which is the one piece of this that
is free. `workers-rs` supports DOs in Rust. What has no equivalent is
`tokio::time` driving the step loop.

**Alarms are not a metronome.** Four ticks a second is at the edge of what DO
alarms are for; they are scheduled, not periodic, so each tick reschedules the
next and the drift is real. A room that steps late is a room whose clients all
see it step late — the server is the clock, per [networking.md]. Whether that
is acceptable is measurable and nobody has measured it.

**CPU is metered per invocation.** Stepping a 16384-chunk torus four times a
second is the most expensive thing in the game and it happens whether or not
anybody is looking. On a VPS that is a core; on a DO it is a bill and possibly
a limit. The infinite worlds are fine — they only hold what players have
touched — and the large tori are the question.

**Storage is not a file.** A `.ckw` is one blob and DO storage is key-value
with a per-value ceiling, so persistence becomes many keys rather than one
write. That suits the format better than it sounds — the file is chunk-based
already — but `server::persist` is written as a stream over a whole world and
would be rewritten rather than adapted.

### The cheap answer, which is probably the right first one

Pages for the client, the Rust server unchanged on anything that runs a
container, and Cloudflare in front of it proxying the websocket. No port, no
DO, and the only code change is the configured-server-address one above, which
is wanted anyway the moment there is more than one server — see
[many servers](#many-servers-and-what-must-not-be-decentralised), which is the
entry this eventually collides with.

The order that avoids wasted work: **the default-server address first**,
because every arrangement here needs it and it is small; then the page on
Pages, which is then free; then the server wherever it is cheapest, with
Durable Objects looked at only if per-room isolation or scale-to-zero turns out
to be worth a port.

[networking.md]: networking.md#the-server-is-the-clock


## Parties

**Built** — see [server.md](server.md#parties) for the wire and the tables and
[game.md](game.md#parties-and-asking-somebody-in) for the page. A party is a
group of people with a **private set of worlds only they can see or join**.
Not one room — a set — so it is somewhere a group *lives* rather than a game
they are currently in: a room is a world and is over when it is over, and a
party outlives every room in it.

### It is a membership, which is why it is a list of people

A private room is reached by a six-character code, which is a **bearer
credential**: whoever it is forwarded to gets in, and the room cannot tell.
That is right for reading six characters to somebody sitting next to you and
wrong for a group that persists — a party somebody left should stop being a
party they can rejoin, and a code cannot express that. So a party is a set of
`PersonId`s in `server::parties`, its worlds have no code, and the door into
one — `Rooms::may_enter`, the same door an invitation opens — asks whether you
are on the list. Leaving takes the worlds with it, which is the sentence a
code could never say.

### Built on today's person, on purpose

This entry said parties wait on [identity being a keypair](#identity-is-a-keypair-and-today-it-is-not),
because "invite Alice" needs a durable name for Alice. It is built on the
per-server `PersonId` a secret is exchanged for, on exactly the pattern
`Rooms::challenges` already used — a `PersonId`-keyed table, saved in a
`.jsonl` beside the others — and the reasoning is the one
[what to do next](#what-to-do-next) gives for the leaderboard. A `PersonId` is
already durable per server: it keys ratings, lockers, seats in every `.ckw`,
challenges and the outbox. What a key adds is that the *same* person is the
same on a second server, and there is one server. The price is the doc's own
cost item 1: when a person becomes a key fingerprint, every row in
`parties.jsonl` and `rooms.jsonl` resets, or is claimed under whatever
time-limited migration ratings get. That is one line in a release note, and
the leaderboard already pays it.

Two things that looked like identity problems were not. Room metadata was
in-memory, so a private world came back from a restart listed, codeless and
nobody's, and ownership could not be checked — fixed on its own first, since it
was a standing wrong on its own. And a client on the menu was nobody, because
`Caller.person` was set by a `Welcome` and by nothing else — fixed by
`ClientMessage::Hello`, which is the pre-seat state the keypair handshake will
need and is where a signed presentation goes when there is one.

### The room list stays one answer

`ClientMessage::Rooms` is answered without a seat and is the same list for
everybody, and stays so. A party listing is the first thing on the wire whose
answer depends on who is asking, and it is a **second message**, `Parties`,
rather than a filter on the room list — because it wants different *contents*
(who is in the party, who is online, which of its worlds are running), and
because the room list is the one message a client sends before it is anybody.
A connection that has presented no key is answered an empty list, which is
true rather than a refusal.

A member's *online* flag is one server answering its own members about each
other. The line drawn under [presence](#friends-searching-and-inviting-somebody-in-particular)
is about a game server reporting to a directory, and nothing here is reported
anywhere.

### What it must not become, and has not

An account system. There are still no accounts — no email, no password, no way
to contact anybody outside the game — and a party is a list of ids a server
issued, which is the same thing a lobby already shows. Inviting somebody names
a person the server has met and nobody else.

### What is left

**Signed invitations.** An invitation into a room or a party is a row the
server keeps and does not expire. When identity is a key it becomes a signed
statement — *this key admits that key until this time* — and gains the
"until", delivery through a directory when both are connected, and a link that
names you rather than a bearer token. The room side is already the set lookup
that design asked for.

**Re-keying.** The migration in cost item 1 has to cover `parties.jsonl` and
`rooms.jsonl` as well as `profiles.jsonl`, or say plainly that it does not.

**Handing a room on.** A party's world stays its maker's when the party goes,
and there is no way to give a room to somebody else; that wants ownership to be
transferable, which is the same signed statement as an invitation.

## A minimap

**Noted, not designed.** Not a scaled-down picture of the board — a picture of
**where the territory is**, which is a different question and has a much better
answer. Trace the outlines of who holds what with **marching squares** and draw
the borders, so a glance says where everybody's country is and where it meets.

Two reasons it is worth doing that way rather than by shrinking the world:

- Territory is exactly what marching squares is for. The input is a scalar per
  cell — here, whether a square belongs to a given player — and the output is a
  set of contours, which is a *border*. Shrinking a picture of the cells gives
  a smudge at minimap size; a contour stays a line however small it is drawn.
- It is the one view that stays legible when the board does not. The whole
  [texels nothing samples](#texels-nothing-samples) problem is that art stops
  being samplable below a couple of pixels a cell; a contour has no art in it.

**In a compute shader, and possibly on the server.** The cell grid is already
the natural input, one workgroup per tile of cells, and the marching-squares
table is the standard sixteen-case lookup. Server-side is the interesting half
of the idea: a contour set is small — some line segments per player, against a
world of millions of cells — so a server that computed it once a generation
could **send** it, and every client would get a minimap without holding the
world it describes. That is the first thing in the game that would let a client
see a shape it has not been sent the cells for, which is a change to what
[subscription](networking.md) means and wants thinking about before it is
built.

What it runs into: a server has no GPU in most of the places one would deploy
it — see [Cloudflare](#cloudflare-and-which-half-of-this-fits) — so "in a
compute" and "on the server" pull against each other, and the CPU version wants
costing before the GPU one is assumed.

### Not from the client

**Not yet**, and the reason is not effort. A client holds the chunks it subscribed to, which is its own screen and a margin — so a minimap drawn from what the client has is a picture of where you already are, which is the one place you do not need a map for.

A real one needs a **coarse summary from the server**: something like a byte per chunk, saying which player holds most of it and how strongly, broadcast on a cadence the way `Standing` is. That is a small message and a straightforward pass over the world.

What it runs into is the boundless world. "The whole map" has no edge, so a minimap of one is either a window around the action or the bounding box of everything anybody holds, and both change size as people play. On a **torus** there is no such question: the world is a fixed rectangle and a minimap is exactly that rectangle.

Which suggests the order. Do it for wrapping worlds first, where it is nearly free, and let matches be where it lands — a match wants a fixed arena anyway, and a match is where knowing who holds what without panning across the world actually decides something.
