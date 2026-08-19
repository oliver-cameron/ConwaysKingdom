//! The authoritative side.
//!
//! Holds the whole world, owns the tick, and assigns player numbers. Links
//! [`crate::sim`] and [`crate::net`] and nothing else — built with
//! `--no-default-features`, neither wgpu nor winit is compiled at all.
//!
//! No transport yet. [`Server::handle`] is where one would land: it takes a
//! decoded [`ClientMessage`] and returns the replies, so whatever carries the
//! bytes is somebody else's problem.

pub mod persist;
#[cfg(feature = "server")]
pub mod ws;

use std::collections::HashMap;
use std::path::Path;

use crate::net::{ChunkId, ClientMessage, ServerMessage, Stamped, Tick};
use crate::sim::{Player, PlayerId, World};

pub struct Server {
    world: World,
    players: HashMap<PlayerId, Player>,
    /// Chunks each player has asked to be kept informed about.
    subscriptions: HashMap<PlayerId, Vec<ChunkId>>,
    /// Actions received for a tick that has not been simulated yet.
    pending: Vec<Stamped>,
    tick: Tick,
}

impl Server {
    pub fn new(world: World) -> Self {
        Self {
            world,
            players: HashMap::new(),
            subscriptions: HashMap::new(),
            pending: Vec::new(),
            tick: 0,
        }
    }

    pub fn tick(&self) -> Tick {
        self.tick
    }

    pub fn world(&self) -> &World {
        &self.world
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
    fn next_player_id(&self) -> Option<PlayerId> {
        (1..=PlayerId::MAX)
            .map(PlayerId)
            .find(|id| !self.players.contains_key(id))
    }

    pub fn join(&mut self, name: impl Into<String>) -> Result<PlayerId, String> {
        let Some(id) = self.next_player_id() else {
            let name = name.into();
            log::warn!("refused \"{name}\": server full at {} players", PlayerId::MAX);
            return Err(format!("server full ({} players)", PlayerId::MAX));
        };
        let mut player = Player::new(id, name);
        player.last_seen = self.tick;
        log::info!(
            "join: {:?} \"{}\" at tick {} ({} online)",
            id,
            player.name,
            self.tick,
            self.players.len() + 1
        );
        self.players.insert(id, player);
        Ok(id)
    }

    /// Restore from a save, or start a fresh world if there is no file yet.
    /// Anything else -- a corrupt file, a mismatched cell width -- is an error
    /// rather than a silent reset, because silently discarding a world is the
    /// worst possible response to a bad read.
    pub fn load_or_new(path: &Path, fresh: impl FnOnce() -> World) -> std::io::Result<Self> {
        match persist::load(path) {
            Ok(snap) => {
                let mut s = Self::new(snap.world);
                s.tick = snap.tick;
                for p in snap.players {
                    s.players.insert(p.id, p);
                }
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::new(fresh())),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let players: Vec<_> = self.players.values().cloned().collect();
        persist::save(path, &self.world, &players, self.tick)
    }

    pub fn leave(&mut self, id: PlayerId) {
        if let Some(p) = self.players.remove(&id) {
            log::info!(
                "leave: {:?} \"{}\" after {} ticks ({} online)",
                id,
                p.name,
                self.tick.saturating_sub(p.last_seen),
                self.players.len()
            );
        }
        self.subscriptions.remove(&id);
    }

    /// Decoded message in, replies out. Deliberately transport-agnostic.
    pub fn handle(&mut self, from: Option<PlayerId>, msg: ClientMessage) -> Vec<ServerMessage> {
        if let Some(id) = from {
            let tick = self.tick;
            if let Some(p) = self.players.get_mut(&id) {
                p.last_seen = tick;
            }
        }
        match msg {
            ClientMessage::Join { name } => match self.join(name) {
                Ok(you) => vec![ServerMessage::Welcome { you, tick: self.tick }],
                Err(reason) => vec![ServerMessage::Rejected { reason }],
            },
            ClientMessage::Act(stamped) => {
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
                    "subscribe: {:?} asked for {} chunks, sending {} that hold life",
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
            ClientMessage::Checkpoint { tick, chunks } => {
                // Only meaningful for the tick the server is on; an older one
                // would need a history of past states to compare against.
                if tick != self.tick {
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
                if wrong.is_empty() {
                    Vec::new()
                } else {
                    log::warn!("desync: {:?} disagrees on {} chunks at tick {tick}", from, wrong.len());
                    vec![ServerMessage::Resync { tick, chunks: wrong }]
                }
            }
        }
    }

    fn chunk_message(&self, chunk: ChunkId) -> Option<ServerMessage> {
        self.world.chunk_at(chunk).map(|c| ServerMessage::ChunkData {
            tick: self.tick,
            chunk,
            cells: c.as_bytes().to_vec(),
        })
    }

    /// Apply everything queued for this tick, advance one generation, and hand
    /// back what every client needs to stay in step.
    pub fn step(&mut self) -> Vec<ServerMessage> {
        let applied = std::mem::take(&mut self.pending);
        for stamped in &applied {
            self.apply(stamped);
        }
        self.world.step();
        self.tick += 1;

        if applied.is_empty() {
            Vec::new()
        } else {
            vec![ServerMessage::Actions(applied)]
        }
    }

    fn apply(&mut self, stamped: &Stamped) {
        crate::net::apply(&mut self.world, stamped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Action;
    use crate::sim::CHUNK_N;

    #[test]
    fn player_numbers_start_at_one_and_are_reused() {
        let mut s = Server::new(World::infinite());
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        assert_eq!((a, b), (PlayerId(1), PlayerId(2)));
        assert!(a.is_owned(), "zero is reserved for unowned cells");
        s.leave(a);
        assert_eq!(s.join("c").unwrap(), PlayerId(1), "the gap is filled");
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
        // A blinker well away from the starting glider.
        s.handle(
            Some(me),
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                action: Action::Paint { cells: vec![(100, 100), (100, 101), (100, 102)] },
            }),
        );
        s.step();
        let live = s.world().live_cells();
        assert!(live.contains(&(99, 101)), "the blinker should have rotated");
        let owner = s
            .world()
            .chunk_at((99 / CHUNK_N as i32, 101 / CHUNK_N as i32))
            .map(|c| c[((99 % CHUNK_N as i32) as usize, (101 % CHUNK_N as i32) as usize)])
            .unwrap();
        assert_eq!(owner.player(), me, "live cells carry the painter's number");
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
                action: Action::Paint { cells: vec![(40, 40), (40, 41), (40, 42)] },
            }),
        );
        for _ in 0..25 {
            s.step();
        }
        s.save(&path).unwrap();

        let back = Server::load_or_new(&path, World::infinite).unwrap();
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
        assert!(Server::load_or_new(&missing, World::infinite).is_ok());

        let corrupt = dir.join("corrupt.ckw");
        std::fs::write(&corrupt, b"not a world file at all").unwrap();
        assert!(
            Server::load_or_new(&corrupt, World::infinite).is_err(),
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
        act(&mut s, Action::Paint { cells: vec![(0, 0), (0, 1), (1, 0), (1, 1)] });
        assert_eq!(s.value_of(me), Some(start - 4));

        // Reclaiming two of your own pays two back.
        act(&mut s, Action::Erase { cells: vec![(0, 0), (0, 1)] });
        assert_eq!(s.value_of(me), Some(start - 2));

        // Erasing empty space is neither earned nor spent.
        act(&mut s, Action::Erase { cells: vec![(90, 90)] });
        assert_eq!(s.value_of(me), Some(start - 2));
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
            action: Action::Paint { cells: vec![(50, 50), (50, 51), (51, 50), (51, 51)] },
        }));
        s.step();
        assert_eq!(s.world().cell_at(50, 50).map(|c| c.player()), Some(a));

