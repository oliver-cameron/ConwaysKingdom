use bytemuck::{Pod, Zeroable};
use std::ops::{Index, IndexMut};

pub const N: usize = 16;
pub const CELLS: usize = N * N;

/// Low nibble = cell kind (0 = dead). High nibble = player / attribution.
/// A zeroed byte is therefore a valid "dead, unowned" cell.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct Cell(pub u8);

impl Cell {
    #[inline] pub const fn new(kind: u8, player: u8) -> Self {
        debug_assert!(kind < 16 && player < 16);
        Self((player << 4) | (kind & 0x0F))
    }
    #[inline] pub const fn kind(self)   -> u8 { self.0 & 0x0F }
    #[inline] pub const fn player(self) -> u8 { self.0 >> 4 }
    #[inline] pub const fn is_alive(self) -> bool { self.kind() != 0 }
    #[inline] pub fn set_kind(&mut self, k: u8)   { self.0 = (self.0 & 0xF0) | (k & 0x0F); }
    #[inline] pub fn set_player(&mut self, p: u8) { self.0 = (self.0 & 0x0F) | (p << 4); }
}

#[repr(transparent)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Chunk { cells: [Cell; CELLS] }

impl Chunk {
    pub fn zeroed() -> Box<Self> { bytemuck::zeroed_box() }
    pub fn as_bytes(&self) -> &[u8] { bytemuck::bytes_of(self) }
    pub fn as_bytes_mut(&mut self) -> &mut [u8] { bytemuck::bytes_of_mut(self) }
    pub const fn bytes_per_row() -> u32 { N as u32 }
}
impl Index<(usize, usize)> for Chunk {
    type Output = Cell;
    #[inline] fn index(&self, (r, c): (usize, usize)) -> &Cell { &self.cells[r * N + c] }
}
impl IndexMut<(usize, usize)> for Chunk {
    #[inline] fn index_mut(&mut self, (r, c): (usize, usize)) -> &mut Cell { &mut self.cells[r * N + c] }
}

/// Exactly what the fragment shader computes from textureLoad(...).r
fn shader_unpack(texel: u32) -> (u32, u32) { (texel & 15u32, texel >> 4u32) }

fn main() {
    let mut ch = Chunk::zeroed();

    // --- 1. same allocation, two views -------------------------------------
    let struct_addr = &*ch as *const Chunk as usize;
    let bytes_addr  = ch.as_bytes().as_ptr() as usize;
    assert_eq!(struct_addr, bytes_addr, "bytes must alias the struct, not a copy");
    assert_eq!(ch.as_bytes().len(), CELLS);
    println!("struct @ {struct_addr:#x} == bytes @ {bytes_addr:#x}   len {}", ch.as_bytes().len());

    // --- 2. write as cells, read as texels ---------------------------------
    ch[(0, 0)] = Cell::new(1, 0);    // kind 1, player 0
    ch[(0, 1)] = Cell::new(3, 7);    // kind 3, player 7
    ch[(1, 0)] = Cell::new(15, 15);  // max of both nibbles
    ch[(5, 9)] = Cell::new(2, 4);

    let raw = ch.as_bytes();
    assert_eq!(raw[0], 0x01);
    assert_eq!(raw[1], 0x73);
    assert_eq!(raw[N], 0xFF);
    assert_eq!(raw[5 * N + 9], 0x42);
    println!("packed bytes: [0]={:#04x} [1]={:#04x} [N]={:#04x} [5*N+9]={:#04x}",
             raw[0], raw[1], raw[N], raw[5 * N + 9]);

    // --- 3. CPU view and shader view agree, cell for cell ------------------
    for r in 0..N { for c in 0..N {
        let cell = ch[(r, c)];
        let (k, p) = shader_unpack(raw[r * N + c] as u32);
        assert_eq!(k, cell.kind() as u32);
        assert_eq!(p, cell.player() as u32);
    }}
    println!("CPU nibbles == shader nibbles for all {CELLS} cells");

    // --- 4. zeroed memory is a valid empty world ---------------------------
    let empty = Chunk::zeroed();
    assert!(empty.as_bytes().iter().all(|&b| b == 0));
    assert!(!empty[(3, 3)].is_alive() && empty[(3, 3)].player() == 0);
    println!("zeroed_box -> every cell dead and unowned: OK");

    // --- 5. mutate through the byte view, observe through the cell view ----
    ch.as_bytes_mut()[2 * N + 2] = 0x59;
    assert_eq!(ch[(2, 2)].kind(), 9);
    assert_eq!(ch[(2, 2)].player(), 5);
    println!("byte-view write visible through cell-view read: OK");

    // --- 6. a grid of chunks is contiguous, but NOT in layer row-major -----
    let grid: Vec<Chunk> = (0..4).map(|_| *Chunk::zeroed()).collect();
    let flat: &[u8] = bytemuck::cast_slice(&grid);
    println!("4 chunks cast_slice -> {} bytes contiguous (chunk-major, not layer-major)", flat.len());

    println!("\nSAME BYTES, BOTH VIEWS: VERIFIED");
}
