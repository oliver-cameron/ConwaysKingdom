//! The authoritative side.
//!
//! One [`Server`] is one **room**: one world, one player table, one tick. A
//! process runs several of them side by side — see [`rooms`] — so "the server"
//! in the sense of the address you connect to is a [`rooms::Rooms`], and this
//! is one of the worlds behind it.
//!
//! Links [`crate::sim`] and [`crate::net`] and nothing else — built with
//! `--no-default-features`, neither wgpu nor winit is compiled at all.
//!
//! [`Server::handle`] takes a decoded [`ClientMessage`] and returns the
//! replies, so whatever carries the bytes is somebody else's problem;
//! [`ws`] is what carries them today.

pub mod console;
pub mod matches;
pub mod persist;
pub mod rooms;
#[cfg(feature = "server")]
pub mod ws;

use std::collections::HashMap;
use std::path::Path;

use crate::net::{ChunkId, ClientMessage, RoomName, ServerMessage, Stamped, Tick, DEFAULT_ROOM};
use crate::sim::{Player, PlayerId, World};
use matches::{Phase, Victory};

pub struct Server {
    /// Which room this is. Not stored in the save file: the file's name *is*
    /// the room's name, and two places to keep one fact is one too many.
    room: RoomName,
    world: World,
    players: HashMap<PlayerId, Player>,
    /// Chunks each player has asked to be kept informed about.
    subscriptions: HashMap<PlayerId, Vec<ChunkId>>,
    /// Actions received for a tick that has not been simulated yet.
    pending: Vec<Stamped>,
    /// Whether this room is a match, and what it is doing. [`Phase::Open`] is
    /// an ordinary room: steps forever, anybody may join, nobody wins.
    phase: Phase,
    /// How this match is won, once it is running. `None` on an open room.
    victory: Option<Victory>,
}

/// A secret nobody can guess, for a player to come back with.
///
/// `RandomState` is seeded by the operating system for every instance, which
/// is what hashing relies on to resist collision attacks — two of them give
/// 128 bits without a dependency. Strong enough for what this is: a claim
/// ticket for a game with no accounts, not a credential worth attacking.
fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let half = || RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", half(), half())
}

impl Server {
    /// A room called [`DEFAULT_ROOM`]. What a test or a single-world server
    /// wants; [`Server::named`] is what [`rooms::Rooms`] uses.
    pub fn new(world: World) -> Self {
        Self::named(DEFAULT_ROOM, world)
    }

    pub fn named(room: impl Into<RoomName>, world: World) -> Self {
        Self {
            room: room.into(),
            world,
            players: HashMap::new(),
            subscriptions: HashMap::new(),
            pending: Vec::new(),
            phase: Phase::Open,
            victory: None,
        }
    }

    /// Make this room a match, gathering and not yet stepping.
    pub fn make_match(&mut self, victory: Victory) {
        self.phase = Phase::Gathering;
        self.victory = Some(victory);
    }

