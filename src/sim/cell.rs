use bytemuck::{Pod, Zeroable};

use super::dir::Dir;
use super::player::PlayerId;
use super::rule::{next_cell, Neighbours, MINE_UPKEEP_ODDS, UPKEEP_STREAM};
use super::seed::{mix, Roll};
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell: two bytes, and the second of them is a sprite.
///
/// ```text
///  byte 0 (R)                byte 1 (G)
/// | player  | spare |       |    kind     |I |A |
///  7 6 5 4 3  2 1 0          7 6 5 4 3 2   1  0
/// ```
///
/// Byte 0 holds the player at the top, so the number extracts with a shift and
/// no mask, and three spare flag bits below it.
///
/// Byte 1 is **the tile this cell draws**. Alive and iced are its bottom two
/// bits and the kind is the rest, so a kind's four states are four consecutive
/// tiles — and the byte is the index straight into the sheet, low nibble
/// across, high nibble down. That is the whole of the mapping: no layer to
/// choose, no UV to carry, and nothing to keep in step but this diagram.
///
/// Uploaded as `Rg8Uint`. Uint rather than Unorm because these are bit fields,
/// not colours: Unorm hands the shader floats in 0..1 and reading a field back
/// means multiplying by 255 and rounding, where a driver rounding one step the
/// other way silently changes a cell's kind. Nothing samples this texture —
/// the shader only `textureLoad`s it — so filtering, the one thing Unorm buys,
/// is not in play.
///
/// Stored as explicit bytes rather than a `u16`, so the order is ours rather
/// than the host's, and so alignment stays 1 — which is what lets a chunk be
/// cast straight out of a save file or a wire frame at any offset.
///
/// A zeroed cell is dead, unowned and of kind zero, which is what makes zeroed
/// memory a valid empty world. Never give a zero byte a live meaning.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Pod, Zeroable)]
pub struct Cell(pub [u8; 2]);

/// The bit layout. Adjust here and in the matching block at the top of
/// `render/shaders/grid.wgsl` — the shader cannot read Rust constants, so those
/// are the one thing kept in step by hand.
pub mod bits {
    /// Byte 1, bit 0: alive or dead.
    pub const ALIVE: u8 = 1;

    /// Byte 1, bit 1: a pane covers this cell. Independent of `ALIVE`: a cell
    /// may be alive, iced, both, or neither. Ice freezes what it covers, so
    /// the rule returns such a cell unchanged.
    pub const ICE: u8 = 1 << 1;

    /// Byte 1, bits 2..8: what kind of cell this is. Also the top six bits of
    /// its tile index, which is why a kind's four states are consecutive.
    pub const KIND_SHIFT: u8 = 2;
    pub const KIND_WIDTH: u8 = 6;
    pub const KIND_MASK: u8 = (1 << KIND_WIDTH) - 1;

    /// Byte 0, bits 3..8: the player number, at the top so it extracts with a
    /// shift alone.
    pub const PLAYER_SHIFT: u8 = 3;
    pub const PLAYER_WIDTH: u8 = 5;
    pub const PLAYER_MASK: u8 = (1 << PLAYER_WIDTH) - 1;

    /// Byte 0, bit 0: **granted ground**, which never decays.
    ///
    /// Territory is lost as well as gained now, and a player who lost all of
    /// theirs could place nothing and so could never come to own anything
    /// again. This is the floor: the patch handed out on joining stays yours
    /// while nobody takes it, so there is always somewhere to build from.
    ///
    /// It marks a *square*, not a lineage, which is why a birth keeps the
    /// dead cell's copy of it rather than the parent's — everything else about
    /// a newborn comes from the parent.
    pub const HOME: u8 = 1;

    /// Byte 0, bits 0..3: flags that are about *whose* a cell is rather than
    /// what it looks like — anything that changes the picture belongs in the
    /// kind byte, where the sheet can see it. [`HOME`] is the first of them,
    /// and two are still free.
    ///
    /// Preserved across a change of owner, which is what keeps `HOME` on a
    /// square when the ground changes hands.
    pub const SPARE_MASK: u8 = (1 << PLAYER_SHIFT) - 1;
}

