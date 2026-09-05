//! Where a player is seated, and what they are granted when they get there.
//!
//! Placement geometry rather than placement policy: which square of the world
//! a player starts on, how far apart two starts are, and the patch of ground
//! and the block that make an opening move possible. It is `net`'s because
//! both sides work it out and both have to agree, cell for cell — see
//! [docs/game.md#where-you-may-build].

use super::{world_seed, ChunkId, RoomId};
use crate::sim::{Cell, PlayerId, World};

/// How wide a patch of ground a player is granted when they join, in cells.
///
/// A player may only place where their own influence reaches, so somebody who
/// owned nothing could do nothing at all. The grant is what makes that wall
/// safe: a patch that never decays, with a live gradient around it, so there
/// is always somewhere to build. It is also the seed the rest spreads from.
pub const SPAWN_N: i32 = 12;
/// How many grants sit along one edge of a **torus's** grid. Six across is
/// thirty-six seats, comfortably over the fifteen a four-bit owner field holds.
///
/// Only the torus needs a fixed figure. Its ground is finite and has to be
/// divided whatever the roster turns out to be, so the grid is sized for the
/// worst case and spread over what there is. An infinite world has room, and
/// sizes its grid to the players who actually turned up — see [`seat`].
pub(super) const SPAWN_ACROSS: i32 = 6;

/// How much of somebody else's ground in a seat makes it not worth taking.
///
/// A bar rather than "any at all": a few stray cells cost nothing, and what
/// this looks for is a *country* — a seat inside somebody's territory, where
/// a new player could not build.
pub(super) const SPAWN_CROWDED: usize = (SPAWN_N * SPAWN_N / 4) as usize;

/// How many seats out from their own a player will look for somewhere emptier.
///
/// A bound rather than a sweep of the world: what is wanted is *near enough to
/// be in the same game and far enough to be nobody's*, and a search that ran
/// until it found perfect emptiness would put a latecomer half a map away from
/// everybody on any world that has been running a while.
pub(super) const SPAWN_SEARCH: i32 = 64;

/// Which seat a player takes, as a square **spiral** out from the origin.
///
/// A spiral fills a square at every size, so however many turn up everybody
/// has neighbours on more than one side — a fixed grid filled in reading
/// order puts the first six players in a line, and a line is a corridor.
/// A function of the player's number alone, so a seat never moves.
pub(super) fn seat(n: i32) -> (i32, i32) {
    let (mut row, mut col) = (0, 0);
    let (mut dr, mut dc) = (0, 1);
    let mut left = n.max(0);
    let mut run = 1;
    loop {
        // Two sides of the square per turn of the run length: right one, up
        // one, left two, down two, right three ... which is what makes the
        // path close each ring before starting the next.
        for _ in 0..2 {
            for _ in 0..run {
                if left == 0 {
                    return (row, col);
                }
                row += dr;
                col += dc;
                left -= 1;
            }
            (dr, dc) = (dc, -dr);
        }
        run += 1;
    }
}

/// The top-left of the patch a seat stands on.
pub(super) fn patch_at(seat: (i32, i32)) -> (i32, i32) {
    (seat.0 * SPAWN_PITCH, seat.1 * SPAWN_PITCH)
}

/// Whether this patch is one this player has already been granted.
///
/// [`Cell::is_home`] never decays, so a patch handed out stays marked as long
/// as its owner holds it — which is what keeps a seat still. `grant` runs
/// again on every rejoin, and a spawn that moved would hand a returning player
/// a second patch somewhere else every time the world around their first one
/// changed.
pub(super) fn already_granted(world: &World, (row, col): (i32, i32), player: PlayerId) -> bool {
    (row..row + SPAWN_N).any(|r| {
        (col..col + SPAWN_N).any(|c| {
            world.cell_at(r, c).is_some_and(|cell| cell.is_home() && cell.player() == player)
        })
    })
}

/// How much of a patch, and the ground just around it, is somebody else's.
///
/// The margin is there because a seat whose own squares are free but which
/// backs onto a neighbour's border is not somewhere to start. Counted rather
/// than tested, so "emptier" can be compared when nowhere is empty.
/// A teammate's ground is not a crowd and needs no rule saying so: a side is
/// one player, so a patch the side already holds *is* this player's.
pub(super) fn crowding(world: &World, (row, col): (i32, i32), player: PlayerId) -> usize {
    let margin = SPAWN_N / 2;
    let mut taken = 0;
    for r in row - margin..row + SPAWN_N + margin {
        for c in col - margin..col + SPAWN_N + margin {
            if world
                .cell_at(r, c)
                .is_some_and(|cell| cell.player().is_owned() && cell.player() != player)
            {
                taken += 1;
            }
        }
    }
    taken
}

