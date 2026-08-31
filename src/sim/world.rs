use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::cell::{Cell, Chunk, Halo, Kind, Mined, CHUNK_N};
use super::dir::Dir;
use super::player::PlayerId;
use super::rule;
use super::seed::Roll;

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
/// **Because a shape arrives over the wire.** `ClientMessage::Create` carries
/// a `WorldKind` straight off a socket, and a torus is allocated whole -- so
/// `rows: 0` reached an `assert!` and killed the process, and `100000x100000`
/// overflowed the `i32` multiply that sizes the allocation. Either one was a
/// whole server, every room in it, from one message on a connection that had
/// not joined anything. The release profile is `panic = "abort"`, so it did
/// not even unwind.
///
/// The numbers are what a server can actually step four times a second rather
/// than what it can hold: 16384 chunks is a little over four million cells,
/// and the per-side cap stops a 1x16384 world that fits the budget and is a
/// corridor nobody can play in.
pub const MAX_TORUS_SIDE: i32 = 512;
pub const MAX_TORUS_CHUNKS: i64 = 16_384;

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

    /// Advance on a clock, and total what was mined over however many
    /// generations that turned out to be.
    pub fn update(&mut self, dt: f32, span: f32) -> Mined {
        let mut mined = Mined::default();
        if span <= 0.0 {
            return mined;
        }
        self.elapsed += dt;
        let mut steps = 0;
        while self.elapsed >= span && steps < MAX_CATCHUP_STEPS {
            self.elapsed -= span;
            mined.add(&self.step());
            steps += 1;
        }
        if steps == MAX_CATCHUP_STEPS {
            self.elapsed = 0.0;
        }
        mined
    }

    /// Advance one generation, and say what each player mined doing it.
    ///
    /// The tally is returned rather than applied: a world holds cells, not
    /// purses, and the server and the client each fold it into the number they
    /// keep. Summed in the order chunks are stepped, which is sorted, though
    /// integer addition would not care.
    pub fn step(&mut self) -> Mined {
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

        let mut mined = Mined::default();
        for (i, &coord) in active.iter().enumerate() {
            let halo = self.scratch[i];
            let at = (coord.0 * CHUNK_N as i32, coord.1 * CHUNK_N as i32);
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into(chunk, generation, at, &mut mined);
            }
        }

        self.active = active;
        self.generation += 1;
        self.break_ice_from(seeds);
        self.fire_turrets();
        self.dirty = true;
        self.prune();
        mined
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

    /// Break every pane that life has reached.
    ///
    /// Any live cell in the eight neighbours breaks a pane — placed or born,
    /// whoever owns it — and takes the whole connected run of ice with it,
    /// because a pane is one object and cracking a corner of it does not leave
    /// the rest standing.
    ///
    /// One exception: a cell that is itself under ice. It is frozen, and a
    /// pane must not be broken by what it covers, or none could be laid over
    /// life at all.
    ///
    /// Connectivity is orthogonal. Panes are laid as rectangles, and two that
    /// meet only at a corner are two panes rather than one; joining them
    /// diagonally would let a break travel between panes that merely touch.
    ///
    /// Run after the rules, so it sees the generation that actually did the
    /// touching. Absolute coordinates throughout, so a pane spanning chunks
    /// breaks as one rather than stopping at a boundary.
    fn ice_seeds(&self) -> Vec<(i32, i32)> {
        // Life reaches diagonally, so a pane is touched by any of the eight.
        self.ice_cells()
            .into_iter()
            .filter(|&(row, col)| {
                Dir::ALL.iter().any(|dir| {
                    let (dr, dc) = dir.delta();
                    self.cell_at(row + dr, col + dc).is_some_and(|c| c.is_alive() && !c.is_ice())
                })
            })
            .collect()
    }

    /// Break every pane reached from these cells, and everything each pane is
    /// joined to.
    fn break_ice_from(&mut self, seeds: Vec<(i32, i32)>) {
        if seeds.is_empty() {
            return;
        }

        let mut broken: HashSet<(i32, i32)> = HashSet::new();
        let mut queue = seeds;
        while let Some(at) = queue.pop() {
            if !broken.insert(at) {
                continue;
            }
            for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let next = (at.0 + dr, at.1 + dc);
                if !broken.contains(&next)
                    && self.cell_at(next.0, next.1).is_some_and(|c| c.is_ice())
                {
                    queue.push(next);
                }
            }
        }

        for (row, col) in broken {
            if let Some(cell) = self.cell_at(row, col) {
                self.set_cell_at(row, col, cell.with_ice(false));
            }
        }
    }

    /// Every turret takes the nearest square it acts on, all at once.
    ///
    /// A pass rather than a rule, and for the same reason shattering ice is
    /// one: every rule in [`super::rule`] is a pure function of a cell and its
    /// eight neighbours, which is what lets a generation run out of a `Halo`
    /// with no bounds checks and no knowledge of topology. "The nearest square
    /// that is not mine" is a search no halo can answer.
    ///
    /// **Searched first, applied second**, which is the same discipline as
    /// gathering every halo before writing any of the next generation. Every
    /// turret reads the world as this generation left it, so no turret's
    /// answer depends on which turret went first — two aiming at one square
    /// simply agree or overwrite, and the list is sorted, so which of them
    /// wins is the same on every peer.
    fn fire_turrets(&mut self) {
        let turrets = self.turrets();
        if turrets.is_empty() {
            return;
        }

        // The same seed a cell's own rules roll from, at the same position
        // and generation. It does not have to be a different number, because a
        // turret asks on its own **stream** — which is what streams are for,
        // and is stronger than two constants nobody can check are unrelated.
        let generation = super::seed::generation_seed(self.seed, self.generation);

        let mut shots = Vec::new();
        for (at, owner, live) in turrets {
            let seed = super::seed::cell_seed(generation, at.0, at.1);
            let (targets, hit) = self.turret_targets(at, owner, live, seed);
            for &target in &targets[..hit] {
                let cell = self.cell_at(target.0, target.1).unwrap_or(Cell::DEAD);
                shots.push((
                    target,
                    if live {
                        // **Planted at full**, not nudged. The rule assigns a
                        // square the strongest claim reaching it rather than
                        // adding to what is there, so a push of three would be
                        // wiped the next time that square worked itself out --
                        // a turret that nudged would achieve nothing at all.
                        //
                        // Planting a flag instead is what a turret always did,
                        // and the level field gives it the brake the old one
                        // needed a constant for: a planted square with nothing
                        // of its owner's near enough to feed it falls back on
                        // its own, so what a turret holds is however much it
                        // can plant against however fast the rule takes it
                        // back.
                        cell.with_player(owner).with_level(rule::TURRET_PUSH)
                    } else {
                        // The mirror, and it takes the square to nothing in one
                        // go for the same reason: half-draining it would be
                        // undone before it mattered. A live cell must have an
                        // owner -- `Cell::alive` asserts it, because unowned
                        // life would have nobody to attribute a birth to -- so
                        // taking a square away from its owner kills whatever
                        // stood on it, which is why a dead turret kills
                        // without a rule about killing.
                        cell.with_alive(false).with_player(PlayerId::UNOWNED).with_level(0)
                    },
                ));
            }
        }

        for ((row, col), cell) in shots {
            self.set_cell_at(row, col, cell);
        }
    }

    /// The squares a turret acts on: the [`rule::TURRET_POWER`] nearest that
    /// answer its question, nearest first, and however many fewer it found.
    ///
    /// One search per square rather than one search for all of them, each
    /// excluding what the last took. Nearest-first falls out of that, and it
    /// costs a second walk of a box already in cache — where collecting the
    /// whole box and sorting it would allocate per turret per generation to
    /// answer a question about its first few entries.
    ///
    /// Each shot mixes its own index into the seed, so a volley does not break
    /// every tie the same way.
    fn turret_targets(
        &self,
        at: (i32, i32),
        owner: PlayerId,
        live: bool,
        seed: u64,
    ) -> ([(i32, i32); rule::TURRET_POWER], usize) {
        let mut chosen = [(0, 0); rule::TURRET_POWER];
        let mut hit = 0;
        // A live turret asks for ground that is not its owner's, and only when
        // there is none within reach does it fall back to reinforcing its own.
        // Falling back once rather than per shot, so a volley that ran out of
        // frontier finishes on the thin ground behind it.
        let mut aim = if live { Aim::Take } else { Aim::Give };
        while hit < rule::TURRET_POWER {
            let shot = super::seed::mix(seed, hit as u64);
            match self.turret_target(at, owner, aim, shot, &chosen[..hit]) {
                Some(next) => {
                    chosen[hit] = next;
                    hit += 1;
                }
                None if aim == Aim::Take => aim = Aim::Reinforce,
                None => break,
            }
        }
        (chosen, hit)
    }

    /// The square a turret acts on: the nearest one that answers its question
    /// and is not already `taken` by this volley, and one of them at random
    /// where several tie.
    ///
    /// The tie-break is the whole reason there is a roll here. A ring holds
    /// many squares at the same distance, and letting the scan order choose
    /// between them would have every turret in the world prefer the same
    /// direction — territory would grow in a lopsided plume that reads as a
    /// bug rather than as a rule.
    ///
    /// Two passes over the box rather than a list of candidates: the first
    /// finds the nearest distance and counts how many share it, the second
    /// walks to the one the roll picked. That costs a second read of a box
    /// that is already in cache and saves allocating per turret per
    /// generation.
    ///
    /// A disc, not a square. The box is what is walked, `d > reach²` is what
    /// makes the reach the same in every direction.
    fn turret_target(
        &self,
        at: (i32, i32),
        owner: PlayerId,
        aim: Aim,
        seed: u64,
        taken: &[(i32, i32)],
    ) -> Option<(i32, i32)> {
        let reach = rule::TURRET_REACH;
        let mut best = i32::MAX;
        let mut ties = 0usize;
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                let d = dr * dr + dc * dc;
                if d == 0 || d > reach * reach || d > best {
                    continue;
                }
                if taken.contains(&(at.0 + dr, at.1 + dc)) {
                    continue;
                }
                if !self.turret_wants((at.0 + dr, at.1 + dc), owner, aim) {
                    continue;
                }
                if d < best {
                    best = d;
                    ties = 1;
                } else {
                    ties += 1;
                }
            }
        }
        if ties == 0 {
            return None;
        }

        let mut nth = Roll::new(seed).pick(rule::TURRET_STREAM, ties);
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                let d = dr * dr + dc * dc;
                if d != best {
                    continue;
                }
                let target = (at.0 + dr, at.1 + dc);
                if taken.contains(&target) || !self.turret_wants(target, owner, aim) {
                    continue;
                }
                if nth == 0 {
                    return Some(target);
                }
                nth -= 1;
            }
        }
        unreachable!("the second pass walks the same squares the first counted")
    }

    /// Whether a turret will act on this square, for the [`Aim`] it is asking
    /// with.
    ///
    /// **Dead squares only**, for both of a live turret's aims: claiming a
    /// living cell would hand its owner the cell itself rather than the square
    /// under it, there being one owner field, and territory has never worked
    /// that way. Unheld ground counts as dead and unowned, which is exactly
    /// what an absent chunk reads as and exactly what a turret is for
    /// reaching.
    ///
    /// A **dead** turret is the mirror and takes its owner's own squares,
    /// alive or not. `HOME` is exempt for the same reason it never decays: it
    /// is the ground its owner can still build on at the base rate, and a
    /// machine of theirs that failed should not be what takes that away.
    ///
    /// `HOME` is exempt from reinforcing too, and needs no arm saying so:
    /// granted ground is a source, so [`Cell::influence`] already reads it as
    /// full and there is nothing to top up.
    ///
    /// Ice is exempt from all three. A pane stops time over what it covers,
    /// and a pane's cover is not claimed out from under it.
    fn turret_wants(&self, at: (i32, i32), owner: PlayerId, aim: Aim) -> bool {
        let cell = self.cell_at(at.0, at.1).unwrap_or(Cell::DEAD);
        if cell.is_ice() {
            return false;
        }
        match aim {
            Aim::Take => !cell.is_alive() && cell.player() != owner,
            Aim::Reinforce => {
                !cell.is_alive() && cell.player() == owner && cell.influence() < rule::TURRET_PUSH
            }
            Aim::Give => cell.player() == owner && !cell.is_home(),
        }
    }

    /// Every turret, in absolute coordinates, with its owner and whether it is
    /// alive.
    ///
    /// **Sorted**, because `stored` walks a `HashMap` on an infinite world and
    /// a `HashMap` iterates differently in different processes. Two turrets
    /// aiming at one square is decided by which fires last, so an unsorted
    /// list would let a client and a server disagree about who owns it.
    ///
    /// A scan rather than an index, the way `ice_cells` is. The world has no
    /// list of anything, and a turret is found by looking, which costs one
    /// pass over what is held per generation.
    fn turrets(&self) -> Vec<((i32, i32), PlayerId, bool)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen turret does not fire: a pane stops time over
                    // whatever it covers, and that is every rule, not just the
                    // ones inside `rule`.
                    if cell.kind() != Kind::TURRET || cell.is_ice() {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        (crow * CHUNK_N as i32 + row as i32, ccol * CHUNK_N as i32 + col as i32),
                        cell.player(),
                        cell.is_alive(),
                    ));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Every iced cell, in absolute coordinates.
    fn ice_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    if chunk[(row, col)].is_ice() {
                        out.push((
                            crow * CHUNK_N as i32 + row as i32,
                            ccol * CHUNK_N as i32 + col as i32,
                        ));
                    }
                }
            }
        }
        out
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

