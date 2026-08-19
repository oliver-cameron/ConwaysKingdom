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

Four bytes, little-endian, uploaded as `Rgba8Uint`.

```
 15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
|   player    |F |G |       kind        | A |     <- R and G
                                                  <- B and A: the tile's u, v
```

| field | bits | meaning |
|---|---|---|
| alive | 0 | living or not |
| kind | 1..9 | what it is, and the index of its sprite |
| glass | 9 | a pane covers it; independent of alive |
| flags | 10 | spare |
| player | 11..16 | owner, 0 = unowned, so 31 players |
| u, v | bytes 2 and 3 | which tile of its sheet it draws |

The player sits at the top of the word, so extracting it is a shift with no mask, and raw cell values order by player first.

Stored as `[u8; 4]` rather than a `u16` and two `u8`s, so the byte order is ours rather than the host's **and alignment stays 1** — which is what lets a chunk be cast straight out of a save file or a wire frame at any offset. A `u16` field would force alignment 2 and panic on an odd offset.

A zeroed cell is dead, unowned and unglassed, so zeroed memory is a valid empty world. Never give bit 0 clear a live meaning.

`Cell::alive` asserts the player is non-zero: **a live cell always has an owner**, because unowned life would have nobody to attribute a birth to.

## The rules

`sim::rule` evaluates one cell at a time, given whole neighbours rather than a count, so a rule can branch on what a cell *is*:

```rust
cell.update(&neighbours, seed) -> Cell
```

- A glassed cell is returned unchanged. Checked before the kind, so a pane freezes anything without every kind having to remember to honour it.
- Otherwise Conway. Survival and death change **only the alive bit**, so a dead cell keeps its owner and metadata — "recently died, and whose it was" exists without a field for it. Those corpses are inert: nothing counts a dead cell.
- A birth sets the owner, because it has none to keep.

### Whose birth is it

At random, from the three parents — but seeded, from the generation and the chunk coordinate with each cell mixing in its own position, through SplitMix64's finaliser. Every peer rolls the same number without exchanging one. All three parents are reachable and the same seed always gives the same answer; both are tested.

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

## Glass

A pane freezes what it covers, whether or not that is alive. Alive and glass are independent: a cell may be either, both, or neither.

A pane touched by a **living, unglassed** cell shatters, and takes the whole connected run with it — a pane is one object, so cracking a corner does not leave the rest standing. Connectivity is **orthogonal**: panes are laid as rectangles, and two meeting only at a corner are two panes, so a diagonal join would let a break travel between panes that merely touch.

Shattering runs after the rules, in absolute coordinates, so a pane spanning chunks breaks as one.

One consequence is emergent rather than designed, and worth knowing: **a frozen cell still counts as a neighbour**, so a pane laid over life causes life to be born around itself, and that newborn breaks the pane. Glass shelters what is under it without sealing it off from the world. If that is not wanted, the change is to stop frozen cells counting as neighbours, in `Halo::step_into`.

## Worlds

`World::infinite` is an unbounded plane. `World::toroidal(rows, cols)` is a fixed grid in one contiguous allocation, with coordinates wrapping.

`World::demo` is what the app and server open with: two Gosper glider guns owned by different players, facing each other so their streams collide. Guns rather than a still life for a specific reason — a still life or an oscillator looks identical whether it arrived from the server or the client regenerated it, so it cannot tell a working join from a broken one. A gun's trail of gliders says how long the server has been up, and a client starting from nothing cannot invent it.
