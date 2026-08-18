use crate::cell::{Cell, Chunk, CHUNK_N};

/// Never advance more than this many generations in a single frame. Without a
/// cap, a long stall (debugger breakpoint, window drag) would try to catch up
/// all at once and stall again.
const MAX_CATCHUP_STEPS: u32 = 8;

/// The world. For now: exactly one chunk, every neighbour unloaded, which
/// `Chunk::get` reads as dead. Chunk loading comes later.
pub struct World {
    front: Box<Chunk>,
    back: Box<Chunk>,
    elapsed: f32,
    pub generation: u64,
    /// Set when `front` has changed since the last upload to the GPU.
    pub dirty: bool,
}

impl World {
    pub fn new() -> Self {
        let mut front = Chunk::zeroed();
        seed_glider(&mut front, CHUNK_N / 2 - 2, CHUNK_N / 2 - 2, 1);
        Self {
            front,
            back: Chunk::zeroed(),
            elapsed: 0.0,
            generation: 0,
            dirty: true,
        }
    }

    pub fn chunk(&self) -> &Chunk {
        &self.front
    }

    /// Accumulate frame time and advance a generation every `span` seconds.
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
            self.elapsed = 0.0; // gave up catching up; resync rather than spiral
        }
    }

    fn step(&mut self) {
        self.front.step(&mut self.back);
        std::mem::swap(&mut self.front, &mut self.back); // pointer swap, not a copy
        self.generation += 1;
        self.dirty = true;
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// The standard glider, travelling down-right.
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
