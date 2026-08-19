use bytemuck::{Pod, Zeroable};

use super::dir::Dir;
use super::player::PlayerId;
use super::rule::{mix, next_cell, Neighbours};
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell: sixteen bits, little-endian.
///
/// ```text
///  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
/// |   player    |F |G |       kind        | A |
/// ```
///
/// Bit 0 is alive, bits 1..9 the kind, bits 9..11 flags. The top five are the
/// player,
/// and being the top field means the number extracts with a single shift and
/// no mask, and that comparing two raw cells orders them by player first.
///
/// Four bytes per cell, uploaded as `Rgba8Uint`: the sixteen bits above in R
/// and G, then **U and V in B and A** — the tile this cell shows from its
/// sprite sheet. A sheet is a 16x16 grid of 16x16 tiles, so a `u8` each is
/// ample, and a structure spanning several cells gives each one a different
/// tile so the parts line up.
///
/// Stored as explicit bytes rather than a `u16` and two `u8`s, so the order is
/// ours rather than the host's, and so alignment stays 1 — which is what lets a
/// chunk be cast straight out of a save file or a wire frame at any offset.
///
/// A zeroed cell is dead and unowned, which is what makes zeroed memory a valid
/// empty world. Never give bit 0 clear a live meaning.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct Cell(pub [u8; 4]);

/// The bit layout. Adjust here and in the matching block at the top of
/// `render/shaders/grid.wgsl` — the shader cannot read Rust constants, so those
/// are the one thing kept in step by hand.
pub mod bits {
    /// Bit 0: alive or dead.
    pub const ALIVE: u16 = 1;

    /// Bits 1..9: what kind of cell this is. Also the index of its sprite, so
    /// every kind necessarily has art — see `render::atlas`.
    pub const KIND_SHIFT: u16 = 1;
    pub const KIND_WIDTH: u16 = 8;
    pub const KIND_MASK: u16 = (1 << KIND_WIDTH) - 1;

    /// Bits 9..11: flags.
    pub const FLAG_SHIFT: u16 = 9;
    pub const FLAG_WIDTH: u16 = 2;
    pub const FLAG_MASK: u16 = (1 << FLAG_WIDTH) - 1;

    /// A pane covers this cell. Independent of `ALIVE`: a cell may be alive,
    /// glassed, both, or neither. Glass freezes what it covers, so the rule
    /// returns such a cell unchanged.
    pub const FLAG_GLASS: u16 = 1 << 9;

    /// Bits 11..16: player number, at the top of the word.
    pub const PLAYER_SHIFT: u16 = 11;
    pub const PLAYER_WIDTH: u16 = 5;
    pub const PLAYER_MASK: u16 = (1 << PLAYER_WIDTH) - 1;
}

const _: () = {
    assert!(size_of::<Cell>() == 4 && align_of::<Cell>() == 1);
    // The fields must tile all sixteen bits with no overlap and no gap.
    assert!(bits::ALIVE == 1);
    assert!(bits::KIND_SHIFT == 1);
    assert!(bits::FLAG_SHIFT == bits::KIND_SHIFT + bits::KIND_WIDTH);
    assert!(bits::PLAYER_SHIFT == bits::FLAG_SHIFT + bits::FLAG_WIDTH);
    assert!(bits::FLAG_GLASS == 1 << bits::FLAG_SHIFT);
    assert!(bits::PLAYER_SHIFT + bits::PLAYER_WIDTH == 16);
    // R16Uint is read little-endian by the GPU.
    assert!(cfg!(target_endian = "little"));
};

impl Cell {
    pub const DEAD: Self = Self([0, 0, 0, 0]);

