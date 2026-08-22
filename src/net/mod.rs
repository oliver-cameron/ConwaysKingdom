//! Wire types shared by client and server.
//!
//! Scaffolding: the shapes are here so the seams exist, but no transport,
//! encoding or session handling is implemented yet.
//!
//! The model this is shaped for: both sides hold a copy of the world and run
//! the same deterministic step from [`crate::sim`]. The client holds less of it
//! — roughly its viewport and a margin — and advances it locally. The server is
//! authoritative and is consulted only for what a client cannot derive:
//!
//! 1. other players' actions,
//! 2. changes with no local cause (spawns, scripted events, admin edits),
//! 3. chunks the client does not hold, when its viewport moves.
//!
//! Nothing here may depend on [`crate::render`].

pub mod codec;
pub mod token;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
#[cfg(target_arch = "wasm32")]
pub mod link_web;
#[cfg(target_arch = "wasm32")]
pub use link_web as link;

use serde::{Deserialize, Serialize};

use crate::sim::{Cell, Coord, PlayerId, World};

/// A chunk is identified by where it is. There is no separate id to allocate,
/// keep unique, or reconcile after a reconnect — two peers naming the same
/// coordinate mean the same chunk. On a toroidal world, fold with
/// [`crate::sim::World::canonical`] before comparing.
pub type ChunkId = Coord;

/// Generation number. The unit of lockstep: an action is applied *at* a tick,
/// so both sides apply it at the same point in the sequence.
pub type Tick = u64;

/// What a player is putting down.
///
/// Named rather than carried as raw cell bits: the server has to be able to
/// judge whether a placement is allowed, and it can only do that against a
/// vocabulary it understands. A client that could send arbitrary bits could
/// place anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    /// Life, owned by whoever placed it. Named for what is placed rather than
    /// for what holds it: a cell is the square, and life is one of the two
    /// things that can be on it.
    Life,
    /// A pane. Freezes what it covers, and is independent of whether the cell
    /// beneath is alive.
    Ice,
}

impl Placement {
    /// Lay this over whatever is already there.
    ///
    /// A transform rather than a value, because alive and ice are
    /// independent: laying a pane over a living cell must leave the cell
    /// living, and building a cell under an existing pane must leave the pane.
    /// Replacing the cell outright would silently destroy one to place the
    /// other.
    pub fn apply_to(self, existing: Cell, player: PlayerId) -> Cell {
        match self {
            Self::Life => existing.with_alive(true).with_player(player),
            // The pane belongs to whoever laid it. There is one owner field
            // per cell, so icing another player's living cell takes the
            // cell with it -- deliberate, and the reason a pane costs what it
            // does.
            Self::Ice => existing.with_ice(true).with_player(player),
        }
    }

    /// What one of these costs to put down.
    ///
    /// Life is cheap because it is drawn by the stroke rather than placed cell
    /// by cell: a pencil lays tens of cells in a gesture, and at five a cell
    /// that is a gesture nobody can afford. Ice stays dear because a pane is a
    /// wall, and a wall that costs what a cell costs is not a decision.
    ///
    /// Life at one against reclaiming at one means putting a cell down and
    /// taking it back is free, which is deliberate: you may rearrange your own
    /// board as much as you like. What drains value is the rule — a cell that
    /// dies of its neighbours cannot be reclaimed, so the sink is mortality
    /// rather than the act of placing.
    pub const fn cost(self) -> i32 {
        match self {
            Self::Life => 1,
            Self::Ice => 5,
        }
    }

    /// Whether a player may take this back once it is down.
    ///
    /// Ice may not. A pane stops time over whatever it covers, and being able
    /// to lift one at will would make it cheap to undo as well as strong to
    /// place. What removes ice is life reaching it — something an opponent can
    /// arrange with a glider and the owner cannot simply click away.
    pub const fn can_be_taken(self) -> bool {
        match self {
            Self::Life => true,
            Self::Ice => false,
        }
    }

