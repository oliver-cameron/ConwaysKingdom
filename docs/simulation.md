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
| kind | G bits 2..8 | what it is |
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

Every non-empty chunk, plus any neighbour it carries life towards. A chunk with no life on the edge facing its neighbour cannot cause a birth there.

## Territory

A dead cell next to living ones takes the owner of one of them, chosen by the seed, most generations. It stays dead: the rule sets the owner and nothing else. Ice is checked first, so a pane's cover is not claimed while it stands.

**And it creeps, and it fades.** Where nothing alive is touching a dead cell, it takes the owner of a neighbouring cell — **whoever that is, including nobody**. That one rule does both jobs, which is why there is no threshold anywhere trying to work out what a shape is:

- Deep inside a region every neighbour agrees, so nothing moves.
- At an edge, a cell with five owned neighbours and three empty ones has five-in-eight odds of staying and three-in-eight of going, and the square just outside it has the same odds the other way. The border is an unbiased walk: it neither runs away nor rots.
- A thin trail is nearly all empty neighbours, so it goes quickly.

Every threshold tried failed, and for a reason worth writing down: **a corner of a solid region and a cell just outside a straight edge both have exactly three owned neighbours.** No count of neighbours can tell them apart, so any rule built on one either erodes every blob from its corners or grows every edge outward forever. Measured: at four of eight a glider's trail reached 454 squares and climbing; at three of eight an abandoned patch grew from 169 to 807.

On top of that, a slow **decay** — `rule::DECAY`, two in sixty-four — so ground nothing lives on eventually goes rather than settling into a shape and staying. Without it, a patch nobody has touched for four hundred generations is still two hundred squares; with it, none.

Territory used to only ever spread, which meant a glider left a permanent trail and an infinite world grew for as long as anything moved. A glider crossing four hundred generations used to hold twenty-five chunks and climbing, for five live cells.

Life holds the ground around it, because a square touching something alive is claimed by the rule above before creep or decay ever see it. So territory is a halo around your life that fluctuates at its edge — `cargo run --no-default-features --example territory` draws it.

The exception is **granted ground**, marked by `bits::HOME` and exempt. Without a floor, a player whose life died out would lose every square they had — and placing is confined to your own territory, so they could never place again. The mark is on the *square*, not the lineage, which is why a birth keeps the dead cell's copy of it while taking everything else from its parent. It travels with the ground when the ground changes hands.

This makes ownership meaningful on dead ground, which changes what an empty chunk is. `Chunk::is_empty` now asks about ownership as well as life and ice — without that, `prune` would drop a chunk on the very step its ground was claimed, and territory outside a chunk that also held life could never last a generation. The cost is that an infinite world grows with territory as well as with life, and there is no die-off yet, so it only grows.

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
