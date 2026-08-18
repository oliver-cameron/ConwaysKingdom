use bytemuck::{Pod, Zeroable};
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell, laid out so a chunk is directly uploadable as an `Rgba8Uint`
/// texture: R = kind, G = player, B = age, A = flags.
///
/// `kind == 0` means dead, which makes zeroed memory a valid empty world.
/// Never give kind 0 a live meaning — `Chunk::zeroed` depends on it.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct Cell {
    pub kind: u8,
    pub player: u8,
    pub age: u8,
    pub flags: u8,
}

impl Cell {
    pub const DEAD: Self = Self { kind: 0, player: 0, age: 0, flags: 0 };

    pub const fn alive(player: u8) -> Self {
        Self { kind: 1, player, age: 0, flags: 0 }
    }

    #[inline]
    pub const fn is_alive(self) -> bool {
        self.kind != 0
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

    fn live_neighbours(&self, row: usize, col: usize) -> u32 {
        let mut n = 0;
        for dr in -1..=1i32 {
            for dc in -1..=1i32 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                if self.get(row as i32 + dr, col as i32 + dc).is_alive() {
                    n += 1;
                }
            }
        }
        n
    }

    /// The player owning a majority of this cell's live neighbours. Used to
    /// attribute a birth; ties go to the lowest player id.
    fn dominant_player(&self, row: usize, col: usize) -> u8 {
        let mut tally = [0u8; 256];
        for dr in -1..=1i32 {
            for dc in -1..=1i32 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let c = self.get(row as i32 + dr, col as i32 + dc);
                if c.is_alive() {
                    tally[c.player as usize] += 1;
                }
            }
        }
        tally
            .iter()
            .enumerate()
            .max_by_key(|&(player, &count)| (count, std::cmp::Reverse(player)))
            .map(|(player, _)| player as u8)
            .unwrap_or(0)
    }

    /// Conway's rules, writing into `next`. Reads only `self`, so the two
    /// buffers never alias.
    pub fn step(&self, next: &mut Chunk) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let cur = self[(row, col)];
                let n = self.live_neighbours(row, col);
                next[(row, col)] = match (cur.is_alive(), n) {
                    (true, 2) | (true, 3) => Cell { age: cur.age.saturating_add(1), ..cur },
                    (false, 3) => Cell::alive(self.dominant_player(row, col)),
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
    assert!(size_of::<Cell>() == 4 && align_of::<Cell>() == 1);
    assert!(size_of::<Chunk>() == CHUNK_CELLS * 4);
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
    fn edges_and_corners_do_not_panic() {
        let c = Chunk::zeroed();
        for r in 0..CHUNK_N {
            for k in 0..CHUNK_N {
                assert_eq!(c.live_neighbours(r, k), 0);
            }
        }
    }

    #[test]
    fn unloaded_neighbour_reads_as_dead() {
        let c = seed(&[(0, 0)]);
        assert_eq!(c.get(-1, -1), Cell::DEAD);
        assert_eq!(c.get(CHUNK_N as i32, 5), Cell::DEAD);
        assert_eq!(c.live_neighbours(0, 0), 0);
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
        assert_eq!(next[(5, 5)].player, 3);
    }
}