    /// Take this away, and leave everything else alone.
    ///
    /// The inverse of [`Self::apply_to`], and the reason clicking a living
    /// cell under ice kills the life without taking the pane with it. Life and
    /// ice are independent flags, so removing one must not touch the other —
    /// clearing the cell outright would destroy a pane the player did not aim
    /// at, and at five a cell that is an expensive misunderstanding.
    ///
    /// The owner stays. A cell keeps its owner when it dies of the rule, and
    /// `Chunk::is_empty` asks about life and ice rather than about ownership,
    /// so a cleared cell still lets its chunk be dropped.
    pub fn remove_from(self, existing: Cell) -> Cell {
        match self {
            Self::Life => existing.with_alive(false),
            Self::Ice => existing.with_ice(false),
        }
    }
}

/// Something a player did. Deliberately not raw keystrokes: input is resolved
/// to a world effect before it goes on the wire, so the server validates an
/// intent rather than replaying a keyboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Put something down for this player, at absolute cell coordinates.
    Paint {
        cells: Vec<(i32, i32)>,
        placement: Placement,
    },
    /// Take a placement away at absolute cell coordinates, leaving whatever
    /// else is on those cells. Carries what to remove for the same reason
    /// `Paint` carries what to lay: the server judges an intent, and "clear
    /// this square" is a different intent from "kill the life on it".
    Erase {
        cells: Vec<(i32, i32)>,
        placement: Placement,
    },
}

/// An action stamped with who did it and when, which is what makes replay on
/// another peer produce the same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    pub tick: Tick,
    pub player: PlayerId,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Asking to play. `token` is the secret a previous `Welcome` handed out,
    /// if this client has one — presenting it asks for that player back rather
    /// than a new one.
    Join {
        name: String,
        token: Option<String>,
    },
    /// What this player did, and when they believe it happened.
    Act(Stamped),
    /// The chunks the client now needs, because its viewport moved.
    Subscribe { chunks: Vec<ChunkId> },
    /// Chunks the client has dropped and no longer wants updates for.
    Unsubscribe { chunks: Vec<ChunkId> },
    /// Per-chunk digests of what the client holds, so the server can spot a
    /// desync. Per chunk rather than whole-world: a client holds only what its
    /// viewport covers, so a world digest would always disagree.
    Checkpoint { tick: Tick, chunks: Vec<(ChunkId, u64)> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Accepted, and here is the number your cells will carry — and where the
    /// ground you were granted is.
    ///
    /// The spawn is sent rather than derived because where it lands depends on
    /// the shape of the world, and the client does not know that until it is
    /// told. A client that guessed would look at empty ground and find it
    /// could build on none of it.
    Welcome {
        you: PlayerId,
        tick: Tick,
        spawn: (i32, i32),
        /// Keep this. Presenting it on a later `Join` asks for this player
        /// back — the same number, the same value, the same ground.
        token: String,
        /// What this player has to spend.
        ///
        /// Sent, because a returning player has a value already and the client
        /// has no way to know it. Assuming the starting figure left the two
        /// disagreeing from the first frame: the client would offer to spend
        /// money the server knows is gone, and the server would refuse the
        /// difference without the client having anything to show for it.
        value: i32,
    },
    Rejected { reason: String },
    /// One generation happened. `tick` is the generation the world is on
    /// **after** it, and `actions` is what was applied on the way there.
    ///
    /// The unit of lockstep, and the reason it carries the tick rather than
    /// just the actions: a step is a pure function of state and tick, so two
    /// peers only stay identical while they step at the same ticks. A client
    /// that kept its own clock drifted from the server within seconds — same
    /// nominal rate, different phase, nothing correcting it — and every seed
    /// is derived from the generation, so births chose different owners and
    /// territory spread differently on each side. Late joining still looked
    /// right, because that is a snapshot; everything after it was one world
    /// each.
    ///
    /// So the server is the clock. A connected client advances when told and
    /// never on its own.
    Step { tick: Tick, actions: Vec<Stamped> },
    /// Full contents of a chunk the client does not hold. Bytes are a chunk's
    /// cells exactly as `Chunk::as_bytes` produces them.
    ChunkData { tick: Tick, chunk: ChunkId, cells: Vec<u8> },
    /// The client's copy of these chunks is wrong; here they are again.
    Resync { tick: Tick, chunks: Vec<ChunkId> },
}

/// How wide a patch of ground a player is granted when they join, in cells.
///
/// Placing is confined to your own territory, so a player who owned nothing
/// could never place a first cell and so could never grow any. The grant is
/// the seed the rest spreads from.
pub const SPAWN_N: i32 = 12;

