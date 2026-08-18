use std::collections::{HashMap, HashSet};

use super::cell::{Cell, Chunk, Halo, CHUNK_N};
use super::dir::Dir;
use super::player::PlayerId;

/// Never advance more than this many generations in a single frame.
const MAX_CATCHUP_STEPS: u32 = 8;

/// Chunk coordinate, (row, col). Row increases south, column increases east.
pub type Coord = (i32, i32);

#[inline]
fn offset((row, col): Coord, dir: Dir) -> Coord {
    let (dr, dc) = dir.delta();
    (row + dr, col + dc)
}

/// Where a world's chunks live, and what "no chunk here" means.
///
/// Neither variant stores neighbour links: a chunk's neighbours are *computed*
/// from its coordinate. That is what lets a chunk be its own neighbour on a
/// small torus, and what makes an unloaded chunk simply an absent key.
enum Storage {
    /// Unbounded plane. Only non-empty chunks are stored; an absent key is an
    /// empty chunk, which reads as dead and is recreated on demand.
    Infinite(HashMap<Coord, Chunk>),
    /// A fixed `rows` x `cols` torus, always fully allocated in one contiguous
    /// block. Coordinates wrap, so global coordinates map many-to-one onto
    /// these chunks.
    Toroidal {
        rows: i32,
        cols: i32,
        chunks: Box<[Chunk]>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorldKind {
    Infinite,
    Toroidal { rows: i32, cols: i32 },
}

pub struct World {
    storage: Storage,
    /// Reused between generations so stepping allocates nothing.
    scratch: Vec<Halo>,
    active: Vec<Coord>,
    elapsed: f32,
    pub generation: u64,
    pub dirty: bool,
}

impl World {
    /// An unbounded plane with nothing in it. Loading a saved world starts
    /// here and fills the chunks back in.
    pub fn infinite_empty() -> Self {
        Self::new(Storage::Infinite(HashMap::new()))
    }

    /// Put a chunk at a coordinate wholesale, creating it if need be. Used when
    /// restoring a save or accepting one from the server.
    pub fn put_chunk(&mut self, coord: Coord, chunk: Chunk) {
        let coord = self.canonical(coord);
        self.ensure(coord);
        if let Some(slot) = self.chunk_at_mut(coord) {
            *slot = chunk;
            self.dirty = true;
        }
    }

    /// The world the app and the server open with.
    ///
    /// A Gosper glider gun at the origin. It stays put, so there is always
    /// something to join to, and it emits a glider every thirty generations,
    /// so the trail of them says how long the server has been up.
    ///
    /// That second property is the point. A still life or an oscillator looks
    /// identical whether it arrived from the server or the client regenerated
    /// it locally, so it cannot tell a working join from a broken one. The
    /// length of the glider trail is not something a client starting from
    /// nothing can invent.
    pub fn demo() -> Self {
        let mut w = Self::infinite_empty();
        for (row, col) in GOSPER_GUN {
            w.set_cell_at(row, col, Cell::alive(PlayerId(1)));
        }
        w.generation = 0;
        w
    }

    pub fn infinite() -> Self {
        let mut chunk = Chunk::dead();
        seed_glider(&mut chunk, CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, PlayerId(1));
        let mut chunks = HashMap::new();
        chunks.insert((0, 0), chunk);
        Self::new(Storage::Infinite(chunks))
    }

    /// A `rows` x `cols` torus. Size is a runtime value rather than a const
    /// generic: const generics would infect every signature that touches a
    /// world, and buy nothing here since the dimensions are never used in a
    /// type-level computation.
    pub fn toroidal(rows: i32, cols: i32) -> Self {
        assert!(rows > 0 && cols > 0, "a torus needs at least one chunk");
        let mut chunks = vec![Chunk::dead(); (rows * cols) as usize].into_boxed_slice();
        seed_glider(&mut chunks[0], CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, PlayerId(1));
        Self::new(Storage::Toroidal { rows, cols, chunks })
    }

    fn new(storage: Storage) -> Self {
        Self {
            storage,
            scratch: Vec::new(),
            active: Vec::new(),
            elapsed: 0.0,
            generation: 0,
            dirty: true,
        }
    }

    /// Adopt a generation number. A birth's owner is seeded from it, so a
    /// client that simulated at a different tick from the server would make
    /// different choices even from identical cells.
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    /// Chunk coordinates covering a rectangle of cells, inclusive.
    pub fn chunks_covering(min: (i32, i32), max: (i32, i32)) -> Vec<Coord> {
        let n = CHUNK_N as i32;
        let (r0, c0) = (min.0.div_euclid(n), min.1.div_euclid(n));
        let (r1, c1) = (max.0.div_euclid(n), max.1.div_euclid(n));
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).collect()
    }

    pub fn kind(&self) -> WorldKind {
        match &self.storage {
            Storage::Infinite(_) => WorldKind::Infinite,
            Storage::Toroidal { rows, cols, .. } => WorldKind::Toroidal {
                rows: *rows,
                cols: *cols,
            },
        }
    }

    /// Reduce a global coordinate to the one chunk that actually holds it. On
    /// a torus this is many-to-one: many global coordinates, one chunk.
    #[inline]
    pub fn canonical(&self, (row, col): Coord) -> Coord {
        match &self.storage {
            Storage::Infinite(_) => (row, col),
            Storage::Toroidal { rows, cols, .. } => (row.rem_euclid(*rows), col.rem_euclid(*cols)),
        }
    }

    /// The cells at a coordinate, or `None` if nothing is stored there. An
    /// absent chunk is empty, which is why callers can treat `None` as dead
    /// rather than as an error.
    pub fn chunk_at(&self, coord: Coord) -> Option<&Chunk> {
        let coord = self.canonical(coord);
        match &self.storage {
            Storage::Infinite(map) => map.get(&coord),
            Storage::Toroidal { cols, chunks, .. } => {
                Some(&chunks[(coord.0 * cols + coord.1) as usize])
            }
        }
    }

    fn chunk_at_mut(&mut self, coord: Coord) -> Option<&mut Chunk> {
        let coord = self.canonical(coord);
        match &mut self.storage {
            Storage::Infinite(map) => map.get_mut(&coord),
            Storage::Toroidal { cols, chunks, .. } => {
                Some(&mut chunks[(coord.0 * *cols + coord.1) as usize])
            }
        }
    }

    /// Write one cell, creating the chunk if an infinite world does not yet
    /// hold it. Bringing a cell to life in empty space is exactly how a player
    /// action reaches the world, so this must be able to grow it.
    pub fn set_cell(&mut self, chunk: Coord, (row, col): (usize, usize), cell: Cell) {
        let chunk = self.canonical(chunk);
        if cell.is_alive() {
            self.ensure(chunk);
        }
        if let Some(c) = self.chunk_at_mut(chunk) {
            c[(row, col)] = cell;
            self.dirty = true;
        }
    }

    /// Write one cell addressed in absolute cell coordinates, splitting it
    /// into chunk and offset. Callers dealing in world positions should use
    /// this rather than doing the arithmetic themselves — getting it wrong
    /// puts the cell in the wrong place rather than failing.
    pub fn set_cell_at(&mut self, row: i32, col: i32, cell: Cell) {
        let n = CHUNK_N as i32;
        let chunk = (row.div_euclid(n), col.div_euclid(n));
        let local = (row.rem_euclid(n) as usize, col.rem_euclid(n) as usize);
        self.set_cell(chunk, local, cell);
    }

    /// Make sure a coordinate has storage. A no-op on a torus, where every
    /// chunk is allocated up front and never removed.
    fn ensure(&mut self, coord: Coord) {
        if let Storage::Infinite(map) = &mut self.storage {
            map.entry(coord).or_insert_with(Chunk::dead);
        }
    }

    /// Every chunk currently held, with its canonical coordinate.
    pub fn stored(&self) -> Vec<(Coord, &Chunk)> {
        match &self.storage {
            Storage::Infinite(map) => map.iter().map(|(&c, chunk)| (c, chunk)).collect(),
            Storage::Toroidal { rows, cols, chunks } => (0..*rows)
                .flat_map(|r| (0..*cols).map(move |c| (r, c)))
                .map(|(r, c)| ((r, c), &chunks[(r * cols + c) as usize]))
                .collect(),
        }
    }

    pub fn stored_count(&self) -> usize {
        match &self.storage {
            Storage::Infinite(map) => map.len(),
            Storage::Toroidal { rows, cols, .. } => (rows * cols) as usize,
        }
    }

    /// Chunks that must be stepped: every non-empty chunk, plus any neighbour
    /// it carries life towards. A chunk with no life on the edge facing its
    /// neighbour cannot cause a birth there, so the neighbour can be skipped.
    fn compute_active(&mut self) {
        let mut set: HashSet<Coord> = HashSet::new();
        for (coord, chunk) in self.stored() {
            if chunk.is_empty() {
                continue;
            }
            set.insert(coord);
            for dir in Dir::ALL {
                if edge_has_life(chunk, dir) {
                    set.insert(self.canonical(offset(coord, dir)));
                }
            }
        }
        self.active.clear();
        self.active.extend(set);
        // A HashSet iterates in an order that varies between processes. The
        // outcome does not currently depend on it -- every halo is gathered
        // before any is written -- but a client and server must not diverge
        // because of a future change here, so pin the order now.
        self.active.sort_unstable();
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn update(&mut self, dt: f32, span: f32) {
        if span <= 0.0 {
            return;
        }
        self.elapsed += dt;
        let mut steps = 0;
        while self.elapsed >= span && steps < MAX_CATCHUP_STEPS {
            self.elapsed -= span;
            self.step();
            steps += 1;
        }
        if steps == MAX_CATCHUP_STEPS {
            self.elapsed = 0.0;
        }
    }

    pub fn step(&mut self) {
        self.compute_active();
        let active = std::mem::take(&mut self.active);

        for &coord in &active {
            self.ensure(coord);
        }

        // Snapshot generation G into halos before writing any of G+1. The halo
        // *is* the double buffer: because it already holds G, results can be
        // written straight back into the stored chunk, so no chunk needs a
        // second `next` buffer.
        self.scratch.clear();
        self.scratch.reserve(active.len());
        for &coord in &active {
            let halo = self.gather_halo(coord);
            self.scratch.push(halo);
        }

        for (i, &coord) in active.iter().enumerate() {
            let halo = self.scratch[i];
            // Seeded by generation and chunk, so a birth's owner is chosen the
            // same way on every peer without exchanging a random number.
            let seed = super::rule::mix(
                super::rule::mix(0x0C01_1FE0, self.generation),
                (coord.0 as u32 as u64) << 32 | coord.1 as u32 as u64,
            );
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into(chunk, seed);
            }
        }

        self.active = active;
        self.generation += 1;
        self.dirty = true;
        self.prune();
    }

    /// Copy a chunk and the facing strip of each neighbour into a flat padded
    /// grid. Neighbours are looked up by coordinate, so a chunk being its own
    /// neighbour -- which happens on any torus smaller than 3x3 -- is just a
    /// repeated read, not an aliasing problem.
    fn gather_halo(&self, coord: Coord) -> Halo {
        let mut halo = Halo::dead();
        if let Some(cells) = self.chunk_at(coord) {
            halo.set_centre(cells);
        }

        let last = CHUNK_N - 1;
        for dir in Dir::ALL {
            let Some(n) = self.chunk_at(offset(coord, dir)) else {
                continue;
            };
            match dir {
                Dir::N => (0..CHUNK_N).for_each(|c| halo.set(0, c + 1, n[(last, c)])),
                Dir::S => (0..CHUNK_N).for_each(|c| halo.set(CHUNK_N + 1, c + 1, n[(0, c)])),
                Dir::W => (0..CHUNK_N).for_each(|r| halo.set(r + 1, 0, n[(r, last)])),
                Dir::E => (0..CHUNK_N).for_each(|r| halo.set(r + 1, CHUNK_N + 1, n[(r, 0)])),
                Dir::Nw => halo.set(0, 0, n[(last, last)]),
                Dir::Ne => halo.set(0, CHUNK_N + 1, n[(last, 0)]),
                Dir::Sw => halo.set(CHUNK_N + 1, 0, n[(0, last)]),
                Dir::Se => halo.set(CHUNK_N + 1, CHUNK_N + 1, n[(0, 0)]),
            }
        }
        halo
    }

    /// Drop empty chunks. Safe unconditionally: an absent chunk reads as dead,
    /// and `compute_active` recreates any coordinate that life reaches, zeroed
    /// -- which is exactly what was discarded. So an infinite world stores
    /// only the chunks that actually contain life.
    fn prune(&mut self) {
        if let Storage::Infinite(map) = &mut self.storage {
            map.retain(|_, chunk| !chunk.is_empty());
        }
    }

    /// A digest of the whole world state, for spotting a client/server desync
    /// cheaply. Order-independent inputs only: chunks are folded in sorted
    /// coordinate order, and every byte of every stored chunk contributes, so
    /// two worlds agreeing here agree on player and age as well as liveness.
    ///
    /// FNV-1a rather than `DefaultHasher`, whose output is explicitly not
    /// guaranteed stable across Rust versions -- which would make a digest
    /// compare fail between a server and client built at different times.
    pub fn digest(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut chunks = self.stored();
        chunks.sort_unstable_by_key(|&(coord, _)| coord);

        let mut h = OFFSET;
        let eat = |bytes: &[u8], h: &mut u64| {
            for &b in bytes {
                *h ^= b as u64;
                *h = h.wrapping_mul(PRIME);
            }
        };
        for (coord, chunk) in chunks {
            // An empty chunk is indistinguishable from an absent one, so skip
            // it: an infinite and a toroidal world holding the same life agree.
            if chunk.is_empty() {
                continue;
            }
            eat(&coord.0.to_le_bytes(), &mut h);
            eat(&coord.1.to_le_bytes(), &mut h);
            eat(chunk.as_bytes(), &mut h);
        }
        h
    }

    /// Replace one chunk's contents. Test-only: real edits arrive as actions.
    #[cfg(test)]
    fn with_chunk_for_test(mut self, coord: Coord, chunk: Chunk) -> Self {
        if let Some(slot) = self.chunk_at_mut(coord) {
            *slot = chunk;
        }
        self
    }

    /// A digest of one chunk, for comparing a partial view against the server.
    ///
    /// A whole-world digest is useless to a client, which holds only what its
    /// viewport covers and would therefore always disagree. Per chunk, a
    /// client can check exactly what it has.
    pub fn chunk_digest(&self, coord: Coord) -> Option<u64> {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let chunk = self.chunk_at(coord)?;
        let mut h = OFFSET;
        for &b in chunk.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(PRIME);
        }
        Some(h)
    }