/// What a turret is looking for on one shot.
///
/// A live turret has **two** aims and takes them in order, which is what
/// makes it work from the middle of a country as well as from its edge.
/// The rule here has never changed; the world around it did. Before
/// territory was a level, a player's ground was a tight halo, so ground
/// that was not theirs was within six cells of anywhere they would put a
/// turret. Now granted ground is a source and a country reaches much
/// further, so a turret standing inside one finds its whole disc already
/// owned and had nothing to do.
///
/// Reinforcing is strictly the fallback, and that is the whole of why it
/// is safe. Making it the only rule was tried when levels arrived and
/// quietly ruined the piece: influence falls off, so from the middle of a
/// country the nearest thin square is a step or two away and a turret
/// spent its life topping up ground it already held instead of pushing on
/// anybody. Asked second, it only ever fires when there was nobody to push
/// on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Aim {
    /// A live turret's first choice: ground that is not its owner's.
    Take,
    /// Its fallback: its owner's own thinnest ground, planted back up to
    /// full, which feeds the frontier through the sum rather than at it.
    Reinforce,
    /// A dead turret, running the first backwards.
    Give,
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

    /// Dead ground held by `player`, so a turret can stand somewhere already
    /// its owner's and the nearest square it wants is out of reach of the
    /// territory rule — which claims everything beside a living cell and would
    /// otherwise be indistinguishable from the turret doing it.
    fn own_ground(w: &mut World, rows: (i32, i32), cols: (i32, i32), player: PlayerId) {
        for row in rows.0..=rows.1 {
            for col in cols.0..=cols.1 {
                let cell = w.cell_at(row, col).unwrap_or(Cell::DEAD);
                w.set_cell_at(row, col, cell.with_player(player));
            }
        }
    }

    fn owned_by(w: &World, player: PlayerId) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in w.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    if chunk[(row, col)].player() == player {
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

    fn turret(player: PlayerId) -> Cell {
        Cell::alive(player).with_kind(Kind::TURRET)
    }

    /// One square a generation, and the nearest one that is not its owner's.
    ///
    /// The patch is nine across with the turret in the middle, so the four
    /// squares just outside it tie at five cells and everything else is
    /// further — and all four are far enough from the only living cell that
    /// the territory rule cannot have been what claimed them.
    #[test]
    fn a_turret_claims_the_nearest_square_that_is_not_its_owners() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        own_ground(&mut w, (0, 8), (0, 8), me);
        w.set_cell_at(4, 4, turret(me));

        let before = owned_by(&w, me);
        w.fire_turrets();
        let after = owned_by(&w, me);

        let nearest = [(-1, 4), (9, 4), (4, -1), (4, 9)];
        assert!(
            rule::TURRET_POWER <= nearest.len(),
            "this test's geometry assumes a volley fits in the four squares that tie"
        );
        assert_eq!(
            after.len(),
            before.len() + rule::TURRET_POWER,
            "a turret takes TURRET_POWER squares a generation"
        );
        for claimed in after.iter().filter(|c| !before.contains(c)) {
            assert!(nearest.contains(claimed), "{claimed:?} is not one of the nearest four");
        }
    }

    /// A volley aims each shot somewhere new. Without excluding what the last
    /// shot took, every shot in a volley finds the same nearest square — the
    /// world is not written until all the searching is done, so the square is
    /// still there to be found.
    #[test]
    fn a_volley_does_not_aim_twice_at_one_square() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        let r = rule::TURRET_REACH;
        own_ground(&mut w, (-r, r), (-r, r), me);
        w.set_cell_at(0, 0, turret(me));
        // Two gaps in the owner's ground at different distances, so there is
        // a nearest and a next-nearest and no tie to break.
        w.set_cell_at(0, 3, Cell::DEAD);
        w.set_cell_at(0, 5, Cell::DEAD);

        let first = w.turret_target((0, 0), me, Aim::Take, 0, &[]).expect("a gap within reach");
        assert_eq!(first, (0, 3), "the nearer gap");
        let second =
            w.turret_target((0, 0), me, Aim::Take, 0, &[first]).expect("and the one past it");
        assert_eq!(second, (0, 5), "once the nearer one is spoken for");
    }

    /// **A turret in the middle of a country still works.** With nobody to
    /// push on it reinforces instead: it takes nobody's ground, and plants its
    /// owner's thinnest square back up to full, which feeds the frontier
    /// through the sum rather than at it.
    ///
    /// Before territory was a level this case could barely arise, because a
    /// player's halo was tight enough that ground which was not theirs sat
    /// within reach of anywhere they would stand one. A country reaches much
    /// further now, and the turret had nothing to do.
    #[test]
    fn a_turret_inside_its_owners_ground_reinforces_it() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        let r = rule::TURRET_REACH;
        own_ground(&mut w, (-r, r), (-r, r), me);
        w.set_cell_at(0, 0, turret(me));

        let before = owned_by(&w, me);
        let thin = |w: &World| {
            owned_by(w, me)
                .into_iter()
                .filter(|&(r, c)| {
                    w.cell_at(r, c).is_some_and(|x| x.influence() >= rule::TURRET_PUSH)
                })
                .count()
        };
        let full_before = thin(&w);
        w.fire_turrets();

        assert_eq!(owned_by(&w, me), before, "nothing within reach was anyone else's to take");
        assert_eq!(
            thin(&w),
            full_before + rule::TURRET_POWER,
            "a turret with no frontier in reach should have planted on its own thin ground"
        );
    }

    /// Reinforcing is strictly the **fallback**. Making it the only rule was
    /// tried when levels arrived and ruined the piece: from inside a country
    /// the nearest thin square is a step away, so a turret would top up ground
    /// it already held rather than push on anybody.
    #[test]
    fn a_turret_with_a_frontier_in_reach_pushes_rather_than_reinforcing() {
        let mut w = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        // The whole disc is its owner's and every square of it is thin --
        // unowned ground would be takeable, so the box has to cover the reach
        // -- with one square of somebody else's four cells out, further than
        // the thin ground going begging right beside it.
        let r = rule::TURRET_REACH;
        own_ground(&mut w, (-r, r), (-r, r), me);
        w.set_cell_at(0, 0, turret(me));
        w.set_cell_at(0, 4, Cell::DEAD.with_player(them));

        w.fire_turrets();

        assert_eq!(
            w.cell_at(0, 4).map(|c| c.player()),
            Some(me),
            "the frontier is what a turret goes for, thin ground beside it or not"
        );
    }

    /// A turret takes **ground**, so it wants a dead square that is not its
    /// owner's. Not the life standing on one — there is a single owner field,
    /// so claiming a living cell would hand over the cell rather than the
    /// square, and territory has never worked that way.
    #[test]
    fn a_turret_claims_ground_and_not_the_life_standing_on_it() {
        let mut w = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        w.set_cell_at(0, 1, Cell::DEAD.with_player(them));
        w.set_cell_at(0, 2, Cell::alive(them));
        w.set_cell_at(0, 3, Cell::DEAD.with_player(me));
        w.set_cell_at(0, 4, Cell::DEAD.with_player(them).with_ice(true));

        assert!(w.turret_wants((0, 1), me, Aim::Take), "their ground is what a turret takes");
        assert!(!w.turret_wants((0, 2), me, Aim::Take), "their life is not");
        assert!(!w.turret_wants((0, 3), me, Aim::Take), "nor is what is already theirs");
        assert!(
            !w.turret_wants((0, 4), me, Aim::Take),
            "a pane's cover is not claimed from under it"
        );
        assert!(w.turret_wants((5, 5), me, Aim::Take), "and ground nobody holds counts");
    }

    /// A live cell must have an owner, so taking a square away from its owner
    /// kills whatever was standing on it. The killing is that invariant rather
    /// than a rule of its own.
    #[test]
    fn a_dead_turret_takes_its_owners_ground_back_and_kills_what_stands_on_it() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        w.set_cell_at(0, 0, turret(me).with_alive(false));
        w.set_cell_at(0, 1, Cell::alive(me));

        w.fire_turrets();

        let hit = w.cell_at(0, 1).unwrap();
        assert!(!hit.is_alive(), "unowning a living square kills it");
        assert_eq!(hit.player(), PlayerId::UNOWNED);
    }

    /// Granted ground is exempt, for the reason it never decays: it is the
    /// ground its owner still builds on at the base rate, so a machine of
    /// theirs that failed must not be what takes that away.
    #[test]
    fn a_dead_turret_does_not_eat_home_ground() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        w.set_cell_at(0, 0, turret(me).with_alive(false));
        w.set_cell_at(0, 1, Cell::DEAD.with_player(me).with_home(true));
        w.set_cell_at(0, 5, Cell::DEAD.with_player(me));

        w.fire_turrets();

        let home = w.cell_at(0, 1).unwrap();
        assert_eq!(home.player(), me, "home ground is not what a dead turret takes");
        assert!(home.is_home());
        assert_eq!(
            w.cell_at(0, 5).unwrap().player(),
            PlayerId::UNOWNED,
            "so it reached past it for the next square that was the owner's"
        );
    }

    /// A pane stops time over whatever it covers, and that is every rule
    /// rather than only the ones inside `rule`.
    #[test]
    fn a_turret_under_ice_does_not_fire() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        w.set_cell_at(0, 0, turret(me).with_ice(true));

        let before = owned_by(&w, me);
        w.fire_turrets();
        assert_eq!(owned_by(&w, me), before, "a frozen turret claimed ground");
    }

    /// Four turrets in a block: the cheapest thing in Conway that never dies
    /// and never gives birth, which is why a turret is placed in fours. One on
    /// its own has no live neighbours and is gone in a generation.
    #[test]
    fn a_block_of_four_turrets_never_dies_and_never_breeds() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        let block = [(0, 0), (0, 1), (1, 0), (1, 1)];
        for (row, col) in block {
            w.set_cell_at(row, col, turret(me));
        }
        for _ in 0..50 {
            w.step();
        }
        for (row, col) in block {
            let cell = w.cell_at(row, col).unwrap();
            assert!(cell.is_alive(), "({row}, {col}) died");
            assert_eq!(cell.kind(), Kind::TURRET, "({row}, {col}) stopped being a turret");
        }
        assert_eq!(w.live_cells().len(), 4, "a still life gives no births, so it stays four");
    }

    #[test]
    fn a_lone_turret_dies_of_loneliness() {
        let mut w = World::infinite_empty();
        w.set_cell_at(0, 0, turret(PlayerId(1)));
        w.step();
        assert!(!w.cell_at(0, 0).unwrap().is_alive());
    }

    /// **A world's number changes its dice and not its life.**
    ///
    /// Which is what makes a seed per room safe. The dice decide who owns a
    /// birth, when a square works out what reaches it, and which of several
    /// tied squares a turret takes — none of which is whether a cell lives. So
    /// two rooms holding the same pattern watch it do the same thing and
    /// disagree about whose it is, and a pattern carried in from somewhere
    /// else behaves the way it is written down.
    ///
    /// The pair of assertions is the point. Either alone would pass for the
    /// wrong reason: identical liveness with identical ownership means the
    /// seed is doing nothing, and different ownership with different liveness
    /// means it is doing far too much.
    #[test]
    fn a_worlds_own_number_changes_its_dice_and_not_its_life() {
        let build = |seed: u64| {
            let mut w = World::infinite_empty();
            w.set_seed(seed);
            // Two players growing into each other, so births are contested and
            // there is something for the dice to decide.
            for (r, c) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
                w.set_cell_at(r, c, Cell::alive(PlayerId(1)));
                w.set_cell_at(r, c + 6, Cell::alive(PlayerId(2)));
            }
            w
        };
        let (mut a, mut b) = (build(0), build(0x9E37_79B9_7F4A_7C15));
        for _ in 0..60 {
            a.step();
            b.step();
        }

        assert_eq!(a.live_cells(), b.live_cells(), "the seed changed which cells are alive");

        // Over the **ground**, not over the life. Whose a live cell is comes
        // from its parents, and two patterns far enough apart never share one,
        // so the roll that picks a parent has nothing to pick between. Where
        // the dice show is territory: `rule::LEVEL_ADJUST` decides *when* a
        // square works out what reaches it, so the two fields settle along
        // different paths even where they would settle the same in the end.
        let held = |w: &World| {
            (-8..24)
                .flat_map(|r| (-8..24).map(move |c| (r, c)))
                .map(|(r, c)| w.cell_at(r, c).unwrap_or(Cell::DEAD).0[0])
                .collect::<Vec<_>>()
        };
        assert_ne!(held(&a), held(&b), "the seed changed nothing about who holds what");
    }

    /// **The liveness of this world is plain Conway, exactly.**
    ///
    /// Which is the premise of everything Golly-shaped — see
    /// [planned.md](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#experiments):
    /// a pattern written down by somebody else runs here the way it runs
    /// anywhere, or reading fifty years of other people's work is reading it
    /// wrong. It is not obvious from the code that it holds. Three of the four
    /// things this simulation adds to Conway do not touch whether a cell
    /// lives — territory writes the owner byte of dead squares, a mine is a
    /// tally, and ice is inert until a pane is laid — but that is an argument,
    /// and this is the measurement.
    ///
    /// Against a reference stepper written out longhand rather than against a
    /// recorded answer, so it says *which generation* diverged and not merely
    /// that something did.
    ///
    /// The two things that **do** touch liveness are turrets and ice, and
    /// neither is on this board. That is the whole of the caveat and it is
    /// worth stating where somebody will find it.
    #[test]
    fn liveness_is_exactly_b3_s23() {
        const N: usize = 64;
        let mut soup = [[false; N]; N];
        let mut w = World::toroidal_empty(4, 4);
        assert_eq!(w.size_in_cells(), Some((N as i32, N as i32)));

        // A fixed soup, from the dice the simulation already uses, so this
        // starts from something busy rather than from something chosen.
        for r in 0..N {
            for c in 0..N {
                let seed = super::super::seed::mix(0x5011_5EED, (r as u64) << 32 | c as u64);
                if Roll::new(seed).chance(0, 24) {
                    soup[r][c] = true;
                    w.set_cell_at(r as i32, c as i32, Cell::alive(PlayerId(1)));
                }
            }
        }

        let step_reference = |grid: &[[bool; N]; N]| {
            let mut next = [[false; N]; N];
            for r in 0..N {
                for c in 0..N {
                    let mut live = 0;
                    for dr in [N - 1, 0, 1] {
                        for dc in [N - 1, 0, 1] {
                            if (dr, dc) == (0, 0) {
                                continue;
                            }
                            if grid[(r + dr) % N][(c + dc) % N] {
                                live += 1;
                            }
                        }
                    }
                    next[r][c] = if grid[r][c] { live == 2 || live == 3 } else { live == 3 };
                }
            }
            next
        };

        for generation in 1..=200 {
            w.step();
            soup = step_reference(&soup);

            let theirs: Vec<(i32, i32)> = (0..N)
                .flat_map(|r| (0..N).map(move |c| (r, c)))
                .filter(|&(r, c)| soup[r][c])
                .map(|(r, c)| (r as i32, c as i32))
                .collect();
            assert_eq!(w.live_cells(), theirs, "generation {generation} is not what Conway does");
        }
        assert!(!w.live_cells().is_empty(), "the soup died out; test proves nothing");
    }

    /// The pass is part of the step, so it is under the same contract: two
    /// worlds given the same start stay byte-identical. The tie-break is a
    /// seeded roll and the turret list is sorted, which is what makes that
    /// true across processes where `stored` walks a `HashMap`.
    #[test]
    fn firing_turrets_is_deterministic() {
        let build = || {
            let mut w = World::infinite_empty();
            for (player, at) in [(1u8, (0, 0)), (2, (7, 9)), (1, (20, 3))] {
                let me = PlayerId(player);
                own_ground(&mut w, (at.0, at.0 + 3), (at.1, at.1 + 3), me);
                w.set_cell_at(at.0 + 1, at.1 + 1, turret(me));
                w.set_cell_at(at.0 + 2, at.1 + 2, turret(me).with_alive(false));
            }
            w
        };
        let (mut a, mut b) = (build(), build());
        for _ in 0..40 {
            a.step();
            b.step();
        }
        assert_eq!(a.digest(), b.digest());
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
    fn an_infinite_world_stores_only_chunks_that_hold_something() {
        let mut w = World::infinite();
        for _ in 0..400 {
            w.step();
        }
        // Nothing wholly empty is kept: that is what pruning is for, and
        // without it the glider's wake grew without bound.
        for (coord, chunk) in w.stored() {
            assert!(!chunk.is_empty(), "chunk {coord:?} is empty but stored");
        }

        // And the wake is bounded, which is what territory decay bought.
        //
        // A glider claims the ground it crosses. With no die-off it kept every
        // square it had ever touched, so a chunk was held for each and the
        // world grew for as long as anything moved -- twenty-five chunks and
        // climbing after four hundred generations, for five live cells.
        // Ground with nothing alive beside it now loses its owner, so the
        // trail fades behind the glider and only what it is currently over is
        // held.
        assert!(
            w.stored_count() <= 8,
            "a glider should hold a handful of chunks, not a trail; got {}",
            w.stored_count()
        );

        let claimed = w
            .stored()
            .iter()
            .filter(|(_, c)| {
                (0..CHUNK_N).any(|r| (0..CHUNK_N).any(|k| c[(r, k)].player().is_owned()))
            })
            .count();
        assert_eq!(claimed, w.stored_count(), "every chunk kept is kept for a reason");
    }

    /// Territory has to be able to leave the chunk it started in.
    ///
    /// A granted patch lands flush against a chunk's top-left corner, so two of
    /// its edges *are* the boundary. While only life woke a neighbouring chunk,
    /// ground crept right and down — which is interior — and never up or left,
    /// which is exactly what it looked like on screen.
    #[test]
    fn territory_creeps_across_a_chunk_boundary() {
        let me = PlayerId(1);
        let mut w = World::infinite_empty();
        // Claimed ground in the corner of chunk (0, 0), with a block standing
        // on it so the ground is held rather than fading.
        for r in 0..6 {
            for c in 0..6 {
                w.set_cell_at(r, c, Cell::DEAD.with_player(me));
            }
        }
        for (r, c) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
            w.set_cell_at(r, c, Cell::alive(me));
        }

        let (mut north, mut west) = (false, false);
        for _ in 0..400 {
            w.step();
            north |=
                (-4..0).any(|r| (0..6).any(|c| w.cell_at(r, c).is_some_and(|x| x.player() == me)));
            west |=
                (0..6).any(|r| (-4..0).any(|c| w.cell_at(r, c).is_some_and(|x| x.player() == me)));
        }
        assert!(north, "nothing ever crept north out of the chunk");
        assert!(west, "nothing ever crept west out of the chunk");
    }

    /// Ground is lost as well as won, and granted ground is not.
    #[test]
    fn territory_decays_where_nothing_is_alive_but_home_does_not() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);

        // A patch of plain claimed ground, and a patch of granted ground, both
        // with nothing alive anywhere near them.
        for c in 0..8 {
            w.set_cell_at(0, c, Cell::DEAD.with_player(me));
            w.set_cell_at(4, c, Cell::DEAD.with_player(me).with_home(true));
        }

        let owned = |w: &World, row: i32| {
            (0..8).filter(|&c| w.cell_at(row, c).unwrap().player() == me).count()
        };
        assert_eq!(owned(&w, 0), 8);

        // Long enough that a one-in-sixteen chance has almost certainly come
        // up for every square.
        for _ in 0..200 {
            w.step();
        }

        assert_eq!(owned(&w, 0), 0, "ground with nothing alive beside it fades");
        assert_eq!(owned(&w, 4), 8, "granted ground is the floor and stays");
    }

    /// Decay only reaches ground that life has left. A pattern holds the
    /// squares around it for as long as it is alive, or a blinker would flicker
    /// its own ground away.
    #[test]
    fn territory_beside_life_is_held() {
        let mut w = World::infinite_empty();
        let me = PlayerId(1);
        // A block: four cells that live forever without moving.
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            w.set_cell_at(r, c, Cell::alive(me));
        }
        for _ in 0..200 {
            w.step();
        }
        // The eight squares around it are touching life every generation, so
        // they are re-claimed as fast as they could ever decay.
        let ring = [(-1, -1), (-1, 0), (-1, 1), (-1, 2), (0, -1), (0, 2), (2, 0), (2, 1)];
        for (r, c) in ring {
            assert_eq!(
                w.cell_at(r, c).unwrap().player(),
                me,
                "({r}, {c}) beside a block should stay claimed"
            );
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
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
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
            assert_eq!(w.live_cells().len(), 5, "{rows}x{cols}: the glider must survive wrapping");
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
                assert!(std::ptr::eq(w.chunk_at((row, col)).unwrap(), w.chunk_at(canon).unwrap()));
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

    /// A wrapping world repeats for as far as anyone can pan.
    ///
    /// The renderer asks every chunk position the viewport covers which chunk
    /// fills it, so this is the whole of it: fold a viewport a hundred worlds
    /// from the origin and every position lands on a real chunk. It used to
    /// draw a fixed number of copies either side of the original instead, and
    /// panning off the last of them fell into blank space forever.
    #[test]
    fn a_torus_repeats_however_far_you_pan() {
        let w = World::toroidal(2, 3);

        // A viewport far out in both axes, and one far out negative -- the
        // world has no origin, so east and west must behave alike.
        for corner in [(200, 300), (-200, -300), (201, -299)] {
            let covering: Vec<Coord> = (corner.0..corner.0 + 4)
                .flat_map(|r| (corner.1..corner.1 + 6).map(move |c| (r, c)))
                .collect();
            let landed: HashSet<Coord> = covering.iter().map(|&c| w.canonical(c)).collect();

            for &at in &covering {
                let onto = w.canonical(at);
                assert!(
                    w.chunk_at(onto).is_some(),
                    "{at:?} folded to {onto:?}, which is not a chunk"
                );
            }
            assert_eq!(landed.len(), 6, "a viewport that wide should cover every chunk");
        }

        // And the many-to-one is what makes it repeat: one chunk fills many
        // positions, exactly one world apart.
        assert_eq!(w.canonical((0, 0)), w.canonical((2, 3)));
        assert_eq!(w.canonical((0, 0)), w.canonical((-200, 300)));
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

        // Same cells alive, different owner. Any chunk will do so long as it
        // holds life -- most of them now hold only claimed ground, since a
        // glider's trail is kept for its owner marks.
        let coord = b
            .stored()
            .iter()
            .find(|(_, c)| (0..CHUNK_N).any(|r| (0..CHUNK_N).any(|k| c[(r, k)].is_alive())))
            .expect("something should still be alive")
            .0;
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

    /// A pane is one object: touching any of it breaks all of it, however far
    /// it runs and across however many chunks.
    #[test]
    fn touching_a_pane_shatters_the_whole_run() {
        let mut w = World::infinite_empty();
        // A run of ice 40 cells long, which spans three chunks at 16 wide.
        for col in 0..40 {
            w.set_cell_at(0, col, Cell::DEAD.with_ice(true).with_player(PlayerId(2)));
        }
        // A block far along it, so the break has to travel back to the start.
        for (r, c) in [(1, 38), (1, 39), (2, 38), (2, 39)] {
            w.set_cell_at(r, c, Cell::alive(PlayerId(1)));
        }
        assert!(w.cell_at(0, 0).unwrap().is_ice());

        w.step();

        for col in 0..40 {
            assert!(
                !w.cell_at(0, col).map(|c| c.is_ice()).unwrap_or(false),
                "ice at column {col} survived"
            );
        }
    }

    /// Two panes that meet only at a corner are two panes. Breaking one must
    /// not break the other, or every pane on a diagonal falls together.
    #[test]
    fn a_break_does_not_jump_a_diagonal_join() {
        let mut w = World::infinite_empty();
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            w.set_cell_at(r, c, Cell::DEAD.with_ice(true).with_player(PlayerId(2)));
        }
        // Touching only at (1,1)'s corner.
        for (r, c) in [(2, 2), (2, 3), (3, 2), (3, 3)] {
            w.set_cell_at(r, c, Cell::DEAD.with_ice(true).with_player(PlayerId(2)));
        }
        // Life against the first pane only.
        for (r, c) in [(-1, -1), (-1, 0), (-2, -1), (-2, 0)] {
            w.set_cell_at(r, c, Cell::alive(PlayerId(1)));
        }

        w.step();

        assert!(!w.cell_at(0, 0).unwrap().is_ice(), "the touched pane breaks");
        assert!(
            w.cell_at(3, 3).unwrap().is_ice(),
            "the pane it only meets at a corner should stand"
        );
    }

    /// Ice with nothing alive beside it is left alone.
    #[test]
    fn an_untouched_pane_stands() {
        let mut w = World::infinite_empty();
        for col in 0..8 {
            w.set_cell_at(0, col, Cell::DEAD.with_ice(true).with_player(PlayerId(2)));
        }
        for _ in 0..10 {
            w.step();
        }
        for col in 0..8 {
            assert!(w.cell_at(0, col).unwrap().is_ice(), "column {col} broke");
        }
    }

    /// A pane with a margin round the life it covers is not broken by that
    /// life, and this is why: every cell that could be born from the frozen
    /// pattern lies inside the pane, where the rule returns it unchanged, so
    /// nothing is ever born outside to touch it. Lay the pane tightly instead
    /// and it breaks at once, which is `life_born_beside_a_pane_breaks_it`.
    ///
    /// Says nothing about life arriving from elsewhere — see the test below,
    /// which is what actually breaks a pane like this.
    #[test]
    fn a_pane_with_a_margin_is_not_broken_by_what_it_covers() {
        let mut w = World::infinite_empty();
        for col in 40..43 {
            w.set_cell_at(40, col, Cell::alive(PlayerId(1)));
        }
        // One cell of margin on every side.
        for row in 39..42 {
            for col in 39..44 {
                let cell = w.cell_at(row, col).unwrap_or(Cell::DEAD);
                w.set_cell_at(row, col, cell.with_ice(true).with_player(PlayerId(1)));
            }
        }

        for _ in 0..40 {
            w.step();
        }

        assert!(w.cell_at(39, 39).unwrap().is_ice(), "the pane should still stand");
        assert_eq!(
            w.live_cells().len(),
            3,
            "and what it covers should be exactly as it was, frozen"
        );
    }

    /// What a pane is for. Shattering clears the ice flag and nothing else,
    /// so a schematic drawn under one — alive here, deliberately dead there,
    /// and whoever owns each cell — starts living exactly as it was drawn the
    /// moment the cover goes. Anything that reset the cells underneath would
    /// make a pane useless for laying a pattern out over several generations,
    /// which is the whole of why it freezes rather than blocks.
    #[test]
    fn shattering_leaves_what_was_under_it_exactly_as_it_was() {
        let mut w = World::infinite_empty();

        // A schematic: two live cells with different owners, and a gap that
        // has to stay a gap.
        w.set_cell_at(11, 11, Cell::alive(PlayerId(1)));
        w.set_cell_at(11, 13, Cell::alive(PlayerId(2)));
        for row in 10..=12 {
            for col in 10..=14 {
                let cell = w.cell_at(row, col).unwrap_or(Cell::DEAD);
                w.set_cell_at(row, col, cell.with_ice(true));
            }
        }
        let before: Vec<Cell> = (10..=12)
            .flat_map(|r| (10..=14).map(move |c| (r, c)))
            .map(|(r, c)| w.cell_at(r, c).unwrap())
            .collect();

        // A block against the pane, ice-free and alive, to break it.
        for (r, c) in [(13, 10), (13, 11), (14, 10), (14, 11)] {
            w.set_cell_at(r, c, Cell::alive(PlayerId(3)));
        }

        w.step();

        assert!(!w.cell_at(10, 14).unwrap().is_ice(), "the pane should have gone");
        let after: Vec<Cell> = (10..=12)
            .flat_map(|r| (10..=14).map(move |c| (r, c)))
            .map(|(r, c)| w.cell_at(r, c).unwrap())
            .collect();
        for (was, is) in before.iter().zip(&after) {
            assert_eq!(
                was.with_ice(false),
                *is,
                "the ice flag is the only thing shattering may change"
            );
        }
    }

    /// A cell against a pane breaks it even if this is the generation it dies
    /// in. It is alive now and it is touching now, and that is the whole of
    /// what breaking means — a cell about to die has still crashed into it.
    ///
    /// This is what taking the seeds before the rule buys. Taken after, a cell
    /// that died on the way would already be gone and the pane would stand,
    /// which reads as ice ignoring something that plainly hit it.
    #[test]
    fn a_cell_that_dies_this_generation_still_breaks_the_pane() {
        let mut w = World::infinite_empty();
        for col in 0..6 {
            w.set_cell_at(0, col, Cell::DEAD.with_ice(true).with_player(PlayerId(2)));
        }
        // One cell, alone, against the pane: it dies of loneliness on the very
        // step it would break it.
        w.set_cell_at(1, 2, Cell::alive(PlayerId(1)));

        w.step();

        assert!(!w.cell_at(1, 2).unwrap().is_alive(), "it should have died");
        for col in 0..6 {
            assert!(
                !w.cell_at(0, col).unwrap().is_ice(),
                "and taken the pane with it: ice at column {col} survived"
            );
        }
    }

    /// And what breaks it: anything alive that arrives. A glider is the
    /// cheapest way to reach a pane you cannot get next to, and it shatters
    /// the whole run the moment it touches — sealing a pattern in buys you
    /// time, not safety.
    #[test]
    fn a_glider_shatters_a_pane_it_reaches() {
        let mut w = World::infinite_empty();
        for col in 40..43 {
            w.set_cell_at(40, col, Cell::alive(PlayerId(1)));
        }
        for row in 39..42 {
            for col in 39..44 {
                let cell = w.cell_at(row, col).unwrap_or(Cell::DEAD);
                w.set_cell_at(row, col, cell.with_ice(true).with_player(PlayerId(1)));
            }
        }

        // A glider up and to the left, travelling down and to the right: one
        // cell each way every four generations.
        for (r, c) in [(30, 31), (31, 32), (32, 30), (32, 31), (32, 32)] {
            w.set_cell_at(r, c, Cell::alive(PlayerId(2)));
        }

        let mut broke_at = None;
        for step in 1..=60 {
            w.step();
            if !w.cell_at(39, 39).unwrap_or(Cell::DEAD).is_ice() {
                broke_at = Some(step);
                break;
            }
        }
        let broke_at = broke_at.expect("the glider should have reached the pane and broken it");
        assert!(broke_at > 1, "it should have had to travel, not start touching");

        for row in 39..42 {
            for col in 39..44 {
                assert!(
                    !w.cell_at(row, col).unwrap_or(Cell::DEAD).is_ice(),
                    "({row}, {col}) survived, so the run did not break as one"
                );
            }
        }
    }

    /// `--torus 18x18` has to mean the same thing to the client and the
    /// server, so the parsing is one function and its refusals are pinned.
    #[test]
    fn a_torus_size_is_read_the_same_way_everywhere() {
        assert_eq!(parse_torus("18x18"), Ok(WorldKind::Toroidal { rows: 18, cols: 18 }));
        assert_eq!(parse_torus("4X7"), Ok(WorldKind::Toroidal { rows: 4, cols: 7 }));
        assert_eq!(parse_torus(" 4 x 7 "), Ok(WorldKind::Toroidal { rows: 4, cols: 7 }));

        // A world with no chunks in it is not a world.
        assert!(parse_torus("0x4").is_err());
        assert!(parse_torus("-3x4").is_err());
        assert!(parse_torus("18").is_err());
        assert!(parse_torus("axb").is_err());
        assert!(parse_torus("").is_err());
    }

    /// The shape is a choice made at startup, and both shapes have to work.
    #[test]
    fn either_shape_builds_and_steps() {
        for mode in [WorldKind::Infinite, WorldKind::Toroidal { rows: 4, cols: 4 }] {
            let mut w = mode.build();
            w.set_cell_at(2, 2, Cell::alive(PlayerId(1)));
            w.set_cell_at(2, 3, Cell::alive(PlayerId(1)));
            w.set_cell_at(3, 2, Cell::alive(PlayerId(1)));
            w.set_cell_at(3, 3, Cell::alive(PlayerId(1)));
            for _ in 0..5 {
                w.step();
            }
            assert_eq!(w.live_cells().len(), 4, "{mode:?}: a block should hold");
        }
    }

    /// A pane is not broken by what it covers. The cell underneath is frozen,
    /// and frozen is not "alive and ice-free", so it cannot be the seed --
    /// otherwise every pane laid over life would shatter on the next tick.
    #[test]
    fn a_pane_is_not_broken_by_the_cell_it_covers() {
        let mut w = World::infinite_empty();
        // Alone, so nothing can be born next to it: a single live cell gives
        // any neighbour one live neighbour, and a birth needs three.
        w.set_cell_at(0, 0, Cell::alive(PlayerId(1)).with_ice(true));

        for _ in 0..10 {
            w.step();
        }

        let c = w.cell_at(0, 0).unwrap();
        assert!(c.is_ice(), "the pane broke from the inside");
        assert!(c.is_alive(), "and what it covers should be frozen, not dead");
    }

    /// Frozen cells still count as neighbours, so a pane laid *exactly* on
    /// life makes life immediately outside itself -- and that newborn breaks
    /// the pane. Note the "exactly": see the test below for what one cell of
    /// margin does, which is the opposite.
    #[test]
    fn life_born_beside_a_pane_breaks_it() {
        let mut w = World::infinite_empty();
        for col in 0..6 {
            w.set_cell_at(0, col, Cell::alive(PlayerId(1)).with_ice(true));
        }

        // Two steps, not one. Seeds are taken before the rule runs, so a cell
        // born during a generation is against the pane from the *next* one --
        // it crashes into it a beat after it appears.
        w.step();
        assert!(w.cell_at(0, 0).unwrap().is_ice(), "nothing was beside it yet");
        w.step();
        assert!(
            !w.cell_at(0, 0).unwrap().is_ice(),
            "a cell born beside the pane should have broken it"
        );
    }

    #[test]
    fn an_infinite_world_never_folds_coordinates() {
        let w = World::infinite();
        assert_eq!(w.canonical((-5, 12)), (-5, 12));
    }

    /// **A shape arrives over a wire**, so two numbers a sender chose freely
    /// have to be refused rather than allocated. Each of these used to take
    /// the whole process down from one `Create` on a connection that had not
    /// joined anything: the first two through the `assert!` in
    /// `toroidal_empty`, the third through the `i32` multiply that sizes it.
    #[test]
    fn a_torus_a_client_asked_for_is_refused_before_it_is_built() {
        for (rows, cols) in [(0, 4), (-1, -1), (4, 0), (100_000, 100_000), (1, 100_000)] {
            let asked = WorldKind::Toroidal { rows, cols };
            assert!(asked.checked().is_err(), "{rows}x{cols} was accepted");
        }
        // And the shapes the documentation tells people to run still build.
        for (rows, cols) in [(1, 1), (18, 18), (40, 40), (MAX_TORUS_SIDE, 32)] {
            let asked = WorldKind::Toroidal { rows, cols };
            assert_eq!(asked.checked(), Ok(asked), "{rows}x{cols} was refused");
        }
        assert_eq!(WorldKind::Infinite.checked(), Ok(WorldKind::Infinite));
    }

    /// The command line and the wire agree about what a torus may be, because
    /// there is one answer and `parse_torus` asks it.
    #[test]
    fn the_command_line_refuses_what_the_wire_refuses() {
        assert!(parse_torus("0x4").is_err());
        assert!(parse_torus("100000x100000").is_err());
        assert_eq!(parse_torus("18x18"), Ok(WorldKind::Toroidal { rows: 18, cols: 18 }));
    }
}
