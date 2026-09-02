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
pub mod people;
pub mod persist;
pub mod profiles;
pub mod rating;
pub mod rooms;
#[cfg(feature = "server")]
pub mod ws;

use std::collections::HashMap;
use std::path::Path;

use crate::net::{
    ChunkId, ClientMessage, RoomName, Rules, ServerMessage, Stamped, Tick, DEFAULT_ROOM,
};
use crate::sim::{Player, PlayerId, World};
use matches::{Phase, Victory};

pub struct Server {
    /// Which room this is. Not stored in the save file: the file's name *is*
    /// the room's name, and two places to keep one fact is one too many.
    room: RoomName,
    world: World,
    players: HashMap<PlayerId, Player>,
    /// Actions received for a tick that has not been simulated yet.
    pending: Vec<Stamped>,
    /// Whether this room is a match, and what it is doing. [`Phase::Open`] is
    /// an ordinary room: steps forever, anybody may join, nobody wins.
    phase: Phase,
    /// How this match is won, once it is running. `None` on an open room.
    victory: Option<Victory>,

    /// The sides this match has, in the order they are numbered, as the
    /// [`PlayerId`] each one **is**.
    ///
    /// Empty in a free-for-all. A side has a row in `players` like anybody
    /// else — that is what makes it a player — and this is the only thing that
    /// separates the two: a row named here is a side, and every other row is
    /// a seat somebody sits in. Side rows are never `online` and never carry a
    /// person or a token, so every listing that filters on those already
    /// leaves them out.
    ///
    /// What this replaces is a `Sides` array copied onto the wire and an
    /// `allied()` call threaded through placement, pricing, spawning, mining,
    /// scoring and colour.
    sides: Vec<PlayerId>,
    /// Stopped, and not stepping until somebody says otherwise.
    ///
    /// Every room steps four times a second for as long as the process lives,
    /// whether or not anybody is in it — a world somebody built in and walked
    /// away from costs its full simulation for nobody. Sleeping is the answer,
    /// and it is nearly free because **the tick is the generation**: a world
    /// that is not stepping is not moving, so waking is indistinguishable from
    /// never having slept and a client adopts the tick it left off at.
    asleep: bool,
    /// Who blew the whistle, once somebody has.
    ///
    /// `None` for a match the console started, which is the operator rather
    /// than a player — and for one nobody has started yet. Not folded into
    /// `Phase::Running` because the phase is on the wire and in every
    /// `RoomInfo`, and a room list does not need to know whose match it was.
    started_by: Option<PlayerId>,
    /// Somebody joined, left, or the phase moved, and the lobby on every
    /// client is now out of date.
    ///
    /// A flag rather than a cadence, because a gathering match **does not
    /// step** — there is no tick to hang "every so often" from, and a lobby
    /// that only refreshed when the world moved would never refresh at all.
    lobby_changed: bool,
    /// **What the game is doing in this room**, as against what the match is.
    ///
    /// [`Rules::default`] everywhere but a laboratory, where the clock is a
    /// control and the two placing rules can be taken off — see
    /// [`Self::set_rules`]. Held here rather than on the client because a
    /// client that answered these for itself would predict placements this
    /// server refuses.
    rules: Rules,
    /// Grants made since the last step, waiting to be announced.
    granted: Vec<(PlayerId, (i32, i32))>,
    /// Actions taken since the last drain, to go out **now** rather than with
    /// the step they belong to. Drained by whatever carries the bytes; see
    /// [`ServerMessage::Acted`].
    announce: Vec<ServerMessage>,
}

/// The most chunks one message may fetch.
///
/// **Because the reply is unbounded and the request is not.** A `Subscribe`
/// naming a million chunks costs a few kilobytes to send and, on a torus where
/// every chunk exists, half a gigabyte of `ChunkData` queued into the
/// connection's unbounded channel. A viewport with its margin covers a few
/// hundred at the widest zoom, so this is far above anything a client asks for
/// and far below anything that hurts.
pub(crate) const MOST_CHUNKS_AT_ONCE: usize = 4096;

/// How often the standings go out, in generations.
///
/// Every other second at the usual tick rate. It is a pass over the world to
/// work out and a bar nobody can read is worse than one that lags.
const STANDING_EVERY: u64 = 8;

