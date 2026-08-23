//! Wire types shared by client and server.
//!
//! The transport is [`link`] on the client and [`crate::server::ws`] on the
//! server; the encoding is [`codec`]. What lives here is the vocabulary the
//! two speak, and the handful of rules -- pricing, territory, grants -- that
//! both sides have to answer identically or they disagree about what happened.
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
pub mod keep;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
#[cfg(target_arch = "wasm32")]
pub mod link_web;
#[cfg(target_arch = "wasm32")]
pub use link_web as link;

use serde::{Deserialize, Serialize};

use crate::sim::{Cell, Coord, Kind, PlayerId, World, WorldKind, CHUNK_N};

/// A chunk is identified by where it is. There is no separate id to allocate,
/// keep unique, or reconcile after a reconnect — two peers naming the same
/// coordinate mean the same chunk. On a toroidal world, fold with
/// [`crate::sim::World::canonical`] before comparing.
pub type ChunkId = Coord;

/// Generation number. The unit of lockstep: an action is applied *at* a tick,
/// so both sides apply it at the same point in the sequence.
pub type Tick = u64;

/// What a room is called. A room is a whole separate world on one server, so
/// this names the world a player is in, not a channel inside a shared one.
pub type RoomName = String;

/// The room a client that names none is put in.
pub const DEFAULT_ROOM: &str = "main";

/// The longest a room name may be. Short enough to read in a log line and in
/// the HUD, and long enough to be a word rather than a code.
pub const ROOM_NAME_MAX: usize = 24;