/// No-man's-land between one grant's edge and the next, in cells.
///
/// **In cells, and it used to be in chunks.** The reasoning for chunks was
/// that they are the unit the world is drawn in and "how far away is my
/// neighbour" is a question about the map. The number underneath it was never
/// about chunks at all: forty-eight cells is far enough that neither player
/// can see the other's opening and near enough that a glider crosses in a
/// hundred generations, and both of those are distances in *cells*.
///
/// It mattered the day a chunk went from sixteen cells a side to sixty-four,
/// which quadrupled the gap without anybody deciding to — four times the
/// no-man's-land, four times the glider's journey, and a small torus that
/// could no longer seat people at the spacing it wanted.
pub(super) const SPAWN_GAP: i32 = 48;

/// Centre to centre between neighbouring grants, in cells: a patch, plus the
/// ground between it and the next one.
pub(super) const SPAWN_PITCH: i32 = SPAWN_N + SPAWN_GAP;

/// The ground a player is granted on joining: a square of claimed but empty
/// cells, far enough from everyone else's to be their own.
///
/// One seat per number, and a side is a number, so this takes a [`PlayerId`]
/// and nothing else. Laid out in a square rather than a line, so territory
/// meeting territory is something that happens.
///
/// The world decides the spacing: an infinite one has room and uses a fixed
/// pitch centred on the origin; a torus is finite and is shared out, and
/// **every number still gets its own square**. Computed rather than
/// searched for, so the answer never depends on what a peer happens to hold
/// — but it depends on the world's shape, which is why `Welcome` carries
/// the spawn rather than leaving a client to work it out and be wrong.
pub fn spawn_for(player: PlayerId, world: &World) -> (i32, i32) {
    let n = player.0 as i32;

    match world.size_in_cells() {
        None => {
            // Their own seat if it is still theirs, wherever the search below
            // put it last time -- a granted patch keeps its `HOME` marks, and
            // a spawn that moved would hand a returning player a second patch.
            let seats = || (0..SPAWN_SEARCH).map(|k| patch_at(seat(n - 1 + k)));
            if let Some(ours) = seats().find(|&at| already_granted(world, at, player)) {
                return ours;
            }

            // Otherwise the nearest seat that is nobody's, and failing that
            // the emptiest within reach. A latecomer whose seat is in the
            // middle of somebody's country is put out where there is room,
            // rather than inside a country they cannot build in.
            let mut best = patch_at(seat(n - 1));
            let mut fewest = usize::MAX;
            for at in seats() {
                let crowd = crowding(world, at, player);
                if crowd <= SPAWN_CROWDED {
                    return at;
                }
                if crowd < fewest {
                    (fewest, best) = (crowd, at);
                }
            }
            best
        }
        Some((height, width)) => {
            let (down, along) = torus_grid(height, width);
            // From nought, so a torus and a plane agree that the first number
            // sits in the corner they both start from. It used to index from
            // `n`, leaving seat nought empty and every player one place along
            // from where the infinite branch put them.
            let (row, col) = ((n - 1) / along, (n - 1) % along);
            let pitch = |extent: i32, across: i32| extent / across;
            (row * pitch(height, down), col * pitch(width, along))
        }
    }
}

/// How many seats across and down a torus is divided into.
///
/// **Sized by the roster first and the world second**, which is a fix rather
/// than a preference: sized by comfortable pitches and folded with `%`, a
/// 128x128 world had four seats for fifteen numbers and players 1, 5, 9 and
/// 13 stood on one patch, each claiming the last one's ground.
///
/// So: the smallest grid that holds every number, never finer than the world
/// has whole patches for. [`too_cramped_for_grants`] is what says a world
/// cannot do even that.
pub(super) fn torus_grid(height: i32, width: i32) -> (i32, i32) {
    // How many whole patches fit at all: the hard limit, since two patches
    // that overlap are not two patches.
    let room = |extent: i32| (extent / SPAWN_N).max(1);
    // And what the world would like, given room to spare.
    let want = |extent: i32| (extent / SPAWN_PITCH).clamp(1, SPAWN_ACROSS);
    let (mut down, mut along) = (want(height), want(width));
    // Widen until it holds every number, the shorter side first so the grid
    // stays near square. At most a few dozen turns: both are bounded by
    // `room`, and `room` is bounded by the world.
    while down * along < PlayerId::MAX as i32 {
        if along <= down && along < room(width) {
            along += 1;
        } else if down < room(height) {
            down += 1;
        } else if along < room(width) {
            along += 1;
        } else {
            break;
        }
    }
    (down, along)
}

/// Whether a world is too small to give every number a square of its own.
///
/// Asked of the grid rather than of a fixed size, so it answers the question
/// it is named for. It used to test for two pitches each way — which is a
/// four-seat grid, declared roomy enough for fifteen players.
pub fn too_cramped_for_grants(world: &World) -> bool {
    world.size_in_cells().is_some_and(|(h, w)| {
        let (down, along) = torus_grid(h, w);
        down * along < PlayerId::MAX as i32
    })
}