    /// Keeps the UV: changing what a cell *is* should not move which tile it
    /// draws, or a pane would scramble its picture every generation.
    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        let [lo, hi] = bits.to_le_bytes();
        Self([lo, hi, 0, 0])
    }

    #[inline]
    pub const fn bits(self) -> u16 {
        u16::from_le_bytes([self.0[0], self.0[1]])
    }

    /// Replace the sixteen bits, keeping the UV.
    #[inline]
    const fn set_bits(self, bits: u16) -> Self {
        let [lo, hi] = bits.to_le_bytes();
        Self([lo, hi, self.0[2], self.0[3]])
    }

    /// Which tile of its sheet this cell draws, as (u, v).
    #[inline]
    pub const fn uv(self) -> (u8, u8) {
        (self.0[2], self.0[3])
    }

    #[inline]
    pub const fn with_uv(self, u: u8, v: u8) -> Self {
        Self([self.0[0], self.0[1], u, v])
    }

    /// A live cell belonging to `player`.
    ///
    /// Player zero means unowned, and unowned life would have nobody to
    /// attribute a birth to, so the invariant is checked here rather than
    /// discovered later as a cell nobody can claim.
    pub const fn alive(player: PlayerId) -> Self {
        assert!(player.is_owned(), "a live cell must have a non-zero player");
        Self::DEAD.set_bits(bits::ALIVE).with_player(player)
    }

    #[inline]
    pub const fn is_alive(self) -> bool {
        self.bits() & bits::ALIVE != 0
    }

    /// The player field is the top of the word, so this is a shift with no
    /// mask — and arithmetic on the result is ordinary arithmetic.
    #[inline]
    pub const fn player(self) -> PlayerId {
        PlayerId((self.bits() >> bits::PLAYER_SHIFT) as u8)
    }

    /// What kind of cell this is, which is also its sprite index.
    #[inline]
    pub const fn kind(self) -> Kind {
        Kind(((self.bits() >> bits::KIND_SHIFT) & bits::KIND_MASK) as u8)
    }

    #[inline]
    pub const fn flags(self) -> u16 {
        (self.bits() >> bits::FLAG_SHIFT) & bits::FLAG_MASK
    }

    /// Under glass, and therefore not updating: a pane stops time inside
    /// itself. Says nothing about whether the cell is alive.
    #[inline]
    pub const fn is_glass(self) -> bool {
        self.bits() & bits::FLAG_GLASS != 0
    }

    #[inline]
    pub const fn with_alive(self, alive: bool) -> Self {
        if alive {
            self.set_bits(self.bits() | bits::ALIVE)
        } else {
            self.set_bits(self.bits() & !bits::ALIVE)
        }
    }

    #[inline]
    pub const fn with_player(self, player: PlayerId) -> Self {
        let cleared = self.bits() & !(bits::PLAYER_MASK << bits::PLAYER_SHIFT);
        self.set_bits(cleared | ((player.0 as u16 & bits::PLAYER_MASK) << bits::PLAYER_SHIFT))
    }

    #[inline]
    pub const fn with_kind(self, kind: Kind) -> Self {
        let cleared = self.bits() & !(bits::KIND_MASK << bits::KIND_SHIFT);
        self.set_bits(cleared | ((kind.0 as u16 & bits::KIND_MASK) << bits::KIND_SHIFT))
    }

    #[inline]
    pub const fn with_glass(self, glass: bool) -> Self {
        if glass {
            self.set_bits(self.bits() | bits::FLAG_GLASS)
        } else {
            self.set_bits(self.bits() & !bits::FLAG_GLASS)
        }
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("alive", &self.is_alive())
            .field("kind", &self.kind().0)
            .field("glass", &self.is_glass())
            .field("uv", &self.uv())
            .field("player", &self.player().0)
            .finish()
    }
}

/// What a cell is. The number is also the index of the cell's sprite, so a
/// kind cannot exist without art — `render::atlas` asserts every one is drawn.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Pod, Zeroable)]
pub struct Kind(pub u8);

impl Kind {
    /// An ordinary living cell.
    pub const NORMAL: Self = Self(0);
    /// Every kind. Each must have art at its own index in `render::atlas`;
    /// extend this and the sprite list beside it, or it will not compile.
    pub const ALL: [Self; 1] = [Self::NORMAL];
    pub const COUNT: usize = Self::ALL.len();
}