const _: () = {
    assert!(size_of::<Cell>() == 2 && align_of::<Cell>() == 1);
    // The kind byte must tile all eight bits with no overlap and no gap, or a
    // state would share a tile with a kind.
    assert!(bits::ALIVE == 1);
    assert!(bits::ICE == 2);
    assert!(bits::KIND_SHIFT == 2);
    assert!(bits::KIND_SHIFT as u32 + bits::KIND_WIDTH as u32 == 8);
    // And so must the player byte.
    assert!(bits::PLAYER_SHIFT as u32 + bits::PLAYER_WIDTH as u32 == 8);
    assert!(bits::SPARE_MASK == 0b111);
    // A tile index is a byte, and the sheet is sixteen tiles each way.
    assert!(u8::MAX as usize + 1 == 16 * 16);
};

impl Cell {
    pub const DEAD: Self = Self([0, 0]);

    /// The byte that says whose this is, and the byte that says what it looks
    /// like. Named rather than indexed, because `self.0[1]` at a call site
    /// tells nobody which is which.
    #[inline]
    const fn owner_byte(self) -> u8 {
        self.0[0]
    }

    /// Also the tile index into the sheet: low nibble across, high nibble
    /// down. Everything that changes the picture lives here.
    #[inline]
    pub const fn tile(self) -> u8 {
        self.0[1]
    }

    #[inline]
    const fn with_tile(self, tile: u8) -> Self {
        Self([self.0[0], tile])
    }

    /// A live cell belonging to `player`.
    ///
    /// Player zero means unowned, and unowned life would have nobody to
    /// attribute a birth to, so the invariant is checked here rather than
    /// discovered later as a cell nobody can claim.
    pub const fn alive(player: PlayerId) -> Self {
        assert!(player.is_owned(), "a live cell must have a non-zero player");
        Self::DEAD.with_tile(bits::ALIVE).with_player(player)
    }

    #[inline]
    pub const fn is_alive(self) -> bool {
        self.tile() & bits::ALIVE != 0
    }

    /// The player field is the top of its byte, so this is a shift with no
    /// mask — and arithmetic on the result is ordinary arithmetic.
    #[inline]
    pub const fn player(self) -> PlayerId {
        PlayerId(self.owner_byte() >> bits::PLAYER_SHIFT)
    }

    /// What kind of cell this is. Not the tile on its own: the tile carries
    /// the state as well, which is what makes a kind's four pictures four
    /// consecutive entries in the sheet.
    #[inline]
    pub const fn kind(self) -> Kind {
        Kind((self.tile() >> bits::KIND_SHIFT) & bits::KIND_MASK)
    }

    /// Under ice, and therefore not updating: a pane stops time inside
    /// itself. Says nothing about whether the cell is alive.
    #[inline]
    pub const fn is_ice(self) -> bool {
        self.tile() & bits::ICE != 0
    }

    #[inline]
    pub const fn with_alive(self, alive: bool) -> Self {
        if alive {
            self.with_tile(self.tile() | bits::ALIVE)
        } else {
            self.with_tile(self.tile() & !bits::ALIVE)
        }
    }

    #[inline]
    pub const fn with_player(self, player: PlayerId) -> Self {
        let spare = self.owner_byte() & bits::SPARE_MASK;
        Self([spare | ((player.0 & bits::PLAYER_MASK) << bits::PLAYER_SHIFT), self.0[1]])
    }

    #[inline]
    pub const fn with_kind(self, kind: Kind) -> Self {
        let state = self.tile() & !(bits::KIND_MASK << bits::KIND_SHIFT);
        self.with_tile(state | ((kind.0 & bits::KIND_MASK) << bits::KIND_SHIFT))
    }

    /// Granted ground: this square is somebody's home patch and its owner does
    /// not decay. Says nothing about who owns it now — ground changes hands by
    /// life growing over it, and takes this with it.
    #[inline]
    pub const fn is_home(self) -> bool {
        self.owner_byte() & bits::HOME != 0
    }