/// Whether `player` may put something down on this cell.
///
/// Only inside their own territory: the cell must already carry their number.
/// Territory is the owner field on dead cells, which the rule spreads outward
/// from living ones, so reach grows where life goes and nowhere else — and
/// ground nobody has ever reached belongs to nobody and is closed to everyone.
///
/// Unheld ground reads as unowned, and so as closed. That is the honest answer
/// rather than a hopeful one: a client cannot know what it does not hold, and
/// guessing yes there would let it predict a placement the server refuses.
pub fn may_place(world: &World, player: PlayerId, row: i32, col: i32) -> bool {
    world.cell_at(row, col).is_some_and(|c| c.player() == player)
}

/// How many grants sit along one edge of the square they are laid out in.
/// Six covers all 31 players a five-bit field can hold.
const SPAWN_ACROSS: i32 = 6;

/// Centre to centre between neighbouring grants, in cells. Four times the
/// patch, so there is three patches' worth of unclaimed ground between any two
/// players — enough to build in before anyone's territory meets.
const SPAWN_PITCH: i32 = SPAWN_N * 4;

/// The ground a player is granted on joining: a square of claimed but empty
/// cells, far enough from everyone else's to be their own.
///
/// Laid out in a **square** rather than a line. A line puts the last player
/// thirty patches from the first, so the two could never reach each other and
/// the map is a corridor; a square keeps every player within a few patches of
/// several others, which is the only arrangement in which territory meeting
/// territory is something that happens.
///
/// The world decides the spacing. An infinite one has room, so the grid sits
/// at a fixed pitch centred on the origin, and the world then grows in every
/// direction rather than off into one quadrant. A torus does not: its ground
/// is finite and has to be shared out, so the same grid is spread over
/// whatever there is and **every player still gets their square**, on a small
/// world as much as a large one.
///
/// Computed rather than searched for, so the answer never depends on what a
/// peer happens to hold. It does depend on the world's shape, which a client
/// cannot know until it is told — and that is why `Welcome` carries the spawn
/// rather than leaving the client to work it out and be wrong.
pub fn spawn_for(player: PlayerId, world: &World) -> (i32, i32) {
    let n = player.0 as i32;
    let (row, col) = (n / SPAWN_ACROSS, n % SPAWN_ACROSS);

    match world.size_in_cells() {
        None => {
            let middle = SPAWN_ACROSS / 2;
            ((row - middle) * SPAWN_PITCH, (col - middle) * SPAWN_PITCH)
        }
        Some((height, width)) => {
            // Never closer together than the patch is wide, or grants would
            // overlap before they even wrapped. On a world too small for even
            // that they do overlap, and `grant` leaves claimed ground alone,
            // so the earlier players simply keep theirs.
            let pitch = |extent: i32| (extent / SPAWN_ACROSS).max(SPAWN_N);
            (row * pitch(height), col * pitch(width))
        }
    }
}

/// Whether a world is too small to give every player a square of their own.
pub fn too_cramped_for_grants(world: &World) -> bool {
    world
        .size_in_cells()
        .is_some_and(|(h, w)| h < SPAWN_N * SPAWN_ACROSS || w < SPAWN_N * SPAWN_ACROSS)
}

/// Claim a player's starting ground, with a block standing on it.
///
/// Here rather than on the server because an offline client needs the same
/// grant — placing is confined to territory, so a player who owns nothing can
/// place nothing, and a game of one would have no opening move at all.
///
/// The block is a 2x2 still life: four cells that hold their shape forever
/// under Conway's rules. Everyone starts with the same one, so nobody begins
/// ahead, and because it never changes it costs nothing to leave alone while
/// you decide what to build. It is also what keeps the ground: territory
/// spreads from living cells, so a grant with nothing alive on it would never
/// grow past the patch it was given.
pub fn grant(world: &mut World, player: PlayerId) {
    let (row, col) = spawn_for(player, world);
    for r in row..row + SPAWN_N {
        for c in col..col + SPAWN_N {
            let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
            // Never over someone else's: territory is taken by life reaching
            // it, not handed out on top of what is already held.
            if !cell.player().is_owned() {
                world.set_cell_at(r, c, cell.with_player(player));
            }
        }
    }

    // In the middle, so it has room to grow in any direction the player
    // chooses and is not against an edge of what they own.
    let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
    for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let at = (middle.0 + r, middle.1 + c);
        if world.cell_at(at.0, at.1).is_some_and(|c| c.player() == player) {
            world.set_cell_at(at.0, at.1, Cell::alive(player));
        }
    }
}