/// Normalise a room name, or say why it is not one.
///
/// Lowercased, because the name is also the save file's name and a
/// case-insensitive filesystem would make `Lobby` and `lobby` two rooms on one
/// machine and one room on another — which is a world that appears and
/// disappears depending on where the server is running.
///
/// The charset is narrow for the same reason: a room name reaches the
/// filesystem, and `../` or a path separator in it would be a name that
/// escapes the directory rooms live in. Validated here rather than at the file
/// layer so a client can refuse the same name locally and for the same reason.
pub fn room_name(raw: &str) -> Result<RoomName, String> {
    let name = raw.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("a room needs a name".into());
    }
    if name.len() > ROOM_NAME_MAX {
        return Err(format!("a room name is at most {ROOM_NAME_MAX} characters"));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_'))
    {
        return Err(format!(
            "a room name is letters, digits, - and _; {bad:?} is not one of them"
        ));
    }
    Ok(name)
}

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
    /// A living cell that pays its owner every time one of its kind is born.
    ///
    /// Bought once and inherited afterwards: a birth copies its parent, so a
    /// mine's children are mines, and since a birth picks one of three parents
    /// at random the kind spreads through a mixed population rather than being
    /// handed down whole. What you are paying for is a **lineage**, not a
    /// cell — which is why it costs what a stroke of life costs ten times over.
    Mine,
    /// A living cell that claims ground at range: every generation it takes
    /// the nearest square that is not its owner's and makes it theirs, and a
    /// dead one runs that backwards over the ground behind it.
    ///
    /// The opposite of a mine, and priced by the **emplacement** rather than
    /// by the cell. A mine is bought once per lineage, because a birth copies
    /// it; a turret is not inherited, so it is bought once per cell forever —
    /// and one turret is one live cell and dies of loneliness in a generation,
    /// so the smallest turret that works is four of them in a block.
    Turret,
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
            Self::Life => existing
                .with_alive(true)
                .with_player(player)
                // Placed life is ordinary life. Without this, drawing over a
                // mine's corpse would hand you a free mine -- the kind is on
                // the cell and outlives the life that carried it.
                .with_kind(Kind::NORMAL),
            Self::Mine => existing
                .with_alive(true)
                .with_player(player)
                .with_kind(Kind::MINE),
            Self::Turret => existing
                .with_alive(true)
                .with_player(player)
                .with_kind(Kind::TURRET),
            // The pane belongs to whoever laid it. There is one owner field
            // per cell, so icing another player's living cell takes the
            // cell with it -- deliberate, and the reason a pane costs what it
            // does.
            Self::Ice => existing.with_ice(true).with_player(player),
        }
    }

    /// Whether this is what the square already holds — the question a click
    /// asks to decide whether it places or takes back.
    ///
    /// Not "would taking it away change anything", which is what this used to
    /// be. Life and a mine are both taken away by clearing the same bit, so a
    /// mine held over ordinary life read as already there and the click killed
    /// the cell rather than converting it. What a player holding Mine over
    /// their own life means is *make this a mine*, and the only click that
    /// should take a mine back is one holding a mine.
    ///
    /// So life and a mine are **different things to hold**, where life and ice
    /// are independent things to hold: clicking one over the other replaces
    /// the kind, and clicking Life on a living cell under a pane still kills
    /// the life and leaves the pane standing.
    ///
    /// The owner is no part of it. Somebody else's life is still life, so a
    /// click holding Life takes it — which is what lets you clear a glider
    /// that has flown onto your ground, priced at [`RECLAIM`] because taking
    /// another player's should not be free.
    ///
    /// A corpse is not what it was. A mine's kind outlives the life that
    /// carried it, so a dead mine holds no life for either placement to take
    /// and a click over it places, which is what stops drawing over a corpse
    /// handing out a free mine.
    pub fn is_on(self, existing: Cell) -> bool {
        match self {
            Self::Life => existing.is_alive() && existing.kind() == Kind::NORMAL,
            Self::Mine => existing.is_alive() && existing.kind() == Kind::MINE,
            Self::Turret => existing.is_alive() && existing.kind() == Kind::TURRET,
            Self::Ice => existing.is_ice(),
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
            Self::Life => LIFE_COST,
            Self::Ice => ICE_COST,
            Self::Mine => MINE_COST,
            Self::Turret => TURRET_COST,
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
            // A mine is a live cell like any other, so taking it back is
            // taking back the life -- and at the reclaim rate, so a misplaced
            // one costs what it cost minus one. That is the commitment a mine
            // should carry without being a trap.
            Self::Mine => true,
            // As with a mine: it is a live cell, so taking it back is taking
            // back the life. A misplaced turret is dear, and a turret you
            // cannot pick up would make the fourth click of an emplacement a
            // trap rather than a decision.
            Self::Turret => true,
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
            // The kind stays on the corpse, as it does when a cell dies of the
            // rule: what is being taken back is the life.
            Self::Life | Self::Mine | Self::Turret => existing.with_alive(false),
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

/// One room, as a menu needs to show it.
///
/// Enough to choose by and no more: which world, whether anybody is in it, and
/// whether it ends. Not the tick, not the chunk count — a room is picked on
/// what it is like to be in, and neither of those says anything about that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomInfo {
    pub name: RoomName,
    /// Players connected right now, not players the room has ever seen. The
    /// second number is the one the world remembers and the wrong one to
    /// choose a room by.
    pub players: u32,
    pub world: WorldKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Asking to play. `token` is the secret a previous `Welcome` handed out,
    /// if this client has one — presenting it asks for that player back rather
    /// than a new one.
    Join {
        name: String,
        token: Option<String>,
        /// Which world to join. `None` takes the server's default room, so a
        /// client with nothing to say about rooms still lands somewhere.
        ///
        /// A room is a separate world, not a channel inside a shared one, so
        /// this decides which cells the player will ever see — and player
        /// numbers, value, territory and the rejoin token are all per room. A
        /// player in two rooms is two players.
        room: Option<RoomName>,
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
    /// What rooms are here?
    ///
    /// Answerable **before joining**, and that is the point of it: a player
    /// has to see the worlds before picking one, and a room is a world, so
    /// there is no way to look first and choose after without asking from
    /// outside every room. It names no world itself, which is why it is one of
    /// the two messages a connection with no seat may send.
    Rooms,
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
        /// Which room this is. Named back rather than assumed, because the
        /// client may have asked for none and because the token it is about to
        /// keep is filed under this name — a token stored against the wrong
        /// room returns you to the wrong world, which is worse than having no
        /// token at all.
        room: RoomName,
        /// The shape of the world, so the client builds the same one.
        ///
        /// Without it a client joining a toroidal server built an infinite
        /// world locally: coordinates never folded, so the far side of the
        /// world was somewhere else entirely, chunk digests were taken against
        /// coordinates the server had never heard of, and the seam showed as
        /// soon as anything crossed it. The shape is not something a client can
        /// derive — nothing it can see says whether the ground ends.
        world: WorldKind,
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
    /// What this player actually has to spend.
    ///
    /// Sent in reply to a `Checkpoint`, which is the only regular thing a
    /// client says. Value used to be predictable from a client's own actions
    /// alone; mining made it depend on births anywhere in the world, and a
    /// client holds a viewport — so its number drifts below the server's for
    /// as long as it plays, and nothing else would ever correct it.
    Purse { value: i32 },
    /// The rooms this server has, in the order it lists them.
    ///
    /// Ordered by the server rather than sorted by the client, so two players
    /// looking at the same menu see the same list in the same order — and so
    /// the order is one thing a server can decide rather than a thing that
    /// happens.
    Rooms { rooms: Vec<RoomInfo> },
}

/// How wide a patch of ground a player is granted when they join, in cells.
///
/// Placing outside your own territory costs ten times as much, so a player who
/// owned nothing could still act but would pay a mine's price for a cell of
/// life. The grant is the ground the cheap rate applies on, and the seed the
/// rest spreads from.
pub const SPAWN_N: i32 = 12;

/// Whether this cell is inside `player`'s own territory — the cell carries
/// their number.
///
/// Territory is the owner field on dead cells, which the rule spreads outward
/// from living ones, so a player's ground grows where their life goes.
///
/// This used to be `may_place`, and placing anywhere else was refused. It is a
/// **price** now: outside costs [`OUTSIDE_MULTIPLIER`] times as much, and the
/// question this asks is which of the two rates applies. Refusing made
/// territory a wall — a player whose life went out could never place again,
/// and reaching a neighbour meant growing all the way to them — where a price
/// lets somebody buy their way somewhere and feel it.
///
/// Somebody else's ground and ground nobody has reached are the same answer,
/// because they cost the same: what is being paid for is putting something
/// where your own life has not got to.
///
/// Unheld ground reads as outside, which is the honest answer rather than a
/// hopeful one: a client cannot know what it does not hold, and guessing yes
/// there would let it predict a cheaper price than the server charges.
pub fn own_ground(world: &World, player: PlayerId, row: i32, col: i32) -> bool {
    world.cell_at(row, col).is_some_and(|c| c.player() == player)
}

/// What one cell of a placement costs here: the placement's own price inside
/// the player's territory, and [`OUTSIDE_MULTIPLIER`] times it outside.
///
/// Per cell rather than per action, because a drag crosses the boundary all
/// the time — a stroke that starts on your ground and runs off it is the
/// ordinary case, and charging the whole of it at either rate would be wrong
/// in one direction or the other.
pub fn cell_cost(world: &World, player: PlayerId, row: i32, col: i32, placement: Placement) -> i32 {
    let rate = if own_ground(world, player, row, col) { 1 } else { OUTSIDE_MULTIPLIER };
    placement.cost() * rate
}

/// How many grants sit along one edge of the square they are laid out in.
/// Six covers all 31 players a five-bit field can hold.
const SPAWN_ACROSS: i32 = 6;

/// No-man's-land between one grant's edge and the next, in cells.
///
/// Measured in **chunks** because that is the unit the world is built and
/// drawn in, and because "how far away is my neighbour" is a question about
/// the map rather than about the size of a patch. Three of them is far enough
/// that neither player can see the other's opening at a comfortable zoom, and
/// near enough that a glider crosses it in a hundred generations.
///
/// It was three patches' worth, thirty-six cells, and that read as close: two
/// grants nearly touching at chunk scale, with the halo each block holds
/// already a third of the way across. What the gap is really buying is the
/// time before anyone's territory meets, and it wants to be enough to build a
/// machine in.
const SPAWN_GAP: i32 = 3 * CHUNK_N as i32;

/// Centre to centre between neighbouring grants, in cells: a patch, plus the
/// ground between it and the next one.
const SPAWN_PITCH: i32 = SPAWN_N + SPAWN_GAP;

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
            // A torus has finite ground and has to share it out, so the
            // spacing comes from the world rather than from `SPAWN_PITCH` --
            // which means widening the gap above does nothing here, and a
            // small torus puts players close together however it is set.
            //
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

    // **Dead ground is claimed whoever it belonged to.** It used to be claimed
    // only where nobody held it, on the principle that territory is taken by
    // life reaching it rather than handed out over what is already held. That
    // principle costs a player the game.
    //
    // Territory only ever spreads -- there is no die-off -- so on a world with
    // an edge it eventually covers everything. A player joining after that got
    // a patch of nothing: no ground, and therefore no block, since the block
    // is only placed on ground they own. Placing is confined to your own
    // territory, so they could place nothing, could never come to own
    // anything, and were locked out of a world they were looking at. On a
    // torus that is not an edge case, it is what happens to the second player
    // to arrive at a world that has been running.
    //
    // Living cells are still untouched. A grant takes ground, never anybody's
    // life or their panes -- and dead ground is the thing the rule hands
    // around freely anyway, since a corpse's owner flips to whoever grows over
    // it.
    for r in row..row + SPAWN_N {
        for c in col..col + SPAWN_N {
            let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
            if !cell.is_alive() && !cell.is_ice() {
                // Marked as home, which is what keeps it: territory decays
                // where nothing alive is touching it, and a granted patch is
                // mostly empty by definition. Without the mark a player's
                // opening ground would fade under them in a few seconds and
                // take their ability to place anything with it.
                world.set_cell_at(r, c, cell.with_player(player).with_home(true));
            }
        }
    }

    // A 2x2 block, as near the middle as there is room for: in the middle it
    // has space to grow in any direction and is not against an edge of what
    // they own. Searched rather than placed blind, because the middle four may
    // be somebody's life or under their pane, and a block with a cell missing
    // is not a still life -- it is three cells that die.
    if let Some((r0, c0)) = block_site(world, player, row, col) {
        for (dr, dc) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            world.set_cell_at(r0 + dr, c0 + dc, Cell::alive(player));
        }
    } else {
        log::warn!("{player:?} was granted ground with nowhere to stand a block");
    }
}