    #[inline]
    pub const fn with_home(self, home: bool) -> Self {
        if home {
            Self([self.0[0] | bits::HOME, self.0[1]])
        } else {
            Self([self.0[0] & !bits::HOME, self.0[1]])
        }
    }

    #[inline]
    pub const fn with_ice(self, ice: bool) -> Self {
        if ice {
            self.with_tile(self.tile() | bits::ICE)
        } else {
            self.with_tile(self.tile() & !bits::ICE)
        }
    }
}

impl core::fmt::Debug for Cell {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cell")
            .field("alive", &self.is_alive())
            .field("kind", &self.kind().0)
            .field("ice", &self.is_ice())
            .field("tile", &self.tile())
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
    /// A cell that pays its owner when it is **born**.
    ///
    /// Not a marker on the ground and not a rule about death: income is a
    /// property of a lineage. A birth copies its parent, kind and all, so a
    /// mine's children are mines — and since a birth picks one of its three
    /// parents at random, the kind spreads through a mixed population rather
    /// than being handed down whole. One mine dropped into a growing pattern
    /// takes about a third of the next births and drifts from there.
    ///
    /// What that makes valuable is **turnover**, not holdings. A block of
    /// mines is a still life and never gives birth, so it earns nothing. An
    /// oscillator earns every period, and a gun earns forever — which is the
    /// right shape for a game about patterns that work.
    pub const MINE: Self = Self(1);
    /// Every kind. Each must have art at its own index in `render::atlas`;
    /// extend this and the sprite list beside it, or it will not compile.
    pub const ALL: [Self; 2] = [Self::NORMAL, Self::MINE];
    pub const COUNT: usize = Self::ALL.len();
}

/// What each player's mines did in one generation, indexed by the number the
/// cell carries.
///
/// Counts rather than a sum of money: what a birth or a death is *worth* is the
/// economy's business and the economy lives in `net`. The rule counts.
///
/// `born` is a count of births. `upkeep` is a count of **charges falling due**
/// on dead mines — not of deaths. A mine's corpse costs its owner for as long
/// as it lies there, one generation in [`super::rule::MINE_UPKEEP_ODDS`], so a
/// square can be counted many times and a square that dies and is never
/// counted is possible too.
///
/// Two counts rather than one net figure, so the two can be priced apart —
/// which is what lets the rule decide *how often* a corpse is charged and
/// `net` decide *how much*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mined {
    pub born: [u32; PlayerId::COUNT],
    pub upkeep: [u32; PlayerId::COUNT],
}

