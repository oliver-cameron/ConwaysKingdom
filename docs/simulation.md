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
| player  | spare |       |    kind     |I |A |
 7 6 5 4 3  2 1 0          7 6 5 4 3 2   1  0
```

| field | where | meaning |
|---|---|---|
| alive | G bit 0 | living or not |
| ice | G bit 1 | a pane covers it; independent of alive |
| kind | G bits 2..8 | what it is — see `kinds!` |
| home | R bit 0 | granted ground, which never decays |
| spare | R bits 1..3 | nothing yet |
| player | R bits 3..8 | owner, 0 = unowned, so 31 players |

**Byte 1 is the tile this cell draws**, whole and unshifted: low nibble across the sheet, high nibble down it. Alive and ice sit in its bottom bits and the kind in the rest, so a kind's four states are four consecutive tiles and finding a cell's picture is arithmetic rather than a lookup. There is no layer to choose and no UV to carry — what a cell looks like is one number.

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

The newborn takes **everything** from that parent and nothing from the corpse it lands on. That is not a detail: it is how a kind travels. A mine's children are mines, and because the parent is chosen at random rather than by vote, a kind spreads through a mixed population instead of being handed down whole — one mine beside two ordinary cells wins about a third of the births there, and drifts from that. Three mines placed beside a starting block converted a whole growing colony inside thirty generations.

**Except for the kinds that are not inherited, which pass over ownership alone.** Pick a turret as the parent and the newborn is ordinary life belonging to the turret's owner: the ground still changes hands and the machine does not copy itself. Kinds are declared in one list, `kinds!` in `sim::cell`, which writes `Kind::ALL`, the count and `Kind::inherits` from the same rows for the same reason `rules!` writes the rule chain and its names — so what a kind is, what it looks like and whether it travels cannot drift apart.

The split is what the two kinds are *for*. **An inheriting kind is an investment in a lineage; a non-inheriting one is a machine somebody placed.** A mine is bought once and spreads, so what was paid for is the lineage. A turret works by standing where it is put rather than by breeding, and a turret whose children were turrets would make any gun a factory that claims the map — so it is bought once per cell, forever, and costs more for it.

The carve-out is applied **after** the roll rather than before it, so which parent is chosen does not depend on what kind it turned out to be. Every peer must reach the same parent from the same seed whatever is standing there; `not_inheriting_does_not_move_the_roll` pins it.

Ice is cleared on a birth because a parent may be *under a pane* and still count as a live neighbour while frozen. Without that, a cell born outside the pane would inherit it.

### Mines, and what the rule counts

`Kind::MINE` pays its owner when one of its kind is **born**, and costs its owner **once** for each corpse it leaves — `rule::MINE_UPKEEP`, sixteen times in sixty-four, and when the charge falls due the square loses its kind and is ordinary ground. The rule does not know what either is worth: it counts them, per player, and hands the tally back from `World::step` as a `Mined`, which is two arrays rather than one net figure so the two can be priced apart. The rule decides how *often* a corpse is charged and `net` decides how *much*.

Note what `upkeep` counts. Not deaths — charges falling due. A corpse reborn before its charge lands escapes it entirely, which is the whole of why a blinker pays and a glider does not: one re-uses its ground and the other abandons it.

Counted inside `Halo::step_into`, which is the one place that holds a cell before and after in the same breath, so it costs a comparison and no second pass over the world. The tally is returned rather than applied, because a world holds cells and not purses: the server folds it into the authoritative values and a client folds it into its predicted one.

What that rewards is a **machine that stays where you put it**. A blinker is three cells and two corpses and pays; a glider drags twenty corpses behind it and bleeds; an r-pentomino of sprawl bleeds badly. A block of mines neither earns nor costs, because nothing is ever born on it and nothing ever dies.

The drain is bounded by territory decay rather than by a timer: a corpse with nothing alive beside it loses its owner soon enough and stops costing anybody, while corpses inside a living colony are re-claimed every generation and go on costing.

## Chunks

A chunk is 16×16 cells. Neighbours are **computed** from a coordinate, never stored:

- infinite: coordinate arithmetic, and an absent chunk reads as dead;
- toroidal: `rem_euclid`, so global coordinates fold onto chunks many-to-one.

Computing rather than storing is what lets a chunk be its own neighbour, which happens on any torus smaller than 3×3. A graph of `Rc<RefCell<Chunk>>` cannot express that: gathering the neighbourhood would borrow one cell twice and panic.

### The halo

Stepping copies a chunk and the facing strip of each of its eight neighbours into a flat padded grid, `(16+2)²`. That gives the inner loop one array with no bounds checks and no knowledge of topology, and an absent or empty neighbour simply contributes nothing.

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

A dead cell next to living ones takes the owner of one of them, chosen by the seed, most generations. It stays dead: the rule sets the owner and nothing else. Ice is checked first, so a pane's cover is not claimed while it stands.

**`SPREAD` does not spread territory — `CREEP` does**, and the names are the wrong way round for what people expect of them. Spread only ever acts on a square something is **alive** beside, and the two branches are disjoint: a square with a living neighbour takes that rule and returns, so creep and decay never see it. Turn creep off and territory stops expanding altogether, however high spread goes, because nothing can then put your number on a square nothing of yours is alive next to. What you are left with is the **footprint of life**: the union of every square something of yours has ever been alive beside, growing exactly as fast as your patterns reach new ground and not at all if they sit still. Measured with creep and decay at zero, a block holds 169 squares at generation 0 and 169 at generation 400, while a glider goes from 169 to 902 — a diagonal stripe, one cell of halo either side of where it flew.

Spread is also a **transfer** rather than only a gain: the owners it picks between are those of every living neighbour whoever they belong to, so a dead square of yours touching somebody else's life becomes theirs at the same rate. With creep and decay at zero that is the only way the rule moves ground at all.

**And it creeps, and it fades.** Where nothing alive is touching a dead cell, it takes the owner of a neighbouring cell — **whoever that is, including nobody**. That one rule does both jobs, which is why there is no threshold anywhere trying to work out what a shape is:

- Deep inside a region every neighbour agrees, so nothing moves.
- At an edge, a cell with five owned neighbours and three empty ones has five-in-eight odds of staying and three-in-eight of going, and the square just outside it has the same odds the other way. The border is an unbiased walk: it neither runs away nor rots.
- A thin trail is nearly all empty neighbours, so it goes quickly.

Every threshold tried failed, and for a reason worth writing down: **a corner of a solid region and a cell just outside a straight edge both have exactly three owned neighbours.** No count of neighbours can tell them apart, so any rule built on one either erodes every blob from its corners or grows every edge outward forever. Measured: at four of eight a glider's trail reached 454 squares and climbing; at three of eight an abandoned patch grew from 169 to 807.

On top of that, a slow **decay** — `rule::DECAY`, two in sixty-four — so ground nothing lives on eventually goes rather than settling into a shape and staying. Without it, a patch nobody has touched for four hundred generations is still two hundred squares; with it, none.

Territory used to only ever spread, which meant a glider left a permanent trail and an infinite world grew for as long as anything moved. A glider crossing four hundred generations used to hold twenty-five chunks and climbing, for five live cells.

Life holds the ground around it, because a square touching something alive is claimed by the rule above before creep or decay ever see it. So territory is a halo around your life that fluctuates at its edge — `cargo run --no-default-features --example territory` draws it.

The exception is **granted ground**, marked by `bits::HOME` and exempt. Without a floor, a player whose life died out would lose every square they had, and with it the only ground they can build on at the base rate — placing outside costs `rule::OUTSIDE_MULTIPLIER` times as much, so they would not be locked out, but a hundred of value would buy them ten cells. The mark is on the *square*, not the lineage, which is why a birth keeps the dead cell's copy of it while taking everything else from its parent. It travels with the ground when the ground changes hands.

This makes ownership meaningful on dead ground, which changes what an empty chunk is. `Chunk::is_empty` now asks about ownership as well as life and ice — without that, `prune` would drop a chunk on the very step its ground was claimed, and territory outside a chunk that also held life could never last a generation. The cost is that an infinite world grows with territory as well as with life, and there is no die-off yet, so it only grows.

## Turrets

A turret claims ground at range. Every generation it takes the nearest square that is not its owner's and makes it theirs; a dead turret runs that backwards, taking the nearest square that *is* its owner's and giving it up.

**A live cell must have an owner** — `Cell::alive` asserts it, because unowned life would have nobody to attribute a birth to — so taking a square away from its owner kills whatever was standing on it. That is the whole of why a dead turret kills. It is the invariant, not a rule of its own, and it is the only thing in the game that kills a live cell outside Conway.

### Why it is a pass and not a rule

Every rule in `sim::rule` is a pure function of a cell and its eight neighbours. That is what lets a generation run out of a `Halo` — one flat 18×18 grid per chunk, no bounds checks and no knowledge of topology — and "the nearest square that is not mine" is a search no halo can answer.

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

Which means a turret inside its owner's ground finds everything within reach already theirs and idles. It only ever works from a frontier, and nothing had to be written to make that true.

### Fours

One turret is one live cell with no live neighbours, and it is gone in a generation. So a turret is only ever placed as part of something that survives, and the cheapest thing that survives is the 2×2 block: four turrets, still, never dying and never giving birth, claiming four squares a generation for as long as nothing disturbs them. At `TURRET_POWER` of one that settles at about a hundred and thirty squares, eleven across.

Which is the exact shape that is worthless for a mine. A block of mines never gives birth so never earns — [the game](game.md#mining) calls it forty spent on nothing — and it is the best thing a turret can be, because a turret works by standing there. **The still life is a mine's worst shape and a turret's best.**

It also means the inheritance split rarely fires for a turret in its natural form: a block gives no births, so there is no parent to pick. Non-inheritance is for the turret somebody drops into a live pattern, which is the case it exists to stop.

### The corpse

`rule::TURRET_DECAY` returns a dead turret to ordinary ground, four times in sixty-four, the way `MINE_UPKEEP` does for a dead mine — slower, because the two punish different things. A dead mine is a bill that wants a bottom to it; a dead turret is a machine firing backwards over the ground behind it, and four in sixty-four leaves it doing that for about sixteen generations.

Nothing is tallied for it. What a dead turret costs its owner is the ground it hands back and the life it takes with it, and that is applied rather than priced — which is why `Mined` says nothing about turrets and `Halo::step_into` only decays them.

Note what that means read literally: the mirror of "the nearest square that is not the owner's" is "the nearest that is", so a dead turret eats its owner's ground and shoots its owner's life, including the other three cells of its own block. A failing emplacement dismantles itself.

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
