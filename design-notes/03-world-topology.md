# World topology and the F-pentomino partition

The world is an ordinary infinite square lattice of chunks. The F-pentomino tiling is a **partition drawn over that lattice**, not a topology — the pentominoes rotate, but the cells underneath them never do. Nothing in the simulation or the renderer needs to know a pentomino exists.

This matters because the alternative reading is expensive. If pentominoes were glued together abstractly to form a non-planar space, every adjacency edge would need to carry a transform from the dihedral group of the square, the fragment shader would need to apply it when reading, and arbitrary gluings introduce curvature at vertices so the world would not lay flat without seams. None of that is needed here.

## The tiling

Searched for a translational fundamental domain and verified the result lifts to a genuine plane tiling rather than being a torus-wraparound artifact.

- **5x2 fundamental domain**, ten chunks, two pentominoes.
- **Rotations only** — no reflections required.
- Only **two of the four rotations** appear, and they are 180 degrees apart:

```
  orientation A     orientation B
     .##               .#.
     ##.               .##
     .#.               ##.
```

Checked over a 30x30 region: every chunk covered exactly once, every piece a genuine F rotation. A patch, each letter one pentomino:

```
    A A B B C C D D E E F F G G
    A B B C C D D E E F F G G H
    A J B K C L D M E N F O G P
    R J J K K L L M M N N O O P
    J J K K L L M M N N O O P P
    S S T T U U V V W W X X Y Y
    S T T U U V V W W X X Y Y Z
    S b T c U d V e W f X g Y h
    j b b c c d d e e f f g g h
    b b c c d d e e f f g g h h
```

Period 5 vertically, period 2 horizontally, interlocking in a pinwheel.

## Membership is a ten-entry lookup

The domain holds exactly ten chunks and exactly two pentominoes of five chunks each, so there is a bijection between residue classes mod (5,2) and (piece, chunk-within-piece). Pentomino identity is therefore O(1) with no traversal and nothing stored.

```rust
/// LUT[row.rem_euclid(5)][col.rem_euclid(2)] -> (piece, dr, dc)
/// where (dr, dc) is this chunk's offset from its pentomino's lattice anchor.
const LUT: [[(u8, i32, i32); 2]; 5] = [
    [(0, 0, 0), (0, 0,  1)],
    [(0, 1, 0), (0, 1, -1)],
    [(0, 2, 0), (1, 2,  1)],
    [(1, 3, 2), (1, 3,  1)],
    [(1, 4, 0), (1, 4,  1)],
];

/// Which pentomino owns the chunk at (row, col)?
pub fn pentomino_of(row: i32, col: i32) -> (i32, i32, u8) {
    let (piece, dr, dc) = LUT[row.rem_euclid(5) as usize][col.rem_euclid(2) as usize];
    ((row - dr).div_euclid(5), (col - dc).div_euclid(2), piece)
}
```

Verified against the tiling rebuilt from first principles: 5,760 chunks checked, zero mismatches, every fully-sampled pentomino exactly five chunks. `rem_euclid`/`div_euclid` rather than `%` and `/` because chunk coordinates go negative on an infinite grid.

## Adjacency

Keep a neighbour reference on each chunk rather than inlining coordinate arithmetic. It costs nothing and it is what makes the other topologies cheap: an unbounded plane and a torus differ only in what an edge neighbour resolves to, so switching between them becomes a world-generation change rather than a simulation rewrite.

```rust
pub struct Chunk {
    pub cells:    Box<[Cell; CELLS]>,
    pub next:     Box<[Cell; CELLS]>,
    pub edges:    [Option<ChunkId>; 8],   // None == unloaded, reads as dead
    pub all_dead: bool,                   // skip simulation entirely
}
```

No transform field. A pentomino partition never needs one.

The simulation then splits cleanly: interior cells index directly with no bounds checks and no topology awareness, and only the border strips consult `edges`. That also removes the class of bug currently in `src/cell.rs`, because the interior loop is never handed an out-of-range coordinate and the border path returns dead for `None` rather than panicking.

## If the glued reading is ever wanted

The machinery would be an `EdgeTransform` on each adjacency edge covering the eight symmetries of the square, a matching `apply_frame` in the fragment shader, and a breadth-first placement walk from the camera's chunk accumulating screen offset and composed transform. A chunk visited twice — as on a torus smaller than the viewport — simply emits two instances. It is about eight lines of shader and a modest amount of graph code, but it is not needed for a planar partition and should not be built speculatively.
