use std::collections::HashMap;

use crate::cell::{Cell, Chunk, Halo, CHUNK_N};

/// Never advance more than this many generations in a single frame. Without a
/// cap, a long stall would try to catch up all at once and stall again.
const MAX_CATCHUP_STEPS: u32 = 8;

/// Index into `World::slots`. Stable for the life of the world: slots are never
/// removed, only promoted.
pub type ChunkId = usize;

/// The eight neighbours of a chunk. Row increases south, column increases east.
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
    pub const fn index(self) -> usize {
        self as usize
    }

    /// (row, col) step in chunk coordinates.
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

    #[inline]
    pub const fn opposite(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::Ne => Dir::Sw,
            Dir::E => Dir::W,
            Dir::Se => Dir::Nw,
            Dir::S => Dir::N,
            Dir::Sw => Dir::Ne,
            Dir::W => Dir::E,
            Dir::Nw => Dir::Se,
        }
    }
}

/// A chunk's eight neighbours by id. `None` means "no neighbour in this
/// direction" — a real edge in a tiled world, or not yet wired in an infinite
/// one.
pub type Links = [Option<ChunkId>; 8];

/// What occupies a slot.
///
/// The three states differ in two independent ways: whether there are cells to
/// simulate, and whether the slot participates in the adjacency graph.
pub enum Neighbour {
    /// No cells and no links. Reads as dead. A frontier placeholder in an
    /// infinite world, created so a coordinate has an identity before anything
    /// lives there. May be promoted; may eventually be discarded.
    Unloaded,
    /// No cells, but wired into the graph and permanent. Tiled worlds use these
    /// for chunks that are currently empty: the topology is fixed and known up
    /// front, so the links are worth keeping even while nothing lives there.
    Idle { links: Links },
    /// Has cells and is simulated. `next` is the write target for the coming
    /// generation; the two are swapped once every chunk has been stepped.
    CellChunk {
        cells: Box<Chunk>,
        next: Box<Chunk>,
        links: Links,
    },
}

impl Neighbour {
    pub fn links(&self) -> Option<&Links> {
        match self {
            Neighbour::Unloaded => None,
            Neighbour::Idle { links } | Neighbour::CellChunk { links, .. } => Some(links),
        }
    }

    pub fn links_mut(&mut self) -> Option<&mut Links> {
        match self {
            Neighbour::Unloaded => None,
            Neighbour::Idle { links } | Neighbour::CellChunk { links, .. } => Some(links),
        }
    }

    /// The cells, if any. `Unloaded` and `Idle` are both empty, which is why
    /// both read as dead through a halo without a special case.
    pub fn cells(&self) -> Option<&Chunk> {
        match self {
            Neighbour::CellChunk { cells, .. } => Some(cells),
            _ => None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self, Neighbour::CellChunk { .. })
    }
}

/// How the world's chunk set behaves at its frontier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorldKind {
    /// Unbounded plane. Chunks appear on demand: reaching a coordinate for the
    /// first time creates an `Unloaded` slot, and life arriving promotes it.
    Infinite,
    /// A fixed set of chunks with explicit links, which may wrap. Empty chunks
    /// are `Idle` rather than `Unloaded` — they keep their links and are never
    /// removed, because the topology is not derivable from coordinates.
    Tiled,
}

pub struct World {
    kind: WorldKind,
    slots: Vec<Neighbour>,
    /// Slot id -> chunk coordinate, in (row, col). Used for rendering
    /// placement. In a tiled world the links, not these, define adjacency.
    locs: Vec<(i32, i32)>,
    coords: HashMap<(i32, i32), ChunkId>,
    elapsed: f32,
    pub generation: u64,
    /// Set when any chunk's cells changed since the last upload to the GPU.
    pub dirty: bool,
}

impl World {
    /// An unbounded plane holding a single glider.
    pub fn infinite() -> Self {
        let mut w = Self::empty(WorldKind::Infinite);
        let id = w.ensure_slot_at((0, 0));
        w.load(id);
        if let Neighbour::CellChunk { cells, .. } = &mut w.slots[id] {
            seed_glider(cells, CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, 1);
        }
        w.ensure_frontier();
        w
    }

    /// A `rows` x `cols` torus of chunks, all `Idle` but fully wired, with a
    /// glider in one of them. Nothing here is ever created or destroyed after
    /// construction.
    pub fn tiled(rows: i32, cols: i32) -> Self {
        assert!(rows > 0 && cols > 0);
        let mut w = Self::empty(WorldKind::Tiled);

        for row in 0..rows {
            for col in 0..cols {
                let id = w.slots.len();
                w.slots.push(Neighbour::Idle { links: [None; 8] });
                w.locs.push((row, col));
                w.coords.insert((row, col), id);
            }
        }
        // Wrap in both axes: this is what makes it a torus rather than a box.
        for row in 0..rows {
            for col in 0..cols {
                let id = w.coords[&(row, col)];
                for dir in Dir::ALL {
                    let (dr, dc) = dir.delta();
                    let target = ((row + dr).rem_euclid(rows), (col + dc).rem_euclid(cols));
                    let nid = w.coords[&target];
                    if let Some(links) = w.slots[id].links_mut() {
                        links[dir.index()] = Some(nid);
                    }
                }
            }
        }

        let start = w.coords[&(0, 0)];
        w.load(start);
        if let Neighbour::CellChunk { cells, .. } = &mut w.slots[start] {
            seed_glider(cells, CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, 1);
        }
        w.ensure_frontier();
        w
    }

