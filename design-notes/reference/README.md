# Reference code

Everything here was compiled and run. It is not part of the game — it exists so the claims in the notes can be re-checked rather than trusted.

## `layout-probe/`

A standalone crate asserting the memory-layout properties in [../01-cell-layout.md](../01-cell-layout.md).

```
cargo run --bin layout-probe  # size/align, row-major order, byte aliasing, mem::swap
cargo run --bin nibble    # 4+4 nibble packing, CPU view vs shader view
cargo run --bin two       # repr(C) two-byte cell vs u16 newtype, endianness hazard
```

Flip `pub const N` in `src/main.rs` between 16 and 256 — every assertion holds at both, which is the point.

## `tiling/`

```
python3 find_domain.py    # searches for an F-pentomino translational fundamental domain
python3 verify_lut.py     # confirms it lifts to a valid plane tiling; checks the O(1) LUT
python3 show_tiling.py    # prints a patch of the tiling and the rotations in use
```

`verify_lut.py` is the one that matters: it rebuilds the tiling from first principles and compares 5,760 chunks against the ten-entry lookup table in [../03-world-topology.md](../03-world-topology.md).
