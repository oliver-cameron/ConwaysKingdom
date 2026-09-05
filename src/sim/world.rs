use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::cell::{Cell, Chunk, ChunkMask, Halo, Kind, Takings, CHUNK_N};
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
/// **Because a shape arrives over the wire.** `ClientMessage::Create` carries
/// a `WorldKind` straight off a socket, and a torus is allocated whole -- so
/// `rows: 0` reached an `assert!` and killed the process, and `100000x100000`
/// overflowed the `i32` multiply that sizes the allocation. Either one was a
/// whole server, every room in it, from one message on a connection that had
/// not joined anything. The release profile is `panic = "abort"`, so it did
/// not even unwind.
///
/// **The numbers are what a server can actually step four times a second**,
/// rather than what it can hold — so they are a count of *cells* wearing a
/// count of chunks, and they moved when a chunk did.
///
/// `examples/frametime` measures about 41 nanoseconds a cell, near enough flat
/// across every size, so a quarter-second budget is a little over four million
/// cells with nothing left for the sockets. At sixteen cells to a chunk edge
/// that was 16384 chunks; at sixty-four it is **1024**, because a chunk holds
/// sixteen times as many cells and the budget did not change.
///
/// The per-side cap is the same division, and it is doing a different job:
/// stopping a 1x1024 world that fits the budget and is a corridor nobody can
/// play in.
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

