use bytemuck::{Pod, Zeroable};

// --- Option A: two named bytes. Byte order == declaration order, guaranteed. --
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct CellC { pub kind: u8, pub player: u8 }

// --- Option B: one u16, bit-packed. Byte order == host endianness. -----------
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Pod, Zeroable)]
pub struct CellU16(pub u16);
impl CellU16 {
    pub const fn new(kind: u16, player: u16) -> Self { Self((player << 12) | (kind & 0x0FFF)) }
    pub const fn kind(self) -> u16 { self.0 & 0x0FFF }
    pub const fn player(self) -> u16 { self.0 >> 12 }
}

const _: () = {
    assert!(size_of::<CellC>() == 2   && align_of::<CellC>() == 1);
    assert!(size_of::<CellU16>() == 2 && align_of::<CellU16>() == 2);
};

fn main() {
    println!("CellC   size {} align {}", size_of::<CellC>(),   align_of::<CellC>());
    println!("CellU16 size {} align {}", size_of::<CellU16>(), align_of::<CellU16>());

    // Option A: what lands in the texture is exactly what you declared.
    let a = CellC { kind: 0x12, player: 0x34 };
    let ab = bytemuck::bytes_of(&a);
    println!("\nCellC {{ kind: 0x12, player: 0x34 }} -> bytes {:02x?}", ab);
    assert_eq!(ab, &[0x12, 0x34], "repr(C) field order is declaration order, on every target");
    println!("  Rg8Uint: R = kind = {:#04x}, G = player = {:#04x}   (target-independent)", ab[0], ab[1]);

    // Option B: the same logical value, but the byte order is the host's.
    let b = CellU16::new(0x123, 0x4);
    let bb = bytemuck::bytes_of(&b);
    println!("\nCellU16::new(kind=0x123, player=0x4) -> u16 {:#06x} -> bytes {:02x?}", b.0, bb);
    println!("  to_le_bytes {:02x?}   to_be_bytes {:02x?}   to_ne_bytes {:02x?}",
             b.0.to_le_bytes(), b.0.to_be_bytes(), b.0.to_ne_bytes());
    println!("  host is {}-endian", if b.0.to_ne_bytes() == b.0.to_le_bytes() { "little" } else { "big" });
    println!("  -> as Rg8Uint the shader would see R={:#04x} G={:#04x}, which is NOT the nibble split",
             bb[0], bb[1]);

    // Round-trip through a byte buffer, the way write_texture would carry it.
    let grid = vec![CellC { kind: 7, player: 3 }; 4];
    let raw: &[u8] = bytemuck::cast_slice(&grid);
    println!("\n4 CellC -> {:02x?}", raw);
    let back: &[CellC] = bytemuck::cast_slice(raw);
    assert_eq!(back[2].kind, 7);
    assert_eq!(back[2].player, 3);
    println!("cast_slice round-trip preserves fields: OK");

    // The upgrade path, same pattern, 4 independent fields.
    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug, Pod, Zeroable)]
    struct Cell4 { kind: u8, player: u8, age: u8, flags: u8 }
    const _: () = assert!(size_of::<Cell4>() == 4 && align_of::<Cell4>() == 1);
    let c = Cell4 { kind: 1, player: 2, age: 3, flags: 4 };
    println!("\nCell4 -> Rgba8Uint bytes {:02x?}  (R=kind G=player B=age A=flags)",
             bytemuck::bytes_of(&c));
}
