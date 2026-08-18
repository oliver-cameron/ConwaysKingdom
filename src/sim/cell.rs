use bytemuck::{Pod, Zeroable};

pub(crate) use super::player::PlayerId;
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell: sixteen bits, little-endian.
///
/// ```text
///  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
/// |   player    |      metadata / flags      | A |
/// ```
///
/// Bit 0 is alive. The ten above it are yours. The top five are the player,
/// and being the top field means the number extracts with a single shift and
/// no mask, and that comparing two raw cells orders them by player first.
///
/// Stored as two explicit little-endian bytes rather than a `u16` field, so the
/// in-memory order is ours rather than the host's. The texture is `R16Uint`,
/// which GPUs read little-endian, so the assertion below fires at compile time
/// on a big-endian target rather than producing quietly scrambled cells.
///
/// A zeroed cell is dead and unowned, which is what makes zeroed memory a valid
/// empty world. Never give bit 0 clear a live meaning.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct Cell(pub [u8; 2]);

/// The bit layout. Adjust here and in the matching block at the top of
/// `render/shaders/grid.wgsl` — the shader cannot read Rust constants, so those
/// are the one thing kept in step by hand.
pub mod bits {
    /// Bit 0: alive or dead.
    pub const ALIVE: u16 = 1;

    /// Bits 1..11: metadata and flags, undivided. Carve as needed.
    pub const META_SHIFT: u16 = 1;
    pub const META_WIDTH: u16 = 10;
    pub const META_MASK: u16 = (1 << META_WIDTH) - 1;

    /// Bits 11..16: player number, at the top of the word.
    pub const PLAYER_SHIFT: u16 = 11;
    pub const PLAYER_WIDTH: u16 = 5;
    pub const PLAYER_MASK: u16 = (1 << PLAYER_WIDTH) - 1;
}

const _: () = {
    assert!(size_of::<Cell>() == 2 && align_of::<Cell>() == 1);
    // The fields must tile all sixteen bits with no overlap and no gap.
    assert!(bits::ALIVE == 1);
    assert!(bits::META_SHIFT == 1);
    assert!(bits::PLAYER_SHIFT == bits::META_SHIFT + bits::META_WIDTH);
    assert!(bits::PLAYER_SHIFT + bits::PLAYER_WIDTH == 16);
    // R16Uint is read little-endian by the GPU.
    assert!(cfg!(target_endian = "little"));
};

impl Cell {
    pub const DEAD: Self = Self([0, 0]);

    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits.to_le_bytes())
    }

    #[inline]
    pub const fn bits(self) -> u16 {
        u16::from_le_bytes(self.0)
    }

    pub const fn alive(player: PlayerId) -> Self {
        Self::from_bits(bits::ALIVE).with_player(player)
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

    #[inline]
    pub const fn meta(self) -> u16 {
        (self.bits() >> bits::META_SHIFT) & bits::META_MASK
    }

    #[inline]
    pub const fn with_alive(self, alive: bool) -> Self {
        if alive {
            Self::from_bits(self.bits() | bits::ALIVE)
        } else {
            Self::from_bits(self.bits() & !bits::ALIVE)
        }
    }

    #[inline]
    pub const fn with_player(self, player: PlayerId) -> Self {
        let cleared = self.bits() & !(bits::PLAYER_MASK << bits::PLAYER_SHIFT);
        Self::from_bits(cleared | ((player.0 as u16 & bits::PLAYER_MASK) << bits::PLAYER_SHIFT))
    }

    #[inline]
    pub const fn with_meta(self, meta: u16) -> Self {
        let cleared = self.bits() & !(bits::META_MASK << bits::META_SHIFT);
        Self::from_bits(cleared | ((meta & bits::META_MASK) << bits::META_SHIFT))
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("alive", &self.is_alive())
            .field("meta", &self.meta())
            .field("player", &self.player().0)
            .finish()
    }
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

    /// No live cells anywhere. An empty chunk contributes nothing to any
    /// neighbour, so it need be neither stored nor stepped.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| !c.is_alive())
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
        halo.step_into(next);
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

    /// The player holding a majority of the live cells around a halo position.
    /// Only consulted on a birth, so the tally is not paid for per cell.
    fn dominant_player(&self, hr: usize, hc: usize) -> PlayerId {
        let mut tally = [0u16; (PlayerId::MAX as usize) + 1];
        for dr in 0..3 {
            for dc in 0..3 {
                if dr == 1 && dc == 1 {
                    continue;
                }
                let n = self.get(hr + dr - 1, hc + dc - 1);
                if n.is_alive() {
                    tally[n.player().0 as usize] += 1;
                }
            }
        }
        tally
            .iter()
            .enumerate()
            .max_by_key(|&(player, &count)| (count, std::cmp::Reverse(player)))
            .map(|(player, _)| PlayerId(player as u8))
            .unwrap_or(PlayerId::UNOWNED)
    }

    pub fn step_into(&self, next: &mut Chunk) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let (hr, hc) = (row + 1, col + 1);
                let cur = self.get(hr, hc);

                let mut alive = 0u32;
                for dr in 0..3 {
                    for dc in 0..3 {
                        if dr == 1 && dc == 1 {
                            continue;
                        }
                        if self.get(hr + dr - 1, hc + dc - 1).is_alive() {
                            alive += 1;
                        }
                    }
                }

                next[(row, col)] = match (cur.is_alive(), alive) {
                    (true, 2) | (true, 3) => cur,
                    (false, 3) => Cell::alive(self.dominant_player(hr, hc)),
                    _ => Cell::DEAD,
                };
            }
        }
    }
}

