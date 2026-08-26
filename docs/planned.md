# Not built yet

**One file.** There used to be two — a roadmap for directions decided on and not costed, and this for designs costed and not built — and the split did not survive contact. Entries only ever moved one way, nothing moved back, and both files went stale in the same way: by describing things that had since been built as though they had not. Status is a label on an entry now, not a file it lives in.

| status | means |
|---|---|
| **Built** | in, and kept here only for what is *left* — the design itself is in [the rest of docs/](README.md) |
| **Being built** | on a branch now |
| **Designed** | worked out, including what it costs; not started |
| **Decided** | a direction agreed and not costed. What used to be the roadmap |

The system as it actually stands is [the rest of docs/](README.md). Everything here is an intention. Where an idea was borrowed from somebody, [inspiration.md](inspiration.md) says whom and for what.

## Contents

| | status | |
|---|---|---|
| [Making rooms from the client](#making-rooms-from-the-client) | Built | a world, a match or a private game, from the menu |
| [Spectating](#spectating) | Built | a room with no seat in it |
| [Games and matches by code](#games-and-matches-by-code) | Built | private rooms, and what is left of the idea |
| [The mercy rule](#the-mercy-rule) | Designed | a player who cannot act becomes a spectator |
| [Teams](#teams) | Designed | more than one player to a side |
| [Rating](#rating) | Decided | an Elo-shaped number, and what it would have to survive |
| [Many servers](#many-servers-and-what-must-not-be-decentralised) | Decided | decentralise discovery and identity; never a world |
| [Better interfaces](#better-interfaces) | Decided | the menu had two passes; everything else had none |
| [Bots](#bots) | Decided | a player the server plays, and no protocol change |
| [A leaderboard](#a-leaderboard) | Decided | the second half of rating, waiting on the same thing |
| [The session comes out of the battle view](#the-session-comes-out-of-the-battle-view) | Designed | the one place the architecture does not hold |
| [Rooms per server](#rooms-per-server) | Built | what is left is lifetime |
| [Auto-mining](#auto-mining) | Built | |
| [Turrets](#turrets) | Built | |
| [Stamps](#stamps) | Built | including what would make them outlive the tab |
| [Territory as a level, not a flag](#territory-as-a-level-not-a-flag) | Built | |
| [Matches](#matches) | Built | |
| [Type, and the numbers that jitter](#type-and-the-numbers-that-jitter) | Decided | |
| [A minimap](#a-minimap) | Decided | |
| [Mobile](#mobile) | Designed | |
| [Known, and left alone](#known-and-left-alone) | — | |

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

**Built for matches** — see [game.md](game.md#teams) and [server.md](server.md#sides). Solo or sides is chosen on the creation form; how many sides is chosen there too; who is on which, and what each is called, is settled in the lobby.

It turned out as small as the design said, and for the reason it gave: the cell already carries an owner and the rules already read it, so what a side changes is not what a cell *is* but **what counts as yours**. That is `net::reach`, which is `influence` with one comparison widened from `==` to `Sides::allied`, and `net::value_delta`, where an ally's cell reclaims at your own rate rather than a raid's. `sim` learned nothing, exactly as matches did not: the `territory` rule still contests per player, so two allies keep a border between their ground and simply cannot be hurt by it.

`Sides` is a fixed sixteen bytes indexed by `PlayerId` rather than a map from side to members, because the question every caller asks is "are these two allied" and the other direction is wanted once, in the lobby.

Three decisions worth keeping:

**Scoring sums at the one place a result is decided**, `matches::leader_of`, rather than by teaching the rule about teams. The winner it names is the winning side's highest-holding member, because everything downstream — `Phase::Over`, the result panel, the record — is written in terms of a player.

**The balance check is at the whistle, not in the lobby.** A lobby that refuses to let you join your friend because the sides would be uneven makes people argue about the order they clicked in; one that refuses to *start* until everybody has picked and no side is empty is one where they sort it out and press it again. Sizes beyond that are not checked: three against two is a match people arrange on purpose.

**Sides are settled once it starts.** Changing them mid-match would hand your ground to the people you were fighting, which the scoring could not sensibly explain.

### What is left

**The colour is in** — see [game.md](game.md#teams). A team takes a golden-ratio step and its members spread over a narrow arc around it, so allies read as one colour across a screen. The client works out the whole table, because where a member sits in their family depends on who else is on their team, and hands it to the shader in the camera uniform; the shader looks a hue up rather than computing one, and nothing else about a cell changes with the player.

What is left of it is a **measurement**. The arc is a twelfth of the circle and the families provably do not overlap at `MAX_TEAMS`, which is a test; whether two allies a twelfth apart are actually distinguishable at four pixels a cell, and whether two *teams* are, has not been looked at on a screen. That is the number to revisit before anybody plays eight-a-side.

**Friendly fire is on**, and that is the honest first answer rather than a decision. A glider is a weapon whoever built it, and a rule making allied life pass through allied life would be a rule in `sim` — which is what this design exists to avoid. Teams are about scoring and building, not immunity.

**Nothing in a world.** Teams are a match feature because a team is a way of deciding a result and a world has none. A persistent world with standing alliances is a different feature wearing the same word.

**The lobby cannot lock a team**, so anybody may join any team including one that is already full. That is deliberate — see the balance check above — and it does mean a five-player match can end up four against one if people are careless. The whistle allows it; whether it should is a playtest question.

## Rating

**Decided, not costed.** A number that says how good somebody is, updated by results, in the shape of Elo.

Most of what it needs exists. A match already has a winner, `Victory` already says how it was decided, and `client::record` already keeps what this client has played — so the *client* half of showing a rating is nearly free. Elo itself is a dozen lines: expected score from the rating difference, and a K-factor times the surprise.

What it runs into is that **a rating is a fact about a person, and this game has no people.** It has `PlayerId`, which is a seat in one room and is reused; and a rejoin token, which is a secret filed per room and is the closest thing to an identity there is. So a rating cannot be stored until there is something to store it against, and that is the same missing piece as [fifteen slots and more than fifteen clients](#fifteen-slots-and-more-than-fifteen-clients): a person becomes a UUID, and a seat becomes a thing that person holds.

Two more, both real:

**It cannot live on the client.** `client::record` is a browser's `localStorage` — a player who wants a better number can edit it, and one who clears their cache loses it. A rating that anybody can set is a rating nobody reads. So this is a **server** table keyed by that identity, which is the first persistent thing the server would keep that is not a world.

**Elo is for two players.** A match here is up to fifteen, and multiplayer Elo is a genuine choice rather than a formula: treat the result as every pairwise outcome (everybody you beat, everybody who beat you), or score against the field average, or rate only the winner. The pairwise reading is the usual answer and is what a free-for-all wants, and it falls out naturally once [teams](#teams) exist, because a team result is one pairwise outcome per opposing pair.

The order that makes sense is identity, then teams, then this. Doing it before identity means building a rating on a number that gets handed to somebody else next week — and the identity in question is the keypair in [many servers](#many-servers-and-what-must-not-be-decentralised), which is the same missing piece seen from a different side.

[A leaderboard](#a-leaderboard) is the other half of this and waits on exactly the same thing; the tiers, the placement matches and the decay are written up there rather than here, because they are what a rating is *shown* as and this is what it is.

## Many servers, and what must not be decentralised

**Decided, and partly costed.** A client that knows several servers rather than one, and a way to find them that is not a list somebody maintains.

Start with what does **not** move, because it is the constraint everything else is arranged around. **A world has exactly one authority, and that is not a limitation to be engineered away — it is what makes the simulation deterministic.** The tick is the unit of lockstep: an action is applied *at* a generation, a birth's owner is seeded from the generation, and two peers stepping the same cells at different ticks produce different worlds within a few seconds. Splitting one world across two authorities means agreeing on a tick across a network, which is precisely the problem this design exists to avoid — see [simulation.md](simulation.md) and [networking.md](networking.md#the-server-is-the-clock). A federated world is not a hard version of this feature; it is a different game.

So what decentralises is **discovery and identity**. Three pieces, in the order they depend on each other.

### Identity, which is the real unlock

Today a player is a `PlayerId` — a seat in one room, reused when a world forgets somebody — plus a **rejoin token**, which is a secret the *server* issues and the client files under a room name. That has a bug in it already: the token is keyed by room and not by server, so two servers both holding a room called `main` share one secret and visiting the second costs you your player on the first. `client::record` has the same hole from the other end: it files a game under a room's display name, so two servers' `arena` are one line of history.

Replace the token with a **keypair the client generates and never sends**. Joining becomes: the server offers a challenge, the client signs it, and the server knows it is talking to the same person as last time without ever having issued them anything. That inverts who owns an identity, and everything else here follows from it:

- **A person is the same person on every server**, with no registry, no account, and nothing to federate. This is the whole of what "decentralised identity" needs to mean here.
- **A seat becomes something a person holds**, which is the missing piece [fifteen slots and more than fifteen clients](#fifteen-slots-and-more-than-fifteen-clients) and [rating](#rating) are both waiting on.
- **The store keys by server**, because a public key needs no room to be filed under. The two bugs above stop being bugs rather than being fixed.

What it costs is a signature scheme in a crate that builds for wasm32 — ed25519 is the obvious one and does — plus a decision about what happens when somebody loses their key, which is that they are somebody new. That is the honest answer and it should be said out loud on screen rather than discovered.

### Multi-homing the client

`BattleApp` holds one `Link`. Knowing several servers does not mean holding several sockets — you are in one world at a time, so one socket is right — it means the **store** holds a list of servers rather than the last one, and the Play screen lists rooms from more than one.

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

What it runs into: **a bot that plays well makes a match unwinnable, and one that plays badly is a candidate for [the mercy rule](#the-mercy-rule)** — which would oust it, freeing its seat mid-match, which is either exactly right or very confusing and has not been played enough to say.

Determinism is not a problem, which is worth stating because it looks like one. A bot's choices are made once, on the server, and reach every client as ordinary actions at a stated tick — so two clients never disagree about what it did, and a bot may use whatever randomness it likes without touching `sim::seed`.

## A leaderboard

**Decided.** Who is best on this server, and a screen that says so.

It is the second half of [rating](#rating) and it waits on the same thing: a rating is a fact about a person, and until a person is a keypair rather than a seat there is nothing to key a table by — see [many servers](#many-servers-and-what-must-not-be-decentralised). Building it before that means a table of numbers that get handed to somebody else next week.

Once there is an identity, the work is ordinary: a table on the server keyed by it, a `ClientMessage::Leaderboard` answerable without a seat the way `Rooms` is, and a screen. It is the **first thing the server would keep that is not a world**, which is the part worth thinking about rather than the ranking — a save format, a place for it to live, and an answer for what happens when it cannot be written.

Three decisions taken from [MCSR Ranked](inspiration.md#the-dashboard-and-a-rating) and worth taking together:

**Named tiers over a bare number.** Six ranks at thresholds, so a rating is something to reach rather than a figure to read. A raw number tells a player nothing about where they stand.

**Placement matches before a rating is shown**, so one bad first game does not define somebody.

**Decay only at the top, and only on inactivity.** It keeps the top of a table honest without punishing anybody who plays occasionally.

What it runs into is that **a leaderboard is a reason to cheat**, and this game has never had one before. Per server it is manageable: the server is authoritative over its own world, so the only lever is who you play and how often. Across servers it is not — a server can say whatever it likes about its own results — which is why [rating](#rating) stays per-server until there is a reason to solve that properly.

## The session comes out of the battle view

**Designed.** `client::views::battle` is the one place the architecture in [architecture.md](architecture.md) does not hold, and the largest file in the crate by a factor of two.

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

Two things fell out rather than being designed, and both are better than what was planned here. **A live cell must have an owner**, so taking a square away from its owner kills whatever stands on it — the dead turret's killing is that invariant rather than a rule about killing. And a turret inside its owner's ground finds everything within reach already theirs and idles, so **it only works from a frontier** and needed no rule about where it may be placed.

The inheritance problem was answered by splitting kinds into those a birth inherits and those it does not, which is now `kinds!` in `sim::cell` — one list writing `Kind::ALL`, the count and `Kind::inherits`, the way `rules!` writes the rule chain and its names. A kind that does not inherit passes over ownership alone, so a birth beside a turret is ordinary life owned by the turret's owner and a gun is not a turret factory. That made the rest of what was planned here unnecessary: a turret never spreads, so it needs no bill to stop it sprawling, and its balance is its purchase price and its claim rate and nothing emergent.

What is left is numbers rather than mechanism.

**The balance is argued, not measured.** `TURRET_COST` at fifteen, `TURRET_REACH` at six and `TURRET_DECAY` at four in sixty-four were reasoned off the decay arithmetic — a claim a generation against `DECAY` settles at about thirty squares, so a block of four holds about a hundred and thirty — and nothing has run to check it. `examples/balance.rs` is the harness that answered this for mines and prints nothing about turrets. It should, and the shapes to put in it are the block against a lone turret against a turret dropped into a glider, since those are the three things a player will try.

**Under [territory levels](#territory-as-a-level-not-a-flag) a turret plants influence rather than flipping a square**, and `TURRET_POWER` becomes how much level it moves. That is a better shape than a count of squares: it contests properly with everything else pushing on the same ground, instead of overwriting the answer.

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

**Outliving the tab.** A stamp lives as long as the client does — capture a glider gun, close the tab, and it is gone, which makes the library a scratchpad rather than a collection. `net::keep` is already where a client keeps what it has, and `client::record` is now a worked example of putting a versioned, line-per-item format there. What makes it more than a `serde` derive is that a stamp is **cells and their kinds**, so the stored shape has to survive a change to `Placement` or to `Kind`: a library written before turrets existed should not come back as a library of nothing. A version on the file, and a bad line skipped rather than fatal, which is what `record` does.

## Territory as a level, not a flag

**Nothing built.** Ownership on a dead cell becomes a small number — how much of a player's influence reaches that square — instead of a yes or no. Partly inspired by Minecraft's water, which is the same problem: a field with sources, a falloff, and a boundary that has to settle somewhere without anybody deciding where.

### Why the current rule cannot be fixed as it stands

[simulation.md](simulation.md#territory) records the measurement that killed every threshold rule: **a corner of a solid region and a cell just outside a straight edge both have exactly three owned neighbours.** No count can tell them apart, so any rule built on one either erodes every blob from its corners or grows every edge outward forever. That is not a tuning failure. A boolean field has no gradient, so a cell genuinely cannot tell inside from outside, and no amount of arithmetic on eight booleans invents the information.

A level field has a gradient, and those two cells stop looking alike: the corner is surrounded by high numbers and the cell outside the edge by low ones. The decision becomes local *and* correct, which is what the current rule has never managed.

It also collapses three rules into one. `SPREAD`, `CREEP` and `DECAY` are three constants and two disjoint branches, and the names lie — spread does not spread, creep does, which took a documented investigation to establish. One field update replaces all of it.

### The model

**Living cells are sources.** A dead cell's level is the strongest claim reaching it: the best of its neighbours' levels, less a fall per step, with a living cell counting as full. That is Minecraft's water exactly, and it buys the same three things:

- a **bounded** halo with no rule about radius — the fall does it;
- a field that is a pure function of where the life is, so it cannot drift or ratchet;
- a front between two players that settles where the two claims are equal, which is the midpoint, without anything having to compute a midpoint.

The fall is the knob the whole feel hangs on. At one, a source reaches seven squares and a lone blinker holds a disc of about a hundred and fifty; at two it reaches three and holds about thirty. That is the number that answers "a blinker should spread its influence a bit and not gain territory everywhere".

### Strongest claim, or signed sum

Two readings of "look at the neighbours", and they are different games.

**The strongest claim** is Minecraft's: take the best incoming level per player, and the highest wins the square. It is a distance field, so ground goes to whoever's life is *nearest*. Two colonies meet at the line equidistant between them, and a small one holds its half of that line against a large one.

**The signed sum** — Hugh's, and add opponents as negative — is a mass field. A cell with six of your neighbours and two of theirs goes to you by four. Fronts then move on how much is pressing rather than how far away it is, so a big colony pushes a small one back rather than meeting it in the middle.

The sum has a geometry problem worth knowing before choosing it: summing all eight makes a diagonal neighbour count as much as an orthogonal one, so a field built on sums grows as a **square** rather than a disc, and the number stops being a distance, so nothing about it can be read off the screen.

The synthesis is probably: **winner by strongest claim, magnitude by the sum.** Who holds a square is a question about distance; how firmly they hold it is a question about mass. That keeps the field legible and still lets weight matter.

### What the randomness is for now

It changes job, and this is the part most worth getting right. Today the roll decides the **outcome** — which owner a cell takes. Under levels it should decide the **rate**: a cell re-evaluates on some chance per generation, and otherwise keeps what it has.

Recomputed every generation for every cell, the field would be a perfect distance transform that snaps the instant anything moves, so a glider would drag a geometrically exact halo behind it. Updating a fraction per generation makes the field lag and smear, which is what makes it look like a country rather than a Voronoi diagram — and a cell that is not updating costs one roll and nothing else.

### The bits, and the one thing that does not fit

Byte 0 is `player` five bits and three spare, one of which is `HOME`. A nibble each for player and level fills it exactly and leaves **nowhere for `HOME`**.

Two things to say about that. First, it is **fifteen players, not sixteen**: zero has to keep meaning unowned, because `Cell::alive` asserts a live cell has an owner and a zeroed cell has to stay a valid empty one. Second, the kind byte cannot help — it has spare bits, but byte 1 *is* the tile index, so a flag there doubles the sprite sheet.

So: **player four, level three, home one.** Levels nought to seven.

And `HOME` stops being an exception. Today it is "the one square that does not decay", a carve-out bolted beside the rule; under levels it is a **source that is not alive** — a spring, in the same vocabulary as everything else. A granted patch projects a live gradient whether or not anything survives on it, which is the floor expressed as a rule rather than as an escape from one.

The save format is a version bump, from four to five. Chunk bytes are a raw cast, so a version-four file read as version five is not a corrupt world but a plausible one, wrong in every cell — which is exactly what the version byte exists to stop. There is no honest migration: a boolean owner carries no level, so either old worlds load with every owned square at full and settle from there, or version five starts fresh.

### What it costs elsewhere

The territory rule goes back to being **purely local** — one cell and its eight neighbours — so it fits `Halo` exactly, with none of the trouble the turret pass has. Levels ride in the neighbours the halo already copies.

`PlayerId::MAX` is derived from `bits::PLAYER_WIDTH`, so narrowing is one constant and every per-player array shrinks with it. The shader still extracts the player with a bare shift, four instead of five.

The renderer gains the most visible part of this: **the level wants drawing.** A gradient nobody can see is a gradient nobody can play against, so unclaimed-to-yours becomes a shade rather than a switch, and the map starts showing where the pressure is instead of only where the border ended up. That is probably the reason to do the whole thing.

A turret fits better under levels than it does now. It flips a square today; it would **plant influence** instead, and a dead one would drain it — so `TURRET_POWER` becomes how much level it moves rather than how many squares it flips, which is a dial rather than a count and contests properly with everything else pushing on the same ground.

### Placing outside your own ground

**Settled, and settled twice.** Placing anywhere for ten times the price was tried and is out — it was no obstacle at all to anybody with a mine running, and it made the map somewhere you bought your way into rather than somewhere you grew into. Grading that price by how thin your influence was went with it, for a reason worth keeping: once ground stopped being shaded by its level there was nothing on screen to read the price off, and a cost the player cannot see is a cost they cannot play around.

So `net::may_place` is a wall again, at influence nought. What makes that safe where the same wall was not safe before levels is that a granted patch is a **source** — a player whose life has gone out still has a live gradient around their home, and can always build somewhere.

If the level is ever drawn, the graded price is worth reconsidering and not before: the two stand or fall together.

Removing it outright also becomes safe again, if that is what playtesting wants. The reason it was made a price was that a wall left a player whose life went out with nothing they could ever do; home-as-a-source fixes that directly, because everybody always has a patch with a live gradient on it.

### Fifteen slots, and more than fifteen clients

A `PlayerId` stops being a player and becomes a **seat in one world**. A person becomes a UUID, and the mapping from person to seat is per room — which is what the rejoin token already half is, since it is filed per room.

Thirty-one was comfortable as "players a world has ever seen". Fifteen is not, and that is the real work in this part: a world that has met fifteen people is full for ever unless seats can be **reclaimed**. A seat is free when its player is offline and their number appears nowhere — no life, no ice, no ground. `Server::territory` already counts per player in one pass; widen it to count life and panes too and the question is answered by a scan the world is nearly doing anyway.

Nothing about this reaches the cell. The UUID lives in the server's player table and in what a client keeps, and `Join` already carries a token — so the token can *be* the identity, or be looked up to one. What it buys beyond the seat count is that a person in three rooms becomes one person with three seats, where [server.md](server.md#rooms) currently has to say a player in two rooms is two players.

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

- **Thirty-one players is a ceiling on players a world has ever seen**, not on players connected at once, because a number is written into every cell its owner claimed and so can never be reused. Reclaiming numbers whose territory has gone would lift it; widening the field costs a bit from the kind. Rooms make this less pressing than it was — thirty-one is per room now — and no easier to fix.
- **Territory creeps and decays now**, so ground is traded and lost as well as won, with granted ground exempt as the floor. What is unsettled is the floor: "your home patch is permanent" is a strong promise, and it also means an opponent who grows over it keeps a square that will never decay for them either.
- **Building large structures** is still done by freezing ground with ice, which works but is not what ice is for. Deferred deliberately — schematics, a blueprint region, or players simply learning to work within the rules.
- **`client::views::battle` is 1900 lines doing five jobs.** The camera came out of it because it was pure arithmetic that could not be tested without a window; the same argument now applies twice over. The gesture machine — `Gesture`, `Drag`, `Pending`, the stroke and rectangle arithmetic — is already tested without a GPU at the bottom of that file. The session — `pump_link`, `advance_to`, `send_checkpoint`, `subscribe_to_view`, `chose`, `to_menu`, and the `me`, `room`, `value`, `screen` and `subscribed` fields — is everything about talking to a server and nothing about drawing. The menu made that worse rather than better: the screen the client is on and the connection it is holding are the same state machine, and it now lives in the same struct as the sprite atlas.

