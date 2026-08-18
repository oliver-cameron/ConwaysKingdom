use std::collections::{HashMap, HashSet};

use crate::cell::{Cell, Chunk, Halo, CHUNK_N};

/// Never advance more than this many generations in a single frame.
const MAX_CATCHUP_STEPS: u32 = 8;

/// Chunk coordinate, (row, col). Row increases south, column increases east.
pub type Coord = (i32, i32);

/// The eight neighbours of a chunk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
    Nw,
}

impl Dir {
    pub const ALL: [Dir; 8] = [
        Dir::N,
        Dir::Ne,
        Dir::E,
        Dir::Se,
        Dir::S,
        Dir::Sw,
        Dir::W,
        Dir::Nw,
    ];

    #[inline]
    pub const fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (-1, 0),
            Dir::Ne => (-1, 1),
            Dir::E => (0, 1),
            Dir::Se => (1, 1),
            Dir::S => (1, 0),
            Dir::Sw => (1, -1),
            Dir::W => (0, -1),
            Dir::Nw => (-1, -1),
        }
    }
}

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
    pub fn infinite() -> Self {
        let mut chunk = Chunk::dead();
        seed_glider(&mut chunk, CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, 1);
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
        seed_glider(&mut chunks[0], CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, 1);
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
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into(chunk);
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

/// The standard glider, travelling south-east.
///
/// ```text
/// . # .
/// . . #
/// # # #
/// ```
fn seed_glider(chunk: &mut Chunk, row: usize, col: usize, player: u8) {
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

    #[test]
    fn an_infinite_world_never_folds_coordinates() {
        let w = World::infinite();
        assert_eq!(w.canonical((-5, 12)), (-5, 12));
    }
}