/// **A blast that went off**, for whoever is drawing rather than for the rule.
///
/// Where and how big, which is everything an effect needs and nothing the
/// simulation reads back. `by` is whose it was, so the fireball can be their
/// colour rather than a colour the interface chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Blast {
    pub at: (i32, i32),
    pub reach: i32,
    pub by: PlayerId,
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

    /// Run the rule again over every overclocked disc.
    ///
    /// **A pass and not a rule**, for the reason the turret's is one: a disc
    /// is not a question eight neighbours can answer. It runs after the whole
    /// world has stepped and before the generation is called done, so the
    /// generation stays the unit on the wire, in the save and in the digest —
    /// every peer runs the same passes and there is nothing new to agree
    /// about.
    ///
    /// The discs are found from the world **as the pass before left it**, so
    /// a machine that died this generation does not run again; and every halo
    /// is gathered before any cell is written, which is the discipline the
    /// first pass keeps and for the same reason. At the edge of a disc a
    /// masked cell reads neighbours the pass before left and this pass will
    /// not move, and an unmasked cell sees the disc's second state next
    /// generation: the inside runs twice as fast and the outside sees every
    /// other step of it. That is the whole of the border, and it is a hazard
    /// the way a pane's edge is rather than a bug — see [docs/simulation.md].
    ///
    /// The dice are [`super::seed::pass_seed`]'s. Handed the generation's own
    /// seed, a pass would roll every cell the identical dice twice.
    ///
    /// [docs/simulation.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/simulation.md#overclockers
    fn overclock_pass(&mut self, generation: u64, pass: u64, earned: &mut Takings) {
        let masks = self.overclock_masks(&self.overclockers());
        if masks.is_empty() {
            return;
        }
        for &coord in masks.keys() {
            self.ensure(coord);
        }
        // The whole-world pass is done with its halos by now, so the scratch
        // is free and this allocates nothing either.
        self.scratch.clear();
        for &coord in masks.keys() {
            let halo = self.gather_halo(coord);
            self.scratch.push(halo);
        }
        let seed = super::seed::pass_seed(generation, pass);
        for (i, (&coord, mask)) in masks.iter().enumerate() {
            let halo = self.scratch[i];
            let at = (coord.0 * CHUNK_N as i32, coord.1 * CHUNK_N as i32);
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into_where(chunk, seed, at, earned, mask);
            }
        }
    }

    /// The cells every overclocker's disc covers, as a mask per chunk it
    /// touches.
    ///
    /// A `BTreeMap`, so the chunks come out sorted without a second pass, and
    /// a set of bits, so a cell two discs cover — or one a disc wraps onto on
    /// a small torus — is stepped once. Folded onto the chunks the world has
    /// as it goes, the way every absolute coordinate is.
    fn overclock_masks(&self, at: &[(i32, i32)]) -> BTreeMap<Coord, ChunkMask> {
        let n = CHUNK_N as i32;
        let reach = rule::OVERCLOCK_REACH;
        let mut masks = BTreeMap::new();
        for &(row, col) in at {
            for dr in -reach..=reach {
                for dc in -reach..=reach {
                    if dr * dr + dc * dc > reach * reach {
                        continue;
                    }
                    let (r, c) = (row + dr, col + dc);
                    let coord = self.canonical((r.div_euclid(n), c.div_euclid(n)));
                    masks
                        .entry(coord)
                        .or_insert(ChunkMask::NONE)
                        .set(r.rem_euclid(n) as usize, c.rem_euclid(n) as usize);
                }
            }
        }
        masks
    }

    /// Every live, ice-free overclocker, in absolute coordinates. Unsorted,
    /// unlike [`Self::turrets`]: a disc is a set of bits, so nothing about
    /// the pass depends on which was found first.
    fn overclockers(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen one runs nothing: a pane stops time over
                    // whatever it covers, and that is every rule.
                    if cell.kind() != Kind::OVERCLOCK || cell.is_ice() || !cell.is_alive() {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        crow * CHUNK_N as i32 + row as i32,
                        ccol * CHUNK_N as i32 + col as i32,
                    ));
                }
            }
        }
        out
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

    /// Set off every dynamite whose fuse has run out, and scramble the ground
    /// around each one.
    ///
    /// **A pass and not a rule**, because "every square within reach" is not a
    /// question a halo of eight neighbours can answer — the same reason
    /// [`Self::fire_turrets`] and [`Self::break_ice_from`] are passes. What
    /// makes this one cheap enough to be one is that it is **one roll per
    /// square**: `sim::seed` is already a stream per cell per generation that
    /// two peers agree on without exchanging anything, so a probability does
    /// directly what a scoring function would have manufactured.
    ///
    /// It runs at the **top** of the generation, which the other two do not.
    /// That is the warning: a fuse reaches full during one generation's rule,
    /// is drawn full for that whole generation, and goes off at the start of
    /// the next.
    fn detonate(&mut self) {
        self.detonate_with(rule::DYNAMITE_DENSITY, rule::DYNAMITE_REACH);
    }

    /// Take what went off, leaving nothing behind: a blast is drawn once.
    pub fn take_blasts(&mut self) -> Vec<Blast> {
        std::mem::take(&mut self.blasts)
    }

    /// [`Self::detonate`] with the two numbers `examples/blast` sweeps — what
    /// a disc comes up alive at, out of sixty-four, and how far one stick
    /// reaches — so a density or a reach can be measured without editing
    /// `rule.rs` between runs. The game never calls it: [`Self::step`] goes
    /// through the constants.
    pub fn detonate_with(&mut self, density: u64, reach: i32) {
        let ready = self.dynamite_ready();
        if ready.is_empty() {
            return;
        }
        let generation = super::seed::generation_seed(self.seed, self.generation);

        // Gathered before anything is written, so a blast does not scramble
        // the ground the next one is deciding where to land on. Two peers
        // stepping the same generation must make the same choices, and a
        // choice that depended on which dynamite was handled first would
        // depend on the iteration order of a map.
        // **A blob of them is one bomb, not many.** Dynamite standing in each
        // other's disc go off as a single, larger one — see
        // [`rule::blast_reach`], where each is worth a constant area — so a
        // hundred of them reach ten times as far as one rather than a hundred
        // small craters in the same place.
        let mut blasts = Vec::new();
        for group in clusters(&ready, reach) {
            let reach = rule::blast_reach_from(reach, group.len());
            let owner = ready[group[0]].1;
            // The middle of the blob, which is where a bomb made of all of
            // them is. Integer division, so it lands on a square.
            let (rows, cols): (i32, i32) =
                group.iter().fold((0, 0), |(r, c), &i| (r + ready[i].0 .0, c + ready[i].0 .1));
            let at = (rows / group.len() as i32, cols / group.len() as i32);
            let seed = super::seed::cell_seed(generation, at.0, at.1);
            blasts.push((self.blast_centre(at, owner, seed, reach), owner, seed, reach));
        }

        // Every dynamite that went off is consumed, whichever blast it was
        // part of — the first blast's seed decides them all, which is one roll
        // per square either way.
        let seed_for = blasts.first().map(|&(_, _, seed, _)| seed).unwrap_or(generation);
        for ((row, col), owner) in &ready {
            // **Consumed, and it takes the same roll as the ground it threw.**
            // Left alive it is a cell standing in the middle of noise that
            // nothing else in the blast could have produced, which reads as a
            // survivor rather than as a crater; left dead it is a hole in the
            // same way. So it comes up alive or dead on its own square's own
            // roll, exactly like everything else the blast touched.
            let cell = self.cell_at(*row, *col).unwrap_or(Cell::DEAD);
            self.set_cell_at(
                *row,
                *col,
                Self::blasted(cell, *owner, seed_for, density, *row, *col).with_age(0),
            );
        }

        for (centre, owner, seed, reach) in blasts {
            // **Reported, because nothing else says it happened.** A blast is
            // a generation in which a disc of ground quietly becomes
            // different: the cells before and after are both just cells, so
            // the largest thing a player can do reads as the board having
            // glitched. Whoever is drawing takes these; the rule does not care.
            self.blasts.push(Blast { at: centre, reach, by: owner });
            self.scramble(centre, owner, seed, density, reach);
        }
    }

    /// Turn a disc of ground into noise, and light every dynamite it reaches.
    ///
    /// **The blast decides whose noise it is**, which is the whole of what a
    /// dynamite buys. Every square it reaches is re-rolled: the roughly one in
    /// three that comes up alive is *yours*, and the rest is reset to
    /// no-man's-land. So a bomb does not merely animate what was already
    /// there — it breaks a country apart and leaves you a third of the pieces.
    ///
    /// It used to leave the owner alone and set only alive or dead, which
    /// meant a blast into somebody's empty ground **manufactured life for
    /// them**: a disc of theirs at [`rule::DYNAMITE_DENSITY`] where there had
    /// been nothing, on ground they still held. Aimed at an empty frontier a
    /// dynamite was a gift.
    ///
    /// Two squares are left alone. **Ice**, because a pane stops time over
    /// whatever it covers and that is every rule. And **granted ground** —
    /// see [`Cell::is_home`] — which no rule moves: [`rule::territory`] returns
    /// before it, so a home square only ever changes hands by being written,
    /// and `net::already_granted` reads exactly that to keep a returning
    /// player's seat. A blast that took one would evict somebody from their
    /// spawn permanently and hand them a second patch on their next join. Life
    /// standing on it is still scrambled; the owner is not.
    ///
    /// Ground nobody has loaded is not a third. An infinite world holds only
    /// the chunks something has touched, and a disc that ran past them used to
    /// be scrambled on one side and left alone on the other — the same stick
    /// did half as much at a chunk corner as in the middle of one. An absent
    /// chunk reads as dead and nobody's, the way [`Self::turret_wants`] reads
    /// it, and writing there is what loads it.
    fn scramble(
        &mut self,
        centre: (i32, i32),
        owner: PlayerId,
        seed: u64,
        density: u64,
        reach: i32,
    ) {
        let mut chained = Vec::new();
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                if dr * dr + dc * dc > reach * reach {
                    continue;
                }
                let (row, col) = (centre.0 + dr, centre.1 + dc);
                let cell = self.cell_at(row, col).unwrap_or(Cell::DEAD);
                if cell.is_ice() {
                    continue;
                }
                // **The chain, and it cannot recurse.** A dynamite in the blast
                // has its fuse set to full, so it goes off at the top of the
                // *next* generation — a line of them is a fuse and a cluster
                // is one ring a generation, rather than one pass re-entering
                // itself.
                if cell.kind() == Kind::DYNAMITE && cell.is_alive() {
                    chained.push(((row, col), cell.with_age(super::cell::bits::MAX_AGE)));
                    continue;
                }
                self.set_cell_at(row, col, Self::blasted(cell, owner, seed, density, row, col));
            }
        }
        for ((row, col), cell) in chained {
            self.set_cell_at(row, col, cell);
        }
    }

    /// What a blast leaves on one square, which is the whole of what a dynamite
    /// does to the board.
    ///
    /// One roll, and it decides ownership as well as life: alive is *yours*,
    /// dead is nobody's. That is what makes a bomb take ground rather than
    /// only stir it, and it is deliberately the same roll for both — a square
    /// that came up alive for you and stayed somebody else's would be a live
    /// cell of theirs standing in your crater, which is the state this whole
    /// change is about.
    ///
    /// **Full strength when it lives**, because [`Cell::alive`] is: level and
    /// influence have to agree on a source, and a corpse owned at level nought
    /// is a state the rule says cannot exist.
    ///
    /// **Level nought and nobody when it does not.** Ground with an owner and
    /// no strength is the same impossible state from the other side, so the
    /// two move together.
    ///
    /// Granted ground keeps its owner whatever the roll says — see
    /// [`Self::scramble`] for why nothing may move one.
    fn blasted(cell: Cell, owner: PlayerId, seed: u64, density: u64, row: i32, col: i32) -> Cell {
        // Its own square's own roll, on the blast's own stream, so two
        // overlapping blasts do not decide the same square twice the same way
        // — and so a peer that never saw the dynamite placed still lands on the
        // same board.
        let square = super::seed::cell_seed(seed, row, col);
        let alive = Roll::new(square).chance(rule::BLAST_STREAM, density);
        // **The age goes with the kind.** A factory three quarters of the way
        // through its rot, turned into ordinary ground, kept that three — and
        // `Cell::sprite` reads the age as a sheet row, so it drew from a row
        // that only ageing kinds have art in and came out as nothing at all.
        // `Kind::NORMAL` is `Ages::Never`, so nought is the only age it has.
        let cell = cell.with_kind(Kind::NORMAL).with_age(0);
        if cell.is_home() {
            // **Cleared, not scrambled.** Its owner cannot move, so a square
            // that came up alive here would be alive *for them* — which is
            // the gift this whole rule exists to stop, and a spawn is exactly
            // where somebody would aim to exploit it. So the blast may only
            // take life off a granted patch, never put it there.
            return cell.with_alive(false);
        }
        if alive {
            cell.with_player(owner).with_alive(true).with_level(super::cell::bits::MAX_LEVEL)
        } else {
            cell.with_alive(false).with_player(PlayerId::UNOWNED).with_level(0)
        }
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

    /// **Where a blast is worth setting off**, walking outward from the
    /// dynamite until it finds somewhere.
    ///
    /// A blast wasted on its owner's own ground is a blast wasted: a
    /// detonation inside your own country turns your own patterns into your
    /// own noise. So this searches rings at increasing distance for a centre
    /// whose disc is at least [`rule::DYNAMITE_FOREIGN`] not its owner's, takes
    /// the nearest, and breaks a tie with a seeded roll — which is
    /// [`Self::turret_target`] again, in shape: the nearest square answering a
    /// question, with the tie broken so a volley does not always favour one
    /// direction.
    ///
    /// What that buys is that **a dynamite does not have to be placed
    /// exactly.** Placing is confined to your own influence, so without it the
    /// only useful dynamite is one laid on the exact square of your border
    /// nearest something worth hitting — a precision the interface does not
    /// support, against a frontier that moves every generation.
    ///
    /// Bounded by [`rule::DYNAMITE_THROW`], and it goes off where it stands if
    /// nothing within that is better. Unbounded it would be a homing weapon
    /// with a range of the whole world.
    fn blast_centre(&self, at: (i32, i32), owner: PlayerId, seed: u64, reach: i32) -> (i32, i32) {
        let throw = rule::DYNAMITE_THROW;
        // Ring by ring, so the first distance that has any answer is the one
        // taken and a dynamite on its own frontier stops at once. The worst
        // case — one in the middle of a large country — is what the bound is
        // for.
        for ring in 0..=throw {
            let mut ties = 0usize;
            let walk = |count: &mut usize, want: usize| -> Option<(i32, i32)> {
                for dr in -ring..=ring {
                    for dc in -ring..=ring {
                        // The ring and not the disc: the box's inside was
                        // covered by an earlier, nearer ring.
                        if dr.abs().max(dc.abs()) != ring {
                            continue;
                        }
                        let centre = (at.0 + dr, at.1 + dc);
                        if !self.worth_hitting(centre, owner, reach) {
                            continue;
                        }
                        if *count == want {
                            return Some(centre);
                        }
                        *count += 1;
                    }
                }
                None
            };
            walk(&mut ties, usize::MAX);
            if ties == 0 {
                continue;
            }
            let pick = Roll::new(seed).pick(rule::THROW_STREAM, ties);
            let mut n = 0;
            if let Some(found) = walk(&mut n, pick) {
                return found;
            }
        }
        at
    }

    /// Whether a blast centred here would be worth setting off.
    ///
    /// **Ground that is not already yours**, which now includes no-man's-land.
    ///
    /// This counted somebody *else's* ground and skipped the empty kind, on
    /// the reasoning that a blast over no-man's-land does nothing to anybody.
    /// That was true while a blast only disturbed what it reached. It claims
    /// what it reaches now — see [`Self::scramble`] — so open country is worth
    /// hitting, and refusing to go off over it would leave a dynamite unable to
    /// do the thing it was just given.
    ///
    /// What that re-admits is the crater loop the old rule was written to
    /// stop: the debris of a blast is mostly unowned, so one can be aimed at
    /// the last one's hole. It costs [`rule::DYNAMITE_COST`] a time and pays a
    /// third of a disc, which is a worse rate than any of the ordinary ways to
    /// hold ground, so it is priced out rather than ruled out.
    ///
    /// A count and not a cost, and it stops the moment it has seen enough — so
    /// a dynamite on a frontier answers in a handful of reads. Ground nobody
    /// has loaded is nobody's and counts: it used to count for nothing, so a
    /// stick at the edge of what was stored would not walk toward the open
    /// country the rule says is worth hitting.
    fn worth_hitting(&self, centre: (i32, i32), owner: PlayerId, reach: i32) -> bool {
        let mut theirs = 0u64;
        // How many squares of a disc this radius holds, so the threshold is a
        // fraction of the disc rather than of the box around it.
        let total: u64 = (-reach..=reach)
            .flat_map(|dr| (-reach..=reach).map(move |dc| (dr, dc)))
            .filter(|(dr, dc)| dr * dr + dc * dc <= reach * reach)
            .count() as u64;
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                if dr * dr + dc * dc > reach * reach {
                    continue;
                }
                let there = self.cell_at(centre.0 + dr, centre.1 + dc).unwrap_or(Cell::DEAD);
                if there.player() != owner {
                    theirs += 1;
                    if theirs * 64 >= total * rule::DYNAMITE_FOREIGN {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Every dynamite whose fuse has run out, sorted, so two peers set them off
    /// in the same order.
    fn dynamite_ready(&self) -> Vec<((i32, i32), PlayerId)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen dynamite does not go off: a pane stops time over
                    // whatever it covers, and that is every rule.
                    if cell.kind() != Kind::DYNAMITE || cell.is_ice() {
                        continue;
                    }
                    if !cell.is_alive() || cell.age() < super::cell::bits::MAX_AGE {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        (crow * CHUNK_N as i32 + row as i32, ccol * CHUNK_N as i32 + col as i32),
                        cell.player(),
                    ));
                }
            }
        }
        out.sort_unstable();
        out
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

