use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::cell::{Cell, Chunk, ChunkMask, Halo, Kind, Takings, CHUNK_N};
use super::dir::Dir;
use super::player::PlayerId;
use super::rule;
use super::seed::Roll;

mod dynamite;
mod overclock;
mod turrets;

/// A blast is the dynamite pass's own record, and is named here as it was.
pub use dynamite::Blast;

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
#[derive(Clone)]
enum Storage {
    /// Unbounded plane. Only non-empty chunks are stored; an absent key is an
    /// empty chunk, which reads as dead and is recreated on demand.
    Infinite(HashMap<Coord, Chunk>),
    /// A fixed `rows` x `cols` torus, always fully allocated in one contiguous
    /// block. Coordinates wrap, so global coordinates map many-to-one onto
    /// these chunks.
    Toroidal { rows: i32, cols: i32, chunks: Box<[Chunk]> },
}

/// What shape a world is, and how big it is if it wraps.
///
/// Both "which world to open" and "which world this is": one enum, because
/// the two were the same two variants under different names and the shape now
/// travels on the wire, where a second spelling would need a translation that
/// could be got wrong.
///
/// A runtime value rather than a const. As a const, whichever arm was not
/// selected was dead code the compiler had to be told to ignore -- a world you
/// cannot pick is a world nobody plays and nobody notices breaking.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WorldKind {
    /// Grows as life reaches new ground.
    Infinite,
    /// Wraps, `rows` by `cols` chunks. Small enough and a player's granted
    /// ground wraps onto somebody else's.
    Toroidal { rows: i32, cols: i32 },
}

/// The largest torus this will build, per side and in total.
///
/// **Because a shape arrives over the wire**, and a torus is allocated whole:
/// unchecked, `rows: 0` and `100000x100000` each killed a whole server, every
/// room in it, from one message on a connection that had joined nothing.
/// Everything that builds one goes through [`WorldKind::checked`].
///
/// A budget and not a capacity — what a server can *step* four times a second
/// rather than what it can hold — so both numbers move when a chunk's size
/// does, and the per-side cap also refuses a corridor that fits the total.
/// The measurement and the arithmetic are in [docs/README.md#running-it].
///
/// [docs/README.md#running-it]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/README.md#running-it
pub const MAX_TORUS_SIDE: i32 = 128;
pub const MAX_TORUS_CHUNKS: i64 = 1_024;

impl WorldKind {
    /// A world of this shape with nothing in it, which is what both a server
    /// and a client that has just been told the shape start from.
    ///
    /// Only call this on a shape that has been through [`Self::checked`], or
    /// that came from somewhere that has -- [`parse_torus`] for a command
    /// line, and `Welcome` for a client, which is repeating a shape the server
    /// already built.
    pub fn build(self) -> World {
        match self {
            Self::Infinite => World::infinite_empty(),
            Self::Toroidal { rows, cols } => World::toroidal_empty(rows, cols),
        }
    }