    /// Bounding box of all live cells, in absolute cell coordinates, or None
    /// if nothing is alive. Answers "is there anything here, and where".
    pub fn live_bounds(&self) -> Option<((i32, i32), (i32, i32))> {
        let mut it = self.live_cells().into_iter();
        let first = it.next()?;
        let (mut lo, mut hi) = (first, first);
        for (r, c) in it {
            lo = (lo.0.min(r), lo.1.min(c));
            hi = (hi.0.max(r), hi.1.max(c));
        }
        Some((lo, hi))
    }

    /// Live cells in absolute cell coordinates, sorted.
    pub fn live_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    if chunk[(row, col)].is_alive() {
                        out.push((
                            crow * CHUNK_N as i32 + row as i32,
                            ccol * CHUNK_N as i32 + col as i32,
                        ));
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Where to draw, as (global coordinate, the chunk that fills it). On a
    /// torus the same chunk appears at several global coordinates, which is
    /// what makes the tiling tile; `repeats` says how many copies each way.
    pub fn render_tiles(&self, repeats: i32) -> Vec<(Coord, Coord)> {
        match &self.storage {
            Storage::Infinite(map) => map.keys().map(|&c| (c, c)).collect(),
            Storage::Toroidal { rows, cols, .. } => {
                let mut out = Vec::new();
                for tr in -repeats..=repeats {
                    for tc in -repeats..=repeats {
                        for r in 0..*rows {
                            for c in 0..*cols {
                                let global = (tr * rows + r, tc * cols + c);
                                out.push((global, (r, c)));
                            }
                        }
                    }
                }
                out
            }
        }
    }
}

/// Does this chunk carry life on the edge facing `dir`? A live cell there can
/// contribute to a birth in the chunk beyond.
fn edge_has_life(chunk: &Chunk, dir: Dir) -> bool {
    let last = CHUNK_N - 1;
    match dir {
        Dir::N => (0..CHUNK_N).any(|c| chunk[(0, c)].is_alive()),
        Dir::S => (0..CHUNK_N).any(|c| chunk[(last, c)].is_alive()),
        Dir::W => (0..CHUNK_N).any(|r| chunk[(r, 0)].is_alive()),
        Dir::E => (0..CHUNK_N).any(|r| chunk[(r, last)].is_alive()),
        Dir::Nw => chunk[(0, 0)].is_alive(),
        Dir::Ne => chunk[(0, last)].is_alive(),
        Dir::Sw => chunk[(last, 0)].is_alive(),
        Dir::Se => chunk[(last, last)].is_alive(),
    }
}

/// Gosper's glider gun: 36 cells, period 30, stationary, emitting a glider
/// south-east every cycle. Rows and columns are chunk-local, and it is 36 wide
/// so it straddles chunk boundaries at any sane chunk size.
const GOSPER_GUN: [(i32, i32); 36] = [
    (0, 24),
    (1, 22), (1, 24),
    (2, 12), (2, 13), (2, 20), (2, 21), (2, 34), (2, 35),
    (3, 11), (3, 15), (3, 20), (3, 21), (3, 34), (3, 35),
    (4, 0), (4, 1), (4, 10), (4, 16), (4, 20), (4, 21),
    (5, 0), (5, 1), (5, 10), (5, 14), (5, 16), (5, 17), (5, 22), (5, 24),
    (6, 10), (6, 16), (6, 24),
    (7, 11), (7, 15),
    (8, 12), (8, 13),
];

/// The standard glider, travelling south-east.
///
/// ```text
/// . # .
/// . . #
/// # # #
/// ```
fn seed_glider(chunk: &mut Chunk, row: usize, col: usize, player: PlayerId) {
    for (dr, dc) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
        chunk[(row + dr, col + dc)] = Cell::alive(player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The glider is seeded at chunk-local (6, 6) and travels south-east one
    /// cell every four generations, so after 4k steps its absolute cells are
    /// the seed offset by (k, k) -- true only if life crosses chunk borders
    /// correctly.
    fn expected_after(k: i32) -> Vec<(i32, i32)> {
        let (r, c) = ((CHUNK_N / 2 - 2) as i32, (CHUNK_N / 2 - 2) as i32);
        let mut v: Vec<(i32, i32)> = [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]
            .iter()
            .map(|&(dr, dc)| (r + dr + k, c + dc + k))
            .collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn the_glider_crosses_chunk_borders_intact() {
        let mut w = World::infinite();
        for _ in 0..400 {
            w.step();
        }
        assert_eq!(w.live_cells(), expected_after(100));
    }

    #[test]
    fn an_infinite_world_stores_only_chunks_that_hold_life() {
        let mut w = World::infinite();
        for _ in 0..400 {
            w.step();
        }
        // A five-cell glider spans at most four chunks. Without pruning this
        // grew without bound as the glider left empties in its wake.
        assert!(
            w.stored_count() <= 4,
            "expected the trail to be dropped, got {} chunks",
            w.stored_count()
        );
        for (coord, chunk) in w.stored() {
            assert!(!chunk.is_empty(), "chunk {coord:?} is empty but stored");
        }
    }

    /// Every one of a 1x1 torus's eight neighbours is the chunk itself. A
    /// graph of `Rc<RefCell<Chunk>>` cannot express this: gathering a chunk's
    /// neighbourhood would borrow the same cell twice and panic. Computing
    /// neighbours by coordinate makes it an ordinary repeated read.
    #[test]
    fn a_chunk_can_be_its_own_neighbour() {
        let mut w = World::toroidal(1, 1);
        assert_eq!(w.stored_count(), 1);
        let start = w.live_cells();
        // A glider crosses a 16-cell torus in 4 * 16 generations.
        for _ in 0..(4 * CHUNK_N as u32) {
            w.step();
        }
        assert_eq!(w.live_cells(), start, "the glider should return to its start");
    }

    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }

    #[test]
    fn a_torus_wraps_in_both_axes() {
        for (rows, cols) in [(1, 1), (2, 2), (3, 2), (4, 4)] {
            let mut w = World::toroidal(rows, cols);
            let start = w.live_cells();
            // A glider advances one cell diagonally every four generations, so
            // it laps a height x width torus after 4 * lcm(height, width).
            let (h, v) = (rows as u32 * CHUNK_N as u32, cols as u32 * CHUNK_N as u32);
            let period = 4 * (h / gcd(h, v)) * v;
            for _ in 0..period {
                w.step();
            }
            assert_eq!(
                w.live_cells().len(),
                5,
                "{rows}x{cols}: the glider must survive wrapping"
            );
            assert_eq!(w.live_cells(), start, "{rows}x{cols}: expected one full lap");
        }
    }

    #[test]
    fn a_torus_never_changes_size() {
        let mut w = World::toroidal(3, 3);
        for _ in 0..400 {
            w.step();
            assert_eq!(w.stored_count(), 9);
        }
    }

    /// Global coordinates map many-to-one onto chunks, which is what makes the
    /// tiling tile. Neighbours must agree across that mapping.
    #[test]
    fn global_coordinates_fold_onto_chunks_consistently() {
        let w = World::toroidal(3, 2);
        for row in -7..7 {
            for col in -7..7 {
                let canon = w.canonical((row, col));
                assert!((0..3).contains(&canon.0) && (0..2).contains(&canon.1));
                // The same chunk, however you address it.
                assert!(std::ptr::eq(
                    w.chunk_at((row, col)).unwrap(),
                    w.chunk_at(canon).unwrap()
                ));
                // Stepping to a neighbour and folding gives the same answer as
                // folding and then stepping.
                for dir in Dir::ALL {
                    assert_eq!(
                        w.canonical(offset((row, col), dir)),
                        w.canonical(offset(canon, dir)),
                    );
                }
            }
        }
    }

    #[test]
    fn a_torus_draws_each_chunk_at_several_global_positions() {
        let w = World::toroidal(2, 3);
        let tiles = w.render_tiles(1);
        assert_eq!(tiles.len(), 9 * 6, "3x3 copies of a 2x3 torus");
        for (global, canonical) in &tiles {
            assert_eq!(w.canonical(*global), *canonical);
        }
        // Each chunk is drawn nine times: the many-to-one relationship.
        for r in 0..2 {
            for c in 0..3 {
                assert_eq!(tiles.iter().filter(|(_, k)| *k == (r, c)).count(), 9);
            }
        }
    }

    /// Dimensions are chunks, and any size works down to a single chunk.
    #[test]
    fn a_torus_takes_its_dimensions_in_chunks() {
        for (rows, cols) in [(1, 1), (1, 5), (5, 1), (4, 7)] {
            let w = World::toroidal(rows, cols);
            assert_eq!(w.stored_count(), (rows * cols) as usize);
            assert_eq!(w.kind(), WorldKind::Toroidal { rows, cols });
        }
    }

    /// Client-side prediction rests on this: the same start plus the same
    /// inputs must give byte-identical results, in any process, every time.
    #[test]
    fn stepping_is_deterministic() {
        let mut a = World::infinite();
        let mut b = World::infinite();
        assert_eq!(a.digest(), b.digest());
        for g in 0..400 {
            a.step();
            b.step();
            assert_eq!(a.digest(), b.digest(), "diverged at generation {g}");
        }
        assert_eq!(a.live_cells(), b.live_cells());
    }

    /// The digest must notice a difference the live-cell list would miss.
    #[test]
    fn the_digest_covers_more_than_liveness() {
        let mut a = World::infinite();
        let mut b = World::infinite();
        for _ in 0..40 {
            a.step();
            b.step();
        }
        assert_eq!(a.digest(), b.digest());

        // Same cells alive, different owner.
        let coord = b.stored()[0].0;
        let mut edited = *b.chunk_at(coord).unwrap();
        'outer: for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                if edited[(row, col)].is_alive() {
                    edited[(row, col)] = edited[(row, col)].with_player(PlayerId(2));
                    break 'outer;
                }
            }
        }
        let b2 = b.with_chunk_for_test(coord, edited);
        assert_eq!(a.live_cells(), b2.live_cells(), "liveness is unchanged");
        assert_ne!(a.digest(), b2.digest(), "but the digest must differ");
    }

    #[test]
    fn an_infinite_world_never_folds_coordinates() {
        let w = World::infinite();
        assert_eq!(w.canonical((-5, 12)), (-5, 12));
    }
}