/// A chunk's cells, row-major. The first index is the texture's Y, the second
/// its X — the only way in is `chunk[(row, col)]`, so that cannot drift.
#[repr(transparent)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Chunk {
    cells: [Cell; CHUNK_CELLS],
}

impl Chunk {
    /// Allocated zeroed straight onto the heap; every cell dead and unowned.
    pub fn zeroed() -> Box<Self> {
        bytemuck::zeroed_box()
    }

    /// Every cell dead and unowned, by value. Cheap at this chunk size; use
    /// `zeroed` once a chunk is too large to build on the stack.
    pub fn dead() -> Self {
        Self { cells: [Cell::DEAD; CHUNK_CELLS] }
    }

    /// Nothing here worth keeping: no life, and no structure either.
    ///
    /// Not simply "nothing alive". A chunk holding only panes still holds
    /// something, and dropping it would destroy them for good, since a
    /// recreated chunk comes back zeroed.
    ///
    /// Nor is it "every cell exactly `DEAD`". A cell keeps its owner when it
    /// dies, so a chunk life has passed through is full of non-zero corpses.
    /// Those are inert -- nothing counts a dead cell, and a birth takes its
    /// owner from live neighbours -- so discarding them changes nothing, and
    /// refusing to would let an infinite world grow without bound again.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| !c.is_alive() && !c.is_glass())
    }

    /// Exactly the `&[u8]` `Queue::write_texture` wants. No conversion.
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }

    pub const fn bytes_per_row() -> u32 {
        CHUNK_N as u32 * size_of::<Cell>() as u32
    }

    /// Read with no bounds panic: anything outside this chunk is an unloaded
    /// neighbour, and an unloaded neighbour reads as dead.
    #[inline]
    pub fn get(&self, row: i32, col: i32) -> Cell {
        if row < 0 || col < 0 || row >= CHUNK_N as i32 || col >= CHUNK_N as i32 {
            return Cell::DEAD;
        }
        self[(row as usize, col as usize)]
    }

    /// Conway's rules for an isolated chunk: every neighbour unloaded, so the
    /// border reads as dead. Equivalent to stepping a halo with a dead border.
    pub fn step(&self, next: &mut Chunk) {
        let mut halo = Halo::dead();
        halo.set_centre(self);
        halo.step_into(next, 0);
    }
}

pub const HALO_N: usize = CHUNK_N + 2;

/// A chunk plus a one-cell border copied from its eight neighbours.
///
/// Gathering into this first means the generation step reads one flat grid
/// with no bounds checks and no knowledge of chunk topology — and it sidesteps
/// the borrow problem, since the halo is owned data built from shared borrows
/// before anything is mutated.
#[derive(Clone, Copy)]
pub struct Halo {
    cells: [Cell; HALO_N * HALO_N],
}

impl Halo {
    pub fn dead() -> Self {
        Self { cells: [Cell::DEAD; HALO_N * HALO_N] }
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> Cell {
        self.cells[row * HALO_N + col]
    }

    #[inline]
    pub fn set(&mut self, row: usize, col: usize, cell: Cell) {
        self.cells[row * HALO_N + col] = cell;
    }

    /// Copy a chunk's cells into the halo's interior, leaving the border alone.
    pub fn set_centre(&mut self, chunk: &Chunk) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                self.set(row + 1, col + 1, chunk[(row, col)]);
            }
        }
    }

    /// Step every cell. `seed` identifies this chunk at this tick; each cell
    /// mixes its own position in, so the pseudo-randomness a birth uses is the
    /// same on every peer without any of them exchanging a number.
    pub fn step_into(&self, next: &mut Chunk, seed: u64) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let (hr, hc) = (row + 1, col + 1);
                let cell_seed = mix(seed, (row as u64) << 32 | col as u64);
                next[(row, col)] =
                    next_cell(self.get(hr, hc), &self.neighbours(hr, hc), cell_seed);
            }
        }
    }

    /// The eight cells around a halo position, in `Dir::ALL` order.
    #[inline]
    fn neighbours(&self, hr: usize, hc: usize) -> Neighbours {
        let mut out = [Cell::DEAD; 8];
        for (i, dir) in Dir::ALL.iter().enumerate() {
            let (dr, dc) = dir.delta();
            out[i] = self.get(
                (hr as i32 + dr) as usize,
                (hc as i32 + dc) as usize,
            );
        }
        out
    }
}

