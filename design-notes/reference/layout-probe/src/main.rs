use bytemuck::{Pod, Zeroable};
use std::ops::{Index, IndexMut};

// ---- the one knob ----------------------------------------------------------
pub const N: usize = 16;                 // cells per block edge
pub const CELLS: usize = N * N;

// ---- 1 byte per cell -------------------------------------------------------
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct Cell1(pub u8);                // 0 = dead, 1..=254 = owner

// ---- 2 bytes per cell ------------------------------------------------------
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct Cell2 { pub owner: u8, pub age: u8 }

pub trait Texel: Pod + Default + Copy {
    const BYTES: u32 = size_of::<Self>() as u32;
}
impl Texel for Cell1 {}
impl Texel for Cell2 {}

// ---- the block -------------------------------------------------------------
#[repr(transparent)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Block<C: Texel + 'static> { cells: [C; CELLS] }

impl<C: Texel> Block<C> {
    /// Heap-allocated directly: never materialised on the stack.
    pub fn zeroed() -> Box<Self> { bytemuck::zeroed_box() }

    /// Exactly what Queue::write_texture wants. Zero copy, zero conversion.
    pub fn as_bytes(&self) -> &[u8] { bytemuck::bytes_of(self) }

    /// bytes_per_row for the TexelCopyBufferLayout.
    pub const fn bytes_per_row() -> u32 { N as u32 * C::BYTES }

    pub fn row(&self, r: usize) -> &[C] { &self.cells[r * N..(r + 1) * N] }
}

// (row, col) == (texture Y, texture X). Stated once, enforced everywhere.
impl<C: Texel> Index<(usize, usize)> for Block<C> {
    type Output = C;
    #[inline]
    fn index(&self, (row, col): (usize, usize)) -> &C { &self.cells[row * N + col] }
}
impl<C: Texel> IndexMut<(usize, usize)> for Block<C> {
    #[inline]
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut C { &mut self.cells[row * N + col] }
}

// ---- compile-time layout guarantees ---------------------------------------
const _: () = {
    assert!(size_of::<Cell1>() == 1);
    assert!(size_of::<Cell2>() == 2);
    assert!(align_of::<Cell1>() == 1);
    assert!(align_of::<Cell2>() == 1);
    assert!(size_of::<Block<Cell1>>() == CELLS);      // no padding, anywhere
    assert!(size_of::<Block<Cell2>>() == CELLS * 2);
};

fn main() {
    println!("N = {N}");
    println!("Block<Cell1> = {} bytes, bytes_per_row = {}",
             size_of::<Block<Cell1>>(), Block::<Cell1>::bytes_per_row());
    println!("Block<Cell2> = {} bytes, bytes_per_row = {}",
             size_of::<Block<Cell2>>(), Block::<Cell2>::bytes_per_row());

    // Write a recognisable pattern through the ergonomic index...
    let mut b = Block::<Cell1>::zeroed();
    for row in 0..N { for col in 0..N { b[(row, col)] = Cell1((row * N + col) as u8); } }
    b[(0, 1)] = Cell1(200);   // texture (x=1, y=0)
    b[(1, 0)] = Cell1(201);   // texture (x=0, y=1)

    // ...and confirm the raw bytes are row-major in exactly the order
    // write_texture consumes them: row 0 first, then row 1, etc.
    let raw = b.as_bytes();
    assert_eq!(raw.len(), CELLS);
    assert_eq!(raw[1], 200, "cell (row 0, col 1) must be byte 1");
    assert_eq!(raw[N], 201, "cell (row 1, col 0) must be byte N");
    for row in 0..N {
        for col in 0..N {
            assert_eq!(raw[row * N + col], b[(row, col)].0);
        }
    }
    println!("row-major byte order: OK  (raw[1]={}, raw[N]={})", raw[1], raw[N]);

    // Two-byte variant: interleaved RG, ready for Rg8Uint.
    let mut c = Block::<Cell2>::zeroed();
    c[(0, 0)] = Cell2 { owner: 7, age: 9 };
    c[(0, 1)] = Cell2 { owner: 3, age: 4 };
    let raw2 = c.as_bytes();
    assert_eq!(&raw2[0..4], &[7, 9, 3, 4], "must be interleaved owner,age pairs");
    println!("Rg8Uint interleave: OK   (first 4 bytes {:?})", &raw2[0..4]);

    // Double buffering is a pointer swap, not a 64 KiB memcpy.
    let mut front = Block::<Cell1>::zeroed();
    let mut back  = Block::<Cell1>::zeroed();
    back[(3, 4)] = Cell1(42);
    let front_ptr = front.as_bytes().as_ptr();
    std::mem::swap(&mut front, &mut back);
    assert_eq!(front[(3, 4)], Cell1(42));
    assert_eq!(back.as_bytes().as_ptr(), front_ptr, "swap must move pointers only");
    println!("mem::swap double buffer: OK (no memcpy)");

    // Rows are contiguous slices -> the interior sim loop can work on &[C].
    assert_eq!(front.row(3).len(), N);
    assert_eq!(front.row(3)[4], Cell1(42));
    println!("row(3) is a contiguous &[Cell1] slice: OK");
    println!("\nALL LAYOUT GUARANTEES HOLD");
}