/// Which dynamite go off together: the groups standing in each other's disc.
///
/// **Two within one reach of each other are one bomb** — nearer than discs
/// merely overlapping, which would join a pair two reaches apart. Otherwise a
/// blob of them is a hundred craters in the same place, each doing again what
/// the last already did — where what a player built is one charge made of a
/// hundred, and [`rule::blast_reach`] is what that is worth.
///
/// Connected by distance and transitively, so a line of dynamite is one long
/// bomb rather than a chain of pairs. `O(n²)` over the ones that are *ready*,
/// which is a handful in the generations it is not nought.
///
/// The order is the order they came in, which is sorted — so two peers group
/// them identically without exchanging anything.
fn clusters(ready: &[((i32, i32), PlayerId)], reach: i32) -> Vec<Vec<usize>> {
    let mut group: Vec<Option<usize>> = vec![None; ready.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..ready.len() {
        if group[i].is_some() {
            continue;
        }
        let g = out.len();
        out.push(vec![i]);
        group[i] = Some(g);
        // Grown rather than scanned once: reaching a dynamite can bring in
        // others only that one is near, which is what makes a line one bomb.
        let mut k = 0;
        while k < out[g].len() {
            let (at, _) = ready[out[g][k]];
            for j in 0..ready.len() {
                if group[j].is_some() {
                    continue;
                }
                let (dr, dc) = (ready[j].0 .0 - at.0, ready[j].0 .1 - at.1);
                if dr * dr + dc * dc <= reach * reach {
                    group[j] = Some(g);
                    out[g].push(j);
                }
            }
            k += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests;