/// What reclaiming your own costs, or rather pays.
pub const RECLAIM: i32 = 1;

/// What an action is worth to the player who did it.
///
/// Must be read **before** the action is applied, since it depends on what is
/// there now. Shared by client and server for the same reason `apply` is: two
/// implementations of what something costs are two ways to disagree about who
/// can afford what.
///
/// Reclaiming your own living cell earns one. Placing costs one, and so does
/// destroying someone else's cell — taking ground is not free. Erasing empty
/// space is neither earned nor spent.
pub fn value_delta(world: &World, stamped: &Stamped) -> i32 {
    match &stamped.action {
        // Only the cells a placement actually changes are charged for.
        // Charging for the rest made extending a pane cost as much as laying
        // it again, which is what a drag does constantly: the natural way to
        // make a rectangle bigger is to sweep the whole of it a second time.
        //
        // This reads the world, so a client prices against the chunks it
        // holds rather than against all of them. That is already true of
        // `Erase`, and a player can only paint where they can point, which is
        // on screen and therefore held.
        Action::Paint { cells, placement } => {
            let changed = cells
                .iter()
                .filter(|&&(row, col)| {
                    let existing = world.cell_at(row, col).unwrap_or(Cell::DEAD);
                    placement.apply_to(existing, stamped.player) != existing
                })
                .count();
            -(changed as i32) * placement.cost()
        }
        // What counts as "there" depends on what is being taken, since life
        // and ice are independent: removing ice from a living cell with no
        // pane on it is as much a no-op as erasing empty ground.
        Action::Erase { cells, placement } => cells
            .iter()
            .map(|&(row, col)| match world.cell_at(row, col) {
                Some(cell) if placement.remove_from(cell) == cell => 0,
                Some(cell) if cell.player() == stamped.player => RECLAIM,
                Some(_) => -RECLAIM,
                None => 0,
            })
            .sum(),
    }
}