    /// Start the clock. The tick it starts at is what the deadline is measured
    /// from, so a match that gathered for an hour still runs its full length.
    pub fn start_match(&mut self) -> Result<(), String> {
        match self.phase {
            Phase::Gathering => {
                self.phase = Phase::Running { from: self.tick() };
                Ok(())
            }
            Phase::Open => Err("that room is not a match".into()),
            Phase::Running { .. } => Err("that match is already running".into()),
            Phase::Over { .. } => Err("that match is over".into()),
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn victory(&self) -> Option<Victory> {
        self.victory
    }

    /// How much ground each player holds, by their number.
    ///
    /// One pass over what is held, the way `ice_cells` and the turret sweep
    /// are — the world keeps no running total, and a count that was kept up to
    /// date would have to be corrected by every rule that moves ownership.
    ///
    /// **Granted ground does not count.** `HOME` never decays, so a player
    /// whose life was wiped out in the first minute still holds their patch at
    /// the whistle; scoring it would be points for having turned up. The floor
    /// stays — they can still build on it — it simply does not win anything.
    pub fn territory(&self) -> [usize; PlayerId::COUNT] {
        let mut held = [0usize; PlayerId::COUNT];
        for (_, chunk) in self.world.stored() {
            for row in 0..crate::sim::CHUNK_N {
                for col in 0..crate::sim::CHUNK_N {
                    let cell = chunk[(row, col)];
                    if cell.is_home() {
                        continue;
                    }
                    if cell.player().is_owned() {
                        held[cell.player().0 as usize] += 1;
                    }
                }
            }
        }
        held
    }

    /// Has this match been decided, and by whom.
    ///
    /// Checked after a step rather than before, so the generation that met the
    /// condition is the one the score is read from.
    fn decide(&mut self) -> Option<&Phase> {
        let (Some(victory), Phase::Running { from }) = (self.victory, self.phase.clone()) else {
            return None;
        };
        let held = self.territory();
        let (winner, count) = matches::leader(&held);
        let done = match victory {
            Victory::Timer { generations } => self.tick().saturating_sub(from) >= generations,
            Victory::Territory { squares } => count >= squares,
        };
        if !done {
            return None;
        }
        self.phase = Phase::Over { winner, held: count, at: self.tick() };
        log::info!(
            "match \"{}\" is over at tick {}: {}",
            self.room,
            self.tick(),
            match winner {
                Some(id) => format!("{id:?} holds {count} squares"),
                None => "nobody held anything".into(),
            }
        );
        Some(&self.phase)
    }

    pub fn room(&self) -> &str {
        &self.room
    }

    /// The generation the world is on, which is the only tick there is.
    ///
    /// It used to be a counter of its own, incremented beside `World::step`.
    /// Two numbers that must agree is one too many: `load_or_new` restored the
    /// saved tick into the counter and left the world's generation at zero, so
    /// a server started from a save simulated with a different seed sequence
    /// from the one that produced the save — and a client, which adopts *this*
    /// number on `Welcome`, then disagreed with the server about the
    /// generation from its very first step. Every birth's owner is seeded from
    /// it, so the two drifted apart permanently and invisibly.
    pub fn tick(&self) -> Tick {
        self.world.generation
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// For tests that need to arrange a world the rules would not produce on
    /// their own. Not on the message path: everything a client can do goes
    /// through `handle`, which is what judges it.
    #[cfg(test)]
    fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn value_of(&self, id: PlayerId) -> Option<i32> {
        self.players.get(&id).map(|p| p.value)
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn players(&self) -> impl Iterator<Item = &Player> {
        self.players.values()
    }

    /// The lowest unused number. Zero is reserved for unowned cells, and the
    /// cell only has room for [`PlayerId::MAX`], so a full server refuses.
    /// The lowest number nobody has ever been given here.
    ///
    /// Never reused, even once its player has gone. A number is written into
    /// every cell they own, so reissuing one hands their territory to a
    /// stranger — and the ground stays after the connection does not. Thirty
    /// one numbers is therefore a limit on players a world has ever seen, not
    /// on players connected at once, which is what a bearer token is for: a
    /// returning player asks for the number they already have.
    fn next_player_id(&self) -> Option<PlayerId> {
        (1..=PlayerId::MAX)
            .map(PlayerId)
            .find(|id| !self.players.contains_key(id))
    }

    /// Let a player in, or let them back in.
    ///
    /// A token that matches a player already here is that player returning:
    /// they get their number, their value and their ground back, and their
    /// name is refreshed in case they changed it. Anything else is a new
    /// player, who gets a new number and a new secret.
    ///
    /// The grant runs either way. A returning player whose life was wiped out
    /// while they were away would otherwise come back with nowhere to stand,
    /// and re-marking ground they already hold costs nothing.
    pub fn join_with(
        &mut self,
        name: impl Into<String>,
        token: Option<&str>,
    ) -> Result<(PlayerId, String), String> {
        let name = name.into();
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            match self.players.values_mut().find(|p| p.token == token) {
                // Theirs, and they are not using it: this is them coming back.
                Some(p) if !p.online => {
                    p.name = name;
                    p.last_seen = 0;
                    p.online = true;
                    let (id, token) = (p.id, p.token.clone());
                    log::info!("rejoin: {id:?} \"{}\" came back", self.players[&id].name);
                    self.grant_territory(id);
                    return Ok((id, token));
                }
                // Theirs, and somebody is already playing as them. Nobody gets
                // to be two people at once, and nobody gets to be one person
                // twice: two clients on one machine share a token file, and
                // two tabs share a browser's storage, so without this the
                // second player to arrive simply becomes the first -- which is
                // not a multiplayer game, it is one player with two windows.
                Some(p) => log::info!(
                    "{:?} \"{}\" is already connected; joining as somebody new",
                    p.id,
                    p.name
                ),
                None => log::info!("a token nobody here holds; joining as somebody new"),
            }
        }
        let id = self.join(name)?;
        let token = new_token();
        self.players
            .get_mut(&id)
            .expect("just joined")
            .token
            .clone_from(&token);
        Ok((id, token))
    }

    pub fn join(&mut self, name: impl Into<String>) -> Result<PlayerId, String> {
        let Some(id) = self.next_player_id() else {
            let name = name.into();
            log::warn!("refused \"{name}\": server full at {} players", PlayerId::MAX);
            return Err(format!("server full ({} players)", PlayerId::MAX));
        };
        let mut player = Player::new(id, name);
        player.last_seen = self.tick();
        log::info!(
            "join: {:?} \"{}\" in room {} at tick {} ({} online)",
            id,
            player.name,
            self.room,
            self.tick(),
            self.players.len() + 1
        );
        self.players.insert(id, player);
        self.grant_territory(id);
        Ok(id)
    }

    /// Claim a player's starting ground, so they have somewhere to place.
    ///
    /// Granted again on a rejoin, deliberately: a player whose life was wiped
    /// out while they were away would otherwise come back with nowhere to
    /// stand, and re-marking ground they already hold costs nothing.
    fn grant_territory(&mut self, id: PlayerId) {
        crate::net::grant(&mut self.world, id);
        let (row, col) = crate::net::spawn_for(id, &self.world);
        log::info!("{id:?} granted ground at ({row}, {col})");
    }

    /// Restore from a save, or start a fresh world if there is no file yet.
    /// Anything else -- a corrupt file, a mismatched cell width -- is an error
    /// rather than a silent reset, because silently discarding a world is the
    /// worst possible response to a bad read.
    pub fn load_or_new(
        path: &Path,
        room: impl Into<RoomName>,
        fresh: impl FnOnce() -> World,
    ) -> std::io::Result<Self> {
        let room = room.into();
        match persist::load(path) {
            Ok(snap) => {
                let mut world = snap.world;
                // The tick *is* the generation, so restoring one restores both.
                world.set_generation(snap.tick);
                let mut s = Self::named(room, world);
                for p in snap.players {
                    s.players.insert(p.id, p);
                }
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::named(room, fresh())),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let players: Vec<_> = self.players.values().cloned().collect();
        persist::save(path, &self.world, &players, self.tick())
    }

    /// Mark a player gone, and keep them.
    ///
    /// Not removed: their number is their identity, since every cell they own
    /// carries it, so giving it to the next player to arrive would give away
    /// their territory with it. They come back to it with their token.
    pub fn leave(&mut self, id: PlayerId) {
        if let Some(p) = self.players.get_mut(&id) {
            p.online = false;
            let (name, since) = (p.name.clone(), p.last_seen);
            log::info!(
                "leave: {:?} \"{name}\" from room {} after {} ticks ({} still on)",
                id,
                self.room,
                self.tick().saturating_sub(since),
                self.players.values().filter(|p| p.online).count()
            );
        }
        self.subscriptions.remove(&id);
    }

    /// Decoded message in, replies out. Deliberately transport-agnostic.
    pub fn handle(&mut self, from: Option<PlayerId>, msg: ClientMessage) -> Vec<ServerMessage> {
        if let Some(id) = from {
            let tick = self.tick();
            if let Some(p) = self.players.get_mut(&id) {
                p.last_seen = tick;
            }
        }
        match msg {
            // The room was resolved before this message was routed here -- a
            // Server *is* one room, so by the time it arrives the question of
            // which world has been answered.
            // No late joining. A match is a race from a shared start, and
            // somebody arriving at generation four hundred is not in it:
            // everybody else has four hundred generations of ground and they
            // have a block. A player already seated here is unaffected — this
            // is the door, not the room.
            ClientMessage::Join { .. }
                if !self.phase.open_to_newcomers()
                    && !self.players.values().any(|p| {
                        matches!(&msg, ClientMessage::Join { token: Some(t), .. } if &p.token == t)
                    }) =>
            {
                vec![ServerMessage::Rejected {
                    reason: format!("\"{}\" is a match already under way", self.room),
                }]
            }
            ClientMessage::Join { name, token, room: _ } => match self.join_with(name, token.as_deref()) {
                Ok((you, token)) => {
                    let spawn = crate::net::spawn_for(you, &self.world);
                    let value = self.value_of(you).unwrap_or(Player::STARTING_VALUE);
                    vec![ServerMessage::Welcome {
                        you,
                        tick: self.tick(),
                        spawn,
                        token,
                        value,
                        room: self.room.clone(),
                        // Sent rather than left to be derived: nothing a client
                        // can see says whether the ground ends, so a client
                        // told nothing builds an infinite world and disagrees
                        // with a wrapping server about where everything is.
                        world: self.world.kind(),
                    }]
                }
                Err(reason) => vec![ServerMessage::Rejected { reason }],
            },
            ClientMessage::Act(stamped) => {
                // Judged here as well as refused in the client, because a
                // client that sends whatever it likes is the case this exists
                // for. Ice is not liftable, so an erase naming it is not an
                // action, whoever asks.
                if let crate::net::Action::Erase { placement, .. } = &stamped.action {
                    if !placement.can_be_taken() {
                        log::info!("refused {:?}: {placement:?} cannot be taken", stamped.player);
                        return Vec::new();
                    }
                }
                // Placing outside a player's own territory is no longer
                // refused, it is charged ten times over -- so there is nothing
                // to check here and `net::value_delta` below does the whole of
                // it, cell by cell, on the same terms the client priced it on.
                // Cost is charged now, against the world as it stands, rather
                // than when the action is applied at the tick boundary -- the
                // client priced it against the same state, so pricing it later
                // would let the two disagree.
                if let Some(player) = self.players.get(&stamped.player) {
                    let delta = crate::net::value_delta(&self.world, &stamped);
                    if player.value + delta < 0 {
                        log::info!(
                            "refused {:?}: costs {} with {} in hand",
                            stamped.player,
                            -delta,
                            player.value
                        );
                        return Vec::new();
                    }
                    if let Some(p) = self.players.get_mut(&stamped.player) {
                        p.value += delta;
                    }
                    self.pending.push(stamped);
                }
                Vec::new()
            }
            ClientMessage::Subscribe { chunks } => {
                let out: Vec<_> = chunks
                    .iter()
                    .filter_map(|&chunk| self.chunk_message(chunk))
                    .collect();
                log::info!(
                    "subscribe: {:?} asked for {} chunks, sending {} the world holds",
                    from,
                    chunks.len(),
                    out.len()
                );
                if let Some(id) = from {
                    self.subscriptions.entry(id).or_default().extend(chunks);
                }
                out
            }
            ClientMessage::Unsubscribe { chunks } => {
                if let Some(subs) = from.and_then(|id| self.subscriptions.get_mut(&id)) {
                    subs.retain(|c| !chunks.contains(c));
                }
                Vec::new()
            }
            // A room cannot list the rooms: it is one of them, and it knows
            // of no others. `Rooms::handle` answers this before it routes
            // anything here.
            ClientMessage::Rooms => Vec::new(),
            ClientMessage::Checkpoint { tick, chunks } => {
                // Only meaningful for the tick the server is on; an older one
                // would need a history of past states to compare against.
                if tick != self.tick() {
                    return Vec::new();
                }
                // Answer with the chunks that disagree, and with any the client
                // claims to hold that the server no longer does -- an emptied
                // chunk is dropped here but may still be on the client.
                let wrong: Vec<_> = chunks
                    .into_iter()
                    .filter(|&(coord, digest)| self.world.chunk_digest(coord) != Some(digest))
                    .map(|(coord, _)| coord)
                    .collect();

                // The purse rides along, because mining made value something a
                // client cannot predict on its own: earnings depend on births
                // anywhere in the world, and a client holds a viewport. It
                // would drift down for as long as it played, and never
                // correct. The machinery for "your copy is wrong, here is
                // mine" already exists and runs every few seconds, so value
                // uses it rather than growing a second one.
                let mut out = Vec::new();
                if let Some(value) = from.and_then(|id| self.value_of(id)) {
                    out.push(ServerMessage::Purse { value });
                }
                if !wrong.is_empty() {
                    log::warn!("desync: {:?} disagrees on {} chunks at tick {tick}", from, wrong.len());
                    out.push(ServerMessage::Resync { tick, chunks: wrong });
                }
                out
            }
        }
    }

    fn chunk_message(&self, chunk: ChunkId) -> Option<ServerMessage> {
        self.world.chunk_at(chunk).map(|c| ServerMessage::ChunkData {
            tick: self.tick(),
            chunk,
            cells: c.as_bytes().to_vec(),
        })
    }

    /// Apply everything queued for this tick, advance one generation, and hand
    /// back what every client needs to stay in step.
    pub fn step(&mut self) -> Vec<ServerMessage> {
        // A match that has not started yet, or is over, holds still. Actions
        // are still applied — gathering is when the opening is drawn, and a
        // pattern laid into a frozen world is exactly what ice already does
        // for one region, promoted here to the whole of it.
        //
        // The tick does not move either, which is what makes gathering fair:
        // somebody who joined a minute earlier has not had a minute of
        // generations the others did not.
        let applied = std::mem::take(&mut self.pending);
        for stamped in &applied {
            self.apply(stamped);
        }
        if !self.phase.stepping() {
            return if applied.is_empty() {
                Vec::new()
            } else {
                vec![ServerMessage::Step { tick: self.tick(), actions: applied }]
            };
        }
        let mined = self.world.step();

        // What the mines paid out. The world counted the births; the price is
        // here, and this is the only place a purse is authoritative.
        for player in self.players.values_mut() {
            // Floored at zero. A cost that comes from an action is refused
            // when it cannot be paid; a drain arrives whether or not there is
            // anything to take it from, and a player in debt would be a player
            // who cannot act and has no way to stop owing.
            player.value = (player.value + crate::net::earnings(&mined, player.id)).max(0);
        }

        self.decide();

        // Every generation, even an empty one: the tick is what keeps clients
        // in step, and a quiet generation still moves the world on.
        vec![ServerMessage::Step { tick: self.tick(), actions: applied }]
    }

    fn apply(&mut self, stamped: &Stamped) {
        crate::net::apply(&mut self.world, stamped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Only the tests take a chunk apart; the server passes them through whole.
    use crate::sim::{Cell, Chunk, CHUNK_N};

    /// Cells inside a player's granted ground. Placing anywhere else is
    /// refused now, so a test that wants a placement to land has to say where
    /// relative to the grant rather than picking a coordinate off the map.
    fn mine(id: PlayerId, offsets: &[(i32, i32)]) -> Vec<(i32, i32)> {
        // Every test here runs on an infinite world, whose grid of grants does
        // not depend on the world at all -- only a torus has to share out what
        // ground there is. So one is made here rather than threaded through
        // every call and fought with the borrow checker over.
        let (row, col) = crate::net::spawn_for(id, &World::infinite_empty());
        offsets.iter().map(|&(r, c)| (row + r, col + c)).collect()
    }
    use crate::net::{Action, Placement};

    #[test]
    /// Numbers start at one and are **never** reused, which is a change: they
    /// used to fill the gap a departing player left. A number is written into
    /// every cell that player owns, so handing it on hands over their
    /// territory, and the ground outlives the connection. Coming back is what
    /// the token is for.
    fn player_numbers_start_at_one_and_are_never_reused() {
        let mut s = Server::new(World::infinite());
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        assert_eq!((a, b), (PlayerId(1), PlayerId(2)));
        assert!(a.is_owned(), "zero is reserved for unowned cells");

        s.leave(a);
        assert_eq!(
            s.join("c").unwrap(),
            PlayerId(3),
            "a departed player's number is theirs still"
        );
        assert!(!s.players().find(|p| p.id == a).unwrap().online, "and they are marked gone");
    }

    #[test]
    fn the_server_is_full_at_the_cell_field_width() {
        let mut s = Server::new(World::infinite());
        for i in 1..=PlayerId::MAX {
            assert_eq!(s.join(format!("p{i}")).unwrap(), PlayerId(i));
        }
        assert!(s.join("one too many").is_err());
    }

    #[test]
    fn a_painted_cell_belongs_to_the_player_who_painted_it() {
        let mut s = Server::new(World::infinite());
        let me = s.join("me").unwrap();
        // A blinker in the middle of this player's own ground, which is the
        // only place they may put one.
        let cells = mine(me, &[(5, 4), (5, 5), (5, 6)]);
        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                action: Action::Paint { cells: cells.clone(), placement: Placement::Life },
            }),
        );
        s.step();
        let live = s.world().live_cells();
        let (row, col) = (cells[1].0 - 1, cells[1].1);
        assert!(live.contains(&(row, col)), "the blinker should have rotated");
        let owner = s.world().cell_at(row, col).unwrap();
        assert_eq!(owner.player(), me, "live cells carry the painter's number");
    }

    /// The tick and the world's generation are the same number, and a load
    /// has to restore it into the world rather than into a counter beside it.
    ///
    /// Pinned on its own because the failure is silent: every seed is derived
    /// from the generation, so a server restarted from a save rolls a
    /// different sequence from the one that made the save, and a client — which
    /// takes *this* number from `Welcome` — disagrees with the server from its
    /// first step. `a_world_survives_a_save_and_load` catches it eventually,
    /// as a divergence dozens of steps later that says nothing about why.
    #[test]
    fn a_loaded_world_is_on_the_tick_it_was_saved_at() {
        let dir = std::env::temp_dir().join("ck-tick-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("tick.ckw");

        let mut s = Server::new(World::infinite_empty());
        let me = s.join("alice").unwrap();
        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                action: Action::Paint {
                    cells: mine(me, &[(0, 0), (0, 1), (0, 2)]),
                    placement: Placement::Life,
                },
            }),
        );
        for _ in 0..7 {
            s.step();
        }
        assert_eq!(s.tick(), s.world().generation, "they are one number");
        s.save(&path).unwrap();

        let back = Server::load_or_new(&path, DEFAULT_ROOM, World::infinite_empty).unwrap();
        assert_eq!(back.tick(), 7);
        assert_eq!(
            back.world().generation,
            7,
            "the world must come back on the generation it was saved on, or \
             every seed derived from it differs from here on"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_world_survives_a_save_and_load() {
        let dir = std::env::temp_dir().join("ck-persist-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("world.ckw");

        let mut s = Server::new(World::infinite());
        let me = s.join("alice").unwrap();
        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                action: Action::Paint { cells: mine(me, &[(4, 4), (4, 5), (4, 6)]), placement: Placement::Life },
            }),
        );
        for _ in 0..25 {
            s.step();
        }
        s.save(&path).unwrap();

        let back = Server::load_or_new(&path, DEFAULT_ROOM, World::infinite).unwrap();
        assert_eq!(back.tick(), s.tick(), "tick is restored");
        assert_eq!(back.world().digest(), s.world().digest(), "world is restored");
        assert_eq!(back.world().live_cells(), s.world().live_cells());
        assert_eq!(back.player_count(), 1, "players are restored");
        assert_eq!(back.players().next().unwrap().name, "alice");

        // And it keeps stepping identically from there -- the whole point.
        let (mut a, mut b) = (s, back);
        for g in 0..50 {
            a.step();
            b.step();
            assert_eq!(a.world().digest(), b.world().digest(), "diverged at {g}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_starts_fresh_but_a_corrupt_one_does_not() {
        let dir = std::env::temp_dir().join("ck-persist-test");
        let _ = std::fs::create_dir_all(&dir);

        let missing = dir.join("does-not-exist.ckw");
        let _ = std::fs::remove_file(&missing);
        assert!(Server::load_or_new(&missing, DEFAULT_ROOM, World::infinite).is_ok());

        let corrupt = dir.join("corrupt.ckw");
        std::fs::write(&corrupt, b"not a world file at all").unwrap();
        assert!(
            Server::load_or_new(&corrupt, DEFAULT_ROOM, World::infinite).is_err(),
            "a bad file must not be silently replaced with an empty world"
        );
        let _ = std::fs::remove_file(&corrupt);
    }

    #[test]
    fn reclaiming_your_own_cells_pays_and_placing_costs() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        let start = s.value_of(me).unwrap();

        let act = |s: &mut Server, action| {
            s.handle(Some(me), ClientMessage::Act(Stamped { tick: s.tick(), player: me, action }));
            s.step();
        };

        // A 2x2 block: a still life, so it is still where it was put when the
        // next assertion looks. A blinker would have rotated out from under it.
        act(&mut s, Action::Paint { cells: mine(me, &[(0, 0), (0, 1), (1, 0), (1, 1)]), placement: Placement::Life });
        assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost()));

        // Reclaiming two of your own pays two back.
        // Reclaiming pays one each, well short of what they cost to place.
        act(&mut s, Action::Erase { cells: mine(me, &[(0, 0), (0, 1)]), placement: Placement::Life });
        assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost() + 2));

        // Erasing empty space is neither earned nor spent.
        act(&mut s, Action::Erase { cells: mine(me, &[(9, 9)]), placement: Placement::Life });
        assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost() + 2));
    }

    /// And it has to survive the server closing, which is the case it was
    /// failing.
    ///
    /// A player is not saved as online — the flag is not in the format — so
    /// one rebuilt from a file came back marked connected, because
    /// `Player::new` is what a player *joins* with. A player who is online
    /// cannot be returned to by their token, that being what stops two tabs
    /// becoming one player, so everybody who was in the room when it was
    /// written found their token refused on the next run and joined as
    /// somebody new, beside territory they could see and could not build on.
    #[test]
    fn a_token_survives_the_server_closing() {
        let path = std::env::temp_dir()
            .join(format!("ck-restart-{}.ckw", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut before = Server::named("arena", World::infinite_empty());
        let (me, token) = before.join_with("alice", None).unwrap();
        before.players.get_mut(&me).unwrap().value = 42;
        assert!(before.players[&me].online, "connected while playing");
        before.save(&path).unwrap();

        // A new process, reading the file the old one left.
        let mut after =
            Server::load_or_new(&path, "arena", World::infinite_empty).unwrap();
        assert!(!after.players[&me].online, "nobody is connected to a world off a disk");

        let (back, same) = after.join_with("alice", Some(&token)).unwrap();
        assert_eq!(back, me, "the token brings them back to their own number");
        assert_eq!(same, token, "and the same secret, so it goes on working");
        assert_eq!(after.players[&me].value, 42, "with what they had");

        let _ = std::fs::remove_file(&path);
    }

    /// Ground far from anybody's granted patch, so it counts towards a score.
    fn stake(s: &mut Server, id: PlayerId, at: (i32, i32), n: i32) {
        for r in at.0..at.0 + n {
            for c in at.1..at.1 + n {
                s.world.set_cell_at(r, c, Cell::DEAD.with_player(id));
            }
        }
    }

    /// A match runs its length and names whoever holds most.
    #[test]
    fn a_timer_match_ends_and_names_a_winner() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 5 });
        let (alice, _) = s.join_with("alice", None).unwrap();
        let (bob, _) = s.join_with("bob", None).unwrap();

        // Gathering holds still, which is what makes the opening drawn rather
        // than raced: nobody gains generations by arriving early.
        s.step();
        s.step();
        assert_eq!(s.tick(), 0, "a gathering match does not step");

        stake(&mut s, alice, (900, 900), 6);
        stake(&mut s, bob, (900, 940), 4);
        s.start_match().unwrap();

        for _ in 0..5 {
            s.step();
        }
        match s.phase() {
            Phase::Over { winner, held, at } => {
                assert_eq!(*winner, Some(alice), "alice staked more");
                assert!(*held >= 36, "she held {held}");
                assert_eq!(*at, 5, "decided at the generation the clock ran out");
            }
            other => panic!("should be over, not {other:?}"),
        }

        // And it stops: a decided match does not go on running.
        let stopped = s.tick();
        s.step();
        assert_eq!(s.tick(), stopped, "an over match holds still");
    }

    /// The other condition: first to a count rather than most at a whistle.
    #[test]
    fn a_territory_match_ends_when_somebody_reaches_the_count() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Territory { squares: 50 });
        let (alice, _) = s.join_with("alice", None).unwrap();
        s.start_match().unwrap();

        s.step();
        assert!(matches!(s.phase(), Phase::Running { .. }), "nobody holds fifty yet");

        stake(&mut s, alice, (900, 900), 8);
        s.step();
        match s.phase() {
            Phase::Over { winner, held, .. } => {
                assert_eq!(*winner, Some(alice));
                assert!(*held >= 50, "held {held}");
            }
            other => panic!("should be over, not {other:?}"),
        }
    }

    /// Granted ground never decays, so scoring it would be points for having
    /// turned up. The floor stays — they can still build on it — it simply
    /// does not win anything.
    #[test]
    fn granted_ground_does_not_count_towards_a_score() {
        let mut s = Server::named("arena", World::infinite_empty());
        let (alice, _) = s.join_with("alice", None).unwrap();
        assert_eq!(s.territory()[alice.0 as usize], 0, "a grant is not a score");

        stake(&mut s, alice, (900, 900), 3);
        assert_eq!(s.territory()[alice.0 as usize], 9, "ground won is");
    }

    /// **No late joining.** A match is a race from a shared start, and
    /// somebody arriving at generation four hundred is not in it. Somebody
    /// already seated is a different question: a refresh must still get them
    /// back to their own seat.
    #[test]
    fn a_running_match_takes_no_newcomers_but_takes_its_own_back() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 1000 });
        let (alice, token) = s.join_with("alice", None).unwrap();
        s.start_match().unwrap();

        let refused = s.handle(
            None,
            ClientMessage::Join { name: "late".into(), token: None, room: None },
        );
        assert!(
            matches!(refused.as_slice(), [ServerMessage::Rejected { reason }] if reason.contains("already under way")),
            "{refused:?}"
        );

        s.leave(alice);
        let back = s.handle(
            None,
            ClientMessage::Join { name: "alice".into(), token: Some(token), room: None },
        );
        assert!(
            matches!(back.first(), Some(ServerMessage::Welcome { you, .. }) if *you == alice),
            "a player already in the match comes back: {back:?}"
        );
    }

    /// The whole point of the token: a player who drops comes back to their
    /// own number, their own value and their own ground, rather than to a
    /// fresh number beside a patch they can see and cannot build on.
    #[test]
    fn a_token_brings_a_player_back_to_themselves() {
        let mut s = Server::new(World::infinite_empty());
        let (me, token) = s.join_with("alice", None).unwrap();

        // Spend some, so there is state worth coming back to.
        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                action: Action::Paint { cells: mine(me, &[(0, 0), (0, 1)]), placement: Placement::Life },
            }),
        );
        s.step();
        let spent = s.value_of(me).unwrap();
        assert!(spent < Player::STARTING_VALUE, "something should have been spent");

        s.leave(me);

        // Coming back the way a client does, so the welcome itself is what is
        // checked: it has to carry the number, the secret *and* the value, or
        // the client returns believing it has the starting figure and offers
        // to spend money the server knows is gone.
        let welcome = s.handle(
            None,
            ClientMessage::Join { name: "alice".into(), token: Some(token.clone()), room: None },
        );
        match welcome.as_slice() {
            [ServerMessage::Welcome { you, token: back, value, .. }] => {
                assert_eq!(*you, me, "the same number");
                assert_eq!(*back, token, "and the same secret, so it keeps working");
                assert_eq!(*value, spent, "and the value they had");
            }
            other => panic!("expected a welcome, got {other:?}"),
        }
        assert_eq!(s.value_of(me), Some(spent));
    }

    /// Another player's territory has to reach you, or you cannot see whose
    /// ground you are standing next to — and, worse, your own does not reach
    /// you either: `own_ground` reads the owner off the cell, so a client that
    /// never receives the chunk refuses to build on ground that is its own.
    ///
    /// The case that nearly slipped through is a chunk holding *only*
    /// territory. Chunks are sent when the world holds them, and it holds
    /// anything not empty — which counts ownership now. A filter on liveness
    /// would have dropped exactly the chunks this is about.
    #[test]
    fn territory_reaches_the_clients_that_ask_for_it() {
        let mut s = Server::new(World::infinite_empty());
        let (alice, _) = s.join_with("alice", None).unwrap();
        let (bob, _) = s.join_with("bob", None).unwrap();

        let (row, col) = crate::net::spawn_for(alice, s.world());
        let chunk = (row.div_euclid(CHUNK_N as i32), col.div_euclid(CHUNK_N as i32));

        let sent = s.handle(Some(bob), ClientMessage::Subscribe { chunks: vec![chunk] });
        let [ServerMessage::ChunkData { cells, .. }] = sent.as_slice() else {
            panic!("bob should have been sent alice's chunk, got {sent:?}");
        };
        let cells: &Chunk = bytemuck::from_bytes(cells);
        let hers = (0..CHUNK_N)
            .flat_map(|r| (0..CHUNK_N).map(move |c| (r, c)))
            .filter(|&(r, c)| cells[(r, c)].player() == alice)
            .count();
        assert!(hers > 0, "alice's ground should be in what bob was sent");

        // And once her life has gone, the ground still is: a chunk of bare
        // territory is exactly what a returning player needs to be able to
        // build on, and it has no life to be sent for.
        for r in 0..CHUNK_N as i32 {
            for c in 0..CHUNK_N as i32 {
                let at = (chunk.0 * CHUNK_N as i32 + r, chunk.1 * CHUNK_N as i32 + c);
                let cell = s.world().cell_at(at.0, at.1).unwrap();
                s.world_mut().set_cell_at(at.0, at.1, cell.with_alive(false));
            }
        }
        let sent = s.handle(Some(bob), ClientMessage::Subscribe { chunks: vec![chunk] });
        assert!(
            matches!(sent.as_slice(), [ServerMessage::ChunkData { .. }]),
            "bare territory must still be sent, got {sent:?}"
        );
    }

    /// Nobody may be two people at once, and nobody may be one person twice.
    ///
    /// Two clients on one machine share a token file and two tabs share a
    /// browser's storage, so a token already in use has to mean a new player.
    /// Without this the second to arrive simply becomes the first, which is
    /// not a multiplayer game — it is one player with two windows.
    #[test]
    fn a_token_already_in_use_joins_as_somebody_new() {
        let mut s = Server::new(World::infinite_empty());
        let (alice, token) = s.join_with("alice", None).unwrap();

        let (bob, other) = s.join_with("bob", Some(&token)).unwrap();
        assert_ne!(bob, alice, "alice is still playing as alice");
        assert_ne!(other, token, "and bob gets a secret of his own");

        // Once she has gone, her own token brings her back.
        s.leave(alice);
        let (back, _) = s.join_with("alice", Some(&token)).unwrap();
        assert_eq!(back, alice);
    }

    /// A token nobody holds is not an error, it is a new player. Anything else
    /// would lock somebody out over a stale file.
    #[test]
    fn an_unknown_token_joins_as_somebody_new() {
        let mut s = Server::new(World::infinite_empty());
        let (first, token) = s.join_with("alice", None).unwrap();
        let (second, other) = s.join_with("bob", Some("not a token anybody has")).unwrap();
        assert_ne!(first, second);
        assert_ne!(token, other, "and gets a secret of its own");
    }

    /// Two players must never be handed the same secret, or either could
    /// claim the other.
    #[test]
    fn tokens_are_not_shared() {
        let mut s = Server::new(World::infinite_empty());
        let mut seen = std::collections::HashSet::new();
        for i in 0..8 {
            let (_, token) = s.join_with(format!("p{i}"), None).unwrap();
            assert_eq!(token.len(), 32, "128 bits, written as hex");
            assert!(seen.insert(token), "a token was handed out twice");
        }
    }

    /// Ice cannot be taken back, and the server is where that is decided. The
    /// client refuses it too, but a client that sends whatever it likes is the
    /// case this exists for — and a pane liftable by asking twice would be no
    /// pane at all.
    #[test]
    fn the_server_refuses_to_lift_ice() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        let pane = mine(me, &[(0, 0), (0, 1), (0, 2)]);

        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: me,
                action: Action::Paint { cells: pane.clone(), placement: Placement::Ice },
            }),
        );
        s.step();
        let (row, col) = pane[0];
        assert!(s.world().cell_at(row, col).unwrap().is_ice());
        let spent = s.value_of(me);

        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: me,
                action: Action::Erase { cells: pane, placement: Placement::Ice },
            }),
        );
        s.step();
        assert!(
            s.world().cell_at(row, col).unwrap().is_ice(),
            "the pane should still be there"
        );
        assert_eq!(s.value_of(me), spent, "and nothing should have been paid for it");
    }

    #[test]
    fn destroying_another_players_cell_costs() {
        let mut s = Server::new(World::infinite_empty());
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        // A block again, so a's cell survives long enough for b to attack it.
        s.handle(Some(a), ClientMessage::Act(Stamped {
            tick: 0,
            player: a,
            action: Action::Paint { cells: mine(a, &[(0, 0), (0, 1), (1, 0), (1, 1)]), placement: Placement::Life },
        }));
        s.step();
        let (row, col) = mine(a, &[(0, 0)])[0];
        assert_eq!(s.world().cell_at(row, col).map(|c| c.player()), Some(a));

        let before = s.value_of(b).unwrap();
        s.handle(Some(b), ClientMessage::Act(Stamped {
            tick: s.tick(), player: b, action: Action::Erase { cells: mine(a, &[(0, 0)]), placement: Placement::Life },
        }));
        s.step();
        assert_eq!(s.value_of(b), Some(before - 1), "taking ground is not free");
    }

    #[test]
    fn an_action_you_cannot_afford_is_refused() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        let purse = s.value_of(me).unwrap();
        let granted = s.world().live_cells();
        assert_eq!(granted.len(), 4, "the grant is a block, and only that");

        // Inside their own ground, so it is affordability being tested and
        // not the territory rule -- and skipping the block they already own,
        // since painting over what is already there is free and so would not
        // count towards the bill.
        let n = crate::net::SPAWN_N;
        let (row, col) = crate::net::spawn_for(me, &World::infinite_empty());
        let block = n / 2 - 1;
        let too_many: Vec<_> = (0..n)
            .flat_map(|r| (0..n).map(move |c| (r, c)))
            .filter(|&(r, c)| !((block..block + 2).contains(&r) && (block..block + 2).contains(&c)))
            .take((purse / Placement::Life.cost() + 1) as usize)
            .map(|(r, c)| (row + r, col + c))
            .collect();

        s.handle(Some(me), ClientMessage::Act(Stamped {
            tick: 0, player: me, action: Action::Paint { cells: too_many, placement: Placement::Life },
        }));
        s.step();
        assert_eq!(s.value_of(me), Some(purse), "nothing was spent");
        assert_eq!(s.world().live_cells(), granted, "and nothing was placed");
    }

    #[test]
    fn a_matching_digest_asks_for_no_resync() {
        let mut s = Server::new(World::infinite());
        let me = s.join("me").unwrap();
        let held: Vec<_> = s
            .world()
            .stored()
            .iter()
            .map(|&(coord, _)| (coord, s.world().chunk_digest(coord).unwrap()))
            .collect();

        // Agreement is silence about chunks -- but never silence, because the
        // purse rides on every checkpoint now that value is not a thing a
        // client can work out for itself.
        let replies =
            s.handle(Some(me), ClientMessage::Checkpoint { tick: 0, chunks: held.clone() });
        assert!(
            !replies.iter().any(|m| matches!(m, ServerMessage::Resync { .. })),
            "matching digests asked for a resync: {replies:?}"
        );
        assert!(
            replies.iter().any(|m| matches!(m, ServerMessage::Purse { .. })),
            "and the purse should come back with it: {replies:?}"
        );

        // One chunk wrong: only that one comes back.
        let mut bad = held.clone();
        bad[0].1 = !bad[0].1;
        let replies = s.handle(Some(me), ClientMessage::Checkpoint { tick: 0, chunks: bad });
        let resyncs: Vec<_> = replies
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Resync { chunks, .. } => Some(chunks.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(resyncs, vec![vec![held[0].0]], "only the disagreeing chunk");
    }

    /// A compact machine pays. Three cells and two corpses is the cheapest
    /// thing that keeps giving birth, and it is meant to be worth building.
    #[test]
    fn a_blinker_of_mines_pays_because_it_is_compact() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();

        // Three in a row, which flips end over end forever. Clear of the
        // grant's own block, which sits in the middle of the patch.
        place_mines(&mut s, me, &[(1, 1), (2, 1), (3, 1)]);
        s.step();
        // Measured from after the cost, which `handle` charges on receipt.
        let purse = s.value_of(me).unwrap();

        for _ in 0..20 {
            s.step();
        }
        assert!(
            s.value_of(me).unwrap() > purse,
            "two births a generation against two corpses charged one time in \
             eight should pay: {purse} -> {}",
            s.value_of(me).unwrap()
        );
    }

    /// And a mess does not pay, which is the point of charging for corpses.
    ///
    /// An r-pentomino of mines grows into a couple of hundred live cells
    /// dragging eight hundred corpses behind it. Every one of those is charged
    /// one generation in eight, so sprawl costs far more than its own births
    /// bring in — measured at about twenty a generation against it. Without
    /// the upkeep it was the best investment in the game.
    #[test]
    fn sprawling_mines_cost_more_than_they_earn() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        place_mines(&mut s, me, &[(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)]);
        s.step();

        // Given plenty to spend, so the floor at zero does not hide the drain.
        s.players.get_mut(&me).unwrap().value = 100_000;
        let purse = s.value_of(me).unwrap();
        for _ in 0..300 {
            s.step();
        }
        assert!(
            s.value_of(me).unwrap() < purse,
            "sprawl should bleed: {purse} -> {}",
            s.value_of(me).unwrap()
        );
    }

    /// Nothing dies on a still life, so nothing is charged. A block of mines
    /// is free to hold and earns nothing, which is the honest answer for
    /// something that never does anything.
    #[test]
    fn a_block_of_mines_costs_nothing_to_hold() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        place_mines(&mut s, me, &[(1, 1), (1, 2), (2, 1), (2, 2)]);
        s.step();
        let purse = s.value_of(me).unwrap();
        for _ in 0..50 {
            s.step();
        }
        assert_eq!(s.value_of(me).unwrap(), purse, "no births and no corpses");
    }

    /// Lay mines at offsets inside this player's granted ground, and apply
    /// them, without advancing the world.
    fn place_mines(s: &mut Server, id: PlayerId, offsets: &[(i32, i32)]) {
        let tick = s.tick();
        s.handle(
            Some(id),
            ClientMessage::Act(Stamped {
                tick,
                player: id,
                action: Action::Paint { cells: mine(id, offsets), placement: Placement::Mine },
            }),
        );
        // `handle` queues; `step` is what applies. Stepping once here would
        // also advance the world, so the pending action is drained by the
        // caller's own first step.
    }
}