        let before = s.value_of(b).unwrap();
        s.handle(Some(b), ClientMessage::Act(Stamped {
            tick: s.tick(), player: b, action: Action::Erase { cells: vec![(50, 50)] },
        }));
        s.step();
        assert_eq!(s.value_of(b), Some(before - 1), "taking ground is not free");
    }

    #[test]
    fn an_action_you_cannot_afford_is_refused() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        let purse = s.value_of(me).unwrap();
        let too_many: Vec<_> = (0..purse + 1).map(|i| (0, i)).collect();

        s.handle(Some(me), ClientMessage::Act(Stamped {
            tick: 0, player: me, action: Action::Paint { cells: too_many },
        }));
        s.step();
        assert_eq!(s.value_of(me), Some(purse), "nothing was spent");
        assert!(s.world().live_cells().is_empty(), "and nothing was placed");
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
        assert!(s
            .handle(Some(me), ClientMessage::Checkpoint { tick: 0, chunks: held.clone() })
            .is_empty());

        // One chunk wrong: only that one comes back.
        let mut bad = held.clone();
        bad[0].1 = !bad[0].1;
        let replies = s.handle(Some(me), ClientMessage::Checkpoint { tick: 0, chunks: bad });
        match replies.as_slice() {
            [ServerMessage::Resync { chunks, .. }] => {
                assert_eq!(chunks, &[held[0].0], "only the disagreeing chunk");
            }
            other => panic!("expected one Resync, got {other:?}"),
        }
    }
}
