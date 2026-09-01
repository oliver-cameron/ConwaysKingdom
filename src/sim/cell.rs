use bytemuck::{Pod, Zeroable};

use super::dir::Dir;
use super::player::PlayerId;
use super::rule::{
    next_cell, Neighbours, MINE_UPKEEP, TURRET_DECAY, TURRET_ROT_STREAM, UPKEEP_STREAM,
};
use super::seed::Roll;
use std::ops::{Index, IndexMut};

pub const CHUNK_N: usize = 16;
pub const CHUNK_CELLS: usize = CHUNK_N * CHUNK_N;

/// One cell: two bytes, and the second of them is a sprite.
///
/// ```text
///  byte 0 (R)                byte 1 (G)
/// | player |level|H|       | age  | kind |I |A |
///  7 6 5 4  3 2 1  0        7 6 5   4 3 2  1  0
/// ```
///
/// Byte 0 holds the player at the top, so the number extracts with a shift and
/// no mask, then how much of that player's influence reaches this square, then
/// whether the square was granted.
///
/// Byte 1 is what a cell **is**: alive, iced, its kind, and how far through
/// whatever that kind counts. Every field is contiguous and every one extracts
/// with a shift and a mask.
///
/// **It is not the sheet index.** It was — the fields used to be arranged so
/// the byte fell out as one, with the kind split around the age to make the
/// nibbles line up, and reading a kind meant reassembling it from two places.
/// A byte that is a picture is a byte that cannot be read. The sheet position
/// is [`Cell::sprite`], which is four operations, and the sheet's layout is
/// unchanged by the move: a kind's four states are still four columns and its
/// eight ages still eight rows.
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

    /// Byte 1, bits 5..8: **how far through this cell is**, nought to seven.
    ///
    /// Not a count of generations — a step, and what a step means is the
    /// kind's business: see [`super::Ages`], which is where each kind says
    /// what its own counts.
    ///
    /// At the top of the byte, so it extracts with a shift alone. It used to
    /// sit in the middle, splitting the kind in two, because that made the
    /// byte its own sheet index — see [`super::Cell::sprite`], which is what
    /// pays for this instead.
    pub const AGE_SHIFT: u8 = 5;
    pub const AGE_WIDTH: u8 = 3;
    pub const AGE_MASK: u8 = (1 << AGE_WIDTH) - 1;
    pub const MAX_AGE: u8 = AGE_MASK;

    /// Byte 1, bits 2..5: what kind of cell this is. Eight of them.
    ///
    /// **In one piece**, which it was not: the sheet wanted the two low bits
    /// beside the state bits and the third above the age, so a kind was
    /// assembled from bits 2, 3 and 7 every time it was read or written.
    pub const KIND_SHIFT: u8 = 2;
    pub const KIND_WIDTH: u8 = 3;
    pub const KIND_MASK: u8 = (1 << KIND_WIDTH) - 1;

    /// Byte 0, bits 4..8: the player number, at the top so it extracts with a
    /// shift alone.
    ///
    /// Four bits, so **fifteen** players rather than sixteen: zero has to go
    /// on meaning unowned, because a zeroed cell must stay a valid empty one
    /// and [`super::Cell::alive`] asserts that live cells have an owner.
    pub const PLAYER_SHIFT: u8 = 4;
    pub const PLAYER_WIDTH: u8 = 4;
    pub const PLAYER_MASK: u8 = (1 << PLAYER_WIDTH) - 1;

    /// Byte 0, bits 1..4: **how much of that player's influence reaches this
    /// square**, nought to seven.
    ///
    /// Ownership used to be a flag, and a flag has no gradient — which is why
    /// no rule built on counting owned neighbours ever worked. A corner of a
    /// solid region and a square just outside a straight edge both have
    /// exactly three, so no count can tell them apart, and every threshold
    /// either ate blobs from their corners or grew edges outward for ever.
    /// With a level the two stop looking alike: the corner is surrounded by
    /// high numbers and the outside square by low ones.
    ///
    /// Only meaningful on a **dead** square. A living cell is a source and
    /// reads as full whatever is stored here, which is what
    /// [`super::Cell::influence`] is for.
    pub const LEVEL_SHIFT: u8 = 1;
    pub const LEVEL_WIDTH: u8 = 3;
    pub const LEVEL_MASK: u8 = (1 << LEVEL_WIDTH) - 1;

    /// The most influence a square can carry, which is what a source has.
    pub const MAX_LEVEL: u8 = LEVEL_MASK;

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

    /// Byte 0, bit 0 on its own: what a change of owner leaves alone.
    ///
    /// The byte is full now — player, level, home — so this is `HOME` and
    /// nothing else. Kept as a name because what it means is "the part of the
    /// owner byte that is about the *square* rather than about who holds it",
    /// and that is the reason a grant survives the ground changing hands.
    pub const SPARE_MASK: u8 = HOME;
}

