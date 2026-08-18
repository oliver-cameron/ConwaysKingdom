use bytemuck::{Pod, Zeroable};
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell: sixteen bits you carve up as you like.
///
/// Stored as two explicit little-endian bytes rather than a `u16` field, so the
/// in-memory order is ours rather than the host's. The texture is `R16Uint`,
/// which GPUs read little-endian, so the assertion below is what keeps the two
/// agreeing — it fires at compile time on a big-endian target rather than
/// producing quietly scrambled cells.
///
/// `kind == 0` means dead, so zeroed memory is a valid empty world. Never give
/// kind 0 a live meaning; `Chunk::zeroed` depends on it.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub struct Cell(pub [u8; 2]);

/// The bit layout, as (offset, width). Adjust here and in the matching block at
/// the top of `render/shaders/grid.wgsl` — the shader cannot read these, so
/// they are the one thing that must be kept in step by hand.
///
/// ```text
///  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
/// | flags |    age    |  player   |     kind       |
/// ```
pub mod bits {
    pub const KIND: (u16, u16) = (0, 6); // 63 live kinds, 0 = dead
    pub const PLAYER: (u16, u16) = (6, 4); // 15 players, 0 = unowned
    pub const AGE: (u16, u16) = (10, 4); // saturates at 15
    pub const FLAGS: (u16, u16) = (14, 2);

    pub const fn mask((_, width): (u16, u16)) -> u16 {
        (1u16 << width) - 1
    }
    pub const fn max((_, width): (u16, u16)) -> u16 {
        (1u16 << width) - 1
    }
}

const _: () = {
    assert!(size_of::<Cell>() == 2 && align_of::<Cell>() == 1);
    // The fields must tile the 16 bits exactly, with no overlap and no gap.
    assert!(bits::KIND.0 == 0);
    assert!(bits::PLAYER.0 == bits::KIND.0 + bits::KIND.1);
    assert!(bits::AGE.0 == bits::PLAYER.0 + bits::PLAYER.1);
    assert!(bits::FLAGS.0 == bits::AGE.0 + bits::AGE.1);
    assert!(bits::FLAGS.0 + bits::FLAGS.1 == 16);
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

    #[inline]
    const fn get(self, field: (u16, u16)) -> u16 {
        (self.bits() >> field.0) & bits::mask(field)
    }

    #[inline]
    const fn with(self, field: (u16, u16), value: u16) -> Self {
        let cleared = self.bits() & !(bits::mask(field) << field.0);
        Self::from_bits(cleared | ((value & bits::mask(field)) << field.0))
    }

    pub const fn alive(player: u16) -> Self {
        Self::DEAD.with(bits::KIND, 1).with(bits::PLAYER, player)
    }

    #[inline]
    pub const fn kind(self) -> u16 {
        self.get(bits::KIND)
    }
    #[inline]
    pub const fn player(self) -> u16 {
        self.get(bits::PLAYER)
    }
    #[inline]
    pub const fn age(self) -> u16 {
        self.get(bits::AGE)
    }
    #[inline]
    pub const fn flags(self) -> u16 {
        self.get(bits::FLAGS)
    }

    #[inline]
    pub const fn with_kind(self, v: u16) -> Self {
        self.with(bits::KIND, v)
    }
    #[inline]
    pub const fn with_player(self, v: u16) -> Self {
        self.with(bits::PLAYER, v)
    }
    #[inline]
    pub const fn with_flags(self, v: u16) -> Self {
        self.with(bits::FLAGS, v)
    }

    /// Age one generation, stopping at the field's maximum rather than
    /// wrapping round to newborn.
    #[inline]
    pub const fn aged(self) -> Self {
        let a = self.age();
        if a >= bits::max(bits::AGE) {
            self
        } else {
            self.with(bits::AGE, a + 1)
        }
    }

    #[inline]
    pub const fn is_alive(self) -> bool {
        self.kind() != 0
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("kind", &self.kind())
            .field("player", &self.player())
            .field("age", &self.age())
            .field("flags", &self.flags())
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
    fn dominant_player(&self, hr: usize, hc: usize) -> u16 {
        let mut tally = [0u16; 1 << bits::PLAYER.1];
        for dr in 0..3 {
            for dc in 0..3 {
                if dr == 1 && dc == 1 {
                    continue;
                }
                let n = self.get(hr + dr - 1, hc + dc - 1);
                if n.is_alive() {
                    tally[n.player() as usize] += 1;
                }
            }
        }
        tally
            .iter()
            .enumerate()
            .max_by_key(|&(player, &count)| (count, std::cmp::Reverse(player)))
            .map(|(player, _)| player as u16)
            .unwrap_or(0)
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
                    (true, 2) | (true, 3) => cur.aged(),
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
            c[(r, k)] = Cell::alive(1);
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
            c[(0, i)] = Cell::alive(1);
            c[(n, i)] = Cell::alive(1);
            c[(i, 0)] = Cell::alive(1);
            c[(i, n)] = Cell::alive(1);
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

    /// Every field must round-trip, and no field may disturb another.
    #[test]
    fn the_bit_fields_are_independent() {
        let fields: [(&str, fn(Cell, u16) -> Cell, fn(Cell) -> u16, u16); 3] = [
            ("kind", Cell::with_kind, Cell::kind, bits::max(bits::KIND)),
            ("player", Cell::with_player, Cell::player, bits::max(bits::PLAYER)),
            ("flags", Cell::with_flags, Cell::flags, bits::max(bits::FLAGS)),
        ];
        for &(name, set, get, max) in &fields {
            for v in 0..=max {
                assert_eq!(get(set(Cell::DEAD, v)), v, "{name} = {v}");
            }
            // Overflow is masked, not smeared into the neighbouring field.
            let c = set(Cell::DEAD, max + 1);
            for &(other, _, other_get, _) in &fields {
                if other != name {
                    assert_eq!(other_get(c), 0, "{name} overflow leaked into {other}");
                }
            }
        }
    }

    #[test]
    fn a_cell_is_two_bytes_little_endian() {
        assert_eq!(size_of::<Cell>(), 2);
        let c = Cell::from_bits(0xABCD);
        assert_eq!(c.0, [0xCD, 0xAB], "low byte first, whatever the host");
        assert_eq!(c.bits(), 0xABCD);
        assert_eq!(Chunk::bytes_per_row(), CHUNK_N as u32 * 2);
    }

    #[test]
    fn age_saturates_rather_than_wrapping_to_newborn() {
        let mut c = Cell::alive(1);
        for _ in 0..100 {
            c = c.aged();
        }
        assert_eq!(c.age(), bits::max(bits::AGE));
        assert!(c.is_alive(), "ageing must not clear kind");
        assert_eq!(c.player(), 1, "nor player");
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
        c[(4, 5)] = Cell::alive(3);
        c[(5, 4)] = Cell::alive(3);
        c[(5, 6)] = Cell::alive(7);
        let mut next = Chunk::zeroed();
        c.step(&mut next);
        assert!(next[(5, 5)].is_alive());
        assert_eq!(next[(5, 5)].player(), 3);
    }
}