/// A secret nobody can guess, for a player to come back with.
///
/// `RandomState` is seeded by the operating system for every instance, which
/// is what hashing relies on to resist collision attacks — two of them give
/// 128 bits without a dependency. Strong enough for what this is: a claim
/// ticket for a game with no accounts, not a credential worth attacking.
pub fn new_token() -> String {
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

    /// Give this room's world the dice that belong to its **id**.
    ///
    /// Here rather than in [`Self::named`], which takes a display name: the id
    /// is what never changes, and a rename must not re-roll a world's dice.
    /// `Rooms` is the only thing that knows both, so it is the only thing that
    /// can do this — a `Server` on its own is one world with a name on it.
    ///
    /// A room that never goes through here rolls from nought, which is a
    /// perfectly good number and is what a test and an offline game get. What
    /// matters is only that both peers agree, and both derive it the same way
    /// from the same id. See [`crate::net::world_seed`].
    pub fn seeded_by(mut self, id: &crate::net::RoomId) -> Self {
        self.world.set_seed(crate::net::world_seed(id));
        self
    }

    pub fn named(room: impl Into<RoomName>, world: World) -> Self {
        Self {
            room: room.into(),
            world,
            players: HashMap::new(),
            pending: Vec::new(),
            phase: Phase::Open,
            victory: None,
            sides: Vec::new(),
            asleep: false,
            started_by: None,
            lobby_changed: false,
            rules: Rules::default(),
            granted: Vec::new(),
            announce: Vec::new(),
        }
    }

    /// What a player joins this room with.
    ///
    /// Zero in a match, and the reason is the same one that stops anybody
    /// placing before the whistle: value spent while gathering is an opening
    /// bought in wall-clock time, and holding the tick still does not hold a
    /// clock still.
    pub fn starting_value(&self) -> i32 {
        match self.phase {
            Phase::Open => Player::STARTING_VALUE,
            _ => 0,
        }
    }

    /// Make this room a match, gathering and not yet stepping.
    /// The sides, as a lobby needs them: each with its name and who sits on
    /// it. Empty in a free-for-all.
    ///
    /// A side's name is the name on its own row, because a side is a player
    /// and a player has a name. There used to be a `team_names` vector beside
    /// the sides, indexed from one, with a `saturating_sub` to get back to it.
    pub fn teams(&self) -> Vec<crate::net::Team> {
        self.sides
            .iter()
            .map(|&id| crate::net::Team {
                id,
                name: self.players.get(&id).map(|p| p.name.clone()).unwrap_or_default(),
                players: self
                    .players
                    .values()
                    .filter(|p| p.online && p.plays_as == id)
                    .map(|p| p.id)
                    .collect(),
            })
            .collect()
    }

    /// What number a seat's cells carry: its side's, or its own.
    ///
    /// The one lookup that replaces `Sides`. Everything downstream — placing,
    /// pricing, spawning, scoring — takes the answer and never asks again who
    /// is allied with whom, because by then there is nobody to be allied with.
    pub fn plays_as(&self, seat: PlayerId) -> PlayerId {
        self.players.get(&seat).map_or(seat, |p| p.plays_as)
    }

    /// Put this player on a side, or take them off one.
    ///
    /// Only while gathering. Changing sides mid-match would hand your ground
    /// to the people you were fighting, which is not something a lobby should
    /// let anybody do by accident.
    ///
    /// `team` is the side's own number, or the seat's own to step off one.
    pub fn join_team(&mut self, seat: PlayerId, team: PlayerId) -> Result<(), String> {
        if self.sides.is_empty() {
            return Err("this match has no teams".into());
        }
        // A world has no whistle, so its teams are never settled: people join
        // and leave one as they like. A match's are fixed at the start, or
        // changing sides would hand your ground to the people you were
        // fighting.
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        // Their own number steps off a side; anything else has to be a side
        // this match actually has, or a client could put its cells under
        // somebody else's number by asking.
        if team != seat && !self.sides.contains(&team) {
            return Err(format!("this match has {} teams", self.sides.len()));
        }
        let Some(player) = self.players.get_mut(&seat) else {
            return Err("you are not in this match".into());
        };
        player.plays_as = team;
        self.lobby_changed = true;
        Ok(())
    }

    /// Call a side something.
    pub fn name_team(&mut self, team: PlayerId, name: &str) -> Result<(), String> {
        if !self.sides.contains(&team) {
            return Err("no such team".into());
        }
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        let name = crate::net::team_name(name)?;
        let ordinal = self.sides.iter().position(|&s| s == team).unwrap_or(0) as u8 + 1;
        let row = self.players.get_mut(&team).expect("a side has a row");
        row.name = if name.is_empty() { crate::net::default_team_name(ordinal) } else { name };
        self.lobby_changed = true;
        Ok(())
    }

    /// Whether the sides are even enough to start, or what is wrong with them.
    ///
    /// Two things, and both are about a match nobody would want to play rather
    /// than about fairness in the abstract. **Everybody has to be on a side**,
    /// because a player left on nobody's is a free agent in a team game and
    /// the scoring has nowhere to put them. And **no side may be empty**, since
    /// a three-way match with one side unoccupied is a two-way match that
    /// scores as if it were not.
    ///
    /// Sizes are *not* checked beyond that. Three against two is a match
    /// people may well have arranged on purpose, and a server that refuses it
    /// is a server they work around by leaving somebody out.
    fn teams_are_fair(&self) -> Result<(), String> {
        let here: Vec<&Player> = self.players.values().filter(|p| p.online).collect();
        if let Some(stray) = here.iter().find(|p| p.plays_as == p.id) {
            return Err(format!("{} has not picked a team", stray.name));
        }
        if let Some(&empty) = self.sides.iter().find(|&&t| !here.iter().any(|p| p.plays_as == t)) {
            let name = self.players.get(&empty).map(|p| p.name.as_str()).unwrap_or("that side");
            return Err(format!("nobody is on {name}"));
        }
        Ok(())
    }

    /// Give this match sides. Only before it starts, and only on a match.
    /// **A side is a player, so making one takes a number.** They come from
    /// the same pool the seats do and are handed out here, before anybody
    /// joins, so a side and a seat can never be the same number — which is
    /// exactly the collision that used to seat an unaligned player 3 on top of
    /// team 3. What it costs is seats: a world holds [`PlayerId::MAX`]
    /// numbers, and a match with `n` sides has `n` fewer people in it.
    pub fn make_teams(&mut self, n: u8) -> Result<(), String> {
        // A world may have them too. What a team is, is people building as one
        // player — one purse, one patch of ground, one colour — and none of
        // that needs a result to be about. What a *match* adds is that the
        // teams have to be even before the whistle.
        if matches!(self.phase, Phase::Running { .. } | Phase::Over { .. }) {
            return Err("teams are settled once a match starts".into());
        }
        if !(crate::net::MIN_TEAMS..=crate::net::MAX_TEAMS).contains(&n) {
            return Err(format!(
                "a match has between {} and {} teams",
                crate::net::MIN_TEAMS,
                crate::net::MAX_TEAMS
            ));
        }
        for ordinal in 1..=n {
            let Some(id) = self.next_player_id() else {
                return Err("this world has no numbers left for another side".into());
            };
            let mut row = Player::new(id, crate::net::default_team_name(ordinal));
            // A side is not sitting anywhere and has nothing to come back
            // with: it is a number that holds ground and a purse, and the
            // people at its controls have seats of their own.
            row.online = false;
            row.value = self.starting_value();
            self.players.insert(id, row);
            self.sides.push(id);
        }
        Ok(())
    }

    /// How many sides this match has. Nought is a free-for-all.
    pub fn team_count(&self) -> u8 {
        self.sides.len() as u8
    }

    pub fn make_match(&mut self, victory: Victory) {
        self.phase = Phase::Gathering;
        self.victory = Some(victory);
        self.lobby_changed = true;
    }

    /// Start the clock. The tick it starts at is what the deadline is measured
    /// from, so a match that gathered for an hour still runs its full length.
    /// Blow the whistle. `by` is the player who pressed it, or `None` for the
    /// console, which is the operator rather than anybody in the room.
    pub fn start_match(&mut self, by: Option<PlayerId>) -> Result<(), String> {
        match self.phase {
            Phase::Gathering => {
                // **The balance check is here and not in the lobby.** A lobby
                // that refuses to let you join your friend because the sides
                // would be uneven is a lobby that makes people argue about the
                // order they clicked in; one that refuses to *start* until the
                // sides are even is a lobby where they sort it out and press
                // it again.
                if !self.sides.is_empty() {
                    if let Err(why) = self.teams_are_fair() {
                        return Err(why);
                    }
                }
                self.phase = Phase::Running { from: self.tick() };
                self.started_by = by;
                self.lobby_changed = true;
                // **Everybody spawns at the whistle, together.** Granting on
                // arrival would put the first player's block on a world the
                // last player has not seen yet, and would hand out seats in
                // the order people happened to click -- so a match's world is
                // empty until it starts, and then it is laid out all at once.
                //
                // In player order rather than in whatever order the map
                // iterates, because grants are laid on a spiral by number and
                // two peers must lay the same one.
                let mut here: Vec<PlayerId> =
                    self.players.values().filter(|p| p.online).map(|p| p.id).collect();
                here.sort_unstable();
                for id in here {
                    self.grant_territory(id);
                }
                Ok(())
            }
            Phase::Open => Err("that room is not a match".into()),
            Phase::Running { .. } => Err("that match is already running".into()),
            Phase::Over { .. } => Err("that match is over".into()),
        }
    }

    /// Stop or start this world. Refused on a match: a match has a clock and
    /// a deadline measured in generations, and a sleep would be a pause in a
    /// race some of whose runners are asleep and some of whom are not.
    pub fn set_asleep(&mut self, asleep: bool) -> Result<(), String> {
        if !matches!(self.phase, Phase::Open) {
            return Err("a match does not sleep".into());
        }
        if self.asleep == asleep {
            return Err(format!("already {}", if asleep { "asleep" } else { "awake" }));
        }
        self.asleep = asleep;
        Ok(())
    }

    pub fn is_asleep(&self) -> bool {
        self.asleep
    }

    pub fn rules(&self) -> Rules {
        self.rules
    }

    /// Make this room a laboratory. Only at creation: a world that could be
    /// turned into one halfway through is a world whose ground was won under
    /// one set of rules and is being built on under another.
    pub fn make_laboratory(&mut self) {
        self.rules.laboratory = true;
        // Stopped, which is Golly's habit and the right one: the first thing
        // anybody does here is draw, and a world running while you draw into
        // it is a world eating what you drew.
        self.rules.paused = true;
    }

    /// Change what the game is doing here, if that is a thing this room lets
    /// anybody do.
    ///
    /// The whole set at once, so the answer is one broadcast rather than
    /// three, and [`Rules::laboratory`] is not among what can be changed —
    /// see [`Self::make_laboratory`].
    pub fn set_rules(&mut self, asked: Rules) -> Result<Rules, String> {
        if !self.rules.laboratory {
            return Err("this room is a game, so its rules are not yours to change".into());
        }
        self.rules = Rules { laboratory: true, ..asked };
        Ok(self.rules)
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn victory(&self) -> Option<Victory> {
        self.victory
    }

    /// Who started this match, if a player did.
    pub fn started_by(&self) -> Option<PlayerId> {
        self.started_by
    }

    /// What the match is doing and who is in it, as a lobby needs it.
    ///
    /// Only players who are **here now**: a room remembers everybody it has
    /// ever seen, because their number is written into their ground, and a
    /// lobby listing people who left months ago is a lobby nobody can count.
    pub fn lobby(&self) -> ServerMessage {
        let mut players: Vec<crate::net::Seat> = self
            .players
            .values()
            .filter(|p| p.online)
            .map(|p| crate::net::Seat {
                id: p.id,
                name: p.name.clone(),
                who: p.person.clone().map(crate::net::PersonId),
            })
            .collect();
        // By number, which is the order they arrived, so the list does not
        // reshuffle itself between two frames.
        players.sort_by_key(|s| s.id);
        ServerMessage::Match(crate::net::Lobby {
            teams: self.teams(),
            started_by: self.started_by,
            // Both filled in by `Rooms` on the way out — see `rooms::stamp`.
            // A `Server` is one room and knows neither who asked for it nor
            // what code reaches it, the same way it does not know its own id.
            owner: None,
            code: None,
            phase: self.phase.clone(),
            victory: self.victory,
            players,
        })
    }

    /// Who holds how much, most first, as a client is told it.
    ///
    /// Players holding nothing are left out rather than sent as zero: on a
    /// world that has seen fifteen people, most of the list is nobody, and
    /// a bar of length zero says nothing a missing row does not.
    pub fn standing(&self) -> ServerMessage {
        ServerMessage::Standing { tick: self.tick(), held: crate::net::standings(&self.world) }
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
        crate::net::holdings(&self.world, crate::net::Granted::Excluded)
    }

    /// **Every square somebody holds**, granted ground included.
    ///
    /// What a player is *shown*, as against what they are scored on. The two
    /// differ by the patch everybody is handed on joining, and that difference
    /// is the whole reason there are two: a grant is not an achievement, so it
    /// is not a score — and it is very much ground, so a bar reading nought
    /// beside a screen of squares plainly yours is simply wrong. Which is what
    /// it read, for as long as somebody built only inside their own patch: a
    /// block is a still life and never leaves the twelve squares it was given.
    pub fn ground(&self) -> [usize; PlayerId::COUNT] {
        crate::net::holdings(&self.world, crate::net::Granted::Counted)
    }

    /// Whether this number still has anybody playing it.
    ///
    /// **The question a forfeit asks**, and it composes with teams for free: a
    /// seat plays a number, so a number is in the match while at least one
    /// seat playing it is here and has not given up. A lone player is their
    /// own number, so giving up puts them out; a team is out when everybody at
    /// its controls has gone or conceded, and one of three walking away leaves
    /// two hands on it.
    ///
    /// **Not `online`.** A connection that dropped is a player who can come
    /// back with their token — that is what the token is for — so being away
    /// is not being out. Only giving up is.
    pub fn still_in(&self, player: PlayerId) -> bool {
        self.seats().any(|p| p.plays_as == player && !p.forfeited)
    }

    /// Every row somebody sits in, which is every row that is not a team.
    ///
    /// A team has a `Player` row like anybody else — that is what makes it a
    /// player — so anything counting *people* has to leave them out, or a team
    /// counts as one of its own members.
    fn seats(&self) -> impl Iterator<Item = &Player> {
        self.players.values().filter(|p| !self.sides.contains(&p.id))
    }

    /// Every number still playing, in order.
    fn survivors(&self) -> Vec<PlayerId> {
        let mut left: Vec<PlayerId> =
            self.seats().filter(|p| !p.forfeited).map(|p| p.plays_as).collect();
        left.sort_unstable();
        left.dedup();
        left
    }

    /// Give up, for this seat.
    ///
    /// Only while a match is running. A world has nothing to concede — there
    /// is no result — and a gathering match is one you leave rather than lose.
    pub fn forfeit(&mut self, seat: PlayerId) -> Result<(), String> {
        if !matches!(self.phase, Phase::Running { .. }) {
            return Err("no match is running here".into());
        }
        let Some(player) = self.players.get_mut(&seat) else {
            return Err("you are not in this match".into());
        };
        if player.forfeited {
            return Err("you have already given up".into());
        }
        player.forfeited = true;
        let (name, plays) = (player.name.clone(), player.plays_as);
        log::info!("{seat:?} \"{name}\" gave up");
        self.lobby_changed = true;
        // Said out loud when it takes a whole number out, because that is the
        // moment it changes the match rather than one player's evening.
        if !self.still_in(plays) {
            log::info!("{plays:?} is out of match \"{}\"", self.room);
        }
        // **And a match with one number left is over.** Checked here rather
        // than in `decide`, because "one player remains" is only a result when
        // the others *conceded*: a match that simply started with one player
        // in it has not been won by anybody, and putting this in `decide`
        // ended every such match on its first generation.
        let left = self.survivors();
        if left.len() <= 1 {
            let held = self.territory();
            let winner = left.first().copied();
            let count = winner.map_or(0, |w| held[w.0 as usize]);
            self.phase = Phase::Over { winner, held: count, at: self.tick() };
            log::info!("match \"{}\" ended with everybody else out: {winner:?}", self.room);
        }
        Ok(())
    }

    /// Call it off early, with the score as it stands.
    ///
    /// Whoever started the match may end it, which is the same person and the
    /// same reasoning: they arranged it, so they are the one who can say it is
    /// over when it has stopped being worth playing. The result is real —
    /// whoever leads at that moment wins, and it is rated — because a match
    /// that ends with no result is one nobody can be held to.
    pub fn end_match(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::Running { .. }) {
            return Err("no match is running here".into());
        }
        let held = self.territory();
        let (winner, count) = matches::leader(&held);
        self.phase = Phase::Over { winner, held: count, at: self.tick() };
        self.lobby_changed = true;
        log::info!("match \"{}\" was called off at tick {}", self.room, self.tick());
        Ok(())
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
        // **A side is scored as one.** Territory is still contested per player
        // — two allies keep a border between their ground, they simply cannot
        // be hurt by it — so the sum is taken here, at the one place a result
        // is decided, rather than by teaching the rule about teams.
        let (winner, count) = matches::leader(&held);
        let done = match victory {
            Victory::Timer { generations } => self.tick().saturating_sub(from) >= generations,
            Victory::Territory { squares } => count >= squares,
        };
        if !done {
            return None;
        }
        self.phase = Phase::Over { winner, held: count, at: self.tick() };
        self.lobby_changed = true;
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

    /// What this client has to spend: the purse of the player it is playing.
    ///
    /// **Following [`Self::plays_as`] is what makes one purse to a team come
    /// out right everywhere at once** — the `Welcome`, the `Purse` that rides
    /// on a checkpoint, and the refusal in `handle` all ask this and all get
    /// the team's figure. There used to be a copy of the number on every ally
    /// and an invariant keeping them equal.
    pub fn value_of(&self, id: PlayerId) -> Option<i32> {
        self.players.get(&self.plays_as(id)).map(|p| p.value)
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Who finished this match, for whatever wants to rate it.
    ///
    /// Only players with a person: somebody who joined without a key is not
    /// somebody this server can remember, so there is nowhere to put a result
    /// for them. The rest are still rated against each other — see
    /// [`ratings::Ratings::settle`].
    ///
    /// A **side's** score rather than a seat's, because that is what a result
    /// is between: two allies win or lose the same match. Nothing here has to
    /// add a side up any more — a side's cells all carry its own number, so
    /// `territory` has already counted them under it.
    ///
    /// Side rows are skipped for free: they carry no person, and a result has
    /// nowhere to go without one.
    pub fn finishers(&self) -> Vec<crate::server::profiles::Finisher> {
        let held = self.territory();
        self.players()
            .filter_map(|p| {
                Some(crate::server::profiles::Finisher {
                    who: crate::net::PersonId(p.person.clone()?),
                    // What they last joined under, so a profile has a name on
                    // it before anybody looks it up.
                    name: p.name.clone(),
                    team: p.plays_as.0,
                    score: held[p.plays_as.0 as usize],
                })
            })
            .collect()
    }

    /// Everything waiting to go out before the next step. See
    /// [`ServerMessage::Acted`].
    pub fn take_announcements(&mut self) -> Vec<ServerMessage> {
        std::mem::take(&mut self.announce)
    }

    /// Whether this match was decided on the generation just stepped.
    pub fn just_decided(&self) -> bool {
        matches!(self.phase, Phase::Over { at, .. } if at == self.tick())
    }

    pub fn players(&self) -> impl Iterator<Item = &Player> {
        self.players.values()
    }

    /// The lowest number nobody has ever been given here, or `None` when the
    /// room is full.
    ///
    /// Never reused, even once its player has gone. A number is written into
    /// every cell they own, so reissuing one hands their territory to a
    /// stranger — and the ground stays after the connection does not. So
    /// [`PlayerId::MAX`] is a limit on players a room has ever seen, not on
    /// players connected at once; a returning person asks for the number they
    /// already have.
    fn next_player_id(&self) -> Option<PlayerId> {
        (1..=PlayerId::MAX).map(PlayerId).find(|id| !self.players.contains_key(id))
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
        person: Option<&crate::net::PersonId>,
    ) -> Result<PlayerId, String> {
        let name = name.into();
        if let Some(who) = person {
            match self.players.values_mut().find(|p| p.person.as_deref() == Some(who.as_str())) {
                // Their seat, and nobody is in it: this is them coming back.
                Some(p) if !p.online => {
                    p.name = name;
                    p.last_seen = 0;
                    p.online = true;
                    let id = p.id;
                    self.lobby_changed = true;
                    let started = self.phase.accepts_actions();
                    log::info!("rejoin: {id:?} \"{}\" came back", self.players[&id].name);
                    // Nothing is laid out until the match starts; see
                    // `start_match`. An ordinary room grants at once, as it
                    // always did.
                    if started {
                        self.grant_territory(id);
                    }
                    return Ok(id);
                }
                // **Their seat, and they are already in it.** Refused rather
                // than seated twice, which is the change a person makes: a
                // token that was already in use quietly handed out a *new*
                // player, because a token said which seat and not who, so two
                // tabs sharing one were two players and that was the honest
                // reading of it.
                //
                // A person is not two players. Somebody who has carried their
                // secret to a second machine and joined from both wants to
                // know that, and being handed a stranger's seat four hundred
                // generations into a match is not knowing it.
                Some(p) => {
                    log::info!("{:?} \"{}\" is already here", p.id, p.name);
                    return Err("you are already in this room somewhere else".into());
                }
                None => {}
            }
        }
        let id = self.join(name)?;
        self.lobby_changed = true;
        let seat = self.players.get_mut(&id).expect("just joined");
        seat.person = person.map(|p| p.0.clone());
        Ok(id)
    }

    /// Whether this person has a seat here to come back to.
    ///
    /// **The same rule [`Self::join_with`] uses, and it has to be**: a gate
    /// that admits on a weaker test than the one behind it is a gate with a
    /// hole in it. So this asks about a seat that is theirs *and* empty —
    /// somebody already in their seat is not returning, they are here.
    fn returning(&self, person: Option<&crate::net::PersonId>) -> bool {
        let Some(who) = person else { return false };
        self.players.values().any(|p| p.person.as_deref() == Some(who.as_str()) && !p.online)
    }

    /// Move a purse.
    ///
    /// Takes a seat or the player it plays as — either reaches the same purse,
    /// which is what [`Self::value_of`] does for reading and for the same
    /// reason. There is one purse to a team because a team is one player, so
    /// there is nothing here to keep in step: this used to write the same
    /// change to every ally in the room to hold an invariant that a single
    /// number does not need.
    pub(crate) fn credit(&mut self, player: PlayerId, delta: i32) {
        let purse = self.plays_as(player);
        if let Some(row) = self.players.get_mut(&purse) {
            row.value = (row.value + delta).clamp(0, Player::MAX_VALUE);
        }
    }

    pub fn join(&mut self, name: impl Into<String>) -> Result<PlayerId, String> {
        let Some(id) = self.next_player_id() else {
            let name = name.into();
            log::warn!("refused \"{name}\": server full at {} players", PlayerId::MAX);
            return Err(format!("server full ({} players)", PlayerId::MAX));
        };
        let mut player = Player::new(id, name);
        player.last_seen = self.tick();
        // **A match starts everybody with nothing.** An ordinary room hands
        // out `STARTING_VALUE` so somebody can build the moment they arrive;
        // in a match that would be an opening bought rather than played, and
        // whatever a player did with it before the whistle would be a head
        // start measured in wall-clock time rather than in generations.
        player.value = self.starting_value();
        log::info!(
            "join: {:?} \"{}\" in room {} at tick {} ({} online)",
            id,
            player.name,
            self.room,
            self.tick(),
            self.players.len() + 1
        );
        self.players.insert(id, player);
        // A match lays out nothing until the whistle -- `start_match` does
        // every player at once -- where an ordinary room grants on arrival, as
        // it always has.
        if self.phase.accepts_actions() {
            self.grant_territory(id);
        }
        Ok(id)
    }

    /// Give this player their opening patch, if they have not had it.
    ///
    /// Granted again on a rejoin, deliberately: a player whose life was wiped
    /// out while they were away would otherwise come back with nowhere to
    /// stand, and re-marking ground they already hold costs nothing.
    ///
    /// Safe to call more than once — `net::grant` is idempotent — which is
    /// what the rejoin path relies on: a player coming back is granted again
    /// here, and used to get a fresh block on top of whatever they had built.
    fn grant_territory(&mut self, id: PlayerId) {
        let side = self.plays_as(id);
        crate::net::grant(&mut self.world, side);
        let (row, col) = crate::net::spawn_for(side, &self.world);
        log::info!("{id:?} granted ground at ({row}, {col})");
        // **Written down, because a grant changes the world and no client is
        // watching for it.** A player arriving is told their spawn in the
        // `Welcome` and rebuilds their world from it; a match grants everybody
        // at the whistle, when every client has long since joined and
        // subscribed, so their chunks do not change hands and nothing goes
        // back for them. Drained by `step`.
        self.granted.push((id, (row, col)));
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
        self.lobby_changed = true;
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
    }

    /// Decoded message in, replies out. Deliberately transport-agnostic.
    /// `who` is **which person this connection is**, as the server's table
    /// settled it — see [`crate::server::people`]. A room cannot work that out
    /// for itself and must not try: people are a server's table and a room is
    /// one world on it, so fifteen rooms deciding would be fifteen ideas of
    /// who somebody is.
    pub fn handle(
        &mut self,
        from: Option<PlayerId>,
        who: Option<&crate::net::PersonId>,
        msg: ClientMessage,
    ) -> Vec<ServerMessage> {
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
                if !self.phase.open_to_newcomers() && !self.returning(who) =>
            {
                vec![ServerMessage::Rejected {
                    reason: format!("\"{}\" is a match already under way", self.room),
                }]
            }
            ClientMessage::Join { name, room: _, person: _ } => {
                match self.join_with(name, who) {
                    Ok(you) => {
                        let spawn = crate::net::spawn_for(self.plays_as(you), &self.world);
                        let value = self.value_of(you).unwrap_or(Player::STARTING_VALUE);
                        vec![ServerMessage::Welcome {
                            you,
                            tick: self.tick(),
                            spawn,
                            // Filled in by `rooms::Rooms`, which holds the
                            // table: a profile outlives every room here, so a
                            // room does not get to keep one.
                            profile: None,
                            value,
                            room: crate::net::RoomId(self.room.clone()),
                            name: self.room.clone(),
                            // Sent rather than left to be derived: nothing a client
                            // can see says whether the ground ends, so a client
                            // told nothing builds an infinite world and disagrees
                            // with a wrapping server about where everything is.
                            world: self.world.kind(),
                            // And the same argument: nothing a client can see
                            // says whether placing here is free, and one that
                            // guessed would price every action wrongly.
                            rules: self.rules,
                        }]
                    }
                    Err(reason) => vec![ServerMessage::Rejected { reason }],
                }
            }
            ClientMessage::Act(stamped) => {
                // **An action belongs to the connection that sent it**, not to
                // the player it names. Without this the `player` field is a
                // claim rather than an identity: any connection in the room
                // could act as anybody in it, spending their value and placing
                // their cells, and a connection with no seat at all — a
                // spectator — could act as everybody.
                //
                // **Against the sender's side, not their seat**, because in a
                // match the cells carry the side's number and that is what the
                // client stamps. `plays_as` is the seat's own number outside a
                // match, so a free-for-all asks exactly what it always did —
                // and somebody on no side cannot borrow a side's number,
                // because then the two do not match.
                //
                // **Both halves.** `seat` says who sent it and `player` says
                // what number its cells carry, and a client that could lie
                // about either could act as somebody else or put its cells
                // under their number. Checked rather than rewritten, because
                // the two disagreeing is a client that is wrong or lying and
                // neither should be quietly obeyed under a corrected name.
                let acting_as = from.map(|seat| self.plays_as(seat));
                if from != Some(stamped.seat) || acting_as != Some(stamped.player) {
                    log::warn!(
                        "dropped an action from {:?} claiming to be {:?} playing {:?}",
                        from,
                        stamped.seat,
                        stamped.player
                    );
                    return Vec::new();
                }
                // **Before anything walks the list.** Pricing and applying are
                // both linear in it, the whole action is cloned into an
                // `Acted` and broadcast, and every client in the room applies
                // it too -- so an unbounded list is unbounded work on the one
                // task that owns every world, amplified to the room. Cost is
                // no bound: an `Erase` over ground nobody holds prices at
                // nothing however long it is.
                if stamped.action.cells().len() > crate::net::MOST_CELLS_AT_ONCE {
                    log::warn!(
                        "dropped an action from {:?} naming {} cells, over the {} allowed",
                        from,
                        stamped.action.cells().len(),
                        crate::net::MOST_CELLS_AT_ONCE
                    );
                    return Vec::new();
                }
                // And nothing from somebody who has given up: a forfeit is a
                // seat leaving the match, so it must not go on placing.
                if from.is_some_and(|seat| self.players.get(&seat).is_some_and(|p| p.forfeited)) {
                    log::debug!("dropped an action from {from:?}, who gave up");
                    return Vec::new();
                }
                // Nothing happens before the whistle, and nothing after it.
                // Dropped rather than answered, which is what an action the
                // server will not take already does -- the client predicted it
                // locally and the next `Checkpoint` puts the world and the
                // purse back. It will do that until a match's phase reaches
                // the client and it can refuse for itself; see planned.md.
                if !self.phase.accepts_actions() {
                    log::debug!(
                        "dropped an action from {:?}: \"{}\" is {}",
                        stamped.player,
                        self.room,
                        self.phase.name()
                    );
                    return Vec::new();
                }
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
                // Placing is confined to ground the player's own influence
                // reaches. Judged here as well as refused in the client,
                // because a client that sends whatever it likes is the case
                // this exists for -- and all or nothing, matching how the
                // client prices and previews it: a paint half applied is a
                // shape nobody drew.
                if let crate::net::Action::Paint { cells, .. } = &stamped.action {
                    if let Some(&(row, col)) = cells.iter().find(|&&(r, c)| {
                        !crate::net::may_place_under(&self.world, stamped.player, r, c, &self.rules)
                    }) {
                        log::info!(
                            "refused {:?}: nothing of theirs reaches ({row}, {col})",
                            stamped.player
                        );
                        return Vec::new();
                    }
                }
                // Cost is charged now, against the world as it stands, rather
                // than when the action is applied at the tick boundary -- the
                // client priced it against the same state, so pricing it later
                // would let the two disagree.
                if let Some(player) = self.players.get(&stamped.player) {
                    let delta = crate::net::price_under(&self.world, &stamped, &self.rules);
                    if player.value + delta < 0 {
                        log::info!(
                            "refused {:?}: costs {} with {} in hand",
                            stamped.player,
                            -delta,
                            player.value
                        );
                        return Vec::new();
                    }
                    self.credit(stamped.player, delta);
                    // Out at once, so everybody else applies it on the tick it
                    // names rather than when that tick is announced. It rides
                    // in the `Step` as well, because a broadcast can be
                    // dropped and this is a shortcut rather than a promise.
                    self.announce.push(ServerMessage::Acted(stamped.clone()));
                    self.pending.push(stamped);
                }
                Vec::new()
            }
            // **A fetch, whatever it is called.** Chunk contents only ever
            // leave here in reply to one of these; a change is broadcast as a
            // `Step` to everybody in the room, so there is no push that a
            // subscription would select. The server used to keep the list
            // anyway, in a `HashMap<PlayerId, Vec<ChunkId>>` that nothing read
            // — unbounded, undeduplicated, and grown by every resync, with an
            // `Unsubscribe` beside it doing an `O(n*m)` `retain` over
            // attacker-sized input on the one task that owns every room.
            ClientMessage::Subscribe { chunks } => {
                let chunks = &chunks[..chunks.len().min(MOST_CHUNKS_AT_ONCE)];
                let out: Vec<_> =
                    chunks.iter().filter_map(|&chunk| self.chunk_message(chunk)).collect();
                log::info!(
                    "subscribe: {:?} asked for {} chunks, sending {} the world holds",
                    from,
                    chunks.len(),
                    out.len()
                );
                out
            }
            // A room cannot answer these three. It is one room, so it knows
            // of no others to list, cannot make one, and cannot admit a
            // watcher to somewhere that is not itself. `Rooms::handle`
            // answers all three before it routes anything here.
            ClientMessage::Rooms | ClientMessage::Create { .. } | ClientMessage::Watch { .. } => {
                Vec::new()
            }
            // Answered by `Rooms::handle`, which is the only thing that knows
            // who made a room and so the only thing that can judge these.
            ClientMessage::Start | ClientMessage::EndMatch => Vec::new(),
            // Answered here, because giving up is a thing a seat does to the
            // room it is in and needs nobody's permission.
            ClientMessage::Forfeit => {
                let Some(seat) = from else { return Vec::new() };
                match self.forfeit(seat) {
                    Ok(()) => Vec::new(),
                    Err(reason) => vec![ServerMessage::NotStarted { reason }],
                }
            }
            // Answered by `Rooms::handle`, which owns the seat this gives up.
            ClientMessage::Leave => Vec::new(),
            // And by `Rooms`, which holds the table. A room is one world on a
            // server and a person outlives every one of them.
            ClientMessage::Profile { .. } | ClientMessage::People { .. } => Vec::new(),
            // The lobby, which is a place rather than a world: both of these
            // change who is on whose side and neither touches a cell.
            ClientMessage::JoinTeam { team } => {
                let Some(id) = from else { return Vec::new() };
                match self.join_team(id, team) {
                    Ok(()) => {
                        log::info!("{id:?} took side {}", team.0);
                        Vec::new()
                    }
                    // Said rather than dropped: a press in a lobby that does
                    // nothing and explains nothing looks like a broken lobby.
                    Err(reason) => vec![ServerMessage::NotStarted { reason }],
                }
            }
            ClientMessage::NameTeam { team, name } => match self.name_team(team, &name) {
                Ok(()) => Vec::new(),
                Err(reason) => vec![ServerMessage::NotStarted { reason }],
            },
            // A laboratory's clock and its two switches. Broadcast rather than
            // answered to whoever asked: a laboratory is a room several people
            // are in, and a clock that stopped for one of them would be two
            // worlds.
            ClientMessage::SetRules(asked) => match self.set_rules(asked) {
                Ok(rules) => {
                    log::info!("\"{}\" is now {rules:?}", self.room);
                    self.announce.push(ServerMessage::Rules(rules));
                    Vec::new()
                }
                Err(reason) => vec![ServerMessage::NotStarted { reason }],
            },
            ClientMessage::StepOnce => {
                let stepped = self.step_once();
                self.announce.extend(stepped);
                Vec::new()
            }
            ClientMessage::Wipe => match self.wipe() {
                Ok(chunks) if chunks.is_empty() => Vec::new(),
                Ok(chunks) => {
                    // To everybody in the room: several people share a
                    // laboratory, and one of them emptying it is news.
                    self.announce.push(ServerMessage::Resync { tick: self.tick(), chunks });
                    Vec::new()
                }
                Err(reason) => vec![ServerMessage::NotStarted { reason }],
            },
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
                    log::warn!(
                        "desync: {:?} disagrees on {} chunks at tick {tick}",
                        from,
                        wrong.len()
                    );
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

    /// **Empty a laboratory**, keeping its clock.
    ///
    /// Answers with the chunks that had something in them, because a client
    /// cannot be told "it is all gone" — what it can be told is which chunks
    /// to ask for again, which is `Resync` and is machinery that already
    /// exists for a world this one disagrees with.
    ///
    /// The seed and the tick both stay: a generation is a number two peers
    /// agree on, and this clears the ground rather than starting the room
    /// over.
    pub fn wipe(&mut self) -> Result<Vec<ChunkId>, String> {
        if !self.rules.laboratory {
            return Err("this room is a game, so its world is not yours to empty".into());
        }
        let had: Vec<ChunkId> = self.world.stored().iter().map(|&(coord, _)| coord).collect();
        let (seed, tick) = (self.world.seed(), self.tick());
        self.world = self.world.kind().build();
        self.world.set_seed(seed);
        self.world.set_generation(tick);
        // Nothing is pending against a world that no longer has the cells
        // those actions were priced and aimed against.
        self.pending.clear();
        log::info!("\"{}\" was emptied at tick {tick}", self.room);
        Ok(had)
    }

    /// One generation in a stopped room, and stay stopped.
    ///
    /// The pause is lifted for exactly this call rather than toggled around
    /// it: a client that unpaused, stepped and paused again would run the
    /// world for however long the two round trips took, which at four
    /// generations a second is not one step.
    pub fn step_once(&mut self) -> Vec<ServerMessage> {
        if !self.rules.paused {
            return Vec::new();
        }
        self.rules.paused = false;
        let out = self.step();
        self.rules.paused = true;
        out
    }

    /// Apply everything queued for this tick, advance one generation, and hand
    /// back what every client needs to stay in step.
    pub fn step(&mut self) -> Vec<ServerMessage> {
        // Asleep is a whole stop: no generation, and no actions applied
        // either, since an action applied to a world that is not moving would
        // land on a tick that has not happened.
        //
        // A stopped laboratory is the same stop for a different reason —
        // somebody is drawing into it — and [`Self::step_once`] is the way
        // past it.
        if self.asleep || self.rules.paused {
            return Vec::new();
        }
        let mut lobby: Vec<ServerMessage> =
            if std::mem::take(&mut self.lobby_changed) { vec![self.lobby()] } else { Vec::new() };

        // Where anybody was just put, and the ground to go and fetch. Both,
        // and in that order: the first is what a camera needs and the second
        // is what a world needs, and a client that got only the first would
        // look at ground it still believes is empty.
        let granted = std::mem::take(&mut self.granted);
        if !granted.is_empty() {
            let mut chunks: Vec<crate::net::ChunkId> = granted
                .iter()
                .flat_map(|(_, at)| crate::net::grant_chunks(&self.world, *at))
                .collect();
            chunks.sort_unstable();
            chunks.dedup();
            lobby.extend(
                granted
                    .iter()
                    .map(|(player, at)| ServerMessage::Spawned { player: *player, at: *at }),
            );
            lobby.push(ServerMessage::Resync { tick: self.tick(), chunks });
        }

        // A match that has not started yet, or is over, holds still. Pending
        // is emptied rather than left, so an action that arrived in the same
        // breath as the whistle cannot be applied a phase later than it was
        // priced.
        if !self.phase.stepping() {
            self.pending.clear();
            return lobby;
        }
        let applied = std::mem::take(&mut self.pending);
        for stamped in &applied {
            self.apply(stamped);
        }
        let mined = self.world.step();

        // What the mines paid out. The world counted the births; the price is
        // here, and this is the only place a purse is authoritative.
        //
        // **One purse to a side**, which is now the same sentence as one
        // purse to a player: a team's mines carry the team's number, so the
        // world counted them under it and there is nothing to sum. This used
        // to be quadratic in the roster to hold allies at the same figure.
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            let earned = crate::net::earnings(&mined, id);
            if let Some(player) = self.players.get_mut(&id) {
                // Floored at zero. A cost that comes from an action is refused
                // when it cannot be paid; a drain arrives whether or not there
                // is anything to take it from, and a player in debt would be a
                // player who cannot act and has no way to stop owing.
                // And capped, which is a rule rather than a display: income
                // runs away from a big player, so `Player::MAX_VALUE` is a
                // ceiling on hoarding while there is nothing better.
                player.value = (player.value + earned).clamp(0, Player::MAX_VALUE);
            }
        }

        self.decide();

        // Every generation, even an empty one: the tick is what keeps clients
        // in step, and a quiet generation still moves the world on.
        let mut out = lobby;
        out.push(ServerMessage::Step { tick: self.tick(), actions: applied });

        // And the standings on a cadence. One pass over the world to work out,
        // and a bar that moved four times a second would be harder to read
        // than one that moves every couple of seconds -- so this is a rate
        // chosen for eyes rather than for the machine. Sent the moment a match
        // is decided as well, whatever the cadence says, because the last one
        // is the result.
        if self.tick() % STANDING_EVERY == 0
            || matches!(self.phase, Phase::Over { at, .. } if at == self.tick())
        {
            out.push(self.standing());
        }
        out
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
        assert_eq!(s.join("c").unwrap(), PlayerId(3), "a departed player's number is theirs still");
        assert!(!s.players().find(|p| p.id == a).unwrap().online, "and they are marked gone");
    }

    /// **A laboratory is a room, and its clock is a control.**
    ///
    /// It used to be a mode the client was in with no server at all, which is
    /// what made the two placing rules client-held flags — and a client that
    /// answers those for itself predicts placements a server refuses. Held
    /// here, several people can be in one laboratory and the answer is the
    /// same for all of them.
    #[test]
    fn a_laboratory_opens_stopped_and_steps_when_it_is_told_to() {
        let mut s = Server::new(World::infinite());
        s.make_laboratory();
        assert!(s.rules().paused, "the first thing anybody does here is draw");

        let at = s.tick();
        assert!(s.step().is_empty(), "a stopped world does not step on the clock");
        assert_eq!(s.tick(), at);

        assert!(!s.step_once().is_empty(), "and does step when asked");
        assert_eq!(s.tick(), at + 1, "by exactly one generation");
        assert!(s.rules().paused, "and stays stopped afterwards");
    }

    /// The other half: a room that is a game says so rather than quietly
    /// taking the rules off, because everywhere but a laboratory these *are*
    /// the rules.
    #[test]
    fn only_a_laboratory_may_have_its_rules_changed() {
        let mut s = Server::new(World::infinite());
        let free = crate::net::Rules { place_free: true, ..Default::default() };
        assert!(s.set_rules(free).is_err(), "a world is a game");
        assert_eq!(s.rules(), crate::net::Rules::default());

        s.make_laboratory();
        let now = s.set_rules(free).expect("a laboratory's rules are its own");
        assert!(now.place_free && now.laboratory, "and it stays a laboratory");
    }

    /// **What the rules being off actually means**, which is two questions and
    /// not a second simulation: where you may build, and what it costs.
    #[test]
    fn a_free_hand_places_off_your_own_ground_for_nothing() {
        let mut s = Server::new(World::infinite());
        let me = s.join("me").unwrap();
        let value = s.value_of(me).unwrap();
        // A long way from anything granted, so nothing of this player's
        // influence reaches it. A block rather than a cell, because the
        // assertion is read after a step and a lone cell dies of loneliness
        // before it can be looked at.
        let far = vec![(10_000, 10_000), (10_000, 10_001), (10_001, 10_000), (10_001, 10_001)];
        let act = |cells: Vec<(i32, i32)>| {
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
                action: Action::Paint { cells, placement: Placement::Life },
            })
        };

        s.handle(Some(me), None, act(far.clone()));
        s.step();
        assert!(!s.world().live_cells().contains(&far[0]), "not yours to build on");

        s.make_laboratory();
        s.set_rules(crate::net::Rules {
            paused: true,
            place_anywhere: true,
            place_free: true,
            ..Default::default()
        })
        .unwrap();
        s.handle(Some(me), None, act(far.clone()));
        s.step_once();
        assert!(s.world().live_cells().contains(&far[0]), "with the rules off, anywhere");
        assert_eq!(s.value_of(me), Some(value), "and for nothing");
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
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
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
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
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
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
                action: Action::Paint {
                    cells: mine(me, &[(4, 4), (4, 5), (4, 6)]),
                    placement: Placement::Life,
                },
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
            s.handle(
                Some(me),
                None,
                ClientMessage::Act(Stamped { tick: s.tick(), player: me, seat: me, action }),
            );
            s.step();
        };

        // A 2x2 block: a still life, so it is still where it was put when the
        // next assertion looks. A blinker would have rotated out from under it.
        act(
            &mut s,
            Action::Paint {
                cells: mine(me, &[(0, 0), (0, 1), (1, 0), (1, 1)]),
                placement: Placement::Life,
            },
        );
        assert_eq!(s.value_of(me), Some(start - 4 * Placement::Life.cost()));

        // Reclaiming two of your own pays two back.
        // Reclaiming pays one each, well short of what they cost to place.
        act(
            &mut s,
            Action::Erase { cells: mine(me, &[(0, 0), (0, 1)]), placement: Placement::Life },
        );
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
    /// **A seat survives the server closing**, and the person in it is what
    /// finds it again.
    ///
    /// Nobody is connected to a world read off a disk, which is what makes the
    /// way back work: a player who is *online* is not returning, they are
    /// here. `Player::new` is what a player joins with and joining means being
    /// online, so a roster rebuilt from a file used to come back marked
    /// connected — and everybody in it was then refused their own seat on the
    /// next run and given a new one, beside territory they could see and could
    /// not build on.
    #[test]
    fn a_seat_survives_the_server_closing() {
        let path = std::env::temp_dir().join(format!("ck-restart-{}.ckw", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let who = crate::net::PersonId("3f2a".into());

        let mut before = Server::named("arena", World::infinite_empty());
        let me = before.join_with("alice", Some(&who)).unwrap();
        before.players.get_mut(&me).unwrap().value = 42;
        assert!(before.players[&me].online, "connected while playing");
        before.save(&path).unwrap();

        // A new process, reading the file the old one left.
        let mut after = Server::load_or_new(&path, "arena", World::infinite_empty).unwrap();
        assert!(!after.players[&me].online, "nobody is connected to a world off a disk");

        let back = after.join_with("alice", Some(&who)).unwrap();
        assert_eq!(back, me, "the person brings them back to their own number");
        assert_eq!(after.players[&me].value, 42, "with what they had");

        // And somebody else is somebody else, which is the other half of it.
        let other = after.join_with("bob", Some(&crate::net::PersonId("aaaa".into()))).unwrap();
        assert_ne!(other, me, "a different person took the same seat");

        let _ = std::fs::remove_file(&path);
    }

    /// Ground far from anybody's granted patch, so it counts towards a score.
    fn stake(s: &mut Server, id: PlayerId, at: (i32, i32), n: i32) {
        for r in at.0..at.0 + n {
            for c in at.1..at.1 + n {
                // At full influence, or the rule would let it go on the first
                // generation: ground is a level now, and a square holding
                // nothing is a square nobody is holding.
                s.world.set_cell_at(
                    r,
                    c,
                    Cell::DEAD.with_player(id).with_level(crate::sim::bits::MAX_LEVEL),
                );
            }
        }
    }

    /// A match runs its length and names whoever holds most.
    #[test]
    fn a_timer_match_ends_and_names_a_winner() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 5 });
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();

        // Gathering holds still, which is what makes the opening drawn rather
        // than raced: nobody gains generations by arriving early.
        s.step();
        s.step();
        assert_eq!(s.tick(), 0, "a gathering match does not step");

        stake(&mut s, alice, (900, 900), 6);
        stake(&mut s, bob, (900, 940), 4);
        s.start_match(None).unwrap();

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

    /// **A match's world does not exist until it starts.** Granting on
    /// arrival would put the first player's block on a world the last player
    /// has not seen yet, and would hand out ground in the order people
    /// happened to click. So a gathering match is an empty world and a list of
    /// names, and the whistle lays every seat at once.
    #[test]
    fn a_match_spawns_everybody_at_the_whistle_and_nobody_before_it() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 100 });

        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        assert!(s.world().live_cells().is_empty(), "no world yet");
        assert_eq!(s.territory().iter().sum::<usize>(), 0, "and no ground either");
        assert_eq!(s.value_of(alice), Some(0), "and nothing to spend");
        assert_eq!(s.value_of(bob), Some(0));

        s.start_match(None).unwrap();

        // Two blocks, one each, and each on its own granted patch.
        assert_eq!(s.world().live_cells().len(), 8, "a block each, laid together");
        for id in [alice, bob] {
            let (row, col) = crate::net::spawn_for(id, s.world());
            assert_eq!(s.world().cell_at(row, col).unwrap().player(), id);
        }
    }

    /// An ordinary room is unchanged: it grants on arrival and hands out
    /// something to build with, because there is no whistle to wait for.
    #[test]
    fn an_ordinary_room_still_grants_on_arrival() {
        let mut s = Server::named("main", World::infinite_empty());
        let alice = s.join_with("alice", None).unwrap();
        assert_eq!(s.world().live_cells().len(), 4, "a block, at once");
        assert_eq!(s.value_of(alice), Some(Player::STARTING_VALUE));
    }

    /// **A gathering match does not step, so a lobby cannot be told on a
    /// cadence.** There is no tick to hang "every so often" from, and a lobby
    /// that only refreshed when the world moved would never refresh at all —
    /// so it goes out when it changes, and a still world still sends one.
    #[test]
    fn a_lobby_is_told_when_it_changes_even_though_nothing_steps() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 100 });

        let lobby = |out: &[ServerMessage]| {
            out.iter().find_map(|m| match m {
                ServerMessage::Match(crate::net::Lobby { players, phase, .. }) => {
                    Some((players.clone(), phase.clone()))
                }
                _ => None,
            })
        };

        // Making it is a change, and the world is frozen.
        let (players, phase) = lobby(&s.step()).expect("the making of it");
        assert_eq!(phase, Phase::Gathering);
        assert!(players.is_empty());
        assert_eq!(s.tick(), 0, "and still nothing stepped");

        // Quiet in between: a lobby nobody has touched is not resent.
        assert!(lobby(&s.step()).is_none(), "nothing changed, so nothing is said");

        let alice = s.join_with("alice", None).unwrap();
        let (players, _) = lobby(&s.step()).expect("somebody arrived");
        assert_eq!(players.len(), 1);
        assert_eq!((players[0].id, players[0].name.as_str()), (alice, "alice"));

        s.leave(alice);
        let (players, _) = lobby(&s.step()).expect("and left");
        assert!(players.is_empty(), "a lobby lists who is here now: {players:?}");

        // Starting is a change too, and it is the one a client must not miss:
        // a lobby still saying "waiting to start" after it has started is a
        // screen telling a lie.
        s.start_match(None).unwrap();
        let (_, phase) = lobby(&s.step()).expect("the whistle");
        assert!(matches!(phase, Phase::Running { .. }));
    }

    /// Most first, ties by number, and nobody holding nothing.
    ///
    /// The order has to be the same on every peer or rows swap places at a tie
    /// and the bars jump about; leaving out the empty is what stops a world
    /// that has seen fifteen people showing a column of mostly nobody.
    #[test]
    fn the_standing_is_most_first_and_leaves_out_the_empty() {
        let mut s = Server::named("arena", World::infinite_empty());
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        let carol = s.join_with("carol", None).unwrap();

        // **A grant is not a score, and it is very much ground.** Every row
        // here is somebody with a patch and nothing won yet, so the scores are
        // nought and the ground is not — which is the whole reason there are
        // two numbers. The bar shows the second, and read nought for as long
        // as it showed the first.
        let ServerMessage::Standing { held, .. } = s.standing() else { panic!() };
        assert_eq!(held.len(), 3, "a grant is ground: {held:?}");
        for row in &held {
            assert_eq!(row.score, 0, "a grant scored: {row:?}");
            assert!(row.ground >= 100, "a granted patch is ground: {row:?}");
        }

        stake(&mut s, bob, (900, 900), 4);
        stake(&mut s, carol, (900, 940), 4);
        stake(&mut s, alice, (940, 900), 5);

        let ServerMessage::Standing { held, tick } = s.standing() else { panic!() };
        assert_eq!(tick, s.tick());
        let scores: Vec<(PlayerId, u32)> = held.iter().map(|h| (h.who, h.score)).collect();
        assert_eq!(
            scores,
            vec![(alice, 25), (bob, 16), (carol, 16)],
            "most first, and a tie by the lower number"
        );
        // And the ground each holds is their score plus the patch they were
        // given, which is what a player sees on the map.
        for row in &held {
            assert!(row.ground > row.score, "ground left out the grant: {row:?}");
        }
    }

    /// The standings go out on a cadence, and the moment a match is decided
    /// whatever the cadence says — the last one is the result.
    #[test]
    fn the_standing_goes_out_on_a_cadence_and_at_the_whistle() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 3 });
        let alice = s.join_with("alice", None).unwrap();
        stake(&mut s, alice, (900, 900), 4);
        s.start_match(None).unwrap();

        let standing =
            |out: &[ServerMessage]| out.iter().any(|m| matches!(m, ServerMessage::Standing { .. }));
        assert!(!standing(&s.step()), "tick 1 is not on the cadence");
        assert!(!standing(&s.step()), "nor is tick 2");
        // Tick 3 is the whistle, which sends one whatever the cadence says.
        let last = s.step();
        assert!(standing(&last), "the result goes out at once");
        assert!(matches!(s.phase(), Phase::Over { .. }));
    }

    /// **Nothing happens before the whistle.** A match that let people place
    /// while gathering would be fair in generations and unfair in *time*:
    /// somebody who joined ten minutes early has had ten minutes to think and
    /// draw, and holding the tick still does not hold a clock still.
    #[test]
    fn a_gathering_match_takes_no_actions() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 100 });
        let alice = s.join_with("alice", None).unwrap();
        let cells = mine(alice, &[(3, 3), (3, 4)]);

        let before = s.world().live_cells().len();
        s.handle(
            Some(alice),
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: alice,
                seat: alice,
                action: Action::Paint { cells: cells.clone(), placement: Placement::Life },
            }),
        );
        s.step();
        assert_eq!(s.world().live_cells().len(), before, "nothing laid before the whistle");
        assert_eq!(s.value_of(alice), Some(0), "and a match starts you with nothing");
        assert_eq!(before, 0, "nor is there a world yet to lay it on");

        // The whistle: everybody is granted at once, and only then is there
        // anything to act on or with.
        s.start_match(None).unwrap();
        s.players.get_mut(&alice).unwrap().value = 100;
        let before = s.world().live_cells().len();
        assert_eq!(before, 4, "a block, laid at the whistle");
        s.handle(
            Some(alice),
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: alice,
                seat: alice,
                action: Action::Paint { cells, placement: Placement::Life },
            }),
        );
        s.step();
        assert!(s.world().live_cells().len() > before, "and lands once it is running");
        assert!(s.value_of(alice).unwrap() < Player::STARTING_VALUE, "and is paid for");
    }

    /// And nothing after it either: a decided match cannot be played on.
    #[test]
    fn an_over_match_takes_no_actions() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 1 });
        let alice = s.join_with("alice", None).unwrap();
        s.start_match(None).unwrap();
        s.step();
        assert!(matches!(s.phase(), Phase::Over { .. }), "one generation, then over");

        let before = s.world().live_cells().len();
        s.handle(
            Some(alice),
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: alice,
                seat: alice,
                action: Action::Paint { cells: mine(alice, &[(3, 3)]), placement: Placement::Life },
            }),
        );
        s.step();
        assert_eq!(s.world().live_cells().len(), before);
    }

    /// **A side is one seat, one platform and one purse.**
    ///
    /// Allies build on each other's ground and cannot hurt each other, so
    /// seating them separately hands one team two opening positions where a
    /// solo player gets one -- twice the frontage, and a border between them
    /// no rule will ever contest. The size of a side is meant to be the
    /// advantage, not where it starts.
    #[test]
    fn a_team_shares_a_seat_a_platform_and_a_purse() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 1000 });
        s.make_teams(2).unwrap();
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        let carol = s.join_with("carol", None).unwrap();
        s.join_team(alice, PlayerId(1)).unwrap();
        s.join_team(bob, PlayerId(1)).unwrap();
        s.join_team(carol, PlayerId(2)).unwrap();
        s.start_match(None).unwrap();

        // A team is a player, so two people at one team's controls are
        // seated identically because they are asking about the same number.
        let seat = |id| crate::net::spawn_for(s.plays_as(id), s.world());
        assert_eq!(seat(alice), seat(bob), "allies were seated apart");
        assert_ne!(seat(alice), seat(carol), "two teams shared a seat");
        assert_eq!(s.plays_as(alice), s.plays_as(bob), "allies are not one player");
        assert_ne!(s.plays_as(alice), alice, "joining a team did not take its controls");

        // One platform: the second ally to be granted finds the ground already
        // held by their own side and leaves it alone, rather than laying a
        // second block on top of their team's opening.
        let home = seat(alice);
        let blocks = (home.0..home.0 + crate::net::SPAWN_N)
            .flat_map(|r| (home.1..home.1 + crate::net::SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| s.world().cell_at(r, c).is_some_and(|cell| cell.is_alive()))
            .count();
        assert_eq!(blocks, 4, "a team has one 2x2 block, and found {blocks} live cells");

        // A match starts everybody at nothing, which is the invariant's
        // starting condition as well as the game's.
        s.step();
        for id in [alice, bob, carol] {
            assert_eq!(s.value_of(id), Some(0), "a match did not start everybody level");
        }

        // One purse, and it is the team's own — so this is not an invariant
        // being kept across two numbers, it is two clients reading one.
        s.credit(alice, 40);
        assert_eq!(s.value_of(alice), Some(40));
        assert_eq!(s.value_of(bob), Some(40), "an ally was not paid");
        assert_eq!(s.value_of(carol), Some(0), "the other side was");

        s.credit(bob, -15);
        assert_eq!(s.value_of(alice), Some(25), "an ally's spending did not come out of the purse");
        assert_eq!(s.value_of(bob), Some(25));
        assert_eq!(s.value_of(carol), Some(0));
    }

    /// **A world may have teams**, and its teams are never settled: there is
    /// no whistle, so people join and leave one as they like. A match fixes
    /// them at the start, or changing sides would hand your ground to the
    /// people you were fighting.
    #[test]
    fn a_world_has_teams_and_they_are_never_settled() {
        let mut s = Server::new(World::infinite_empty());
        s.make_teams(2).expect("a world was refused teams");
        let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
        let alice = s.join_with("alice", None).unwrap();
        s.join_team(alice, teams[0]).unwrap();
        assert_eq!(s.plays_as(alice), teams[0]);

        // A world steps forever, and the teams still move.
        s.step();
        s.join_team(alice, teams[1]).expect("a world settled its teams");
        assert_eq!(s.plays_as(alice), teams[1]);
        s.join_team(alice, alice).expect("stepping off a team was refused");
        assert_eq!(s.plays_as(alice), alice);

        // And a match's do not, once it is running.
        let mut m = Server::new(World::infinite_empty());
        m.make_match(Victory::Timer { generations: 100 });
        m.make_teams(2).unwrap();
        let teams: Vec<PlayerId> = m.teams().iter().map(|t| t.id).collect();
        let bob = m.join_with("bob", None).unwrap();
        let carol = m.join_with("carol", None).unwrap();
        m.join_team(bob, teams[0]).unwrap();
        m.join_team(carol, teams[1]).unwrap();
        m.start_match(None).unwrap();
        assert!(m.join_team(bob, teams[1]).is_err(), "a running match let somebody change teams");
    }

    /// **Giving up is a seat's decision and being out is a player's**, which
    /// is the distinction a team needs: one of two walking away leaves one
    /// pair of hands on the team, and the team plays on.
    #[test]
    fn one_of_a_team_giving_up_does_not_concede_for_the_team() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 10_000 });
        s.make_teams(2).unwrap();
        let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        let carol = s.join_with("carol", None).unwrap();
        s.join_team(alice, teams[0]).unwrap();
        s.join_team(bob, teams[0]).unwrap();
        s.join_team(carol, teams[1]).unwrap();
        s.start_match(None).unwrap();

        s.forfeit(alice).unwrap();
        assert!(s.still_in(teams[0]), "a team conceded when one of two gave up");
        assert!(matches!(s.phase(), Phase::Running { .. }), "and the match stopped");
        // Twice is not a thing to do, and says so rather than doing nothing.
        assert!(s.forfeit(alice).is_err());

        // The second of them takes the team out, and with one number left the
        // match is over and the survivor has won it.
        s.forfeit(bob).unwrap();
        assert!(!s.still_in(teams[0]), "a team with nobody on it is still in");
        assert!(
            matches!(s.phase(), Phase::Over { winner: Some(w), .. } if *w == teams[1]),
            "the last team standing did not win: {:?}",
            s.phase()
        );
    }

    /// A match that simply *has* one player in it has not been won by them.
    /// Putting the survivor check in `decide` ended every such match on its
    /// first generation.
    #[test]
    fn a_match_with_one_player_is_not_over_before_it_begins() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 50 });
        let alice = s.join_with("alice", None).unwrap();
        s.start_match(None).unwrap();
        s.step();
        assert!(matches!(s.phase(), Phase::Running { .. }), "{:?}", s.phase());
        // And giving up when you are the only one there ends it with nobody.
        s.forfeit(alice).unwrap();
        assert!(matches!(s.phase(), Phase::Over { winner: None, .. }), "{:?}", s.phase());
    }

    /// **A seat that gave up stops placing.** A forfeit that left somebody
    /// able to act would be a concession in the scoreboard and nowhere else.
    #[test]
    fn a_seat_that_gave_up_cannot_act() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 10_000 });
        let alice = s.join_with("alice", None).unwrap();
        let _bob = s.join_with("bob", None).unwrap();
        s.start_match(None).unwrap();
        s.credit(alice, 100);
        let at = crate::net::spawn_for(alice, s.world());
        let act = || {
            ClientMessage::Act(Stamped {
                tick: 0,
                player: alice,
                seat: alice,
                action: crate::net::Action::Paint {
                    cells: vec![at],
                    placement: crate::net::Placement::Life,
                },
            })
        };
        s.handle(Some(alice), None, act());
        assert_eq!(s.pending.len(), 1);
        s.forfeit(alice).unwrap();
        s.handle(Some(alice), None, act());
        assert_eq!(s.pending.len(), 1, "somebody who gave up went on placing");
    }

    /// Calling it off is a real result: whoever leads wins it, and it is over
    /// rather than abandoned. A match nobody can be held to is not one worth
    /// rating.
    #[test]
    fn ending_a_match_early_names_whoever_is_ahead() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 10_000 });
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        s.start_match(None).unwrap();
        stake(&mut s, alice, (5_000, 5_000), 6);
        stake(&mut s, bob, (9_000, 9_000), 2);

        s.end_match().unwrap();
        assert!(
            matches!(s.phase(), Phase::Over { winner: Some(w), held, .. } if *w == alice && *held == 36),
            "{:?}",
            s.phase()
        );
        // And only while one is running.
        assert!(s.end_match().is_err(), "a decided match was ended again");
    }

    /// **An action says who sent it as well as what number it carries**, and
    /// both are checked. They were one question until a team became a player:
    /// several clients share `player`, so it can no longer say which of them
    /// acted, and a client that could name any seat could act as a teammate.
    #[test]
    fn an_action_must_name_the_seat_that_sent_it() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 500 });
        s.make_teams(2).unwrap();
        let teams: Vec<PlayerId> = s.teams().iter().map(|t| t.id).collect();
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        s.join_team(alice, teams[0]).unwrap();
        s.join_team(bob, teams[1]).unwrap();
        s.start_match(None).unwrap();

        let at = crate::net::spawn_for(teams[0], s.world());
        let tick = s.tick();
        let act = |seat, player| {
            ClientMessage::Act(Stamped {
                tick,
                player,
                seat,
                action: crate::net::Action::Paint {
                    cells: vec![at],
                    placement: crate::net::Placement::Life,
                },
            })
        };

        // A match starts everybody at nothing, and a paint costs.
        s.credit(teams[0], 100);
        s.credit(teams[1], 100);

        // Honest: alice, from alice's connection, playing her team.
        s.handle(Some(alice), None, act(alice, teams[0]));
        assert_eq!(s.pending.len(), 1, "an honest action was dropped");

        // A seat that is not the sender. Alice's connection cannot act as bob,
        // which is what the check was already for.
        s.handle(Some(alice), None, act(bob, teams[1]));
        assert_eq!(s.pending.len(), 1, "a connection acted as somebody else");

        // And the sender's own seat under a number they do not play: alice
        // putting cells down as the other team.
        s.handle(Some(alice), None, act(alice, teams[1]));
        assert_eq!(s.pending.len(), 1, "a seat placed under a number it does not play");
    }

    /// **The door is shut once a match is running**, and only somebody coming
    /// back to their own seat gets through it.
    ///
    /// The gate has to ask exactly what `join_with` asks — a gate that admits
    /// on a weaker test than the one behind it is a gate with a hole in it,
    /// and it had one: it asked whether the offered *token* matched anybody's,
    /// and a `Player` started with an empty token, so a client sending an
    /// empty one matched the first seat never issued a secret and was handed a
    /// brand new player four hundred generations into a race.
    #[test]
    fn a_match_under_way_lets_nobody_new_in() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 100 });
        let alice = crate::net::PersonId("3f2a".into());
        s.join_with("alice", Some(&alice)).unwrap();
        s.start_match(None).unwrap();
        let before = s.players().count();

        let door = |who: Option<crate::net::PersonId>| {
            (
                who.clone(),
                ClientMessage::Join { name: "latecomer".into(), room: None, person: None },
            )
        };
        // Nobody, and somebody this room has never seated.
        for offered in [None, Some(crate::net::PersonId("aaaa".into()))] {
            let (who, msg) = door(offered.clone());
            let out = s.handle(None, who.as_ref(), msg);
            assert!(
                matches!(&out[..], [ServerMessage::Rejected { .. }]),
                "{offered:?} got in: {out:?}"
            );
        }
        assert_eq!(s.players().count(), before, "a refusal seated somebody anyway");

        // Somebody whose seat is here and *occupied* is a second machine, not
        // a reconnection, so the door refuses that too.
        let (who, msg) = door(Some(alice.clone()));
        let out = s.handle(None, who.as_ref(), msg);
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "a second machine got in: {out:?}"
        );

        // But a player who actually dropped comes back, because this is the
        // door and not the room.
        let seat = s.players().next().map(|p| p.id).unwrap();
        s.leave(seat);
        let (who, msg) = door(Some(alice));
        let out = s.handle(None, who.as_ref(), msg);
        assert!(matches!(&out[..], [ServerMessage::Welcome { .. }]), "{out:?}");
        assert_eq!(s.players().count(), before, "coming back made a second player");
    }

    /// **The whistle says where everybody landed, and what to go and fetch.**
    ///
    /// A match grants at the whistle rather than on arrival, by which time
    /// every client has joined and subscribed -- so the chunks the grants
    /// landed in do not change hands, nothing re-fetches them, and the ground
    /// appeared for the server and for nobody else. Reloading the page fixed
    /// it, which is what made it look like a bug in the client.
    #[test]
    fn a_whistle_says_where_everybody_landed() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Timer { generations: 100 });
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();
        // Nothing is laid out while gathering, so nothing is announced either.
        assert!(!s.step().iter().any(|m| matches!(m, ServerMessage::Spawned { .. })));

        s.start_match(None).unwrap();
        let out = s.step();

        let spawned: Vec<_> = out
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Spawned { player, at } => Some((*player, *at)),
                _ => None,
            })
            .collect();
        assert_eq!(spawned.len(), 2, "the whistle told nobody where they were: {out:?}");
        assert!(spawned.iter().any(|(p, _)| *p == alice));
        assert!(spawned.iter().any(|(p, _)| *p == bob));

        // And the ground itself, named so a client that already holds those
        // chunks knows they are wrong now.
        let resynced: Vec<_> = out
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Resync { chunks, .. } => Some(chunks.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        for (_, at) in &spawned {
            for chunk in crate::net::grant_chunks(s.world(), *at) {
                assert!(resynced.contains(&chunk), "{chunk:?} was granted and not resynced");
            }
        }
        // Once. A grant that announced itself every step would be a resync
        // storm for as long as the match ran.
        assert!(!s.step().iter().any(|m| matches!(m, ServerMessage::Spawned { .. })));
    }

    /// The other condition: first to a count rather than most at a whistle.
    #[test]
    fn a_territory_match_ends_when_somebody_reaches_the_count() {
        let mut s = Server::named("arena", World::infinite_empty());
        s.make_match(matches::Victory::Territory { squares: 50 });
        let alice = s.join_with("alice", None).unwrap();
        s.start_match(None).unwrap();

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
        let alice = s.join_with("alice", None).unwrap();
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
        let who = crate::net::PersonId("3f2a".into());
        let alice = s.join_with("alice", Some(&who)).unwrap();
        s.start_match(None).unwrap();

        let refused = s.handle(
            None,
            None,
            ClientMessage::Join { name: "late".into(), room: None, person: None },
        );
        assert!(
            matches!(refused.as_slice(), [ServerMessage::Rejected { reason }] if reason.contains("already under way")),
            "{refused:?}"
        );

        s.leave(alice);
        let back = s.handle(
            None,
            Some(&who),
            ClientMessage::Join { name: "alice".into(), room: None, person: None },
        );
        assert!(
            matches!(back.first(), Some(ServerMessage::Welcome { you, .. }) if *you == alice),
            "a player already in the match comes back: {back:?}"
        );
    }

    /// The whole point of being somebody: a player who drops comes back to
    /// their own number, their own value and their own ground, rather than to
    /// a fresh number beside a patch they can see and cannot build on.
    #[test]
    fn a_person_comes_back_to_themselves() {
        let mut s = Server::new(World::infinite_empty());
        let who = crate::net::PersonId("3f2a".into());
        let me = s.join_with("alice", Some(&who)).unwrap();

        // Spend some, so there is state worth coming back to.
        s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
                action: Action::Paint {
                    cells: mine(me, &[(0, 0), (0, 1)]),
                    placement: Placement::Life,
                },
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
            Some(&who),
            ClientMessage::Join { name: "alice".into(), room: None, person: None },
        );
        match welcome.as_slice() {
            [ServerMessage::Welcome { you, value, profile, .. }] => {
                assert_eq!(*you, me, "the same number");
                assert_eq!(*value, spent, "and the value they had");
                // A room does not fill this in. A profile outlives every room
                // on a server, so `Rooms` is what holds the table and what
                // stamps the answer — see `Rooms::profile_of`.
                assert!(profile.is_none());
            }
            other => panic!("expected a welcome, got {other:?}"),
        }
        assert_eq!(s.value_of(me), Some(spent));
    }

    /// Another player's territory has to reach you, or you cannot see whose
    /// ground you are standing next to — and, worse, your own does not reach
    /// you either: `may_place` reads the owner off the cell, so a client that
    /// never receives the chunk refuses to build on ground that is its own.
    ///
    /// The case that nearly slipped through is a chunk holding *only*
    /// territory. Chunks are sent when the world holds them, and it holds
    /// anything not empty — which counts ownership now. A filter on liveness
    /// would have dropped exactly the chunks this is about.
    #[test]
    fn territory_reaches_the_clients_that_ask_for_it() {
        let mut s = Server::new(World::infinite_empty());
        let alice = s.join_with("alice", None).unwrap();
        let bob = s.join_with("bob", None).unwrap();

        let (row, col) = crate::net::spawn_for(alice, s.world());
        let chunk = (row.div_euclid(CHUNK_N as i32), col.div_euclid(CHUNK_N as i32));

        let sent = s.handle(Some(bob), None, ClientMessage::Subscribe { chunks: vec![chunk] });
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
        let sent = s.handle(Some(bob), None, ClientMessage::Subscribe { chunks: vec![chunk] });
        assert!(
            matches!(sent.as_slice(), [ServerMessage::ChunkData { .. }]),
            "bare territory must still be sent, got {sent:?}"
        );
    }

    /// **Nobody may be one person twice.** Somebody already in their seat is
    /// refused a second one rather than handed a stranger's.
    ///
    /// This is where a person is stricter than the token it replaced, and
    /// deliberately so. A token said which *seat*, so two tabs sharing one
    /// were honestly two players and the second quietly got a new number. A
    /// person is not two players, and being told so beats finding out by
    /// building on ground that turns out to be somebody else's.
    #[test]
    fn one_person_cannot_hold_two_seats() {
        let mut s = Server::new(World::infinite_empty());
        let who = crate::net::PersonId("3f2a".into());
        let alice = s.join_with("alice", Some(&who)).unwrap();

        assert!(s.join_with("alice again", Some(&who)).is_err(), "one person took two seats");
        assert_eq!(s.players().count(), 1, "a refused join seated somebody anyway");

        // Once she has gone, she comes back to her own.
        s.leave(alice);
        assert_eq!(s.join_with("alice", Some(&who)).unwrap(), alice);
    }

    /// A person this room has never seated is a new player, not an error.
    /// Anything else would lock somebody out of a room they have not been in.
    #[test]
    fn an_unknown_person_joins_as_somebody_new() {
        let mut s = Server::new(World::infinite_empty());
        let first = s.join_with("alice", Some(&crate::net::PersonId("3f2a".into()))).unwrap();
        let second = s.join_with("bob", Some(&crate::net::PersonId("aaaa".into()))).unwrap();
        assert_ne!(first, second);
    }

    /// **A client with no person is still a player**, and a new one every
    /// time: there is nothing to find a seat by, which is the honest outcome
    /// for a browser that cannot keep a secret rather than a reason to refuse
    /// to let anybody play.
    #[test]
    fn a_client_with_no_person_is_new_every_time() {
        let mut s = Server::new(World::infinite_empty());
        let first = s.join_with("alice", None).unwrap();
        let second = s.join_with("alice", None).unwrap();
        assert_ne!(first, second, "two anonymous joins became one player");
        assert_eq!(s.players().count(), 2);
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
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: me,
                seat: me,
                action: Action::Paint { cells: pane.clone(), placement: Placement::Ice },
            }),
        );
        s.step();
        let (row, col) = pane[0];
        assert!(s.world().cell_at(row, col).unwrap().is_ice());
        let spent = s.value_of(me);

        s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: me,
                seat: me,
                action: Action::Erase { cells: pane, placement: Placement::Ice },
            }),
        );
        s.step();
        assert!(s.world().cell_at(row, col).unwrap().is_ice(), "the pane should still be there");
        assert_eq!(s.value_of(me), spent, "and nothing should have been paid for it");
    }

    /// The whole of what a side buys: allies build on each other's ground and
    /// score as one, and everything else stays exactly as it was.
    #[test]
    fn a_team_builds_together_and_scores_as_one() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Territory { squares: 1_000 });
        s.make_teams(2).unwrap();
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        let c = s.join("c").unwrap();
        s.join_team(a, PlayerId(1)).unwrap();
        s.join_team(b, PlayerId(1)).unwrap();
        s.join_team(c, PlayerId(2)).unwrap();

        // A patch of the team's ground with nothing standing on it. Staked
        // under the *team's* number, because that is the number A places
        // under — which is the whole of what joining a team did.
        let team = s.plays_as(a);
        assert_eq!(team, s.plays_as(b), "two at one team's controls are two players");
        let at = (5_000, 5_000);
        stake(&mut s, team, at, 4);

        // Both may build on it, and the other team may not — and neither
        // question needs anybody to know a team exists. `may_place` takes the
        // number being played and compares it, exactly as it did before there
        // were teams at all.
        assert!(crate::net::may_place(s.world(), s.plays_as(a), at.0, at.1));
        assert!(crate::net::may_place(s.world(), s.plays_as(b), at.0, at.1), "an ally cannot");
        assert!(!crate::net::may_place(s.world(), s.plays_as(c), at.0, at.1), "an enemy can");

        // And it is scored as one. There is nothing to sum: the cells carry
        // the team's number, so `territory` counted them under it.
        let held = s.territory();
        assert_eq!(held[team.0 as usize], 16);
        assert_eq!(held[a.0 as usize], 0, "a seat holds nothing of its own in a team match");
        assert_eq!(crate::server::matches::leader(&held), (Some(team), 16));
    }

    /// Teams are settled once a match starts. Changing them mid-match would
    /// hand your ground to the people you were fighting.
    #[test]
    fn teams_cannot_be_changed_once_the_whistle_has_gone() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 100 });
        s.make_teams(2).unwrap();
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        s.join_team(a, PlayerId(1)).unwrap();
        s.join_team(b, PlayerId(2)).unwrap();
        s.name_team(PlayerId(1), "Reds").unwrap();

        s.start_match(Some(a)).unwrap();
        assert!(s.join_team(a, PlayerId(2)).is_err(), "changed sides mid-match");
        assert!(s.name_team(PlayerId(1), "Blues").is_err(), "renamed a side mid-match");
    }

    /// A match nobody would want to play is refused at the whistle rather than
    /// in the lobby: a lobby that stops you joining your friend makes people
    /// argue about the order they clicked in.
    #[test]
    fn a_lopsided_match_is_refused_at_the_whistle_and_not_before() {
        let mut s = Server::new(World::infinite_empty());
        s.make_match(Victory::Timer { generations: 100 });
        s.make_teams(2).unwrap();
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();

        // Nobody has picked.
        let why = s.start_match(Some(a)).unwrap_err();
        assert!(why.contains("picked"), "{why}");

        // Everybody on one side, so the other is empty.
        s.join_team(a, PlayerId(1)).unwrap();
        s.join_team(b, PlayerId(1)).unwrap();
        let why = s.start_match(Some(a)).unwrap_err();
        assert!(why.contains("Team 2"), "{why}");

        // Three against one is *not* refused: people arrange that on purpose,
        // and a server that forbids it is one they work around.
        s.join_team(b, PlayerId(2)).unwrap();
        let c = s.join("c").unwrap();
        let d = s.join("d").unwrap();
        s.join_team(c, PlayerId(1)).unwrap();
        s.join_team(d, PlayerId(1)).unwrap();
        assert!(s.start_match(Some(a)).is_ok(), "three against one was refused");
    }

    /// An action belongs to the connection that sent it. Without this the
    /// `player` field is a claim rather than an identity: anybody in the room
    /// could act as anybody else in it, spending their value and placing their
    /// cells, and a connection with no seat at all — a spectator — could act
    /// as everybody.
    ///
    /// Measured on the purse rather than on the world, because a single live
    /// cell dies of loneliness in the same step that applies it. The value is
    /// the honest witness: it moves exactly when an action was taken.
    /// Coming back used to hand you a fresh 12×12 patch and a
    /// brand-new 2×2 block on top of whatever you had built — so disconnecting
    /// and returning conjured a still life out of nothing, for free, as often
    /// as you liked.
    #[test]
    fn coming_back_does_not_grant_a_second_platform() {
        let mut s = Server::new(World::infinite_empty());
        let who = crate::net::PersonId("3f2a".into());
        let me = s.join_with("alice", Some(&who)).unwrap();
        let at = crate::net::spawn_for(me, s.world());

        // Clear the block they were given, which is what a player who has
        // played for a while and lost it looks like.
        let block = (at.0..at.0 + crate::net::SPAWN_N)
            .flat_map(|r| (at.1..at.1 + crate::net::SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| s.world().cell_at(r, c).is_some_and(|x| x.is_alive()))
            .collect::<Vec<_>>();
        assert_eq!(block.len(), 4, "the grant stands a block");
        for (r, c) in block {
            let was = s.world().cell_at(r, c).unwrap();
            s.world_mut().set_cell_at(r, c, was.with_alive(false));
        }
        assert_eq!(s.world().live_cells().len(), 0, "nothing of theirs is alive");

        s.leave(me);
        let back = s.join_with("alice", Some(&who)).unwrap();
        assert_eq!(back, me, "they came back to themselves");
        assert_eq!(
            s.world().live_cells().len(),
            0,
            "coming back built a fresh block out of nothing"
        );
    }

    #[test]
    fn an_action_attributed_to_somebody_else_is_dropped() {
        let mut s = Server::new(World::infinite_empty());
        let alice = s.join("alice").unwrap();
        let bob = s.join("bob").unwrap();
        // Ground Alice owns with nothing standing on it, so the only reason
        // an action there could fail is the one being tested.
        let at = (10_000, 10_000);
        stake(&mut s, alice, at, 3);
        let before = s.value_of(alice).unwrap();

        let forged = |tick| Stamped {
            tick,
            player: alice,
            seat: alice,
            action: Action::Paint { cells: vec![at], placement: Placement::Life },
        };

        // Bob's connection, claiming to be Alice.
        s.handle(Some(bob), None, ClientMessage::Act(forged(s.tick())));
        assert_eq!(s.value_of(alice).unwrap(), before, "Alice paid for Bob's action");

        // And a connection with no seat at all, which is what a spectator is.
        s.handle(None, None, ClientMessage::Act(forged(s.tick())));
        assert_eq!(s.value_of(alice).unwrap(), before, "a watcher acted");

        // The same action from Alice's own connection is taken, so this is a
        // test about attribution and not about the action being invalid.
        s.handle(Some(alice), None, ClientMessage::Act(forged(s.tick())));
        assert_eq!(
            s.value_of(alice).unwrap(),
            before - crate::sim::LIFE_COST,
            "Alice's own action was refused too"
        );
    }

    #[test]
    fn destroying_another_players_cell_costs() {
        let mut s = Server::new(World::infinite_empty());
        let a = s.join("a").unwrap();
        let b = s.join("b").unwrap();
        // A block again, so a's cell survives long enough for b to attack it.
        s.handle(
            Some(a),
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: a,
                seat: a,
                action: Action::Paint {
                    cells: mine(a, &[(0, 0), (0, 1), (1, 0), (1, 1)]),
                    placement: Placement::Life,
                },
            }),
        );
        s.step();
        let (row, col) = mine(a, &[(0, 0)])[0];
        assert_eq!(s.world().cell_at(row, col).map(|c| c.player()), Some(a));

        let before = s.value_of(b).unwrap();
        s.handle(
            Some(b),
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: b,
                seat: b,
                action: Action::Erase { cells: mine(a, &[(0, 0)]), placement: Placement::Life },
            }),
        );
        s.step();
        assert_eq!(s.value_of(b), Some(before - 1), "taking ground is not free");
    }

    /// **Cost is no bound on length.** An `Erase` over ground nobody holds
    /// prices at nothing however many cells it names, so affordability would
    /// let a single message spend the room's whole tick — and then the room's
    /// whole broadcast, since every client applies it too. Refused on the
    /// length before anything walks the list.
    #[test]
    fn an_action_naming_more_cells_than_allowed_is_dropped() {
        let mut s = Server::new(World::infinite_empty());
        let me = s.join("me").unwrap();
        let over = crate::net::MOST_CELLS_AT_ONCE + 1;
        let before = s.value_of(me).unwrap();

        let out = s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
                // Far from anything, and free at any length: the point is that
                // nothing about the price would have stopped it.
                action: Action::Erase {
                    cells: (0..over as i32).map(|c| (100_000, c)).collect(),
                    placement: Placement::Life,
                },
            }),
        );
        assert!(out.is_empty());
        assert!(s.take_announcements().is_empty(), "an over-long action was broadcast");
        s.step();
        assert_eq!(s.value_of(me), Some(before), "and nothing was charged for it");

        // One cell under the cap goes through, so it is the length being
        // refused rather than the shape of the message.
        s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped {
                tick: s.tick(),
                player: me,
                seat: me,
                action: Action::Erase {
                    cells: (0..crate::net::MOST_CELLS_AT_ONCE as i32)
                        .map(|c| (100_000, c))
                        .collect(),
                    placement: Placement::Life,
                },
            }),
        );
        assert!(!s.take_announcements().is_empty(), "an action within the cap was dropped");
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

        s.handle(
            Some(me),
            None,
            ClientMessage::Act(Stamped {
                tick: 0,
                player: me,
                seat: me,
                action: Action::Paint { cells: too_many, placement: Placement::Life },
            }),
        );
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
            s.handle(Some(me), None, ClientMessage::Checkpoint { tick: 0, chunks: held.clone() });
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
        let replies = s.handle(Some(me), None, ClientMessage::Checkpoint { tick: 0, chunks: bad });
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
            None,
            ClientMessage::Act(Stamped {
                tick,
                player: id,
                seat: id,
                action: Action::Paint { cells: mine(id, offsets), placement: Placement::Mine },
            }),
        );
        // `handle` queues; `step` is what applies. Stepping once here would
        // also advance the world, so the pending action is drained by the
        // caller's own first step.
    }
}