const _: () = {
    assert!(size_of::<Cell>() == 2 && align_of::<Cell>() == 1);
    // The kind byte must tile all eight bits with no overlap and no gap, or a
    // state would share a tile with a kind.
    assert!(bits::ALIVE == 1);
    assert!(bits::ICE == 2);
    assert!(bits::KIND_SHIFT == 2);
    // Alive, ice, kind, age: bits 0..8 with no overlap and no gap.
    assert!(bits::KIND_SHIFT + bits::KIND_WIDTH == bits::AGE_SHIFT);
    assert!(bits::AGE_SHIFT + bits::AGE_WIDTH == 8);
    // A kind must fit the field it is stored in, or `with_kind` truncates it
    // into a different kind that has art of its own.
    assert!(Kind::COUNT <= 1 << bits::KIND_WIDTH);
    // And so must the owner byte: player, level, home, with nothing spare.
    assert!(bits::PLAYER_SHIFT as u32 + bits::PLAYER_WIDTH as u32 == 8);
    assert!(bits::LEVEL_SHIFT as u32 + bits::LEVEL_WIDTH as u32 == bits::PLAYER_SHIFT as u32);
    assert!(bits::HOME == 1);
    assert!(bits::SPARE_MASK == 1);
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
        // Stored at full, so [`Self::level`] and [`Self::influence`] agree on
        // a source rather than one of them being a special case. It is also
        // what a cell leaves when it dies: death stops it being a source and
        // the ground it was standing on is already at full strength, so it
        // ebbs from there instead of blinking out. Without this a fresh corpse
        // was owned at level nought, which is a state the rule says cannot
        // exist -- true again a generation later, and wrong on the screen in
        // between.
        Self::DEAD.with_tile(bits::ALIVE).with_player(player).with_level(bits::MAX_LEVEL)
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

    /// What kind of cell this is. One shift and one mask, which it was not:
    /// the kind used to be assembled from bits 2, 3 and 7.
    #[inline]
    pub const fn kind(self) -> Kind {
        Kind((self.tile() >> bits::KIND_SHIFT) & bits::KIND_MASK)
    }

    /// **Where this cell's picture is on the sheet**, as a tile index.
    ///
    /// Four operations, and it used to be none: the byte was arranged to *be*
    /// this number, which is why the kind was split around the age. The sheet
    /// is unchanged by moving them — a kind's four states are still four
    /// columns and its eight ages still eight rows — so what this computes is
    /// exactly the byte that used to be stored.
    ///
    /// The column is the kind's low two bits and the two state bits, so four
    /// states sit side by side; the row is the age, with the kind's third bit
    /// choosing which half of the sheet, so kinds 0-3 are the top eight rows
    /// and 4-7 the bottom eight. `render/shaders/grid.wgsl` does the same
    /// arithmetic and is the one thing kept in step by hand.
    #[inline]
    pub const fn sprite(self) -> u8 {
        let kind = self.kind().0;
        let column = ((kind & 3) << 2) | (self.tile() & (bits::ALIVE | bits::ICE));
        let row = self.age() | ((kind >> 2) << 3);
        (row << 4) | column
    }

    /// How far through this cell is, nought to [`bits::MAX_AGE`]. What it
    /// counts is the kind's business — see [`Ages`].
    pub const fn age(self) -> u8 {
        (self.tile() >> bits::AGE_SHIFT) & bits::AGE_MASK
    }

    pub const fn with_age(self, age: u8) -> Self {
        let kept = self.tile() & !(bits::AGE_MASK << bits::AGE_SHIFT);
        self.with_tile(kept | ((age & bits::AGE_MASK) << bits::AGE_SHIFT))
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
        let kept = self.owner_byte() & (bits::SPARE_MASK | (bits::LEVEL_MASK << bits::LEVEL_SHIFT));
        Self([kept | ((player.0 & bits::PLAYER_MASK) << bits::PLAYER_SHIFT), self.0[1]])
    }

    /// How much of its owner's influence is stored on this square.
    ///
    /// The raw field. What a neighbour actually feels is [`Self::influence`],
    /// which is not the same thing on a source.
    #[inline]
    pub const fn level(self) -> u8 {
        (self.owner_byte() >> bits::LEVEL_SHIFT) & bits::LEVEL_MASK
    }

    #[inline]
    pub const fn with_level(self, level: u8) -> Self {
        let rest = self.owner_byte() & !(bits::LEVEL_MASK << bits::LEVEL_SHIFT);
        Self([rest | ((level & bits::LEVEL_MASK) << bits::LEVEL_SHIFT), self.0[1]])
    }

    /// What this square pushes on the ones around it.
    ///
    /// **A living cell is a source**, and so is granted ground, so both read
    /// as full whatever their stored level happens to be. That is the whole of
    /// how the field is fed: everything else is that number falling off with
    /// distance, so where the sources are decides the entire map and nothing
    /// can drift or ratchet away from them.
    ///
    /// Granted ground being a source is what replaces the old carve-out that
    /// `HOME` never decays. It is a **spring** rather than an exception: a
    /// player whose life has gone out still has a patch with a live gradient
    /// on it, said in the same vocabulary as everything else.
    #[inline]
    pub const fn influence(self) -> u8 {
        if self.is_alive() || self.is_home() {
            bits::MAX_LEVEL
        } else {
            self.level()
        }
    }

    #[inline]
    pub const fn with_kind(self, kind: Kind) -> Self {
        let kept = self.tile() & !(bits::KIND_MASK << bits::KIND_SHIFT);
        self.with_tile(kept | ((kind.0 & bits::KIND_MASK) << bits::KIND_SHIFT))
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

/// Every kind of cell, in one list: what it is, the number that is also its
/// sprite, and whether a birth inherits it.
///
/// A macro for the same reason [`super::rule::order::rules!`] is one — the
/// list is written once, so [`Kind::ALL`], [`Kind::COUNT`] and
/// [`Kind::inherits`] cannot drift from each other, and adding a kind is a
/// row rather than four edits in three places.
macro_rules! kinds {
    ($(
        $(#[$doc:meta])* $name:ident = $n:literal,
        inherited: $inherited:literal,
        ages: $ages:expr,
        corpse: $corpse:literal
    ),* $(,)?) => {
        impl Kind {
            $( $(#[$doc])* pub const $name: Self = Self($n); )*

            /// Every kind. Each must have art at its own index in
            /// `render::atlas`, which asserts it, so a kind cannot exist
            /// without a picture.
            pub const ALL: [Self; [$($n),*].len()] = [$(Self::$name),*];
            pub const COUNT: usize = Self::ALL.len();

            /// Whether a birth copies this kind from the parent it chose.
            ///
            /// A birth otherwise takes **everything** from its parent, which
            /// is how a kind travels: a mine's children are mines, and since
            /// the parent is picked at random the kind spreads through a
            /// mixed population rather than being handed down whole.
            ///
            /// That is right for a kind you buy a *lineage* of and wrong for
            /// one you buy a *machine* of. A turret claims ground by standing
            /// there rather than by breeding, so a turret whose children were
            /// turrets would make any gun a factory that claims the map. A
            /// kind that does not inherit **passes over ownership alone** —
            /// pick it as a parent and the newborn is ordinary life belonging
            /// to whoever owned the parent.
            ///
            /// So an inheriting kind is an investment in a lineage and a
            /// non-inheriting one is a machine somebody placed, and the
            /// difference is one row in the list above.
            pub const fn inherits(self) -> bool {
                match self.0 {
                    $( $n => $inherited, )*
                    // A kind with no row here is one nothing can produce:
                    // the placements are a closed vocabulary and a birth
                    // copies an existing cell. Inheriting would propagate a
                    // number that has no art and no rules.
                    _ => false,
                }
            }

            /// **What this kind's age field counts.** See [`Ages`].
            pub const fn ages(self) -> Ages {
                match self.0 {
                    $( $n => $ages, )*
                    _ => Ages::Never,
                }
            }

            /// **Whether a dead one is still one.**
            ///
            /// A mine's corpse costs its owner and a dead turret fires
            /// backwards over the ground behind it, so both go on being what
            /// they were for a while. A payload does neither: it is a fuse,
            /// and a fuse that has gone out is ordinary dead ground. Anything
            /// whose whole behaviour is what it does *alive* belongs on this
            /// side of the row.
            ///
            /// Said once here rather than as a branch per kind in
            /// [`Halo::step_into`], which is where three of them had ended up.
            pub const fn leaves_a_corpse(self) -> bool {
                match self.0 {
                    $( $n => $corpse, )*
                    // A kind nothing can produce leaves nothing behind.
                    _ => false,
                }
            }
        }
    };
}

/// **What a kind's age field counts**, which is the whole reason the field is
/// where it is: eight ages are eight rows of the sheet, so whatever a kind
/// counts is on screen without an interface to read it off.
///
/// One thing or nothing, never two, because there is one field. Which is why
/// what a kind spends it on is worth writing in the table rather than
/// discovering: a mine's is **reserved** for [depleted mines], and a dead
/// mine's clearing is a roll partly so that nothing else takes it.
///
/// [depleted mines]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#depleted-mines
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ages {
    /// Nothing. The field stays at nought and the kind is one row of the
    /// sheet.
    Never,
    /// **A fuse, while it lives**, advancing on this chance a generation —
    /// and certainly from [`super::rule::PAYLOAD_WARN`], so the last sprite is
    /// on screen for exactly one generation.
    ///
    /// The rate is on the row rather than in a constant somewhere else,
    /// because "what it counts" and "how fast" are one fact about a kind and
    /// splitting them is how the two come to disagree.
    ///
    /// A chance and not a count, because it **scatters**: four payloads laid
    /// in one gesture do not go off in lockstep. What happens when it runs out
    /// is not a rule at all; see [`super::World::detonate`].
    Fuse(super::rule::Chance),
}

kinds! {
    /// An ordinary living cell.
    NORMAL = 0, inherited: true, ages: Ages::Never, corpse: false,
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
    ///
    /// It counts nothing **yet**, and that is a reservation rather than an
    /// absence: [depleted mines] is a mine that stops paying, and the age
    /// field is a fade where a flag would be a cliff. Nothing else may spend
    /// it.
    ///
    /// [depleted mines]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#depleted-mines
    MINE = 1, inherited: true, ages: Ages::Never, corpse: true,
    /// A cell that claims ground at range: every generation it takes the
    /// nearest square that is not its owner's and makes it theirs.
    ///
    /// A dead turret runs the same rule backwards — it takes the nearest
    /// square that *is* its owner's and gives it up — and since a live cell
    /// must have an owner, doing that to a living square kills it. It decays
    /// back to ordinary ground after a while, the way a dead mine does.
    ///
    /// The opposite of a mine in every way that matters. A mine earns on
    /// **turnover** and a turret works by **standing still**, so the block
    /// that is a mine's worst shape is a turret's best: four turrets is the
    /// cheapest thing in Conway that never dies and never gives birth, which
    /// is why a turret is placed in fours. It does not inherit, so a turret
    /// is always exactly the cells somebody paid for.
    TURRET = 2, inherited: false, ages: Ages::Never, corpse: true,
    /// A cell that counts down and then scrambles the ground around it.
    ///
    /// The **age** field is the fuse — see [`bits::AGE_SHIFT`], which is where
    /// it sits precisely so that eight ages are eight rows of the sheet and a
    /// payload visibly counts down with no interface at all. It advances on a
    /// chance while the cell is alive and not under ice, and at
    /// [`super::rule::PAYLOAD_WARN`] it always advances, so the last sprite is
    /// on screen for exactly one generation. A weapon with a random warning is
    /// a weapon with no warning.
    ///
    /// What it does is not a rule: "every square within reach" is not a
    /// question eight neighbours can answer, so detonation is a pass — see
    /// [`super::World::detonate`].
    ///
    /// **It travels**, which is the whole of what makes it a weapon rather
    /// than a mine you cannot eat. A birth copies its parent, so a glider that
    /// picks up a payload carries one — and a fuse carries with it, so what
    /// arrives is already part burnt. A pattern that reaches somebody's
    /// country and goes off in it is the piece the design was missing.
    ///
    /// The cost, said plainly because it is real: a gun that catches one is a
    /// factory. What limits it is that the fuse travels too, so a factory's
    /// output goes off near the factory, and that a payload is a live cell
    /// like any other — kill the pattern and there is nothing to inherit.
    PAYLOAD = 3, inherited: true, ages: Ages::Fuse(super::rule::PAYLOAD_FUSE), corpse: false,
}

/// What each player's mines did in one generation, indexed by the number the
/// cell carries.
///
/// Counts rather than a sum of money: what a birth or a death is *worth* is the
/// economy's business and the economy lives in `net`. The rule counts.
///
/// `born` is a count of births. `upkeep` is a count of **charges falling due**
/// on dead mines — not of deaths. A dead mine is charged once, on the
/// generation the roll in [`super::rule::MINE_UPKEEP`] comes up, so one that
/// is painted over, iced or born again before then is never counted at all.
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

    /// Nothing here worth keeping: no life, no ice, and nobody's ground.
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
    ///
    /// Ownership counts because territory lives on dead cells. Without it a
    /// chunk holding nothing but claimed ground reads as empty, and `prune`
    /// drops it on the very step it was claimed — so territory outside a
    /// chunk that also holds life could never last a generation, and a player
    /// granted ground on joining would lose it before their first move.
    ///
    /// It does **not** follow that an infinite world grows without bound.
    /// Territory has a die-off: a square with nothing pushing on it nets
    /// nothing and goes back to nobody's, so what is held tracks where the
    /// life *is* rather than everywhere it has been. Measured on an
    /// R-pentomino, which stores 41 chunks at generation 1103 and 39 two
    /// hundred generations later, with gliders still leaving.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| !c.is_alive() && !c.is_ice() && !c.player().is_owned())
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
        halo.step_into(next, 0, (0, 0), &mut Mined::default());
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

    /// Step every cell. `generation` is [`super::seed::generation_seed`] for
    /// this world at this tick and `at` is the chunk's top-left cell, so each
    /// cell's dice come from its **absolute position** and nothing else — the
    /// same number a compute thread could work out from its own coordinates,
    /// and one that does not change if the chunking does.
    ///
    /// `mined` is added to, never cleared: a caller sums a whole world into one
    /// tally. Counted here because this is the one place that holds a cell
    /// before and after in the same breath, so a birth costs a comparison and
    /// no second pass over the world.
    pub fn step_into(&self, next: &mut Chunk, generation: u64, at: (i32, i32), mined: &mut Mined) {
        for row in 0..CHUNK_N {
            for col in 0..CHUNK_N {
                let (hr, hc) = (row + 1, col + 1);
                let cell_seed =
                    super::seed::cell_seed(generation, at.0 + row as i32, at.1 + col as i32);
                let before = self.get(hr, hc);
                let mut after = next_cell(before, &self.neighbours(hr, hc), cell_seed);
                if after.kind() == Kind::MINE && after.player().is_owned() {
                    if after.is_alive() {
                        if !before.is_alive() {
                            mined.born[after.player().0 as usize] += 1;
                        }
                    } else if Roll::new(cell_seed).chance(UPKEEP_STREAM, MINE_UPKEEP) {
                        // A corpse costs once and is then ordinary ground.
                        // Charging it for as long as it lay there made a mine
                        // field a debt you could not pay off; this way what a
                        // mine costs in the end is bounded by how many died.
                        //
                        // The age goes with the kind, which is the one moment
                        // it resets: a mine's age is its depletion, so ground
                        // that has stopped being a mine has nothing left to be
                        // depleted.
                        mined.upkeep[after.player().0 as usize] += 1;
                        after = after.with_kind(Kind::NORMAL).with_age(0);
                    }
                }
                // A dead turret fires backwards over the ground behind it for
                // as long as it lies there, and then stops being one. Nothing
                // is tallied: what it costs its owner is the ground it hands
                // back and the life it takes with it, which `World::step`
                // applies, not money.
                if after.kind() == Kind::TURRET
                    && !after.is_alive()
                    && Roll::new(cell_seed).chance(TURRET_ROT_STREAM, TURRET_DECAY)
                {
                    after = after.with_kind(Kind::NORMAL);
                }
                // **A kind whose corpse is nothing becomes nothing at once**
                // — see [`Kind::leaves_a_corpse`], which is where that is said
                // for every kind rather than as a branch each.
                //
                // A payload is the one that matters: it never goes off after
                // it dies, and it does not lie there being a bomb either. An
                // armed corpse would take away the one answer that does not
                // need ice, which is that a payload is a live cell and has to
                // be kept alive to be worth anything.
                if !after.is_alive() && !after.kind().leaves_a_corpse() {
                    after = after.with_kind(Kind::NORMAL).with_age(0);
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
            out[i] = self.get((hr as i32 + dr) as usize, (hc as i32 + dc) as usize);
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
        for kind in [0u8, 1, 5, bits::KIND_MASK] {
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
        assert_eq!(
            c.0[0],
            (5 << bits::PLAYER_SHIFT) | (bits::MAX_LEVEL << bits::LEVEL_SHIFT),
            "the owner byte is the player and the level, and a live cell is a source"
        );
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
        for kind in 0..=bits::KIND_MASK {
            let base = Cell::DEAD.with_kind(Kind(kind));
            let tiles: Vec<u8> = [(false, false), (true, false), (false, true), (true, true)]
                .iter()
                .map(|&(alive, ice)| base.with_alive(alive).with_ice(ice).tile())
                .collect();
            let first = tiles[0];
            assert_eq!(tiles, vec![first, first + 1, first + 2, first + 3], "kind {kind}");
            // Four columns wide and aligned to them, so a kind's row of states
            // never straddles the edge of the sheet.
            assert_eq!(first % 4, 0, "kind {kind} starts mid-group");
        }
    }

    /// **Ages are rows**, which is now a property of [`Cell::sprite`] rather
    /// than of where the bits sit.
    ///
    /// It used to fall out of the layout: the age was the low three bits of
    /// the tile byte's high nibble, so the byte *was* the sheet index. Moving
    /// the age to the top of the byte to put the kind back in one piece is
    /// what makes this a test of the arithmetic instead — the sheet did not
    /// change, only what computes a position on it.
    #[test]
    fn a_kinds_ages_are_eight_rows_down_the_sheet() {
        for kind in 0..=bits::KIND_MASK {
            let base = Cell::DEAD.with_kind(Kind(kind)).with_alive(true);
            let rows: Vec<u8> =
                (0..=bits::MAX_AGE).map(|a| base.with_age(a).sprite() >> 4).collect();
            let top = rows[0];
            assert_eq!(rows, (top..top + 8).collect::<Vec<_>>(), "kind {kind}");
            // And the column never moves as a cell ages.
            let cols: Vec<u8> =
                (0..=bits::MAX_AGE).map(|a| base.with_age(a).sprite() & 15).collect();
            assert!(cols.iter().all(|&c| c == cols[0]), "kind {kind} changed column with age");
        }
    }

    /// **The byte and the sheet position are two different numbers now**, and
    /// the position is the one the old byte used to be.
    ///
    /// Worth pinning, because the whole justification for splitting the kind
    /// around the age was that the byte fell out as an index — so the thing to
    /// check when putting it back together is that nothing on the sheet moved.
    #[test]
    fn the_sprite_index_is_the_byte_the_old_layout_stored() {
        for kind in 0..=bits::KIND_MASK {
            for age in 0..=bits::MAX_AGE {
                for (alive, ice) in [(false, false), (true, false), (false, true), (true, true)] {
                    let cell = Cell::DEAD
                        .with_kind(Kind(kind))
                        .with_age(age)
                        .with_alive(alive)
                        .with_ice(ice);
                    // What the old packing produced: alive, ice, the kind's
                    // low two bits, the age, then the kind's third bit.
                    let was = (alive as u8)
                        | ((ice as u8) << 1)
                        | ((kind & 3) << 2)
                        | (age << 4)
                        | ((kind >> 2) << 7);
                    assert_eq!(cell.sprite(), was, "kind {kind} age {age} moved on the sheet");
                }
            }
        }
    }

    /// **The art that exists does not move.** Every kind in play is 0..3 and
    /// every cell today is age nought, so all of it stays in the sheet's first
    /// row, exactly where the old `kind * 4 + state` mapping put it.
    #[test]
    fn the_kinds_that_exist_are_where_they_always_were() {
        for kind in Kind::ALL {
            for (i, (alive, ice)) in
                [(false, false), (true, false), (false, true), (true, true)].iter().enumerate()
            {
                let tile = Cell::DEAD.with_kind(kind).with_alive(*alive).with_ice(*ice).tile();
                assert_eq!(tile, kind.0 * 4 + i as u8, "{kind:?} moved in the sheet");
            }
        }
    }

    /// Age and kind share a byte and must not read each other's bits.
    #[test]
    fn age_and_kind_do_not_collide() {
        for kind in 0..=bits::KIND_MASK {
            for age in 0..=bits::MAX_AGE {
                let cell = Cell::DEAD
                    .with_kind(Kind(kind))
                    .with_age(age)
                    .with_alive(true)
                    .with_ice(true)
                    .with_player(PlayerId(9));
                assert_eq!(cell.kind(), Kind(kind), "age {age} ate the kind");
                assert_eq!(cell.age(), age, "kind {kind} ate the age");
                assert!(cell.is_alive() && cell.is_ice());
                assert_eq!(cell.player(), PlayerId(9), "the owner byte moved");
            }
        }
    }

    /// Nothing counts age yet, so a step must leave it alone rather than
    /// quietly zeroing it — see `bits::AGE_SHIFT`.
    #[test]
    fn nothing_advances_age_yet() {
        let mut w = crate::sim::World::infinite_empty();
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            w.set_cell_at(r, c, Cell::alive(PlayerId(1)).with_age(5));
        }
        w.step();
        assert_eq!(w.cell_at(0, 0).map(Cell::age), Some(5), "a step moved the age");
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