impl Index<(usize, usize)> for Chunk {
    type Output = Cell;
    #[inline]
    fn index(&self, (row, col): (usize, usize)) -> &Cell {
        &self.cells[row * CHUNK_N + col]
    }
}

impl IndexMut<(usize, usize)> for Chunk {
    #[inline]
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Cell {
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
        for meta in [0, 1, 511, bits::META_MASK] {
            for p in 0..=PlayerId::MAX {
                let c = Cell::DEAD
                    .with_alive(true)
                    .with_meta(meta)
                    .with_player(PlayerId(p));
                assert!(c.is_alive());
                assert_eq!(c.meta(), meta);
                assert_eq!(c.player(), PlayerId(p));
                // Clearing alive must leave the other two alone.
                let d = c.with_alive(false);
                assert!(!d.is_alive());
                assert_eq!(d.meta(), meta);
                assert_eq!(d.player(), PlayerId(p));
            }
        }
        // Overflow is masked, not smeared into a neighbouring field.
        let c = Cell::DEAD.with_meta(bits::META_MASK + 1);
        assert_eq!(c.meta(), 0);
        assert_eq!(c.player(), PlayerId::UNOWNED);
        assert!(!c.is_alive());
    }

    #[test]
    fn a_cell_is_two_bytes_little_endian() {
        assert_eq!(size_of::<Cell>(), 2);
        let c = Cell::from_bits(0xABCD);
        assert_eq!(c.0, [0xCD, 0xAB], "low byte first, whatever the host");
        assert_eq!(c.bits(), 0xABCD);
        assert_eq!(Chunk::bytes_per_row(), CHUNK_N as u32 * 2);
    }

    /// The player sits at the top of the word, so extracting it is a shift with
    /// no mask, and raw cell values order by player before anything else.
    #[test]
    fn the_player_occupies_the_high_bits() {
        let c = Cell::alive(PlayerId(5));
        assert_eq!(c.bits() >> bits::PLAYER_SHIFT, 5);
        assert_eq!(c.player(), PlayerId(5));

        let low = Cell::alive(PlayerId(1)).with_meta(bits::META_MASK);
        let high = Cell::alive(PlayerId(2));
        assert!(high.bits() > low.bits(), "player dominates the ordering");
    }

    #[test]
    fn a_zeroed_cell_is_dead_and_unowned() {
        let c = Cell::default();
        assert_eq!(c, Cell::DEAD);
        assert!(!c.is_alive());
        assert_eq!(c.player(), PlayerId::UNOWNED);
        assert_eq!(c.meta(), 0);
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
