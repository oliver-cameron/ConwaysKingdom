use std::ops::Index;
use std::rc::Rc;
#[derive(Debug, Clone, Copy)]
pub enum CellState {
    Alive,
    Dead,
}

#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub state: CellState,
}
#[derive(Debug, Clone)]
pub struct CellChunk {
    pub loc: (i32, i32),
    pub cells: CellArray,
    pub next_cells: CellArray,
    pub neighbors: [Rc<Neighbor>; 8],
}

#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum CellChunkDirection {
    North = 0,
    South = 1,
    East = 2,
    West = 3,
    NorthEast = 4,
    NorthWest = 5,
    SouthEast = 6,
    SouthWest = 7,
}

#[derive(Debug, Clone)]
pub enum Neighbor {
    CellChunk(CellChunk),
    Unloaded,
}
impl Neighbor {
    pub fn is_loaded(&self) -> bool {
        match self {
            Neighbor::CellChunk(_) => true,
            Neighbor::Unloaded => false,
        }
    }
}
impl Index<usize> for Neighbor {
    type Output = [Cell; 16];
    fn index(&self, index: usize) -> &Self::Output {
        match self {
            Neighbor::CellChunk(chunk) => &chunk.cells[index],
            Neighbor::Unloaded => panic!(),
        }
    }
}
#[derive(Debug, Clone)]
struct CellArray {
    pub cells: [[Cell; 16]; 16],
}
impl Index<usize> for CellArray {
    type Output = [Cell; 16];
    fn index(&self, index: usize) -> &Self::Output {
        &self.cells[index]
    }
}
impl std::ops::IndexMut<usize> for CellArray {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.cells[index]
    }
}

impl Index<CellChunkDirection> for [Rc<Neighbor>] {
    type Output = Neighbor;
    fn index(&self, index: CellChunkDirection) -> &Self::Output {
        &self[index as usize]
    }
}
impl CellArray {
    pub fn count_alive_neighbors(&self, x: usize, y: usize, neighbours: &[Rc<Neighbor>]) -> usize {
        let mut count = 0;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue; // Skip the cell itself
                }
                use CellChunkDirection::*;
                let looking_type: CellState = match (dx, dy, x, y) {
                    (-1, -1, 0, 0) => neighbours[NorthWest][15][15].state,
                    (-1, 0, 0, _) => neighbours[North][15][y].state,
                    (-1, 1, 0, 15) => neighbours[NorthEast][15][0].state,
                    (0, -1, _, 0) => neighbours[West][x][15].state,
                    (0, 1, _, 15) => neighbours[East][x][0].state,
                    (1, -1, 15, 0) => neighbours[SouthWest][0][15].state,
                    (1, 0, 15, _) => neighbours[South][0][y].state,
                    (1, 1, 15, 15) => neighbours[SouthEast][0][0].state,
                    _ => self.cells[(x as isize + dx) as usize][(y as isize + dy) as usize].state,
                };
                match looking_type {
                    CellState::Alive => count += 1,
                    CellState::Dead => (),
                }
            }
        }
        count
    }
}

impl CellChunk {
    pub fn calc_generation(&mut self) {
        for x in 0..16 {
            for y in 0..16 {
                let cell_state = &mut self.next_cells[x][y as usize].state;
                let alive_neighbors = self.cells.count_alive_neighbors(x, y, &self.neighbors);
                *cell_state = match (self.cells[x][y as usize].state, alive_neighbors) {
                    (CellState::Alive, 2) | (CellState::Alive, 3) => CellState::Alive,
                    (CellState::Dead, 3) => CellState::Alive,
                    _ => CellState::Dead,
                };
            }
        }
    }
    pub fn apply_generation(&mut self) {
        for x in 0..16 {
            for y in 0..16 {
                let cell = &mut self.cells[x][y as usize];
                cell.state = self.next_cells[x][y as usize].state;
            }
        }
    }
}