/// Where in a granted patch a 2x2 block will fit: four cells that are dead,
/// free of ice, and now this player's.
///
/// Nearest the middle first, so the usual answer is the middle and the search
/// only matters on ground somebody else is already using.
fn block_site(world: &World, player: PlayerId, row: i32, col: i32) -> Option<(i32, i32)> {
    let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
    let free = |r: i32, c: i32| {
        world
            .cell_at(r, c)
            .is_some_and(|cell| cell.player() == player && !cell.is_alive() && !cell.is_ice())
    };
    let fits = |r: i32, c: i32| free(r, c) && free(r, c + 1) && free(r + 1, c) && free(r + 1, c + 1);

    let mut sites: Vec<(i32, i32)> = (row..row + SPAWN_N - 1)
        .flat_map(|r| (col..col + SPAWN_N - 1).map(move |c| (r, c)))
        .filter(|&(r, c)| fits(r, c))
        .collect();
    // Sorted by distance from the middle, and by coordinate to break ties, so
    // the answer never depends on iteration order -- the client works this out
    // for an offline game and must reach the same one.
    sites.sort_unstable_by_key(|&(r, c)| {
        ((r - middle.0).abs() + (c - middle.1).abs(), r, c)
    });
    sites.first().copied()
}

/// The prices themselves live with the rules, in [`crate::sim::rule`] —
/// "life costs one" is the same kind of statement as "a cell survives on two
/// or three", and somebody balancing the game should not have to look in two
/// files. This module names the actions and reads the numbers.
pub use crate::sim::{
    ICE_COST, LIFE_COST, MINE_COST, MINE_DRAIN, MINE_YIELD, OUTSIDE_MULTIPLIER, RECLAIM,
    TURRET_COST,
};

