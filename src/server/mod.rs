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

pub mod api;
pub mod bot;
pub mod console;
pub mod lockers;
pub mod matches;
pub mod parties;
pub mod people;
pub mod persist;
pub mod profiles;
pub mod rating;
pub mod rooms;
pub mod unjoined;
#[cfg(feature = "server")]
pub mod ws;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::net::{
    ChunkId, ClientMessage, Level, RoomName, Rules, ServerMessage, Stamped, Tick, DEFAULT_ROOM,
};
use crate::sim::{Player, PlayerId, World};
use bot::{Bot, Driver};
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
    /// `allied()` call threaded through placement, pricing, spawning, manufacture,
    /// scoring and colour.
    sides: Vec<PlayerId>,
    /// Seconds owed to the next generation — see [`Self::owe`]. Not saved: a
    /// fraction of a tick is not a fact about a world.
    owed: f32,
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
    /// The seats the server plays — see [`bot`]. Bot-ness lives here and
    /// reaches the wire only through [`crate::net::Seat::bot`]: `sim::Player`
    /// is saved, and a match never is, so after a restart a bot's seat is an
    /// offline player like any human who left.
    bots: BTreeMap<PlayerId, Bot>,
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
            owed: 0.0,
            asleep: false,
            started_by: None,
            lobby_changed: false,
            rules: Rules::default(),
            granted: Vec::new(),
            announce: Vec::new(),
            bots: BTreeMap::new(),
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

    /// **Which side somebody arriving should be put on**, or `None` where the
    /// question does not arise — a room with no sides, or a match already
    /// running, where sides are settled.
    ///
    /// **The lobby's decision and not the form's.** A match is described
    /// before anybody is in it, so the person making one cannot say who goes
    /// where; the lobby is where that is known, and it is where somebody can
    /// disagree with the answer by pressing a different side.
    ///
    /// Empty sides first, in the order they were made, because a side nobody
    /// is on is the one thing that stops a whistle — see [`Self::teams_are_fair`].
    /// After that the smallest, which keeps a room that fills up roughly even
    /// without ever refusing the uneven match people meant to arrange.
    fn side_for_somebody_new(&self) -> Option<PlayerId> {
        if self.sides.is_empty() || !self.phase.open_to_newcomers() {
            return None;
        }
        // **People on it, and a side is not a person.** A side is a `Player`
        // row like anybody else and `Player::new` sets `plays_as` to its own
        // id, so counting naively has every side counting itself: none is ever
        // empty, and "empty first" is a clause that never fires.
        let on = |side: PlayerId| {
            self.players
                .values()
                .filter(|p| !self.sides.contains(&p.id) && p.plays_as == side)
                .count()
        };
        self.sides.iter().copied().min_by_key(|&side| (on(side) > 0, on(side), side.0))
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
                    self.teams_are_fair()?;
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
        // **The rate is checked; the three flags cannot be wrong.** A bool is
        // one of two answers and either is a room somebody might want, but a
        // rate is a number off the wire and `0` or `65535` are a stopped world
        // and a busy loop -- see `net::Rules::rate`.
        let bpm = Rules::rate(asked.bpm)?;
        self.rules = Rules { laboratory: true, bpm, ..asked };
        Ok(self.rules)
    }

    /// Set what this room runs at, out of range clamped rather than refused:
    /// this is a server's own default reaching a room, not a client asking.
    pub fn set_rate(&mut self, bpm: u16) {
        self.rules.bpm = bpm.clamp(crate::net::SLOWEST_BPM, crate::net::FASTEST_BPM);
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
                bot: self.bots.contains_key(&p.id),
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
        // **Every seat's name goes through here**, from a room, from the
        // console and from a test alike, so this is the one place it has to be
        // clamped -- see `net::player_name`, which says what a tab in a name
        // did to the profiles file.
        let name = crate::net::player_name(&name.into());
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

    /// The seat this person holds here, online or not. `None` for somebody
    /// the room has not met.
    pub fn seat_of(&self, who: &crate::net::PersonId) -> Option<PlayerId> {
        self.players.values().find(|p| p.person.as_deref() == Some(who.as_str())).map(|p| p.id)
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
            // **Say which kind of full.** A side takes a number out of the
            // same pool a seat does, so a match with fifteen of them has none
            // left -- and "server full (15 players)" told somebody who had
            // just made that match, and was the only person in it, that it was
            // full of people. The sides are the thing they can change.
            let sides = self.sides.len();
            let why = if sides > 0 {
                format!(
                    "no room: {sides} of the {} numbers are sides, and a side costs one",
                    PlayerId::MAX
                )
            } else {
                format!("server full ({} players)", PlayerId::MAX)
            };
            log::warn!("refused \"{name}\": {why}");
            return Err(why);
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
        // **Put on a side by the lobby**, rather than left on nobody's.
        //
        // Everybody has to be on one before the whistle -- see
        // `teams_are_fair` -- and nothing was putting them there, so the
        // person who described the match, made it, and was alone in it could
        // not start it until they had gone and clicked a side out of a list.
        // A default is not a decision taken away: the lobby is where it is
        // shown and one press is where it is changed.
        if let Some(side) = self.side_for_somebody_new() {
            if let Some(player) = self.players.get_mut(&id) {
                player.plays_as = side;
            }
        }
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

    /// Seat a player the server plays — see [`bot`].
    ///
    /// A seat like any other: [`Self::join_with`] hands it out, so a room
    /// full of bots refuses a person with the same words, and the lobby puts
    /// it on a side the way it puts anybody. `team` overrides that. Only
    /// while the room admits anybody, because a seat arriving mid-match is the
    /// late joining `join` refuses.
    pub fn add_bot(
        &mut self,
        name: impl Into<String>,
        level: Level,
        driver: Driver,
        team: Option<PlayerId>,
    ) -> Result<PlayerId, String> {
        if !self.phase.open_to_newcomers() {
            return Err(format!("\"{}\" is a match already under way", self.room));
        }
        // Judged before the seat is taken, because a world never gives a
        // number back -- see `next_player_id` -- and a refused side would
        // otherwise cost the room a seat.
        if let Some(team) = team {
            if self.sides.is_empty() {
                return Err("this match has no teams".into());
            }
            if !self.sides.contains(&team) {
                return Err(format!("this match has {} teams", self.sides.len()));
            }
        }
        let seat = self.join_with(name, None)?;
        if let Some(team) = team {
            self.join_team(seat, team)?;
        }
        let bot = Bot::new(level, driver, self.world.seed(), seat, self.tick());
        log::info!("{seat:?} is a {} bot in room {}", level.name(), self.room);
        self.bots.insert(seat, bot);
        Ok(seat)
    }

    /// Take a bot out again. Only while the room admits anybody: a seat
    /// leaving a running match is a forfeit, and [`Self::forfeit`] is that.
    ///
    /// **In a lobby the number comes back; in a world it is spent.** Nothing
    /// is laid out before the whistle, so a bot removed while a match gathers
    /// has its number in no cell and its row goes with it -- kept, fifteen
    /// presses of add-and-remove from one seat locked the room to everybody
    /// not already in it. A world grants on arrival, so there the seat is left
    /// as a person's who walked away, for the reason [`Self::leave`] gives.
    pub fn remove_bot(&mut self, seat: PlayerId) -> Result<(), String> {
        if !self.bots.contains_key(&seat) {
            return Err(format!("seat {} is not a bot", seat.0));
        }
        if !self.phase.open_to_newcomers() {
            return Err("bots are settled once a match starts".into());
        }
        self.bots.remove(&seat);
        if self.phase.accepts_actions() {
            self.leave(seat);
        } else {
            self.players.remove(&seat);
            self.lobby_changed = true;
            log::info!(
                "bot {seat:?} left room {} before the whistle; its number is free",
                self.room
            );
        }
        Ok(())
    }

    pub fn is_bot(&self, seat: PlayerId) -> bool {
        self.bots.contains_key(&seat)
    }

    pub fn bot(&self, seat: PlayerId) -> Option<&Bot> {
        self.bots.get(&seat)
    }

    /// Every bot here, by seat.
    pub fn bots(&self) -> impl Iterator<Item = (PlayerId, &Bot)> {
        self.bots.iter().map(|(&seat, bot)| (seat, bot))
    }

    /// Let every bot that is due make its move, through [`Self::act`] like
    /// anybody's. Before `pending` is taken, so what it chose goes out in the
    /// `Step` for this generation.
    ///
    /// **And not announced.** `act` says an action out loud so that a cell
    /// appears on everybody's screen before the `Step` that carries it, and
    /// a bot's is taken inside that very step -- an `Acted` for it would leave
    /// with the next thing any client said, a generation or more after the
    /// `Step` had already applied it, and a paint laid again a generation
    /// late is a different paint.
    fn bots_act(&mut self) {
        let tick = self.tick();
        let said = self.announce.len();
        let due: Vec<PlayerId> =
            self.bots.iter().filter(|(_, b)| b.next_at <= tick).map(|(&s, _)| s).collect();
        for seat in due {
            let (plays_as, purse) = (self.plays_as(seat), self.value_of(seat).unwrap_or(0));
            let Some(bot) = self.bots.get_mut(&seat) else { continue };
            bot.next_at = tick + bot.cadence();
            let chosen = match bot.driver {
                Driver::Book => bot.choose(&self.world, &self.rules, plays_as, purse, tick),
                // Priced as it arrived; nothing waits for a step.
                Driver::External => None,
            };
            if let Some(action) = chosen {
                if let Err(why) = self.act(Stamped { tick, player: plays_as, seat, action }) {
                    log::debug!("bot {seat:?} was refused: {why}");
                }
            }
        }
        self.announce.truncate(said);
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
                // Dropped rather than answered when refused, which is what an
                // action the server will not take already does -- the client
                // predicted it locally and the next `Checkpoint` puts the
                // world and the purse back. It will do that until a match's
                // phase reaches the client and it can refuse for itself; see
                // planned.md.
                let _ = self.act(stamped);
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
            // And by `Rooms`, which holds the tables. A room is one world on a
            // server and a person outlives every one of them -- their rating,
            // and the patterns and diary the server merely keeps for them.
            ClientMessage::Profile { .. }
            | ClientMessage::People { .. }
            | ClientMessage::Keep(_)
            // A challenge names a *person* and makes a room, and a room knows
            // of no others — both are `Rooms`' business, like everything else
            // that outlives one world.
            | ClientMessage::Challenge { .. }
            | ClientMessage::Answer { .. }
            // And who somebody is, which is the first of those, and closing a
            // room, which a room cannot do to itself.
            | ClientMessage::Hello { .. }
            | ClientMessage::Close { .. }
            | ClientMessage::Invite { .. }
            // And parties, which outlive every room here.
            | ClientMessage::Parties
            | ClientMessage::MakeParty { .. }
            | ClientMessage::InviteToParty { .. }
            | ClientMessage::JoinParty { .. }
            | ClientMessage::LeaveParty { .. } => Vec::new(),
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
            // A seat the server plays. **Seated players only**: a spectator
            // has no standing in a lobby, and is dropped the way anything
            // from nobody is. Refused the way a side is, into the lobby it
            // was pressed in, for the reason `JoinTeam` is.
            ClientMessage::AddBot { team, level } => {
                let Some(by) = from else { return Vec::new() };
                match self.add_bot(format!("{} bot", level.name()), level, Driver::Book, team) {
                    Ok(seat) => {
                        log::info!("{by:?} seated {seat:?}, a {} bot", level.name());
                        Vec::new()
                    }
                    Err(reason) => vec![ServerMessage::NotStarted { reason }],
                }
            }
            ClientMessage::RemoveBot { seat } => {
                let Some(by) = from else { return Vec::new() };
                match self.remove_bot(seat) {
                    Ok(()) => {
                        log::info!("{by:?} removed bot {seat:?}");
                        Vec::new()
                    }
                    Err(reason) => vec![ServerMessage::NotStarted { reason }],
                }
            }
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
                //
                // **Capped for the reason `Subscribe` is**, and it is the
                // sharper case of the two: a coordinate the server does not
                // hold has no digest, so it *always* mismatches and always
                // comes back. A client naming a million chunks it never had
                // therefore costs a few kilobytes to send and gets a million
                // back, having also walked the map a million times to find out.
                // A client holds a viewport and its margin, which is far
                // inside this.
                let chunks = &chunks[..chunks.len().min(MOST_CHUNKS_AT_ONCE)];
                let wrong: Vec<_> = chunks
                    .iter()
                    .copied()
                    .filter(|&(coord, digest)| self.world.chunk_digest(coord) != Some(digest))
                    .map(|(coord, _)| coord)
                    .collect();

                // The purse rides along, because manufacture made value something a
                // client cannot predict on its own: earnings depend on births
                // anywhere in the world, and a client holds a viewport. It
                // would drift down for as long as it played, and never
                // correct. The machinery for "your copy is wrong, here is
                // ours" already exists and runs every few seconds, so value
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

    /// Take an action, or say why not.
    ///
    /// **Everything an action is judged on, in one place**, whoever it came
    /// from: the wire arm above keeps only the identity check, because that
    /// is a question about the connection and a bot has none. So a bot goes
    /// through here too, and can do nothing a client could not.
    ///
    /// A refusal is a sentence, so the API can hand it back; the wire drops
    /// it and lets the next `Checkpoint` put the client right.
    pub(crate) fn act(&mut self, stamped: Stamped) -> Result<(), &'static str> {
        // **Before anything walks the list.** Pricing and applying are both
        // linear in it, the whole action is cloned into an `Acted` and
        // broadcast, and every client in the room applies it too -- so an
        // unbounded list is unbounded work on the one task that owns every
        // world, amplified to the room. Cost is no bound: an `Erase` over
        // ground nobody holds prices at nothing however long it is.
        if stamped.action.cells().len() > crate::net::MOST_CELLS_AT_ONCE {
            log::warn!(
                "dropped an action from {:?} naming {} cells, over the {} allowed",
                stamped.seat,
                stamped.action.cells().len(),
                crate::net::MOST_CELLS_AT_ONCE
            );
            return Err("that names more cells than one action may");
        }
        // And nothing from somebody who has given up: a forfeit is a seat
        // leaving the match, so it must not go on placing.
        if self.players.get(&stamped.seat).is_some_and(|p| p.forfeited) {
            log::debug!("dropped an action from {:?}, who gave up", stamped.seat);
            return Err("you have given up");
        }
        // Nothing happens before the whistle, and nothing after it.
        if !self.phase.accepts_actions() {
            log::debug!(
                "dropped an action from {:?}: \"{}\" is {}",
                stamped.player,
                self.room,
                self.phase.name()
            );
            return Err("this match is not running");
        }
        // Judged here as well as refused in the client, because a client that
        // sends whatever it likes is the case this exists for. Ice is not
        // liftable, so an erase naming it is not an action, whoever asks.
        if let crate::net::Action::Erase { placement, .. } = &stamped.action {
            if !placement.can_be_taken() {
                log::info!("refused {:?}: {placement:?} cannot be taken", stamped.player);
                return Err("ice cannot be taken back");
            }
        }
        // Placing is confined to ground the player's own influence reaches.
        // Judged here as well as refused in the client, because a client that
        // sends whatever it likes is the case this exists for -- and all or
        // nothing, matching how the client prices and previews it: a paint
        // half applied is a shape nobody drew.
        if let crate::net::Action::Paint { cells, .. } = &stamped.action {
            if let Some(&(row, col)) = cells.iter().find(|&&(r, c)| {
                !crate::net::may_place_under(&self.world, stamped.player, r, c, &self.rules)
            }) {
                log::info!(
                    "refused {:?}: nothing of theirs reaches ({row}, {col})",
                    stamped.player
                );
                return Err("nothing of yours reaches there");
            }
        }
        // Cost is charged now, against the world as it stands, rather than
        // when the action is applied at the tick boundary -- the client priced
        // it against the same state, so pricing it later would let the two
        // disagree.
        let Some(player) = self.players.get(&stamped.player) else {
            return Err("nobody plays that number here");
        };
        let delta = crate::net::price_under(&self.world, &stamped, &self.rules);
        if player.value + delta < 0 {
            log::info!(
                "refused {:?}: costs {} with {} in hand",
                stamped.player,
                -delta,
                player.value
            );
            return Err("you cannot afford that");
        }
        self.credit(stamped.player, delta);
        // Out at once, so everybody else applies it on the tick it names
        // rather than when that tick is announced. It rides in the `Step` as
        // well, because a broadcast can be dropped and this is a shortcut
        // rather than a promise.
        self.announce.push(ServerMessage::Acted(stamped.clone()));
        self.pending.push(stamped);
        Ok(())
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

    /// **Advance if this room owes a generation**, banking the rest.
    ///
    /// The server's ticker is fine and every room decides for itself, because
    /// the rate is one of the room's [`crate::net::Rules`] and a laboratory's
    /// is its own to change. A leftover rather than a reset, so a slow tick
    /// does not lose a generation and a fast one does not run two.
    ///
    /// The same shape `World::update` gives the client, and deliberately: two
    /// clocks banking time differently is two clocks that drift.
    pub fn owe(&mut self, dt: std::time::Duration) -> Vec<ServerMessage> {
        self.owed += dt.as_secs_f32();
        let span = self.rules.generation_span();
        if self.owed < span {
            return Vec::new();
        }
        // At most one a tick however far behind it is: a server that stalled
        // for a second should arrive late rather than run four generations
        // into one frame and hand every client four steps at once.
        self.owed = 0.0;
        self.step()
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
        self.bots_act();
        let applied = std::mem::take(&mut self.pending);
        for stamped in &applied {
            self.apply(stamped);
        }
        let mined = self.world.step();
        // **Said out loud, because nothing else says it.** A blast is a
        // generation in which a disc of ground quietly becomes different, and
        // a client watching that happen sees a glitch -- see
        // `ServerMessage::Blasts`. Taken before anything else looks at the
        // world, so a blast is broadcast once.
        let blasts = self.world.take_blasts();

        // What the factories paid out. The world counted the births; the price is
        // here, and this is the only place a purse is authoritative.
        //
        // **One purse to a side**, which is now the same sentence as one
        // purse to a player: a team's factories carry the team's number, so the
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
                // A blast's own drain arrives here too, and the floor is what
                // answers "what if they cannot pay". **It must not be the
                // detonation that is refused**: the simulation runs identically
                // on every client and no client knows anybody else's purse, so
                // a blast conditional on money would be a blast that happened
                // on one machine and not another. It goes off; the bill is
                // taken as far as it can be.
                player.value = (player.value + earned).clamp(0, Player::MAX_VALUE);
            }
        }

        self.decide();

        // Every generation, even an empty one: the tick is what keeps clients
        // in step, and a quiet generation still moves the world on.
        let mut out = lobby;
        out.push(ServerMessage::Step { tick: self.tick(), actions: applied });
        // After the step it belongs to, so a client draws the fireball over
        // the ground it has already been handed.
        if !blasts.is_empty() {
            out.push(ServerMessage::Blasts(blasts));
        }

        // And the standings on a cadence. One pass over the world to work out,
        // and a bar that moved four times a second would be harder to read
        // than one that moves every couple of seconds -- so this is a rate
        // chosen for eyes rather than for the machine. Sent the moment a match
        // is decided as well, whatever the cadence says, because the last one
        // is the result.
        if self.tick().is_multiple_of(STANDING_EVERY)
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
mod tests;
