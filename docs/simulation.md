# Simulation

Everything here runs on both the client and the server and must produce identical results on each. Client-side prediction depends on it.

## The determinism contract

Nothing in `sim` may:

- depend on the iteration order of a `HashMap` or `HashSet`, which varies between processes;
- use floating point, wall-clock time, thread scheduling, or unseeded randomness;
- touch the GPU, the window, the filesystem or the network.

The step is a pure function of (state, tick). Two worlds given the same start and the same inputs stay byte-identical, and `World::digest` exists so a client and server can confirm it cheaply.

Two places this is enforced rather than hoped for. The active-chunk list is **sorted** before use, because a `HashSet` iterates differently in different processes. And a birth's owner is chosen by **seeded** pseudo-randomness, never `rand`.

## The cell

Two bytes, uploaded as `Rg8Uint`. The second of them is a sprite.

```
 byte 0 (R)                byte 1 (G)
| player |level|H|       |K2| age  |K1 0|I |A |
 7 6 5 4  3 2 1  0        7  6 5 4  3 2  1  0
```

| field | where | meaning |
|---|---|---|
| alive | G bit 0 | living or not |
| ice | G bit 1 | a pane covers it; independent of alive |
| kind | G bits 2..4 and bit 7 | what it is — see `kinds!`, 8 of them |
| age | G bits 4..7 | how old, 0..7 — a step, not a count of generations |
| home | R bit 0 | granted ground, which is a source |
| level | R bits 1..4 | how much of the owner's influence reaches here, 0..7 |
| player | R bits 4..8 | owner, 0 = unowned, so 15 players |

**Byte 1 is the tile this cell draws**, whole and unshifted: low nibble across the sheet, high nibble down it. There is no layer to choose and no UV to carry — what a cell looks like is one number.

The fields are placed so that reads as a **grid**. Alive and ice are the bottom two bits, so a kind's four states are four columns; age is the low three bits of the high nibble, which is the row, so its eight ages are eight rows under them. The kind's third bit is the top bit of the byte, splitting the sheet in half — kinds 0–3 above, 4–7 below.

That split is the price of the placement, and it is worth paying: with the state in bits 0–1 and the age in 4–6, the only bits left for a three-bit kind are 2, 3 and 7. The alternative puts age in bits 5–7 and keeps the kind contiguous, at the cost of a sheet where age advances every *two* rows.