/// Apply an action to a world.
///
/// Shared deliberately: the client predicts by applying actions locally and the
/// server applies the same ones authoritatively, so two implementations of this
/// would be two ways to disagree.
pub fn apply(world: &mut World, stamped: &Stamped) {
    match &stamped.action {
        Action::Paint { cells, placement } => {
            for &(row, col) in cells {
                let existing = world.cell_at(row, col).unwrap_or(Cell::DEAD);
                world.set_cell_at(row, col, placement.apply_to(existing, stamped.player));
            }
        }
        Action::Erase { cells, placement } => {
            for &(row, col) in cells {
                let existing = world.cell_at(row, col).unwrap_or(Cell::DEAD);
                world.set_cell_at(row, col, placement.remove_from(existing));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(cells: Vec<(i32, i32)>, placement: Placement) -> Stamped {
        Stamped { tick: 0, player: PlayerId(1), action: Action::Paint { cells, placement } }
    }

    /// The reason the pricing reads the world at all. A drag is extended by
    /// sweeping the whole rectangle again, so every cell already laid would be
    /// paid for a second time.
    #[test]
    fn painting_what_is_already_there_is_free() {
        let mut world = World::infinite_empty();
        let cells = vec![(0, 0), (0, 1), (0, 2)];

        let first = paint(cells.clone(), Placement::Ice);
        assert_eq!(value_delta(&world, &first), -3 * Placement::Ice.cost());
        apply(&mut world, &first);

        // The same rectangle again, plus one cell it did not cover.
        let mut wider = cells.clone();
        wider.push((0, 3));
        assert_eq!(
            value_delta(&world, &paint(wider, Placement::Ice)),
            -Placement::Ice.cost(),
            "only the cell that changed should be charged for"
        );
    }

    /// Ice and life are independent, so laying one over the other is a
    /// change even though the cell was not empty.
    #[test]
    fn a_pane_over_a_living_cell_is_a_change() {
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Ice)), -Placement::Ice.cost());
        assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Life)), 0);
    }

    /// A pane belongs to whoever laid it, and there is one owner field per
    /// cell, so icing someone else's ice takes it — and taking it is a
    /// change, whatever the flags say.
    #[test]
    fn taking_over_another_players_pane_is_a_change() {
        let mut world = World::infinite_empty();
        let theirs = Stamped {
            tick: 0,
            player: PlayerId(2),
            action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        apply(&mut world, &theirs);
        assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Ice)), -Placement::Ice.cost());
    }

    /// The reason `Erase` carries a placement at all. Life and ice are
    /// independent, so taking the life off an iced cell must leave the pane
    /// standing — clearing the square outright destroyed a pane the player
    /// never aimed at, at five a cell.
    #[test]
    fn taking_the_life_off_an_iced_cell_leaves_the_ice() {
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        apply(&mut world, &paint(vec![(0, 0)], Placement::Ice));

        let take = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
        };
        assert_eq!(value_delta(&world, &take), 1, "reclaiming your own pays one");
        apply(&mut world, &take);

        let cell = world.cell_at(0, 0).unwrap();
        assert!(!cell.is_alive(), "the life should be gone");
        assert!(cell.is_ice(), "the pane should still be standing");
        assert_eq!(cell.player(), PlayerId(1), "and still belong to whoever laid it");
    }

    /// And the other way about, which is what gives a misplaced pane a way
    /// back: holding Ice and clicking one lifts it, and the life under it
    /// carries on.
    #[test]
    fn taking_the_ice_off_a_living_cell_leaves_the_life() {
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        apply(&mut world, &paint(vec![(0, 0)], Placement::Ice));

        let take = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        apply(&mut world, &take);

        let cell = world.cell_at(0, 0).unwrap();
        assert!(cell.is_alive());
        assert!(!cell.is_ice());
    }

    /// Taking away what is not there is neither earned nor spent, and what
    /// counts as "there" depends on what is being taken.
    #[test]
    fn taking_what_is_not_there_is_free() {
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        let before = world.cell_at(0, 0).unwrap();

        let no_pane = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        assert_eq!(value_delta(&world, &no_pane), 0);
        apply(&mut world, &no_pane);
        assert_eq!(
            world.cell_at(0, 0).unwrap(),
            before,
            "there was no pane to lift, so nothing should have moved"
        );
    }

    /// Breaking someone else's costs one, because taking ground is not free —
    /// and that now covers a pane as well as a cell, since both are theirs.
    #[test]
    fn breaking_another_players_pane_costs_one() {
        let mut world = World::infinite_empty();
        let theirs = Stamped {
            tick: 0,
            player: PlayerId(2),
            action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        apply(&mut world, &theirs);

        let mine = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        assert_eq!(value_delta(&world, &mine), -1);
    }

    /// Life is drawn by the stroke and ice is placed as a wall, so they are
    /// not worth the same. Pinned because one flat constant is exactly what
    /// this replaced, and it is an easy thing to fall back to.
    #[test]
    fn life_and_ice_are_priced_apart() {
        assert_eq!(Placement::Life.cost(), 1);
        assert_eq!(Placement::Ice.cost(), 5);

        let world = World::infinite_empty();
        let five: Vec<_> = (0..5).map(|c| (0, c)).collect();
        assert_eq!(value_delta(&world, &paint(five.clone(), Placement::Life)), -5);
        assert_eq!(value_delta(&world, &paint(five, Placement::Ice)), -25);
    }

    /// Placing is confined to a player's own ground, and the grant is what
    /// gives them any. Without it a new player owns nothing, may place
    /// nothing, and so can never come to own anything.
    #[test]
    fn a_player_may_build_only_on_their_own_ground() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        let (row, col) = spawn_for(me, &world);

        assert!(!may_place(&world, me, row, col), "nothing is owned yet");
        grant(&mut world, me);
        assert!(may_place(&world, me, row, col), "granted ground is buildable");
        assert!(!may_place(&world, them, row, col), "and only by its owner");

        // Ground at the edges, and a block standing in the middle of it.
        assert!(!world.cell_at(row, col).unwrap().is_alive(), "the corner is bare");
        let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
        let block: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 1)]
            .iter()
            .map(|(r, c)| world.cell_at(middle.0 + r, middle.1 + c).unwrap())
            .collect();
        assert!(block.iter().all(|c| c.is_alive() && c.player() == me), "a 2x2 block");

        // Beyond the patch is nobody's, and nobody's is closed to everyone.
        assert!(!may_place(&world, me, row, col + SPAWN_N));
        assert!(!may_place(&world, me, 10_000, 10_000));
    }

    /// Every player is within reach of several others. A line put the last
    /// player thirty patches from the first, which is a corridor rather than a
    /// map: two players at opposite ends could never meet.
    #[test]
    fn grants_are_laid_out_in_a_square() {
        let world = World::infinite_empty();
        let spots: Vec<(i32, i32)> =
            (1..=PlayerId::MAX).map(|p| spawn_for(PlayerId(p), &world)).collect();
        let rows: Vec<i32> = spots.iter().map(|s| s.0).collect();
        let cols: Vec<i32> = spots.iter().map(|s| s.1).collect();

        let span = |v: &[i32]| v.iter().max().unwrap() - v.iter().min().unwrap();
        assert!(span(&rows) > 0, "a line has no second axis");
        assert!(
            span(&rows).abs_diff(span(&cols)) <= SPAWN_PITCH as u32,
            "the layout should be square, got {}x{}",
            span(&rows),
            span(&cols)
        );

        // Every player has a neighbour one pitch away, which a line only gives
        // to the two beside you.
        for &(row, col) in &spots {
            let touching = spots
                .iter()
                .filter(|&&(r, c)| {
                    let (dr, dc) = ((r - row).abs(), (c - col).abs());
                    (dr, dc) != (0, 0) && dr <= SPAWN_PITCH && dc <= SPAWN_PITCH
                })
                .count();
            assert!(touching >= 2, "({row}, {col}) has only {touching} neighbours");
        }
    }

    /// Every player gets their square on a torus too, which is what a torus
    /// makes hard: the ground is finite, so a fixed pitch would run off the
    /// end and wrap one player's grant onto another's. The grid is spread over
    /// whatever ground there is instead.
    #[test]
    fn a_torus_still_gives_everyone_a_square() {
        // Big enough that the grid fits without crowding.
        let mut world = World::toroidal_empty(24, 24);
        assert!(!too_cramped_for_grants(&world));
        for id in 1..=PlayerId::MAX {
            grant(&mut world, PlayerId(id));
        }

        for id in 1..=PlayerId::MAX {
            let (row, col) = spawn_for(PlayerId(id), &world);
            let mine = (row..row + SPAWN_N)
                .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
                .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == PlayerId(id))
                .count();
            assert_eq!(
                mine,
                (SPAWN_N * SPAWN_N) as usize,
                "player {id} did not get a whole square"
            );
        }
    }

    /// And a world too small to go round says so rather than pretending. The
    /// earlier players keep theirs; the later ones get what is left.
    #[test]
    fn a_torus_too_small_is_reported() {
        let small = World::toroidal_empty(2, 2);
        assert!(too_cramped_for_grants(&small), "32x32 cells cannot hold 31 squares");
        let roomy = World::toroidal_empty(24, 24);
        assert!(!too_cramped_for_grants(&roomy));
        assert!(!too_cramped_for_grants(&World::infinite_empty()), "infinite has room");
    }

    /// Two players' grants must not overlap, or one would be building on the
    /// other from the first move.
    #[test]
    fn grants_do_not_overlap() {
        let mut world = World::infinite_empty();
        for id in 1..=PlayerId::MAX {
            grant(&mut world, PlayerId(id));
        }
        for id in 1..=PlayerId::MAX {
            let (row, col) = spawn_for(PlayerId(id), &world);
            for r in row..row + SPAWN_N {
                for c in col..col + SPAWN_N {
                    assert_eq!(
                        world.cell_at(r, c).unwrap().player(),
                        PlayerId(id),
                        "({r}, {c}) should belong to {id}"
                    );
                }
            }
        }
    }

    /// Ground nobody holds prices as empty, which is what `apply` writes into
    /// it. The two must agree or a client would be charged for one thing and
    /// given another.
    #[test]
    fn unheld_ground_prices_as_empty() {
        let world = World::infinite_empty();
        let far = vec![(100_000, 100_000)];
        assert!(world.cell_at(far[0].0, far[0].1).is_none());
        assert_eq!(value_delta(&world, &paint(far, Placement::Life)), -Placement::Life.cost());
    }
}