/// What a generation's tally is worth to one player.
///
/// Here rather than in `sim` because it is a price, and the rule should not
/// know prices — it counts births and deaths and this says what they are worth.
pub fn earnings(mined: &crate::sim::Mined, player: PlayerId) -> i32 {
    let at = player.0 as usize;
    mined.born[at] as i32 * MINE_YIELD - mined.upkeep[at] as i32 * MINE_DRAIN
}

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
        Action::Paint { cells, placement } => -cells
            .iter()
            .map(|&(row, col)| {
                let existing = world.cell_at(row, col).unwrap_or(Cell::DEAD);
                if placement.apply_to(existing, stamped.player) == existing {
                    return 0;
                }
                cell_cost(world, stamped.player, row, col, *placement)
            })
            .sum::<i32>(),
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

    /// Why a client must not apply its own action a second time when the
    /// server broadcasts it back.
    ///
    /// A `Paint` is idempotent on the generation it was meant for and not one
    /// generation later: by then the cells it named have moved, and laying
    /// them again puts the original pattern back on top of where it went. The
    /// symptom is a glider that turns into a blob and settles into a still
    /// life, and then snaps back to a glider when the resync lands.
    #[test]
    fn a_paint_applied_late_is_not_the_paint_you_asked_for() {
        let glider = vec![(1, 2), (2, 3), (3, 1), (3, 2), (3, 3)];
        let paint = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Paint { cells: glider, placement: Placement::Life },
        };

        // The server. The action lands after it has already stepped, which is
        // the ordinary case as soon as there is any latency at all, so it lays
        // the cells on untouched ground and steps.
        let mut server = World::infinite_empty();
        server.step();
        apply(&mut server, &paint);
        server.step();

        // A client that predicted the paint a generation earlier, stepped when
        // it was told a generation had happened, and then applied the same
        // action again when the server broadcast it back.
        let mut twice = World::infinite_empty();
        apply(&mut twice, &paint);
        twice.step();
        apply(&mut twice, &paint);
        twice.step();

        // The same client, skipping what it had already predicted.
        let mut once = World::infinite_empty();
        apply(&mut once, &paint);
        once.step();
        once.step();

        assert_eq!(server.live_cells().len(), 5, "the server has a glider");
        assert_eq!(
            once.live_cells().len(),
            5,
            "and so does a client that predicted it: the same five cells, one \
             step out of phase, which is the error prediction is allowed"
        );
        assert!(
            twice.live_cells().len() > 5,
            "where applying it twice leaves {} cells -- the original pattern \
             stamped back over where it went",
            twice.live_cells().len()
        );
    }

    /// Ground already held by `player`, so a price is the base rate rather
    /// than the outside one. Most of these tests are about what a placement
    /// costs, not about where it is, and everywhere is outside on an empty
    /// world.
    fn hold(world: &mut World, cells: &[(i32, i32)], player: PlayerId) {
        for &(row, col) in cells {
            let cell = world.cell_at(row, col).unwrap_or(Cell::DEAD);
            world.set_cell_at(row, col, cell.with_player(player));
        }
    }

    /// The reason the pricing reads the world at all. A drag is extended by
    /// sweeping the whole rectangle again, so every cell already laid would be
    /// paid for a second time.
    #[test]
    fn painting_what_is_already_there_is_free() {
        let mut world = World::infinite_empty();
        let cells = vec![(0, 0), (0, 1), (0, 2), (0, 3)];
        hold(&mut world, &cells, PlayerId(1));
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

    /// Life and a mine are different things to hold, so a click holding one
    /// over the other replaces the kind rather than killing the cell — which
    /// is what `is_on` answers and what `remove_from` could not, since both
    /// are taken away by clearing the same bit.
    #[test]
    fn a_mine_held_over_life_is_not_already_there() {
        let me = PlayerId(1);
        let life = Placement::Life.apply_to(Cell::DEAD, me);
        let mine = Placement::Mine.apply_to(Cell::DEAD, me);

        assert!(Placement::Life.is_on(life), "life is what is on a living cell");
        assert!(!Placement::Mine.is_on(life), "so a mine held over it places");
        assert!(Placement::Mine.is_on(mine));
        assert!(!Placement::Life.is_on(mine), "and life held over a mine places");

        // And placing is what converts, at the price of what is being laid.
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 0)], Placement::Mine)),
            -Placement::Mine.cost(),
            "converting life to a mine costs what a mine costs"
        );
        apply(&mut world, &paint(vec![(0, 0)], Placement::Mine));
        assert_eq!(world.cell_at(0, 0).unwrap().kind(), Kind::MINE);
        assert!(world.cell_at(0, 0).unwrap().is_alive(), "and leaves the cell living");
    }

    /// A turret is bought once per cell forever, where a mine is bought once
    /// per lineage — so it is dearer than a mine, and the price to read is the
    /// **emplacement**: one turret dies of loneliness, and the smallest one
    /// that works is a block of four.
    #[test]
    fn a_turret_is_priced_per_cell_and_placed_in_fours() {
        assert!(TURRET_COST > MINE_COST, "a turret does not inherit, so it costs more");

        let mut world = World::infinite_empty();
        let block = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
        hold(&mut world, &block, PlayerId(1));
        assert_eq!(
            value_delta(&world, &paint(block.clone(), Placement::Turret)),
            -4 * TURRET_COST,
            "an emplacement is four of them"
        );

        apply(&mut world, &paint(block.clone(), Placement::Turret));
        for (row, col) in block {
            let cell = world.cell_at(row, col).unwrap();
            assert!(cell.is_alive());
            assert_eq!(cell.kind(), Kind::TURRET);
        }

        // And it is a third thing to hold, so life over a turret replaces it
        // exactly as life over a mine does.
        let placed = world.cell_at(0, 0).unwrap();
        assert!(Placement::Turret.is_on(placed));
        assert!(!Placement::Life.is_on(placed));
        assert!(!Placement::Mine.is_on(placed));
    }

    /// A corpse holds no life for either placement to take, whatever kind it
    /// kept — which is what stops a click over a dead mine handing out a free
    /// one instead of charging for it.
    #[test]
    fn a_dead_mine_holds_neither_life_nor_a_mine() {
        let corpse = Placement::Mine.apply_to(Cell::DEAD, PlayerId(1)).with_alive(false);
        assert_eq!(corpse.kind(), Kind::MINE);
        assert!(!Placement::Mine.is_on(corpse));
        assert!(!Placement::Life.is_on(corpse));
    }

    /// The owner is no part of the question. Somebody else's life is still
    /// life, so a click holding Life takes it — priced at RECLAIM rather than
    /// converting it for what a cell costs.
    #[test]
    fn somebody_elses_life_is_still_life() {
        let theirs = Placement::Life.apply_to(Cell::DEAD, PlayerId(2));
        assert!(Placement::Life.is_on(theirs));
    }

    /// Ice is independent of life, so a pane is on a square whether or not
    /// anything lives there — and life held over an iced living cell still
    /// takes the life and leaves the pane.
    #[test]
    fn a_pane_is_on_a_square_whatever_lives_under_it() {
        let me = PlayerId(1);
        let iced_life = Placement::Ice.apply_to(Placement::Life.apply_to(Cell::DEAD, me), me);
        assert!(Placement::Ice.is_on(iced_life));
        assert!(Placement::Life.is_on(iced_life));
        assert!(Placement::Life.remove_from(iced_life).is_ice(), "the pane stands");
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
        // Their pane, so their ground: laying over it is a change, and it is
        // one bought at the outside rate.
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 0)], Placement::Ice)),
            -Placement::Ice.cost() * OUTSIDE_MULTIPLIER
        );
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

        let mut world = World::infinite_empty();
        let five: Vec<_> = (0..5).map(|c| (0, c)).collect();
        hold(&mut world, &five, PlayerId(1));
        assert_eq!(value_delta(&world, &paint(five.clone(), Placement::Life)), -5);
        assert_eq!(value_delta(&world, &paint(five, Placement::Ice)), -25);
    }

    /// Placing outside your own ground costs [`OUTSIDE_MULTIPLIER`] times as
    /// much — a price rather than a wall, so a player whose life went out can
    /// still act, and reaching a neighbour is something you can buy instead of
    /// having to grow all the way there.
    ///
    /// Somebody else's ground and ground nobody has reached are the same
    /// answer, because what is being paid for is the same thing.
    #[test]
    fn placing_outside_your_own_ground_costs_ten_times() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        hold(&mut world, &[(0, 0)], me);
        hold(&mut world, &[(0, 1)], them);

        assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Life)), -LIFE_COST);
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 1)], Placement::Life)),
            -LIFE_COST * OUTSIDE_MULTIPLIER,
            "somebody else's ground"
        );
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 2)], Placement::Life)),
            -LIFE_COST * OUTSIDE_MULTIPLIER,
            "and ground nobody has reached, at the same rate"
        );

        // Per cell, not per action: a stroke that runs off your own ground is
        // the ordinary case, and charging the whole of it at either rate would
        // be wrong in one direction or the other.
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 0), (0, 1), (0, 2)], Placement::Life)),
            -LIFE_COST * (1 + 2 * OUTSIDE_MULTIPLIER)
        );
    }

    /// Taking is not what changed. Erasing is priced on whose it is, at the
    /// reclaim rate, wherever it is.
    #[test]
    fn taking_is_not_charged_the_outside_rate() {
        let mut world = World::infinite_empty();
        let them = PlayerId(2);
        apply(
            &mut world,
            &Stamped {
                tick: 0,
                player: them,
                action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Life },
            },
        );
        let mine = Stamped {
            tick: 0,
            player: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
        };
        assert_eq!(value_delta(&world, &mine), -RECLAIM);
    }

    /// The grant is still what a player starts from — not because it is the
    /// only ground they may build on any more, but because it is the ground
    /// the cheap rate applies on.
    #[test]
    fn a_grant_is_ground_at_the_base_rate() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        let (row, col) = spawn_for(me, &world);

        assert!(!own_ground(&world, me, row, col), "nothing is owned yet");
        grant(&mut world, me);
        assert!(own_ground(&world, me, row, col), "granted ground is buildable");
        assert!(!own_ground(&world, them, row, col), "and only by its owner");

        // Ground at the edges, and a block standing in the middle of it.
        assert!(!world.cell_at(row, col).unwrap().is_alive(), "the corner is bare");
        let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
        let block: Vec<_> = [(0, 0), (0, 1), (1, 0), (1, 1)]
            .iter()
            .map(|(r, c)| world.cell_at(middle.0 + r, middle.1 + c).unwrap())
            .collect();
        assert!(block.iter().all(|c| c.is_alive() && c.player() == me), "a 2x2 block");

        // Beyond the patch is nobody's, and nobody's is closed to everyone.
        assert!(!own_ground(&world, me, row, col + SPAWN_N));
        assert!(!own_ground(&world, me, 10_000, 10_000));
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
    /// The bug that locked a player out of a world they were looking at.
    ///
    /// Territory only ever spreads, so a world with an edge eventually
    /// belongs to whoever got there first. A player joining after that used to
    /// be granted nothing -- no ground, and so no block, since the block goes
    /// only on ground they own -- and placing is confined to your own
    /// territory, so they could never come to own anything.
    #[test]
    fn a_grant_on_ground_somebody_else_has_spread_over_still_works() {
        let mut world = World::toroidal_empty(12, 12);
        let first = PlayerId(1);

        // The first player's territory covers the whole world, as it does on
        // any torus that has been running.
        let (rows, cols) = world.size_in_cells().unwrap();
        for r in 0..rows {
            for c in 0..cols {
                world.set_cell_at(r, c, Cell::DEAD.with_player(first));
            }
        }

        let second = PlayerId(2);
        grant(&mut world, second);

        let (row, col) = spawn_for(second, &world);
        let mine = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == second)
            .count();
        assert_eq!(mine, (SPAWN_N * SPAWN_N) as usize, "the whole patch is theirs");

        let alive: Vec<(i32, i32)> = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| world.cell_at(r, c).unwrap().is_alive())
            .collect();
        assert_eq!(alive.len(), 4, "and a block stands on it: {alive:?}");
        assert!(
            alive.iter().all(|&(r, c)| world.cell_at(r, c).unwrap().player() == second),
            "the block is theirs"
        );

        // And they can actually place, which is the whole point.
        assert!(own_ground(&world, second, row, col));
    }

    /// A grant takes ground and never anybody's life or panes -- those are
    /// won by playing, not by arriving.
    #[test]
    fn a_grant_steps_around_life_and_ice() {
        let mut world = World::infinite_empty();
        let second = PlayerId(2);
        let (row, col) = spawn_for(second, &world);

        // Somebody else's living cell and pane, right in the middle where the
        // block wants to go.
        let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
        world.set_cell_at(middle.0, middle.1, Cell::alive(PlayerId(1)));
        world.set_cell_at(middle.0, middle.1 + 1, Cell::DEAD.with_ice(true).with_player(PlayerId(1)));

        grant(&mut world, second);

        let theirs = world.cell_at(middle.0, middle.1).unwrap();
        assert!(theirs.is_alive() && theirs.player() == PlayerId(1), "their life is untouched");
        let pane = world.cell_at(middle.0, middle.1 + 1).unwrap();
        assert!(pane.is_ice() && pane.player() == PlayerId(1), "and their pane");

        // The block went somewhere else in the patch rather than nowhere.
        let alive: Vec<(i32, i32)> = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == second
                && world.cell_at(r, c).unwrap().is_alive())
            .collect();
        assert_eq!(alive.len(), 4, "a whole block, not three cells that die: {alive:?}");
    }

    /// Both sides work it out independently -- the server on a join, the
    /// client for an offline game -- so it must not depend on iteration order.
    #[test]
    fn a_grant_lands_in_the_same_place_every_time() {
        let build = || {
            let mut world = World::toroidal_empty(8, 8);
            for r in 0..40 {
                world.set_cell_at(r, r, Cell::alive(PlayerId(1)));
            }
            grant(&mut world, PlayerId(3));
            world.live_cells()
        };
        let first = build();
        for _ in 0..8 {
            assert_eq!(build(), first);
        }
    }

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

    /// Neighbouring grants are a patch apart plus the gap, and the gap is in
    /// chunks — which is the unit "how far away is my neighbour" is a question
    /// about. Pinned because the spacing is the one number a player feels
    /// before they have done anything.
    #[test]
    fn neighbouring_grants_are_a_gap_apart() {
        let world = World::infinite_empty();
        // Two players side by side in the grid, so the difference is one pitch.
        let (row, col) = spawn_for(PlayerId(1), &world);
        let (next_row, next_col) = spawn_for(PlayerId(2), &world);
        assert_eq!(next_row, row, "consecutive numbers fill a row before a column");
        assert_eq!(next_col - col, SPAWN_PITCH);

        // What is between them is the gap: the pitch less the patch they each
        // stand on.
        assert_eq!(SPAWN_PITCH - SPAWN_N, SPAWN_GAP);
        assert_eq!(SPAWN_GAP, 3 * CHUNK_N as i32, "three chunks of no-man's-land");

        // And the row below is the same distance away, so the grid is square
        // rather than a corridor.
        let (down_row, _) = spawn_for(PlayerId(1 + SPAWN_ACROSS as u8), &world);
        assert_eq!(down_row - row, SPAWN_PITCH);
    }

    /// Ground nobody holds prices as empty, which is what `apply` writes into
    /// it. The two must agree or a client would be charged for one thing and
    /// given another.
    #[test]
    fn unheld_ground_prices_as_empty() {
        let world = World::infinite_empty();
        let far = vec![(100_000, 100_000)];
        assert!(world.cell_at(far[0].0, far[0].1).is_none());
        // Empty, so the cell is a change and is charged for; and outside, so
        // it is charged at the outside rate. A client cannot know what it does
        // not hold, and reading unheld ground as its own would predict a
        // cheaper price than the server charges.
        assert_eq!(
            value_delta(&world, &paint(far, Placement::Life)),
            -Placement::Life.cost() * OUTSIDE_MULTIPLIER
        );
    }
}