impl Index<(usize, usize)> for Chunk {
    type Output = Cell;
    #[inline]
    fn index(&self, (row, col): (usize, usize)) -> &Cell {
        // Without this, a column past the edge lands in the next row instead
        // of panicking: row * CHUNK_N + col stays inside the array, so the
        // write silently aliases another cell.
        debug_assert!(row < CHUNK_N && col < CHUNK_N, "({row}, {col}) is outside the chunk");
        &self.cells[row * CHUNK_N + col]
    }
}

impl IndexMut<(usize, usize)> for Chunk {
    #[inline]
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Cell {
        debug_assert!(row < CHUNK_N && col < CHUNK_N, "({row}, {col}) is outside the chunk");
        &mut self.cells[row * CHUNK_N + col]
    }
}

const _: () = {
    assert!(size_of::<Chunk>() == CHUNK_CELLS * size_of::<Cell>());
};

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(cells: &[(usize, usize)]) -> Box<Chunk> {
        let mut c = Chunk::zeroed();
        for &(r, k) in cells {
            c[(r, k)] = Cell::alive(PlayerId(1));
        }
        c
    }

    fn live(c: &Chunk) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for r in 0..CHUNK_N {
            for k in 0..CHUNK_N {
                if c[(r, k)].is_alive() {
                    v.push((r, k));
                }
            }
        }
        v
    }

    #[test]
    fn every_edge_and_corner_steps_without_panicking() {
        // Life on all four edges and all four corners: the cases the old
        // match-on-(dx, dy, x, y) fell through to an out-of-bounds index.
        let n = CHUNK_N - 1;
        let mut c = Chunk::zeroed();
        for i in 0..CHUNK_N {
            c[(0, i)] = Cell::alive(PlayerId(1));
            c[(n, i)] = Cell::alive(PlayerId(1));
            c[(i, 0)] = Cell::alive(PlayerId(1));
            c[(i, n)] = Cell::alive(PlayerId(1));
        }
        let mut next = Chunk::zeroed();
        c.step(&mut next);
    }

    #[test]
    fn unloaded_neighbour_reads_as_dead() {
        let c = seed(&[(0, 0)]);
        assert_eq!(c.get(-1, -1), Cell::DEAD);
        assert_eq!(c.get(CHUNK_N as i32, 5), Cell::DEAD);

        // A lone corner cell has no live neighbours, so it dies. If the border
        // read as anything but dead this would survive.
        let mut next = Chunk::zeroed();
        c.step(&mut next);
        assert!(!next[(0, 0)].is_alive());
    }

    /// Each field must round-trip, and none may disturb another.
    #[test]
    fn the_bit_fields_are_independent() {
        for kind in [0u8, 1, 200, 255] {
            for p in 0..=PlayerId::MAX {
                for glass in [false, true] {
                    let c = Cell::DEAD
                        .with_alive(true)
                        .with_kind(Kind(kind))
                        .with_glass(glass)
                        .with_player(PlayerId(p));
                    assert!(c.is_alive());
                    assert_eq!(c.kind(), Kind(kind));
                    assert_eq!(c.is_glass(), glass);
                    assert_eq!(c.player(), PlayerId(p));
                    // Clearing alive must leave the others alone.
                    let d = c.with_alive(false);
                    assert!(!d.is_alive());
                    assert_eq!(d.kind(), Kind(kind));
                    assert_eq!(d.is_glass(), glass);
                    assert_eq!(d.player(), PlayerId(p));
                }
            }
        }
        // A kind uses all eight of its bits without touching the flags.
        let c = Cell::DEAD.with_kind(Kind(255));
        assert_eq!(c.kind(), Kind(255));
        assert_eq!(c.flags(), 0);
        assert_eq!(c.player(), PlayerId::UNOWNED);
        assert!(!c.is_alive());
    }

    #[test]
    fn a_cell_is_four_bytes_little_endian() {
        assert_eq!(size_of::<Cell>(), 4);
        let c = Cell::from_bits(0xABCD).with_uv(7, 9);
        assert_eq!(c.0, [0xCD, 0xAB, 7, 9], "low byte first, then u and v");
        assert_eq!(c.bits(), 0xABCD);
        assert_eq!(c.uv(), (7, 9));
        assert_eq!(Chunk::bytes_per_row(), CHUNK_N as u32 * 4);
    }

    /// A cell's tile must survive everything the rules do to it, or a pane
    /// would scramble its picture every generation.
    #[test]
    fn the_uv_survives_every_change() {
        let c = Cell::alive(PlayerId(3)).with_uv(11, 4);
        assert_eq!(c.uv(), (11, 4));
        for changed in [
            c.with_alive(false),
            c.with_alive(true),
            c.with_player(PlayerId(9)),
            c.with_kind(Kind(200)),
            c.with_glass(true),
            c.with_glass(false),
        ] {
            assert_eq!(changed.uv(), (11, 4), "the tile moved");
        }
    }

    /// The player sits at the top of the word, so extracting it is a shift with
    /// no mask, and raw cell values order by player before anything else.
    #[test]
    fn the_player_occupies_the_high_bits() {
        let c = Cell::alive(PlayerId(5));
        assert_eq!(c.bits() >> bits::PLAYER_SHIFT, 5);
        assert_eq!(c.player(), PlayerId(5));

        let low = Cell::alive(PlayerId(1)).with_kind(Kind(255)).with_glass(true);
        let high = Cell::alive(PlayerId(2));
        assert!(high.bits() > low.bits(), "player dominates the ordering");
    }

    #[test]
    fn a_zeroed_cell_is_dead_and_unowned() {
        let c = Cell::default();
        assert_eq!(c, Cell::DEAD);
        assert!(!c.is_alive());
        assert_eq!(c.player(), PlayerId::UNOWNED);
        assert_eq!(c.kind(), Kind::NORMAL);
    }

    #[test]
    fn block_is_still_life() {
        let c = seed(&[(4, 4), (4, 5), (5, 4), (5, 5)]);
        let mut next = Chunk::zeroed();
        c.step(&mut next);
        assert_eq!(live(&next), vec![(4, 4), (4, 5), (5, 4), (5, 5)]);
    }

    #[test]
    fn blinker_oscillates_with_period_two() {
        let a = seed(&[(5, 4), (5, 5), (5, 6)]);
        let mut b = Chunk::zeroed();
        a.step(&mut b);
        assert_eq!(live(&b), vec![(4, 5), (5, 5), (6, 5)]);
        let mut c = Chunk::zeroed();
        b.step(&mut c);
        assert_eq!(live(&c), vec![(5, 4), (5, 5), (5, 6)]);
    }

    #[test]
    fn glider_translates_one_cell_diagonally_every_four_generations() {
        let start = [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)];
        let mut cur = seed(&start.map(|(r, k)| (r + 5, k + 5)));
        let mut next = Chunk::zeroed();
        for _ in 0..4 {
            cur.step(&mut next);
            std::mem::swap(&mut cur, &mut next);
        }
        let expected: Vec<_> = start.iter().map(|&(r, k)| (r + 6, k + 6)).collect();
        assert_eq!(live(&cur), expected);
    }

    #[test]
    fn a_birth_is_attributed_to_the_majority_owner() {
        let mut c = Chunk::zeroed();
        c[(4, 5)] = Cell::alive(PlayerId(3));
        c[(5, 4)] = Cell::alive(PlayerId(3));
        c[(5, 6)] = Cell::alive(PlayerId(7));
        let mut next = Chunk::zeroed();
        c.step(&mut next);
        assert!(next[(5, 5)].is_alive());
        assert_eq!(next[(5, 5)].player(), PlayerId(3));
    }
}