impl Mined {
    /// Fold another generation's tally into this one.
    ///
    /// Saturating, so a world that somehow ran for four billion births does
    /// not wrap a player's earnings round to nothing.
    pub fn add(&mut self, other: &Mined) {
        for (t, n) in self.born.iter_mut().zip(&other.born) {
            *t = t.saturating_add(*n);
        }
        for (t, n) in self.upkeep.iter_mut().zip(&other.upkeep) {
            *t = t.saturating_add(*n);
        }
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
    /// Nothing here worth keeping: no life, no ice, and nobody's ground.
    ///
    /// Ownership counts because territory lives on dead cells. Without it a
    /// chunk holding nothing but claimed ground reads as empty, and `prune`
    /// drops it on the very step it was claimed — so territory outside a
    /// chunk that also holds life could never last a generation, and a player
    /// granted ground on joining would lose it before their first move.
    ///
    /// The cost is that an infinite world now grows with territory as well as
    /// with life, and territory has no die-off yet, so it only ever grows.
    pub fn is_empty(&self) -> bool {
        self.cells
            .iter()
            .all(|c| !c.is_alive() && !c.is_ice() && !c.player().is_owned())
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
        halo.step_into(next, 0, &mut Mined::default());
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
    ///
    /// `mined` is added to, never cleared: a caller sums a whole world into one
    /// tally. Counted here because this is the one place that holds a cell
    /// before and after in the same breath, so a birth costs a comparison and
    /// no second pass over the world.
    pub fn step_into(&self, next: &mut Chunk, seed: u64, mined: &mut Mined) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let (hr, hc) = (row + 1, col + 1);
                let cell_seed = mix(seed, (row as u64) << 32 | col as u64);
                let before = self.get(hr, hc);
                let after = next_cell(before, &self.neighbours(hr, hc), cell_seed);
                if after.kind() == Kind::MINE && after.player().is_owned() {
                    if after.is_alive() {
                        if !before.is_alive() {
                            mined.born[after.player().0 as usize] += 1;
                        }
                    } else if Roll::new(cell_seed).one_in(UPKEEP_STREAM, MINE_UPKEEP_ODDS) {
                        // A corpse costs while it lies there, not once when it
                        // falls. Its own stream, so it is independent of every
                        // roll the rule itself takes.
                        mined.upkeep[after.player().0 as usize] += 1;
                    }
                }
                next[(row, col)] = after;
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
        // The kind field is six bits now: the other two carry alive and ice,
        // which is what makes the byte a tile index.
        for kind in [0u8, 1, 37, bits::KIND_MASK] {
            for p in 0..=PlayerId::MAX {
                for ice in [false, true] {
                    let c = Cell::DEAD
                        .with_alive(true)
                        .with_kind(Kind(kind))
                        .with_ice(ice)
                        .with_player(PlayerId(p));
                    assert!(c.is_alive());
                    assert_eq!(c.kind(), Kind(kind));
                    assert_eq!(c.is_ice(), ice);
                    assert_eq!(c.player(), PlayerId(p));
                    // Clearing alive must leave the others alone.
                    let d = c.with_alive(false);
                    assert!(!d.is_alive());
                    assert_eq!(d.kind(), Kind(kind));
                    assert_eq!(d.is_ice(), ice);
                    assert_eq!(d.player(), PlayerId(p));
                }
            }
        }
        // A kind uses all six of its bits without touching the state below.
        let c = Cell::DEAD.with_kind(Kind(bits::KIND_MASK));
        assert_eq!(c.kind(), Kind(bits::KIND_MASK));
        assert_eq!(c.player(), PlayerId::UNOWNED);
        assert!(!c.is_alive());
        assert!(!c.is_ice());
    }

    #[test]
    fn a_cell_is_two_bytes_owner_then_tile() {
        assert_eq!(size_of::<Cell>(), 2);
        let c = Cell::alive(PlayerId(5)).with_kind(Kind(3)).with_ice(true);
        assert_eq!(c.0[0], 5 << bits::PLAYER_SHIFT, "the owner byte is the player");
        assert_eq!(
            c.0[1],
            (3 << bits::KIND_SHIFT) | bits::ICE | bits::ALIVE,
            "and the tile byte is kind, ice and alive"
        );
        assert_eq!(Chunk::bytes_per_row(), CHUNK_N as u32 * 2);
    }

    /// The tile byte *is* the index into the sheet, so a kind's four states
    /// are four consecutive tiles and the shader needs no table to find them.
    #[test]
    fn a_kinds_four_states_are_four_consecutive_tiles() {
        let base = Cell::DEAD.with_kind(Kind(7));
        let tiles: Vec<u8> = [(false, false), (true, false), (false, true), (true, true)]
            .iter()
            .map(|&(alive, ice)| base.with_alive(alive).with_ice(ice).tile())
            .collect();
        assert_eq!(tiles, vec![7 * 4, 7 * 4 + 1, 7 * 4 + 2, 7 * 4 + 3]);

        // Low nibble across the sheet, high nibble down it.
        let tile = tiles[3];
        assert_eq!((tile & 15, tile >> 4), (31 % 16, 31 / 16));
    }

    /// The player sits at the top of its byte, so extracting it is a shift
    /// with no mask, and the owner byte orders by player before anything else.
    #[test]
    fn the_player_occupies_the_high_bits() {
        let c = Cell::alive(PlayerId(5));
        assert_eq!(c.0[0] >> bits::PLAYER_SHIFT, 5);
        assert_eq!(c.player(), PlayerId(5));

        let low = Cell::alive(PlayerId(1)).with_kind(Kind(bits::KIND_MASK)).with_ice(true);
        let high = Cell::alive(PlayerId(2));
        assert!(high.0[0] > low.0[0], "player dominates the owner byte");
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