    fn empty(kind: WorldKind) -> Self {
        Self {
            kind,
            slots: Vec::new(),
            locs: Vec::new(),
            coords: HashMap::new(),
            elapsed: 0.0,
            generation: 0,
            dirty: true,
        }
    }

    pub fn kind(&self) -> WorldKind {
        self.kind
    }

    pub fn slot(&self, id: ChunkId) -> &Neighbour {
        &self.slots[id]
    }

    pub fn loc(&self, id: ChunkId) -> (i32, i32) {
        self.locs[id]
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn loaded_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_loaded()).count()
    }

    /// Every simulated chunk, with its coordinate.
    pub fn loaded(&self) -> impl Iterator<Item = (ChunkId, (i32, i32), &Chunk)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(move |(id, slot)| slot.cells().map(|c| (id, self.locs[id], c)))
    }

    /// Live cells in absolute cell coordinates, sorted. Used by tests to check
    /// that a pattern crossing a chunk boundary stays intact.
    pub fn live_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for (_, (crow, ccol), chunk) in self.loaded() {
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
        let loaded: Vec<ChunkId> = (0..self.slots.len())
            .filter(|&id| self.slots[id].is_loaded())
            .collect();

        // Gather every halo from generation G before writing any of G+1, so no
        // chunk sees a neighbour that has already advanced.
        let halos: Vec<Halo> = loaded.iter().map(|&id| self.gather_halo(id)).collect();

        for (&id, halo) in loaded.iter().zip(&halos) {
            if let Neighbour::CellChunk { next, .. } = &mut self.slots[id] {
                halo.step_into(next);
            }
        }
        for &id in &loaded {
            if let Neighbour::CellChunk { cells, next, .. } = &mut self.slots[id] {
                std::mem::swap(cells, next);
            }
        }

        self.generation += 1;
        self.dirty = true;
        self.ensure_frontier();
    }

    /// Copy a chunk and the facing strip of each of its eight neighbours into a
    /// flat padded grid. Unloaded and Idle neighbours contribute nothing, which
    /// leaves their strip dead — exactly the old `Unloaded => dead` rule, now
    /// without a special case.
    fn gather_halo(&self, id: ChunkId) -> Halo {
        let mut halo = Halo::dead();
        if let Some(cells) = self.slots[id].cells() {
            halo.set_centre(cells);
        }
        let Some(links) = self.slots[id].links() else {
            return halo;
        };

        let last = CHUNK_N - 1;
        for dir in Dir::ALL {
            let Some(nid) = links[dir.index()] else {
                continue;
            };
            let Some(n) = self.slots[nid].cells() else {
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

    /// After a generation, any chunk whose edge carries life can produce a
    /// birth in the neighbour beyond it, so that neighbour must be simulated
    /// from now on. Collect first, then act: promoting mutates `slots`.
    fn ensure_frontier(&mut self) {
        let mut wanted: Vec<(ChunkId, Dir)> = Vec::new();
        for id in 0..self.slots.len() {
            let Some(cells) = self.slots[id].cells() else {
                continue;
            };
            for dir in Dir::ALL {
                if edge_has_life(cells, dir) {
                    wanted.push((id, dir));
                }
            }
        }
        for (id, dir) in wanted {
            self.ensure_neighbour_loaded(id, dir);
        }
    }

    fn ensure_neighbour_loaded(&mut self, id: ChunkId, dir: Dir) {
        let existing = self.slots[id].links().and_then(|l| l[dir.index()]);
        let nid = match existing {
            Some(nid) => nid,
            None => match self.kind {
                // The frontier grows: give the coordinate an identity, then
                // load it. `load` wires it to its own neighbours in turn.
                WorldKind::Infinite => {
                    let coord = offset(self.locs[id], dir);
                    let nid = self.ensure_slot_at(coord);
                    if let Some(links) = self.slots[id].links_mut() {
                        links[dir.index()] = Some(nid);
                    }
                    nid
                }
                // A tiled world's topology is fixed: a missing link is a real
                // edge, and life reaching it simply falls off.
                WorldKind::Tiled => return,
            },
        };
        if !self.slots[nid].is_loaded() {
            self.load(nid);
        }
    }

    /// Promote a slot to `CellChunk`. An `Idle` slot keeps the links it was
    /// built with; an `Unloaded` one has none, so an infinite world wires it.
    fn load(&mut self, id: ChunkId) {
        let links = match &self.slots[id] {
            Neighbour::CellChunk { .. } => return,
            Neighbour::Idle { links } => *links,
            Neighbour::Unloaded => [None; 8],
        };
        self.slots[id] = Neighbour::CellChunk {
            cells: Chunk::zeroed(),
            next: Chunk::zeroed(),
            links,
        };
        if self.kind == WorldKind::Infinite {
            self.wire(id);
        }
        self.dirty = true;
    }

    /// Populate a newly loaded chunk's eight links, reusing the slot already at
    /// each neighbouring coordinate or creating an `Unloaded` placeholder
    /// there. The reverse link is set too, so the graph stays symmetric —
    /// except into `Unloaded` slots, which hold no links by definition and get
    /// theirs when they are themselves promoted.
    fn wire(&mut self, id: ChunkId) {
        let loc = self.locs[id];
        for dir in Dir::ALL {
            let nid = self.ensure_slot_at(offset(loc, dir));
            if let Some(links) = self.slots[id].links_mut() {
                links[dir.index()] = Some(nid);
            }
            if let Some(links) = self.slots[nid].links_mut() {
                links[dir.opposite().index()] = Some(id);
            }
        }
    }

    fn ensure_slot_at(&mut self, coord: (i32, i32)) -> ChunkId {
        if let Some(&id) = self.coords.get(&coord) {
            return id;
        }
        let id = self.slots.len();
        self.slots.push(Neighbour::Unloaded);
        self.locs.push(coord);
        self.coords.insert(coord, id);
        id
    }
}

fn offset((row, col): (i32, i32), dir: Dir) -> (i32, i32) {
    let (dr, dc) = dir.delta();
    (row + dr, col + dc)
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

/// The standard glider, travelling down-right (south-east).
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

    /// The glider is seeded at chunk-local (6, 6) of chunk (0, 0) and travels
    /// south-east one cell every four generations. In absolute coordinates its
    /// cells are therefore the seed pattern offset by (k, k) after 4k steps --
    /// which stays true only if the halo carries life correctly across chunk
    /// borders.
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
    fn a_fresh_infinite_world_has_one_loaded_chunk() {
        let w = World::infinite();
        assert_eq!(w.loaded_count(), 1);
        assert_eq!(w.live_cells(), expected_after(0));
    }

    #[test]
    fn the_glider_crosses_a_chunk_boundary_intact() {
        let mut w = World::infinite();
        // 40 generations = 10 cells of travel, from local (6,6) to (16,16),
        // which is inside chunk (1, 1) -- so it crosses two borders and a
        // corner on the way.
        for _ in 0..40 {
            w.step();
        }
        assert_eq!(w.live_cells(), expected_after(10));
        assert!(
            w.loaded_count() > 1,
            "crossing a border must have loaded more chunks"
        );
    }

    #[test]
    fn the_glider_keeps_going_for_a_long_time() {
        let mut w = World::infinite();
        for _ in 0..400 {
            w.step();
        }
        assert_eq!(w.live_cells(), expected_after(100));
        assert_eq!(w.live_cells().len(), 5, "a glider is five cells, always");
    }

    #[test]
    fn loading_a_chunk_wires_all_eight_neighbours() {
        let mut w = World::infinite();
        for _ in 0..40 {
            w.step();
        }
        for id in 0..w.slot_count() {
            if !w.slot(id).is_loaded() {
                continue;
            }
            let links = w.slot(id).links().expect("a loaded chunk has links");
            for dir in Dir::ALL {
                let nid = links[dir.index()].expect("every direction is wired");
                // The link must point at the slot actually holding that
                // coordinate, and the reverse link must agree where it exists.
                assert_eq!(w.loc(nid), offset(w.loc(id), dir));
                if let Some(back) = w.slot(nid).links() {
                    assert_eq!(back[dir.opposite().index()], Some(id));
                }
            }
        }
    }

    #[test]
    fn unloaded_slots_hold_no_links_until_promoted() {
        let w = World::infinite();
        let unloaded: Vec<_> = (0..w.slot_count())
            .filter(|&id| matches!(w.slot(id), Neighbour::Unloaded))
            .collect();
        assert!(!unloaded.is_empty(), "the frontier should exist");
        for id in unloaded {
            assert!(w.slot(id).links().is_none());
        }
    }

    #[test]
    fn a_tiled_world_never_grows() {
        let mut w = World::tiled(3, 3);
        assert_eq!(w.slot_count(), 9);
        // Idle chunks are wired from the start, unlike an infinite world's
        // Unloaded placeholders.
        for id in 0..9 {
            assert!(w.slot(id).links().is_some());
        }
        for _ in 0..400 {
            w.step();
            assert_eq!(w.slot_count(), 9, "a tiled world has a fixed chunk set");
        }
        assert_eq!(w.live_cells().len(), 5, "the glider survives wrapping");
    }

    #[test]
    fn a_tiled_world_promotes_idle_chunks_rather_than_creating_them() {
        let mut w = World::tiled(3, 3);
        assert_eq!(w.loaded_count(), 1);
        for _ in 0..60 {
            w.step();
        }
        assert!(w.loaded_count() > 1, "life should have spread");
        assert_eq!(w.slot_count(), 9);
        assert!(
            (0..9).all(|id| !matches!(w.slot(id), Neighbour::Unloaded)),
            "a tiled world holds no Unloaded slots"
        );
    }
}