    /// This shape, or why it is not one.
    ///
    /// **Every path from a client to [`Self::build`] goes through here.** A
    /// shape is three numbers on a wire and two of them can be anything a
    /// sender likes; see [`MAX_TORUS_SIDE`] for what that used to cost.
    pub fn checked(self) -> Result<Self, String> {
        let Self::Toroidal { rows, cols } = self else { return Ok(self) };
        for (n, what) in [(rows, "rows"), (cols, "columns")] {
            if n < 1 {
                return Err(format!("a torus needs at least one chunk of {what}, not {n}"));
            }
            if n > MAX_TORUS_SIDE {
                return Err(format!(
                    "{n} chunks of {what} is more than the {MAX_TORUS_SIDE} a torus may have"
                ));
            }
        }
        // In `i64`, because the product of two legal `i32`s is not one.
        let chunks = rows as i64 * cols as i64;
        if chunks > MAX_TORUS_CHUNKS {
            return Err(format!(
                "{rows}x{cols} is {chunks} chunks, and a torus holds at most {MAX_TORUS_CHUNKS}"
            ));
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct World {
    storage: Storage,
    /// **This game's own number**, mixed into every roll — see
    /// [`super::seed::generation_seed`].
    ///
    /// So two rooms holding identical cells do not roll identical dice, and a
    /// pattern carried from one to the other is contested differently even
    /// though it *lives* identically: the dice decide ownership, upkeep and
    /// tie-breaks, and [liveness is exactly B3/S23] whatever they say. That is
    /// what keeps a per-room seed compatible with reading somebody else's
    /// pattern and expecting it to behave.
    ///
    /// Derived from the room's id rather than sent, so it needs no field on
    /// the wire and no version in the save: `Welcome` already names the room,
    /// and a client that knows the room knows the number. See
    /// [`crate::net::world_seed`].
    ///
    /// [liveness is exactly B3/S23]: World::step
    seed: u64,
    /// Reused between generations so stepping allocates nothing.
    scratch: Vec<Halo>,
    active: Vec<Coord>,
    elapsed: f32,
    pub generation: u64,
    pub dirty: bool,
    /// **Blasts that went off this generation**, for whoever is drawing.
    ///
    /// Not a rule and not a bill — the bill is paid when the stick is laid,
    /// see [`super::rule::DYNAMITE_COST`]. It is here because nothing else
    /// says a blast happened: the cells before and the cells after are both
    /// just cells, so the largest thing a player can do reads as the board
    /// having glitched. Drained by the caller, like [`Self::dirty`] is
    /// cleared by one.
    pub blasts: Vec<Blast>,
}

/// Read a torus size written as `ROWSxCOLS`, in chunks.
///
/// Shared by both binaries so `--torus 18x18` means the same thing to each,
/// and so the error does too.
pub fn parse_torus(text: &str) -> Result<WorldKind, String> {
    let (rows, cols) =
        text.split_once(['x', 'X']).ok_or_else(|| format!("expected ROWSxCOLS, got {text:?}"))?;
    let parse = |v: &str, what: &str| {
        v.trim()
            .parse::<i32>()
            .ok()
            .filter(|&n| n > 0)
            .ok_or_else(|| format!("{what} must be a positive number of chunks, got {v:?}"))
    };
    WorldKind::Toroidal { rows: parse(rows, "rows")?, cols: parse(cols, "cols")? }.checked()
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

    /// An unbounded plane with a glider already on it.
    ///
    /// Not what a game opens with -- that is `infinite_empty`, since the first
    /// life arrives with the first player. This is for tests and examples that
    /// want something already moving.
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
        let mut w = Self::toroidal_empty(rows, cols);
        if let Storage::Toroidal { chunks, .. } = &mut w.storage {
            seed_glider(&mut chunks[0], CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, PlayerId(1));
        }
        w
    }

    /// A torus with nothing on it, which is what a game starts from: the
    /// world opens empty and every player brings a block. `toroidal` seeds a
    /// glider on top, and the tests that want something already moving use it.
    /// The assert is the last line of defence and not the first: a shape that
    /// came over a wire is refused by [`WorldKind::checked`] long before it
    /// reaches this, with a sentence rather than a panic. What is left here is
    /// the invariant, for a caller inside the crate that got it wrong.
    pub fn toroidal_empty(rows: i32, cols: i32) -> Self {
        assert!(rows > 0 && cols > 0, "a torus needs at least one chunk");
        let cells = rows as i64 * cols as i64;
        assert!(cells <= MAX_TORUS_CHUNKS, "a torus of {rows}x{cols} chunks is too big to build");
        let chunks = vec![Chunk::dead(); cells as usize].into_boxed_slice();
        Self::new(Storage::Toroidal { rows, cols, chunks })
    }

    /// How big the world is in cells, or `None` if it does not end.
    ///
    /// What a grant needs to know: on a torus the ground is finite and has to
    /// be shared out, and on an infinite world it does not.
    pub fn size_in_cells(&self) -> Option<(i32, i32)> {
        match &self.storage {
            Storage::Infinite(_) => None,
            Storage::Toroidal { rows, cols, .. } => {
                Some((rows * CHUNK_N as i32, cols * CHUNK_N as i32))
            }
        }
    }

    fn new(storage: Storage) -> Self {
        Self {
            storage,
            seed: 0,
            scratch: Vec::new(),
            active: Vec::new(),
            elapsed: 0.0,
            generation: 0,
            dirty: true,
            blasts: Vec::new(),
        }
    }

    /// Adopt this game's number, so its dice are its own.
    ///
    /// Set by whoever knows which room this is — the server from the room it
    /// named, a client from the room its `Welcome` named. Nought is a world
    /// nobody has said anything about, which is what a test and an offline
    /// game get, and is a perfectly good number to roll from.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    pub fn seed(&self) -> u64 {
        self.seed
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
            Storage::Toroidal { rows, cols, .. } => {
                WorldKind::Toroidal { rows: *rows, cols: *cols }
            }
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
        // Anything but a wholly empty cell needs somewhere to live -- a pane
        // over empty ground is still something, and would otherwise vanish
        // because no chunk was made for it.
        if cell != Cell::DEAD {
            self.ensure(chunk);
        }
        if let Some(c) = self.chunk_at_mut(chunk) {
            c[(row, col)] = cell;
            self.dirty = true;
        }
    }

    /// Read one cell by absolute cell coordinates. `None` where no chunk is
    /// held, which for an infinite world means nothing has ever lived there.
    pub fn cell_at(&self, row: i32, col: i32) -> Option<Cell> {
        let n = CHUNK_N as i32;
        let chunk = self.chunk_at((row.div_euclid(n), col.div_euclid(n)))?;
        Some(chunk[(row.rem_euclid(n) as usize, col.rem_euclid(n) as usize)])
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

    /// **Drop every stored chunk the predicate rejects**, and say how many went.
    ///
    /// Infinite worlds only. A torus is allocated whole — its chunks are an
    /// array and there is nothing to drop — so this answers nought there
    /// rather than pretending.
    ///
    /// A client uses it to stop paying for ground it walked away from an hour
    /// ago; see `client::session::forget_what_is_far`. Nothing on the server
    /// calls it, and nothing should: a server is the world, and a world that
    /// forgot its own chunks would be one where a player's country stopped
    /// existing when nobody was looking at it.
    pub fn forget_chunks(&mut self, keep: impl Fn(Coord) -> bool) -> usize {
        let Storage::Infinite(map) = &mut self.storage else { return 0 };
        let before = map.len();
        map.retain(|coord, _| keep(*coord));
        let gone = before - map.len();
        if gone > 0 {
            // The active set names chunks by coordinate and is rebuilt at the
            // top of every step, but the instance list and the digests are read
            // before that — so say the world moved.
            self.dirty = true;
        }
        gone
    }

    pub fn stored_count(&self) -> usize {
        match &self.storage {
            Storage::Infinite(map) => map.len(),
            Storage::Toroidal { rows, cols, .. } => (rows * cols) as usize,
        }
    }

    /// Chunks that must be stepped: every non-empty chunk, plus any neighbour
    /// something on its edge can reach — life, which can cause a birth there,
    /// or ownership, which can creep there.
    fn compute_active(&mut self) {
        let mut set: HashSet<Coord> = HashSet::new();
        for (coord, chunk) in self.stored() {
            if chunk.is_empty() {
                continue;
            }
            set.insert(coord);
            for dir in Dir::ALL {
                if edge_can_reach(chunk, dir) {
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

    /// Advance on a clock, and total what was earned over however many
    /// generations that turned out to be.
    pub fn update(&mut self, dt: f32, span: f32) -> Takings {
        let mut earned = Takings::default();
        if span <= 0.0 {
            return earned;
        }
        self.elapsed += dt;
        let mut steps = 0;
        while self.elapsed >= span && steps < MAX_CATCHUP_STEPS {
            self.elapsed -= span;
            earned.add(&self.step());
            steps += 1;
        }
        if steps == MAX_CATCHUP_STEPS {
            self.elapsed = 0.0;
        }
        earned
    }

    /// Advance one generation, and say what each player earned doing it.
    ///
    /// The tally is returned rather than applied: a world holds cells, not
    /// purses, and the server and the client each fold it into the number they
    /// keep. Summed in the order chunks are stepped, which is sorted, though
    /// integer addition would not care.
    pub fn step(&mut self) -> Takings {
        // Taken before the rule runs, so a cell touching a pane breaks it even
        // if this is the generation it dies in. It is alive now and it is
        // against the ice now, and that is the whole of what breaking means --
        // a cell that is about to die has still crashed into it.
        //
        // Taken before but acted on after, which is the point of splitting it:
        // shattering here as well would unfreeze what the pane covered in time
        // for this generation's rule, and a pattern drawn under ice would take
        // its first step in the same breath as being uncovered rather than
        // starting from exactly what was drawn.
        let seeds = self.ice_seeds();
        // **Before the rule, where the other two passes are after it.** A fuse
        // that reached full during the rule and went off in the same breath
        // would never be drawn full — and the whole of what makes a dynamite
        // answerable is that its last sprite is on screen for exactly one
        // generation, always. See [`Self::detonate`].
        // **Nothing is owed for it.** A blast used to be billed by area here
        // and fold into the tally the factories use; the whole price is paid
        // when the stick is laid now, because a bill that falls due later is a
        // bill somebody can be broke for — see `rule::DYNAMITE_COST`.
        self.detonate();
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

        // **Once for the generation, not once per chunk.** Everything a
        // cell's dice need that is not the cell: this world's number and the
        // tick. Each cell then mixes in its own absolute position, so a birth's
        // owner is chosen the same way on every peer without exchanging
        // anything, and the choice does not depend on how the world is stored.
        let generation = super::seed::generation_seed(self.seed, self.generation);

        let mut earned = Takings::default();
        for (i, &coord) in active.iter().enumerate() {
            let halo = self.scratch[i];
            let at = (coord.0 * CHUNK_N as i32, coord.1 * CHUNK_N as i32);
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into(chunk, generation, at, &mut earned);
            }
        }

        // **Then the discs again, before the generation is called done.** Every
        // overclocked cell runs the rule once more, reading the world as the
        // pass above left it and rolling dice of its own — see
        // [`Self::overclock_pass`].
        for pass in 1..rule::OVERCLOCK_RATE as u64 {
            self.overclock_pass(generation, pass, &mut earned);
        }

        self.active = active;
        self.generation += 1;
        self.break_ice_from(seeds);
        self.fire_turrets();
        self.dirty = true;
        self.prune();
        earned
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
}

/// Can anything on the edge facing `dir` change the chunk beyond it?
///
/// Life can, by causing a birth. **So can ownership**, now that territory
/// creeps: ground next to your ground becomes your ground, and the ground next
/// to it may be in the next chunk along.
///
/// Life alone was the test, and it made territory unable to cross a chunk
/// boundary at all. Nothing woke the neighbour, so nothing was stepped there,
/// so nothing was ever claimed there. It showed up as a granted patch that
/// crept right and down and not up or left — because a grant lands flush
/// against a chunk's top-left corner, so those two edges *are* the boundary
/// and the other two are interior.
fn edge_can_reach(chunk: &Chunk, dir: Dir) -> bool {
    let spreads = |cell: Cell| cell.is_alive() || cell.player().is_owned();
    let last = CHUNK_N - 1;
    match dir {
        Dir::N => (0..CHUNK_N).any(|c| spreads(chunk[(0, c)])),
        Dir::S => (0..CHUNK_N).any(|c| spreads(chunk[(last, c)])),
        Dir::W => (0..CHUNK_N).any(|r| spreads(chunk[(r, 0)])),
        Dir::E => (0..CHUNK_N).any(|r| spreads(chunk[(r, last)])),
        Dir::Nw => spreads(chunk[(0, 0)]),
        Dir::Ne => spreads(chunk[(0, last)]),
        Dir::Sw => spreads(chunk[(last, 0)]),
        Dir::Se => spreads(chunk[(last, last)]),
    }
}

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
mod tests;