/// The world a server just said this client is in, or a boundless one.
///
/// **A client trusts its server about the shape and not about the size.** A
/// torus is allocated whole, so a server saying `100000x100000` — by malice
/// or an older build — took the browser tab with it. Falling back means the
/// client plays on and disagrees, which the checkpoint says out loud.
pub fn sane_world(kind: crate::sim::WorldKind, room: &RoomId) -> World {
    let mut world = match kind.checked() {
        Ok(kind) => kind.build(),
        Err(why) => {
            log::error!("the server named a world this client will not build ({why})");
            World::infinite_empty()
        }
    };
    // Taken from the room rather than left to the caller, because forgetting
    // it is a client that rolls different dice from its server and finds out
    // at the first contested birth -- which looks like a desync and is not one.
    world.set_seed(world_seed(room));
    world
}

/// Every chunk a grant at this position touches, folded onto the chunks the
/// world actually has.
///
/// A patch is [`SPAWN_N`] cells and a chunk is [`crate::sim::CHUNK_N`], so a grant spans
/// one chunk at best and four at worst — and on a torus it may span four that are
/// nowhere near each other, which is why the folding is not optional.
pub fn grant_chunks(world: &World, (row, col): (i32, i32)) -> Vec<ChunkId> {
    let mut out: Vec<ChunkId> =
        World::chunks_covering((row, col), (row + SPAWN_N - 1, col + SPAWN_N - 1))
            .into_iter()
            .map(|c| world.canonical(c))
            .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Claim a player's starting ground, with a block standing on it.
///
/// Here rather than on the server because an offline client needs the same
/// grant: placing is confined to territory, so a player who owns nothing has
/// no opening move. The block is a 2x2 still life — the same one for
/// everybody, and it holds its shape for ever, so it costs nothing to leave
/// alone while you decide what to build.
pub fn grant(world: &mut World, player: PlayerId) {
    let (row, col) = spawn_for(player, world);

    // **Once each.** A returning player is granted again by `join_with`, and
    // without this that is a fresh patch and a fresh block on top of whatever
    // they had built — a still life conjured out of nothing, over and over.
    //
    // Asked of the world rather than remembered on the player: `HOME` sits on
    // the *square*, survives the ground changing hands, and is in the save.
    // It is also what makes a side share one platform, since the second ally
    // to arrive finds the ground already theirs.
    if already_granted(world, (row, col), player) {
        log::debug!("{player:?} is already granted at ({row}, {col}); not granting again");
        return;
    }

    // **Dead ground is claimed whoever it belonged to.** Claiming only unheld
    // ground costs a player the game: territory only ever spreads, so on a
    // world with an edge it covers everything, and a player joining after that
    // got a patch of nothing — no ground, so no block, so nothing they could
    // ever place. On a torus that is the second player to arrive.
    //
    // Living cells and panes are still untouched.
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
            // Keeping whatever `HOME` the square already had. `Cell::alive`
            // builds a cell from nothing, and the mark lives in the owner
            // byte's spare bits, so laying the block over the patch was
            // rubbing out the mark under its own four squares -- which left
            // the middle of a granted patch decaying like ordinary ground once
            // the block died, and the ring around it permanent. The mark is on
            // the **square**, not on what is standing there.
            let was = world.cell_at(r0 + dr, c0 + dc).unwrap_or(Cell::DEAD);
            world.set_cell_at(r0 + dr, c0 + dc, Cell::alive(player).with_home(was.is_home()));
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
pub(super) fn block_site(
    world: &World,
    player: PlayerId,
    row: i32,
    col: i32,
) -> Option<(i32, i32)> {
    let middle = (row + SPAWN_N / 2 - 1, col + SPAWN_N / 2 - 1);
    let free = |r: i32, c: i32| {
        world
            .cell_at(r, c)
            .is_some_and(|cell| cell.player() == player && !cell.is_alive() && !cell.is_ice())
    };
    let fits =
        |r: i32, c: i32| free(r, c) && free(r, c + 1) && free(r + 1, c) && free(r + 1, c + 1);

    let mut sites: Vec<(i32, i32)> = (row..row + SPAWN_N - 1)
        .flat_map(|r| (col..col + SPAWN_N - 1).map(move |c| (r, c)))
        .filter(|&(r, c)| fits(r, c))
        .collect();
    // Sorted by distance from the middle, and by coordinate to break ties, so
    // the answer never depends on iteration order -- the client works this out
    // for an offline game and must reach the same one.
    sites.sort_unstable_by_key(|&(r, c)| ((r - middle.0).abs() + (c - middle.1).abs(), r, c));
    sites.first().copied()
}