**Two kinds advance it, and `Kind::ages` is the table that says what each counts.** A dynamite's age is its fuse, stepped by the rule while it lives — see [dynamite](planned.md#dynamite) — and a factory's is the wear on the square it was born on, set at birth and never stepped on its own — see [depleted factories](planned.md#depleted-factories). It was six bits of kind, of which three were used, and sixty-one spare kinds is not worth a nibble that does not line up. The art that exists did not move: every kind at age nought is in the sheet's first row exactly where `kind * 4 + state` put it, and the seven rows under a kind that ages are what its fuse or its wear draws from.

The player sits at the top of its byte, so extracting it is a shift with no mask.

`Uint` rather than `Unorm` because these are bit fields, not colours: `Unorm` hands the shader floats in 0..1, so reading a field means multiplying by 255 and rounding, and a driver rounding one step the other way silently changes a cell's kind. Nothing samples this texture — the shader only `textureLoad`s it — so filtering, the one thing `Unorm` buys, is not in play.

Stored as `[u8; 2]` rather than a `u16`, so the byte order is ours rather than the host's **and alignment stays 1** — which is what lets a chunk be cast straight out of a save file or a wire frame at any offset. A `u16` field would force alignment 2 and panic on an odd offset.

A zeroed cell is dead, unowned and clear of ice, so zeroed memory is a valid empty world. Never give bit 0 clear a live meaning.

`Cell::alive` asserts the player is non-zero: **a live cell always has an owner**, because unowned life would have nobody to attribute a birth to.

## The rules

`sim::rule` evaluates one cell at a time, given whole neighbours rather than a count, so a rule can branch on what a cell *is*. It is an **ordered list**, and reading it top to bottom is the game:

```rust
rules! {
    "ice freezes what it covers" => ice,
    "territory is won and lost"  => territory,
    "life and death"             => conway,
}
```

Each takes the cell as the one before it left it and says whether the rules after it still run — ice says they do not. The order is a decision, so it is visible rather than buried in the branches of one function: ice first, so a pane freezes anything without every rule after it having to remember to honour it; territory before life, so ground changes hands on what was alive at the *start* of the generation rather than on what that same generation's births left behind.

The list is a macro rather than an array of function pointers, and that is measured rather than stylistic: an array cannot be inlined through, and three indirect calls per cell per generation cost **54%** of the stepping time — 45 µs a generation against 29. Unrolled, the list costs nothing and is still written once.

The signature is:

```rust
cell.update(&neighbours, seed) -> Cell
```

- An iced cell is returned unchanged. Checked before the kind, so a pane freezes anything without every kind having to remember to honour it.
- Otherwise Conway. Survival and death change **only the alive bit**, so a dead cell keeps its owner and metadata — "recently died, and whose it was" exists without a field for it. Those corpses are inert: nothing counts a dead cell.
- A birth is a **copy of one of its three parents** — owner, kind and all — with ice cleared.

### Whose birth is it

At random, from the three parents — but seeded, from the generation and the chunk coordinate with each cell mixing in its own position, through SplitMix64's finaliser. Every peer rolls the same number without exchanging one. All three parents are reachable and the same seed always gives the same answer; both are tested.

The newborn takes **everything** from that parent and nothing from the corpse it lands on. That is not a detail: it is how a kind travels. A factory's children are factories, and because the parent is chosen at random rather than by vote, a kind spreads through a mixed population instead of being handed down whole — one factory beside two ordinary cells wins about a third of the births there, and drifts from that. Three factories placed beside a starting block converted a whole growing colony inside thirty generations.

**Except for the kinds that are not inherited, which pass over ownership alone.** Pick a turret as the parent and the newborn is ordinary life belonging to the turret's owner: the ground still changes hands and the machine does not copy itself. Kinds are declared in one list, `kinds!` in `sim::cell`, which writes `Kind::ALL`, the count and `Kind::inherits` from the same rows for the same reason `rules!` writes the rule chain and its names — so what a kind is, what it looks like and whether it travels cannot drift apart.

The split is what the two kinds are *for*. **An inheriting kind is an investment in a lineage; a non-inheriting one is a machine somebody placed.** A factory is bought once and spreads, so what was paid for is the lineage. A turret works by standing where it is put rather than by breeding, and a turret whose children were turrets would make any gun a factory that claims the map — so it is bought once per cell, forever, and costs more for it.

The carve-out is applied **after** the roll rather than before it, so which parent is chosen does not depend on what kind it turned out to be. Every peer must reach the same parent from the same seed whatever is standing there; `not_inheriting_does_not_move_the_roll` pins it.

Ice is cleared on a birth because a parent may be *under a pane* and still count as a live neighbour while frozen. Without that, a cell born outside the pane would inherit it.

### Factories, and what the rule counts

`Kind::FACTORY` pays its owner when one of its kind is **born**, and costs its owner **once** for each corpse it leaves — `rule::FACTORY_UPKEEP`, sixteen times in sixty-four, and when the charge falls due the square loses its kind and is ordinary ground. The rule does not know what either is worth: it counts them, per player, and hands the tally back from `World::step` as a `Earned`, which is two arrays rather than one net figure so the two can be priced apart. The rule decides how *often* a corpse is charged and `net` decides how *much*.

Note what `upkeep` counts. Not deaths — charges falling due. A corpse reborn before its charge lands escapes it entirely, which is the whole of why a blinker pays and a glider does not: one re-uses its ground and the other abandons it.

Counted inside `Halo::step_into`, which is the one place that holds a cell before and after in the same breath, so it costs a comparison and no second pass over the world. The tally is returned rather than applied, because a world holds cells and not purses: the server folds it into the authoritative values and a client folds it into its predicted one.

What that rewards is a **machine that stays where you put it**. A blinker is three cells and two corpses and pays; a glider drags twenty corpses behind it and bleeds; an r-pentomino of sprawl bleeds badly. A block of factories neither earns nor costs, because nothing is ever born on it and nothing ever dies.

The drain is bounded by territory decay rather than by a timer: a corpse with nothing alive beside it loses its owner soon enough and stops costing anybody, while corpses inside a living colony are re-claimed every generation and go on costing.

## Chunks

A chunk is 64×64 cells, `CHUNK_N` a side. Neighbours are **computed** from a coordinate, never stored:

- infinite: coordinate arithmetic, and an absent chunk reads as dead;
- toroidal: `rem_euclid`, so global coordinates fold onto chunks many-to-one.

Computing rather than storing is what lets a chunk be its own neighbour, which happens on any torus smaller than 3×3. A graph of `Rc<RefCell<Chunk>>` cannot express that: gathering the neighbourhood would borrow one cell twice and panic.

### The halo

Stepping copies a chunk and the facing strip of each of its eight neighbours into a flat padded grid, `(64+2)²`. That gives the inner loop one array with no bounds checks and no knowledge of topology, and an absent or empty neighbour simply contributes nothing.

It also solves a borrow problem: stepping chunk *i* needs `&mut` on it while reading `&` from its neighbours in the same collection. The halo is owned data built from shared borrows before anything is mutated.

**Every halo for generation G is gathered before any of G+1 is written.** Get that wrong and patterns corrupt at chunk boundaries in ways that look like a rules bug.

### What is stored

An infinite world stores only chunks that hold something. Dropping an empty chunk is safe: an absent chunk reads as dead, and the active set recreates any coordinate life reaches, zeroed, which is what was discarded.

`Chunk::is_empty` means **no life and no structure** — not "nothing alive", which would discard panes for good, and not "every cell exactly DEAD", which would keep every chunk life had ever passed through and grow without bound. Both were tried; both were wrong.

A travelling glider holds one to four chunks indefinitely.

### What is stepped

Every non-empty chunk, plus any neighbour something on its edge **can reach** — life, which can cause a birth there, or **ownership**, which can creep there.

Life alone was the test, and it made territory unable to cross a chunk boundary at all: nothing woke the neighbour, so nothing was stepped there, so nothing was ever claimed there. It showed on screen as a granted patch that crept right and down and never up or left — because a grant lands flush against a chunk's top-left corner, so those two edges *are* the boundary and the other two are interior. `territory_creeps_across_a_chunk_boundary` pins it.

## Territory

Ownership on a dead square is a **level**, not a flag: how much of its owner's influence reaches it, nought to seven. `sim::rule::territory` is one rule where there were three.

**Living cells are sources**, and so is granted ground. A source reads as full whatever is stored on it — `Cell::influence` is where that lives.

A dead square works out **who is pushing hardest**. Each neighbour adds its influence to its own player's total; a player's net is their total less everybody else's, and the highest net takes the square at whatever that net buys, `rule::LEVEL_SPREAD` a level. A net of nothing leaves the square to nobody. Winning ground, losing it and forgetting it are that one sentence.

### Why a flag could not work

The measurement that killed every threshold rule is still in the history and is the reason for all of this: **a corner of a solid region and a square just outside a straight edge both have exactly three owned neighbours.** No count can tell them apart, so every rule built on one either ate blobs from their corners or grew edges outward for ever. That was never a tuning failure. A boolean field has no gradient, so a square genuinely cannot tell inside from outside, and no arithmetic on eight booleans invents the information.

A level field has one, and those two squares stop looking alike: the corner is surrounded by high numbers, the outside square by low ones. Measured, with a block standing still: a bounded halo of sixty-four squares reading 5, 3, 1 outward from the life, unchanged from generation eighty to four hundred. Ground nobody stands on goes to **nothing** rather than settling into a shape. A glider's halo travels with it and leaves no trail.

### The fall is the only bound

There is no rule about radius anywhere. A source is seven, the fall is two, so influence reaches three squares and a lone blinker holds about thirty. At a fall of one it would reach seven and hold about a hundred and fifty. That one number is the whole feel of the map, and because the field is a pure function of where the sources are, it cannot drift or ratchet outward the way the old one did.

The halo is a **square** rather than a disc, because a cell's neighbourhood is the eight around it and the distance that falls out of that is Chebyshev's. Making it round would mean charging more for a diagonal step, which is a real option and not currently taken.

### A sum, and why it needs a cap

A sum makes reach come from **mass**. The best single neighbour would be a distance field — ground to whoever's life is nearest, a lone cell projecting exactly as far as a colony, a small player holding their half of the line against a large one. A sum is a pressure field: a blob pushes further than a blinker, and a border sits where the weight balances rather than where the distance does.

What a sum cannot do on its own is stop. A square with four neighbours at its own level already sums to more than that level, so the field feeds itself — measured, a block filling a 21×21 window at full strength and still growing after four hundred generations. So a claim is capped at `rule::LEVEL_FALL` below the strongest thing feeding it: the sum decides **who** and **how strongly**, the cap decides **how far**, and mass still buys reach by keeping the sum above the cap for longer.

The fall has to be more than one, because a sum sustains a **plateau**. In a broad patch every neighbour sits at the same level, so a cap one below lets the patch shed a single level per ring — a glider drew a sixteen-square plume that widened as it went back.

Measured, at a spread of six and a fall of two: a block holds thirty-two squares in a graded halo, unchanged from generation eighty to four hundred. Ground nobody stands on goes to **nothing**. A glider holds about ninety, with a wake reaching eight squares behind it and tapering.

Ties go to whoever holds the square and then to the lower number, so two peers agree and a border between matched players does not flicker.

### Rising and ebbing

A claim **rises at once and ebbs a step at a time**, `rule::LEVEL_EBB` per update. Assigning outright in both directions is the tidier rule and gives a glider no wake at all: the square behind it goes from held to nobody's the moment it looks. Ground that drains instead leaves a short thinning trail, which is what something passing through ought to leave — measured, about eight squares behind a glider, tapering 7, 5, 3, 1, against a halo three squares deep for something standing still.

Only downwards. A claim that has arrived is felt immediately, or a frontier would lag behind the life pushing it.

A live cell **stores** full level as well as reading as full, so `level` and `influence` agree on a source rather than one of them being a special case — and so that death is only "stop being a source", with the ground already at full strength to ebb from. Without it a fresh corpse was owned at level nought, which is a state the rule says cannot exist: true again a generation later, and wrong on the screen in between.

### The roll decides the rate, not the outcome

`rule::LEVEL_ADJUST` is how often a dead square works out what reaches it — sixteen in sixty-four. This is the other half of the change: the old roll decided *which owner a square took*, and this one decides *when it looks*.

Recomputed every generation for every square, the field would be an exact distance transform that snaps the instant anything moves, and a glider would drag a geometrically perfect halo behind it. Updating a fraction per generation makes it lag and smear, which is the difference between a country and a Voronoi diagram — and a square that is not updating costs one roll and nothing else. Whenever a square does settle it settles to the same thing, so the roll cannot change the answer, only the moment.

### Granted ground is a spring

`bits::HOME` used to be a carve-out: the one square the decay rule skipped. It is a **source that is not alive** now, said in the same vocabulary as everything else, so a granted patch projects a live gradient whether or not anything survives on it and the rule never works it out from its neighbours.

That is what makes the placing rule safe. Placing is confined to ground your own influence reaches, and the reason a wall was abandoned before was that a player whose life went out could never place again. A spring at home means everybody always has somewhere.

### Fifteen players

Four bits, so 1..=15 are real players and zero goes on meaning unowned — a zeroed cell has to stay a valid empty world, and `Cell::alive` asserts a live cell has an owner. Thirty-one was comfortable as "players a world has ever seen"; fifteen is not, and seat reclamation is [not built yet](planned.md#fifteen-slots-and-more-than-fifteen-clients).

The save format is version 5. Chunk bytes are a raw cast, so a version 4 file read as version 5 is not a corrupt world but a plausible one, wrong in every square — which is what the version byte is for. There is no migration: a flag carries no level.

## Turrets

A turret claims ground at range. Every generation it takes the nearest square that is not its owner's and makes it theirs; a dead turret runs that backwards, taking the nearest square that *is* its owner's and giving it up.

**A live cell must have an owner** — `Cell::alive` asserts it, because unowned life would have nobody to attribute a birth to — so taking a square away from its owner kills whatever was standing on it. That is the whole of why a dead turret kills. It is the invariant, not a rule of its own, and it is the only thing in the game that kills a live cell outside Conway.

### Why it is a pass and not a rule

Every rule in `sim::rule` is a pure function of a cell and its eight neighbours. That is what lets a generation run out of a `Halo` — one flat 66×66 grid per chunk, no bounds checks and no knowledge of topology — and "the nearest square that is not mine" is a search no halo can answer.

So `World::fire_turrets` runs after the rules in absolute coordinates, beside `break_ice_from` and for the same reason: a pane spans chunks, so shattering cannot happen inside `next_cell` either. Turrets fire after the ice and before the prune.

**Searched first, applied second**, which is the same discipline as gathering every halo before writing any of the next generation. Every turret reads the world as the generation left it, so no turret's answer depends on which turret went first. Two aiming at one square either agree or overwrite, and the list is sorted, so which of them wins is the same on every peer.

### Finding them, and the reach

There is no index. `World::turrets` scans what is held the way `ice_cells` does, and **sorts**, because `stored` walks a `HashMap` on an infinite world and a `HashMap` iterates differently in different processes — an unsorted list would let a client and a server disagree about who owns a contested square.

`rule::TURRET_REACH` is the whole cost model. A turret reads the `(2R+1)²` box around itself twice per square it flips — once to find the nearest and once to walk to the one the tie-break picked — so at six that is 338 reads a turret a generation and does not matter, and at twenty-four it is 4802 and a hundred turrets cost more than stepping the world does. It is a **disc**, not a square: the box is what is walked, `d > reach²` is what makes the reach the same in every direction.

`rule::TURRET_POWER` is how many squares it flips, and it multiplies that bill directly. One search per square rather than one search for all of them, each excluding what the last took — nearest-first falls out of that, and it costs a second walk of a box already in cache, where collecting the whole box and sorting it would allocate per turret per generation to answer a question about its first few entries. Each shot mixes its own index into the seed so a volley does not break every tie the same way, and a dead turret gives back the same number it would have taken, so the mirror holds however it is set.

It also bounds how far a turret can wake the world. A turret writes through `set_cell_at`, which makes the chunk if there is not one, and a claim is ownership, so the chunk is no longer empty and `compute_active` picks it up next generation without being told. That only holds while the reach is at most one chunk; further and a turret could write two chunks away, past anything `compute_active` has a way to know about.

### The tie-break

A ring holds many squares at the same distance, and letting the scan order choose between them would have every turret in the world prefer the same direction — territory would grow in a lopsided plume that reads as a bug rather than as a rule. So the choice is a seeded roll on a stream of its own, the way `territory` picks among living owners for `SPREAD`, seeded from the generation and the turret's own position so every peer breaks the same tie without exchanging a number.

Two passes over the box rather than a list of candidates: the first finds the nearest distance and counts how many share it, the second walks to the one the roll picked. That costs a second read of a box already in cache and saves allocating per turret per generation.

### What it will and will not touch

A live turret takes **ground**, so it wants a dead square that is not its owner's. Not the life standing on one: there is a single owner field, so claiming a living cell would hand over the cell rather than the square under it, and territory has never worked that way. Ground nobody holds counts, and so does ground in a chunk that was never allocated — an absent chunk reads as dead and unowned, which is exactly what a turret is for reaching.

A dead turret is the mirror and takes its owner's own squares, alive or not. **`HOME` is exempt**, for the reason it never decays: it is the ground its owner can still build on at the base rate, and a machine of theirs that failed must not be what takes that away.

Ice is exempt either way, and a turret under a pane does not fire at all. A pane stops time over whatever it covers, and that is every rule rather than only the ones inside `rule`.

### What it settles at

`DECAY` at two in sixty-four eats N/32 of what is held, so each square flipped per generation holds about thirty — a turret settles at roughly `30 × TURRET_POWER`, and the block it is really bought as at four times that. Against a neighbour's living colony it is far weaker, and that is the number `TURRET_POWER` is really setting: `SPREAD` gives their life the square straight back at forty in sixty-four, so what a turret holds of contested ground is about `TURRET_POWER × 64 / SPREAD` — one and a half squares at one, six at four. **Below about four a turret claims empty land and cannot press on ground anything is alive on.**

A turret whose whole disc is already its owner's has nothing to take, and falls back to **reinforcing**: the nearest square of theirs that is not at full influence, planted to `rule::TURRET_PUSH`. That square then feeds its neighbours as strongly as life does until the rule works it out again, so a turret in the middle of a country pushes the border through the sum rather than at it.

The order is the whole of why that is safe. Reinforcing as the *only* rule was tried when levels arrived and ruined the piece — influence falls off, so the nearest thin square is a step away and a turret spent its life topping up ground it already held. Asked second, it only fires when there was nobody to push on.

The rule for taking has never changed; the world around it did. Before territory was a level a player's halo was tight, so ground that was not theirs sat within six cells of anywhere they would stand a turret, and the case barely arose. Granted ground is a source now and a country reaches much further.

### Fours

One turret is one live cell with no live neighbours, and it is gone in a generation. So a turret is only ever placed as part of something that survives, and the cheapest thing that survives is the 2×2 block: four turrets, still, never dying and never giving birth, claiming four squares a generation for as long as nothing disturbs them. At `TURRET_POWER` of one that settles at about a hundred and thirty squares, eleven across.

Which is the exact shape that is worthless for a factory. A block of factories never gives birth so never earns — [the game](game.md#manufacture) calls it forty spent on nothing — and it is the best thing a turret can be, because a turret works by standing there. **The still life is a factory's worst shape and a turret's best.**

It also means the inheritance split rarely fires for a turret in its natural form: a block gives no births, so there is no parent to pick. Non-inheritance is for the turret somebody drops into a live pattern, which is the case it exists to stop.

### The corpse

`rule::TURRET_DECAY` returns a dead turret to ordinary ground, four times in sixty-four, the way `MINE_UPKEEP` does for a dead factory — slower, because the two punish different things. A dead factory is a bill that wants a bottom to it; a dead turret is a machine firing backwards over the ground behind it, and four in sixty-four leaves it doing that for about sixteen generations.

Nothing is tallied for it. What a dead turret costs its owner is the ground it hands back and the life it takes with it, and that is applied rather than priced — which is why `Earned` says nothing about turrets and `Halo::step_into` only decays them.

Note what that means read literally: the mirror of "the nearest square that is not the owner's" is "the nearest that is", so a dead turret eats its owner's ground and shoots its owner's life, including the other three cells of its own block. A failing emplacement dismantles itself.

## Overclockers

An **overclocker** makes the ground around it step twice a generation. Every live, ice-free one owns a disc of `rule::OVERCLOCK_REACH` cells, and the union of those discs runs the rule a second time after the whole world has run it once — `rule::OVERCLOCK_RATE` is how many times in all, and two is one extra pass. The machine itself is placed like a turret, in fours, and for the turret's reasons: it does not inherit, because a birth that copied it would let any gun claim the map's clock, and one on its own dies of loneliness in a generation.

### Sub-steps, not a faster tick

A generation is the unit everything else is keyed to — the dice, the `Step` on the wire, the checkpoint, the standings, the save — so "twice a generation" is a question about what a generation *is*, and there were two answers. The other one is a tick half as long with ordinary cells stepping every second one. It has exactly these semantics — on the odd tick only overclocked cells move and they read frozen ordinary neighbours, which is the border rule below — and it costs twice the `Step` broadcasts, a tick doubled in every save and checkpoint, a parity guard in every rule and pass, and every per-generation chance in `rule.rs` halved unless rescaled. So the passes run **inside** `World::step`, and the order there is now: ice seeds, detonation, the whole-world pass, the overclock passes, `generation += 1`, shattering, turrets, prune. Nothing outside `sim` knows: one `Step`, one tick, one digest — and because a pass writes cell bytes, the checkpoint already covers it with no new message.

### What the second pass reads

`World::overclock_pass` finds the discs from the world **as the first pass left it**, so a machine that died this generation does not run again, and one under a pane runs nothing, since a pane stops time over what it covers and that is every rule. It gathers a fresh halo for every chunk a disc touches, all before any cell is written — the discipline the first pass keeps, and for the same reason — and then steps only the masked cells through `Halo::step_into_where`. The mask is a bit per cell, so a disc of a hundred and thirteen cells costs a hundred and thirteen evaluations and a few halo copies rather than a chunk's four thousand per overclocker, and two discs that overlap, or one that wraps onto itself on a small torus, step each cell once.

### The edge

A masked cell reads all eight neighbours from the halo whether they are in the disc or not, and the ones outside are as the first pass left them and this pass will not move. An unmasked cell is neither evaluated nor written, so next generation it sees the disc's *second* state. That is the whole of the border: **the inside runs at twice the clock and the outside sees every other state of it.** A blinker wholly inside is flat again every generation; a gun inside fires twice as often; a glider crossing the ring is torn where it crosses, because the cells it needs on the far side are a state behind. That is a hazard of the piece the way a pane's edge is, and the answer is the same — keep what you care about wholly in or wholly out. `a_pattern_straddling_a_disc_is_deterministic_and_is_torn` pins both halves.

### Dice of its own

A pass rolls from `seed::pass_seed(generation_seed, pass)`, and pass nought is the generation's own seed, so nothing that runs once changed its dice for this existing. A second pass handed the same seed would give every cell the identical roll twice — territory's `LEVEL_ADJUST` would fire on the same squares, a birth would pick the same parent, a factory would pay or not exactly as it just had — which is a correlation nothing would think to look for, and the one fact about running the rule again that is not obvious from the rule.

### What it pays, and what it burns

The second pass is the same rule, so it counts what the rule counts: a factory inside a disc is born twice a generation and pays twice as often, a corpse is charged twice as often, and a dynamite inside burns down twice as fast. Those are prices rather than determinism questions, and they are what an overclocked gun is *for*. Ice seeds are taken once, at the top of the generation, so a cell born beside a pane in the second pass breaks it next generation — the same one-beat lag `life_born_beside_a_pane_breaks_it` pins for the first.

`rule::OVERCLOCK_REACH` is at or under `CHUNK_N`, asserted at compile time, for the turret's reason: a disc that reached further could write two chunks away, past anything `compute_active` has a way to know about. Within it a pass makes the chunk it writes into, so life born at the edge of a disc wakes its chunk next generation without being told.

## Ice

A pane freezes what it covers, whether or not that is alive. Alive and ice are independent: a cell may be either, both, or neither.

The seeds are taken before the rule runs and the flood happens after it, so a cell breaks a pane even in the generation it dies in, while what a pane covered does not evolve in the same breath as being uncovered.

A pane touched by a **living, ice-free** cell shatters, and takes the whole connected run with it — a pane is one object, so cracking a corner does not leave the rest standing. Connectivity is **orthogonal**: panes are laid as rectangles, and two meeting only at a corner are two panes, so a diagonal join would let a break travel between panes that merely touch.

Shattering runs after the rules, in absolute coordinates, so a pane spanning chunks breaks as one.

One consequence is emergent rather than designed, and worth knowing: **a frozen cell still counts as a neighbour**, so a pane laid *exactly* on a pattern causes life to be born immediately outside itself, and that newborn breaks the pane.

Note the "exactly". Give the pane one cell of margin and every cell that could be born from the frozen pattern lies inside the pane, where the rule returns it unchanged — so **a pane with a margin is not broken by what it covers**. That is what makes ice work as scaffolding: seal a region and the half-built pattern inside it will not break its own cover.

When a pane goes, **the ice flag is the only thing that changes**. What was under it — alive, dead, and whose — is exactly as it was, which is what makes a pane a schematic rather than a lid: draw the pattern frozen over as many generations as it takes, and it starts living the instant the cover breaks. `shattering_leaves_what_was_under_it_exactly_as_it_was` pins it.

It buys time, not safety. Anything alive arriving from elsewhere breaks it on contact, and a glider is the cheapest way to reach a pane nothing can get beside — `a_glider_shatters_a_pane_it_reaches` flies one in and watches the whole run go. Sealing a pattern in protects it from itself and from nothing else.

The rule is that simple and has one exception. **Any live cell in the eight neighbours breaks a pane** — placed by a player or born of the rule, yours or anyone's, one cell or a glider. The exception is a cell that is itself under ice: it is frozen, and a pane must not be broken by what it covers, or no pane could be laid over life at all. That is the whole of the seed test in `World::shatter_ice`.

## Worlds

`World::infinite` is an unbounded plane. `World::toroidal(rows, cols)` is a fixed grid in one contiguous allocation, with coordinates wrapping.

The world opens empty. Every player brings a 2×2 block on granted ground, so what a join produces is another player's territory and block appearing — which a client starting from nothing cannot invent, and which is what the Gosper guns used to be there to prove.
