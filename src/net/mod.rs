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

pub mod auth;
pub mod codec;
pub mod keep;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
#[cfg(target_arch = "wasm32")]
pub mod link_web;
#[cfg(target_arch = "wasm32")]
pub use link_web as link;

pub use auth::{Claim, Key, PersonId};

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

/// **What a room is**, for as long as its save file exists.
///
/// Distinct from the name, and that separation is the point. A name is typed,
/// read aloud, and may be changed; an id is generated once and never changes.
/// Everything that has to still mean the same room tomorrow keys off this: the
/// save file, the seat a player holds, and the rejoin token filed against it.
/// Rename a room and every one of those survives, because none of them ever
/// knew the name.
///
/// A newtype rather than another `String`. Ids, names and codes are three
/// strings about one room, and the whole failure this prevents is passing one
/// where another was meant — which no amount of care catches and the compiler
/// catches for free.
///
/// The spelling is [`crate::server::rooms`]'s business; `net` only carries it.
/// It is always a legal [`room_name`], because it is also a filename.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoomId(pub String);

impl RoomId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RoomId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for RoomId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::borrow::Borrow<str> for RoomId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// What a room is **called**: what a player reads in the list and types to
/// reach it. Unique on a server, so that typing one is unambiguous, and not
/// what anything durable keys off — see [`RoomId`].
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
        return Err(format!("a room name is letters, digits, - and _; {bad:?} is not one of them"));
    }
    Ok(name)
}

/// The most sides a match may have.
///
/// **A side is a player, so it costs a number**, and there are fifteen of them
/// — see [`PlayerId::MAX`]. Every side needs at least one seat to sit on it,
/// so the most sides a match can actually fill is half the numbers, and that
/// is the cap: seven sides of one, which is a free-for-all with extra words,
/// and anything more useful leaves room to spare.
///
/// It used to be eight, and the reason given was **colour** — allies having to
/// read as allies meant a family of hue per side, and eight families crowd a
/// wheel that holds sixteen hues with difficulty. That argument is gone with
/// the families: a side is one number and therefore one hue, exactly as a
/// player always was.
pub const MAX_TEAMS: u8 = PlayerId::MAX / 2;

/// The fewest. One side is a solo match with extra words.
pub const MIN_TEAMS: u8 = 2;

/// The longest a side may be called, for the same reason a room name is
/// bounded: it has to fit in a lobby row and in a log line.
pub const TEAM_NAME_MAX: usize = 16;

/// What a side is called before anybody names it. Numbered rather than given
/// colours as names, because the colour is already on screen beside it and a
/// team called "Blue" that is drawn green is worse than one called "Team 2".
pub fn default_team_name(ordinal: u8) -> String {
    format!("Team {ordinal}")
}

/// Normalise a side's name, or say why it is not one.
///
/// Looser than [`room_name`] on purpose: a side name is read and never used as
/// a filename or an identifier, so it may have spaces and capitals in it. What
/// it may not have is a length that breaks a lobby row, or control characters.
pub fn team_name(raw: &str) -> Result<String, String> {
    let name: String = raw.trim().chars().filter(|c| !c.is_control()).collect();
    if name.chars().count() > TEAM_NAME_MAX {
        return Err(format!("a side's name is at most {TEAM_NAME_MAX} characters"));
    }
    Ok(name)
}

/// A side, as a lobby needs to show it.
///
/// **The id is a [`PlayerId`] because a side is one.** Everybody on a side
/// places cells carrying that number, spends that number's purse and stands on
/// that number's ground, so "who is allied with whom" is not a question the
/// rules ever have to ask: two allies *are* the same player, and `==` answers
/// it the way it did before there were sides at all.
///
/// What used to be here was a `Sides` array mapping every player to a team
/// number, copied onto the wire, and an `allied()` call threaded through
/// placement, pricing, spawning, scoring and colour. Every one of those is a
/// plain comparison again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: PlayerId,
    pub name: String,
    /// **Who is at its controls**, by their own number rather than the team's.
    ///
    /// A client that joins a team keeps its own identity — its name, its
    /// token, its record — and only its *cells* become the team's. So these
    /// are the people the lobby lists under the team, and `id` is the player
    /// all of them are playing.
    pub players: Vec<PlayerId>,
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
    /// cell — which is why it costs what ten cells of life cost.
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
                .with_level(crate::sim::bits::MAX_LEVEL)
                // Placed life is ordinary life. Without this, drawing over a
                // mine's corpse would hand you a free mine -- the kind is on
                // the cell and outlives the life that carried it.
                .with_kind(Kind::NORMAL),
            Self::Mine => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::MINE),
            Self::Turret => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
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
    Paint { cells: Vec<(i32, i32)>, placement: Placement },
    /// Take a placement away at absolute cell coordinates, leaving whatever
    /// else is on those cells. Carries what to remove for the same reason
    /// `Paint` carries what to lay: the server judges an intent, and "clear
    /// this square" is a different intent from "kill the life on it".
    Erase { cells: Vec<(i32, i32)>, placement: Placement },
}

/// An action stamped with who did it and when, which is what makes replay on
/// another peer produce the same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    pub tick: Tick,
    /// **The number the cells will carry**, which in a team match is the
    /// team's and not the sender's own.
    pub player: PlayerId,
    /// **Who sent it**, which is a different question and has to be asked
    /// separately as soon as two clients can be one player.
    ///
    /// A client applies its own actions the moment it makes them and must not
    /// apply them again when they come back — a `Paint` is idempotent on the
    /// generation it was meant for and not one generation later, so laying it
    /// twice stamps the original pattern back on top of where it has got to.
    /// "Its own" used to be `player`, which was the same question until a team
    /// became a player: a teammate's action carries the team's number, so a
    /// client skipped it as something it had already predicted and **never
    /// applied it at all** — the two copies of the world then differed by
    /// everything the rest of the team did, until a checkpoint dragged the
    /// chunks back.
    ///
    /// The seat, then, because that is what a client is one of. Equal to
    /// `player` outside a match, and offline, where a client is the only thing
    /// at its own controls.
    pub seat: PlayerId,
    pub action: Action,
}

/// What a room is doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchPhase {
    /// Not a match. Steps forever, anybody may join, nobody wins.
    Open,
    /// Made and waiting. Players may join and place, and **the world does not
    /// step** — so the opening is drawn rather than raced, and somebody who
    /// joined a minute earlier has not had a minute of generations the others
    /// did not.
    Gathering,
    /// Running, from the tick it started at. Nobody else may join.
    Running { from: u64 },
    /// Decided. The world has stopped and the result stands.
    Over { winner: Option<PlayerId>, held: usize, at: u64 },
}

impl MatchPhase {
    /// Whether the world should advance.
    pub fn stepping(&self) -> bool {
        matches!(self, Self::Open | Self::Running { .. })
    }

    /// Whether somebody who is not already here may join.
    ///
    /// **No late joining.** A match is a race from a shared start, and a
    /// player arriving at generation four hundred is not in the same race:
    /// everybody else has four hundred generations of ground and they have a
    /// block. Refused rather than allowed-and-hopeless, which reads as the
    /// game being broken rather than as a rule.
    pub fn open_to_newcomers(&self) -> bool {
        matches!(self, Self::Open | Self::Gathering)
    }

    /// Whether a player may change the world.
    ///
    /// **Nothing happens before the whistle.** The same set as
    /// [`Self::stepping`] today, and a different question: a match that let
    /// people place while gathering would be fair in *generations* and unfair
    /// in **time**, since somebody who joined ten minutes early has had ten
    /// minutes to think and draw and the last to arrive has had none. Holding
    /// the tick still does not hold a clock still.
    ///
    /// So a match opens with everybody looking at the same thing, and the
    /// first thing anybody does is done against a running clock — which is a
    /// better opening than a leisurely draw, since hesitating costs
    /// generations rather than nothing.
    pub fn accepts_actions(&self) -> bool {
        matches!(self, Self::Open | Self::Running { .. })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Gathering => "gathering",
            Self::Running { .. } => "running",
            Self::Over { .. } => "over",
        }
    }
}

/// How a match is won.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Victory {
    /// Most ground when this many generations have passed.
    ///
    /// The deadline is a **tick**, not a clock. The tick is the generation and
    /// it is already what a client adopts from its `Welcome`, so a match that
    /// ends at generation N needs no clock synchronisation, cannot be
    /// lengthened by a client that pauses, and is the same instant for
    /// everybody by construction.
    Timer { generations: u64 },
    /// First to hold this many squares.
    Territory { squares: usize },
}

/// A timer match's length in generations, when whoever made it named none.
/// At four generations a second, about eight minutes.
pub const DEFAULT_TIMER: u64 = 2000;
/// A territory match's target in squares, when whoever made it named none.
pub const DEFAULT_TERRITORY: usize = 500;

impl Victory {
    pub fn describe(&self) -> String {
        match self {
            Self::Timer { generations } => {
                format!("most ground after {generations} generations")
            }
            Self::Territory { squares } => format!("first to {squares} squares"),
        }
    }
}

/// A room that now exists: how to reach it, what it is called, and — if it is
/// private — the code to hand to whoever is playing.
///
/// Three fields because they are three different things about one room, which
/// is the whole reason a room is not identified by its name. The id is what
/// `Join` carries and never changes; the name is what a player reads; the code
/// is a credential, and being separate from the id is what makes it possible
/// to change one later without the room becoming a different room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    pub id: RoomId,
    pub name: RoomName,
    /// `None` for a room anybody can find in the listing.
    pub code: Option<String>,
}

/// One room, as a menu needs to show it.
///
/// Enough to choose by and no more: which world, whether anybody is in it, and
/// whether it ends. Not the tick, not the chunk count — a room is picked on
/// what it is like to be in, and neither of those says anything about that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomInfo {
    /// What to send back to join or watch it. Never shown.
    pub id: RoomId,
    /// What to put on the screen. Never sent back.
    pub name: RoomName,
    /// Whether this room is a match and what it is doing. A room and a match
    /// are the same thing to everything else, so the one place the difference
    /// has to show is the list somebody picks from.
    pub phase: MatchPhase,
    /// How it is won, if it is a match.
    pub victory: Option<Victory>,
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
        room: Option<RoomId>,
        /// Who is asking, as against which seat they want back.
        ///
        /// A signature over the [`Challenge`] this connection was sent, so the
        /// server learns who it is talking to without ever having issued them
        /// anything and without being handed anything it could reuse
        /// elsewhere. `None` from a client with no key yet, which plays as
        /// somebody the server will not remember.
        ///
        /// Beside the token rather than instead of it, because the two answer
        /// different questions — this one is *who*, and the token is *which
        /// seat in this room* — and a person may hold a seat in several rooms
        /// at once.
        ///
        /// [`Challenge`]: ServerMessage::Challenge
        person: Option<Claim>,
    },
    /// What this player did, and when they believe it happened.
    Act(Stamped),
    /// The chunks the client now needs, because its viewport moved.
    ///
    /// **A fetch, and named for something it never quite was.** Nothing is
    /// kept: the server answers with the chunks it holds and forgets the
    /// request, because chunk *changes* reach a client as the `Step` for the
    /// generation they happened in, broadcast to the whole room. There is no
    /// push for a subscription to select from.
    ///
    /// There was an `Unsubscribe` beside this for a while, and a list on the
    /// server for it to remove from. Nothing ever read the list and no client
    /// ever sent the message.
    Subscribe { chunks: Vec<ChunkId> },
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
    /// the messages a connection with no seat may send.
    Rooms,
    /// Watch a room without taking a seat in it.
    ///
    /// A spectator is **not a player with the actions taken away**, it is a
    /// connection with a room and no `PlayerId`. That distinction is the whole
    /// design, and it is forced by two facts rather than chosen. A seat is one
    /// of fifteen — `PlayerId::MAX`, four bits in the cell — so spending one
    /// on somebody who is only watching costs a real player their place. And
    /// **no late joining is a rule about players**: somebody arriving at
    /// generation four hundred is exactly what watching is for, so the refusal
    /// has to be able to tell the two apart.
    ///
    /// Answerable without a seat, like `Join`, and it names its own room for
    /// the same reason.
    Watch { room: RoomId },
    /// Give up this seat, without closing the connection.
    ///
    /// **Leaving a world used to have no signal at all.** A client that went
    /// back to the menu kept its seat, on the reasoning that the seat is held
    /// until another `Join` takes its place — which is true of a client that
    /// then rejoins the *same* room, and false of everything else. The player
    /// stayed marked online, so the room went on counting them; and the
    /// rejoin token, which only brings you back to a player who is *not*
    /// online, found them online and issued a new player instead. Leave and
    /// come back three times and a room with one person in it says three.
    ///
    /// Not the same as the socket closing, which already frees a seat. This is
    /// for a client that is still connected and no longer playing: it wants to
    /// keep listing rooms.
    Leave,
    /// Take a side, or leave the one you are on.
    ///
    /// Only while a match is **gathering**. Changing sides mid-match would
    /// hand your ground to the people you were fighting, which is not a
    /// decision a lobby should let somebody make by accident and not one the
    /// scoring could sensibly explain.
    ///
    /// Anybody may take any side, and there is no balance check on the way in
    /// — see `Start`, which is where a lopsided match is refused. A lobby that
    /// stops you joining your friend because the sides would be uneven is a
    /// lobby that makes you argue about the order you clicked in.
    JoinTeam { team: PlayerId },
    /// Call a side something.
    ///
    /// Anybody in the match may name any side, which is the same decision the
    /// room name is: this is a game people play together, and a naming fight
    /// is a smaller problem than a permission system.
    NameTeam { team: PlayerId, name: String },
    /// Call the match off early, with the score as it stands.
    ///
    /// **Whoever started it**, which is the same person and the same reasoning
    /// as `Start`: they arranged the match, so they are the one who can say it
    /// has stopped being worth playing. The result is real and is rated —
    /// a match that ends with no result is one nobody can be held to.
    EndMatch,
    /// Give up, for this seat.
    ///
    /// A **seat** and not a number, which is the distinction a team needs: one
    /// of three walking away leaves two pairs of hands on the team. A number
    /// is out when nobody is left playing it, and a match with one number left
    /// is over.
    Forfeit,
    /// Blow the whistle on a match this connection made.
    ///
    /// Sent with no room, because it names one: the match you are **in**. A
    /// player starting a match they are not in would be starting somebody
    /// else's, and the seat is already the thing that says which room a
    /// message belongs to.
    ///
    /// Refused unless this connection is the one that made the room, which is
    /// the first thing the owner record is used for. Anybody may join a
    /// gathering match; if anybody could also start it, the person who set it
    /// up could not wait for their friends.
    Start,
    /// Make a room that is not here yet.
    ///
    /// Answerable without a seat, like `Rooms` and `Join` and for a sharper
    /// version of the same reason: it names a room that does not exist, so
    /// there is nowhere to be standing when it is sent.
    ///
    /// The fields are what `world new` and `match new` take at the console,
    /// and `victory` is the whole of the difference between them. One message
    /// rather than two, because a world and a match differ by whether there is
    /// a way to win and by nothing else, and two messages would be two
    /// vocabularies for one act.
    ///
    /// **Making a room does not put you in it.** The answer is a name; the
    /// client sends `Join` with it, which is the same `Join` the room list
    /// sends. That keeps one path into a world rather than two.
    Create {
        /// What to call it. Validated by [`room_name`] on both sides — here
        /// so the menu can refuse a name without a round trip, and again on
        /// the server because nothing a client says about a filename is
        /// trusted. The **id** is the server's to choose and is never sent
        /// here: a client naming its own identifiers is a client that can
        /// collide with one.
        name: RoomName,
        shape: WorldKind,
        /// How it is won. `None` makes a world, which is a match with no way
        /// to end.
        victory: Option<Victory>,
        /// How many sides. `None` is a free-for-all, which is what a world
        /// always is — only a match can have teams, because a team is a way
        /// of deciding a result and a world has none.
        teams: Option<u8>,
        /// Kept out of the room listing, reachable only by the code the
        /// server generates — which becomes the room's name, because a
        /// generated name is already unique and already what `Join` carries.
        /// `name` is ignored when this is set.
        private: bool,
    },
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
        /// What this client is rated on this server.
        ///
        /// Everybody starts on the same number, so this is a figure rather
        /// than an option: a client that has never been rated here is not a
        /// special case, it is somebody at the start.
        rating: i32,
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
        room: RoomId,
        /// What that room is called, for the HUD. Sent beside the id rather
        /// than looked up, because a client that has joined by code has never
        /// seen the listing and so has no name for where it is.
        name: RoomName,
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
    Rejected {
        reason: String,
    },
    /// Somebody tried to start a match and could not, and here is why.
    ///
    /// Its own variant rather than `Rejected`, which closes the door on a
    /// connection: this refusal leaves you exactly where you were, in a lobby,
    /// with a reason to read.
    NotStarted {
        reason: String,
    },
    /// Watching, and here is the world to build in order to watch it.
    ///
    /// A `Welcome` without a player: no number, no token, no value and no
    /// spawn, because a spectator has none of those and sending zeroes would
    /// have the client draw a purse and a home patch belonging to nobody.
    /// What is left is what watching actually needs — which world, and where
    /// it has got to.
    Watching {
        room: RoomId,
        name: RoomName,
        tick: Tick,
        world: WorldKind,
    },
    /// The answer to [`ClientMessage::Create`]: what the room ended up being
    /// called, or why there is not one.
    ///
    /// A `Result` rather than two variants, because it is one question with
    /// two answers and because `Rooms::create` and `Rooms::new_match` already
    /// return exactly this — the wire carries what the server already says,
    /// refusal wording included.
    ///
    /// The name is sent back rather than assumed for the reason `Welcome`
    /// sends one: [`room_name`] lowercases and trims, so what was typed and
    /// what the room is called are not always the same string, and joining the
    /// second is the only thing that works.
    Made(Result<Made, String>),
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
    Step {
        tick: Tick,
        actions: Vec<Stamped>,
    },
    /// Where a player's opening ground was laid, and whose it is.
    ///
    /// **Because a grant is a change to the world that nobody was told about.**
    /// `Welcome` carries a spawn, which is right for somebody arriving — and a
    /// match grants everybody at the whistle instead, long after every client
    /// has joined and subscribed. Their chunks do not change hands, so nothing
    /// re-fetches them, and the ground appeared for the server and for nobody
    /// else. A reload fixed it, which is what made it look like a client bug.
    ///
    /// To the whole room rather than to its subject, because knowing where
    /// everybody started is worth having and because a per-player message
    /// would need routing this does not have. A `Resync` naming the same
    /// chunks goes out beside it: this says where, and that says fetch it.
    Spawned {
        player: PlayerId,
        at: (i32, i32),
    },
    /// Somebody's rating here, and what the match just finished moved it by.
    ///
    /// Broadcast to the room rather than sent to its owner alone, because a
    /// result is a comparison and the interesting half of it is what happened
    /// to everybody else. Sent at the moment the match ends rather than left
    /// to be found on the next join: the screen somebody is looking at when it
    /// ends is the one this belongs on.
    Rated {
        who: PersonId,
        rating: i32,
        change: i32,
    },
    /// Something to sign, so a join can say who it is from.
    ///
    /// **Sent unprompted, as soon as the socket opens**, because the client
    /// cannot ask for it without first saying something and the first thing it
    /// says is the join this answers. One per connection and reusable for
    /// every join on it: within a connection a replay is the same client
    /// saying the same true thing twice, and the thing worth preventing is a
    /// *different server* replaying a signature it saw — which this stops by
    /// being that server's own nonce and not something the client chose.
    Challenge {
        nonce: String,
    },
    /// Full contents of a chunk the client does not hold. Bytes are a chunk's
    /// cells exactly as `Chunk::as_bytes` produces them.
    ChunkData {
        tick: Tick,
        chunk: ChunkId,
        cells: Vec<u8>,
    },
    /// The client's copy of these chunks is wrong; here they are again.
    Resync {
        tick: Tick,
        chunks: Vec<ChunkId>,
    },
    /// One action, the moment the server took it, rather than at the end of
    /// the generation it belongs to.
    ///
    /// **This is what makes a cell appear on somebody else's screen in a round
    /// trip instead of at the next tick.** An action is applied by the server
    /// during the step it names, and used to reach other clients only in the
    /// `Step` that announces that step is done — so a click was worth a wait
    /// of half a generation on average, which at four generations a second is
    /// 125 ms of doing nothing on a link that costs four. The client that
    /// *made* the action never waited: it predicts. This lets everybody else
    /// predict the same thing at the same tick.
    ///
    /// The `Step` still carries it. A broadcast can be dropped — `server::ws`
    /// logs `connection lagged` and carries on — so this is a shortcut and not
    /// a replacement, and a client applies whichever reaches it first and
    /// ignores the other; see `Stamped` and the skip in the client's `Step`
    /// handling.
    Acted(Stamped),
    /// What the match in this room is doing, and who is in it.
    ///
    /// Sent on joining and again whenever it changes, because a lobby is a
    /// screen that has to be right rather than eventually right: somebody
    /// looking at "waiting to start" after it has started is looking at a lie.
    ///
    /// Names as well as numbers. A lobby is the one screen where players are
    /// people rather than colours, since the whole of it is finding out who
    /// else turned up.
    Match {
        /// Who blew the whistle, once somebody has. `None` before the start,
        /// and for a match the console started — which is the operator rather
        /// than anybody in the room.
        started_by: Option<PlayerId>,
        /// The sides this match has, what they are called, and who sits on
        /// them. Empty in a free-for-all.
        ///
        /// This is also how a client learns **which number its own cells
        /// carry**: it finds its seat in a side's `seats` and plays as that
        /// side's id. A separate map of who is allied with whom used to ride
        /// here so the client could price a placement beside a teammate the
        /// way the server would; there is nothing to price differently now,
        /// because a teammate's cells are the client's own.
        teams: Vec<Team>,
        /// Whose match this is: the player who may start it.
        ///
        /// A `PlayerId` rather than a "may you start it" flag, because this
        /// message is **broadcast to the whole room** and a flag would have to
        /// be true for one recipient and false for the rest. Every client
        /// compares it with its own number and gets the right answer.
        ///
        /// `None` for a match the console made, which is the operator's and
        /// starts at the console.
        owner: Option<PlayerId>,
        /// The code that reaches this room, if it is private.
        ///
        /// Here rather than only in the `Made` reply, because the reply is
        /// seen once by the person who made it and the code is the thing they
        /// have to hand to somebody — which they do from the lobby, while
        /// waiting, which is the moment they need to be able to read it.
        code: Option<String>,
        phase: MatchPhase,
        victory: Option<Victory>,
        players: Vec<(PlayerId, String)>,
    },
    /// Who holds how much ground, most first.
    ///
    /// **From the server because a client cannot work it out.** A client holds
    /// the chunks it subscribed to, which is its own screen, so counting
    /// locally would score the view rather than the world — and on a match
    /// that is the difference between a scoreboard and a rumour.
    ///
    /// Granted ground is not counted: `HOME` never decays, so a player wiped
    /// out in the first minute would otherwise still be holding their patch at
    /// the whistle, and that is points for having turned up.
    ///
    /// Broadcast on a cadence rather than every generation. It is one pass
    /// over the world to work out, and a bar that moved four times a second
    /// would be harder to read than one that moved every couple of seconds.
    Standing {
        tick: Tick,
        held: Vec<(PlayerId, u32)>,
    },
    /// What this player actually has to spend.
    ///
    /// Sent in reply to a `Checkpoint`, which is the only regular thing a
    /// client says. Value used to be predictable from a client's own actions
    /// alone; mining made it depend on births anywhere in the world, and a
    /// client holds a viewport — so its number drifts below the server's for
    /// as long as it plays, and nothing else would ever correct it.
    Purse {
        value: i32,
    },
    /// The rooms this server has, in the order it lists them.
    ///
    /// Ordered by the server rather than sorted by the client, so two players
    /// looking at the same menu see the same list in the same order — and so
    /// the order is one thing a server can decide rather than a thing that
    /// happens.
    Rooms {
        rooms: Vec<RoomInfo>,
    },
}

/// How wide a patch of ground a player is granted when they join, in cells.
///
/// A player may only place where their own influence reaches, so somebody who
/// owned nothing could do nothing at all. The grant is what makes that wall
/// safe: a patch that never decays, with a live gradient around it, so there
/// is always somewhere to build. It is also the seed the rest spreads from.
pub const SPAWN_N: i32 = 12;

/// How much of `player`'s influence reaches this square, nought to
/// [`crate::sim::bits::MAX_LEVEL`].
///
/// Nought where the square is somebody else's or nobody's — a square carries
/// one owner, so two players' influence never sits on the same one. Which is
/// also why this is a lookup rather than a sum: the contest was settled by the
/// rule when the square was last worked out.
///
/// Unheld ground reads as nought, which is the honest answer rather than a
/// hopeful one: a client cannot know what it does not hold, and guessing would
/// let it predict a cheaper price than the server charges.
pub fn influence(world: &World, player: PlayerId, row: i32, col: i32) -> u8 {
    reach(world, player, row, col)
}

/// The same question with allies counted: how much influence **this player's
/// side** has on this square.
///
/// Territory is still contested per player — two allies keep a border between
/// their ground, and the rule in `sim` knows nothing about teams — so this is
/// a lookup plus one comparison rather than a sum. What a team changes is not
/// what a square holds; it is who that counts for.
pub fn reach(world: &World, player: PlayerId, row: i32, col: i32) -> u8 {
    world.cell_at(row, col).filter(|c| c.player() == player).map(|c| c.influence()).unwrap_or(0)
}

/// Whether `player` may put something down here.
///
/// **Only where their own influence reaches.** Placing anywhere for a multiple
/// of the price was tried and is out: it made the map somewhere you bought
/// your way into rather than somewhere you grew into, and ten times a cell is
/// no obstacle at all to anybody with a mine running. See
/// [docs/game.md#where-you-may-build].
///
/// Safe in a way the same wall was not before levels: granted ground is a
/// **source**, so a player whose life has gone out still has a patch with a
/// live gradient around it and can always build somewhere.
///
/// Territory is the owner field on dead cells, which the rule spreads outward
/// from living ones, so a player's ground grows where their life goes.
///
/// [docs/game.md#where-you-may-build]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md
pub fn may_place(world: &World, player: PlayerId, row: i32, col: i32) -> bool {
    reach(world, player, row, col) > 0
}

/// How many grants sit along one edge of a **torus's** grid. Six covers all 31
/// players a five-bit field can hold.
///
/// Only the torus needs a fixed figure. Its ground is finite and has to be
/// divided whatever the roster turns out to be, so the grid is sized for the
/// worst case and spread over what there is. An infinite world has room, and
/// sizes its grid to the players who actually turned up — see [`seat`].
const SPAWN_ACROSS: i32 = 6;

/// How much of somebody else's ground in a seat makes it not worth taking.
///
/// A bar rather than "any at all", because a seat with a few stray cells in it
/// is still a seat: `grant` claims dead ground whoever held it and steps the
/// block around anything alive, so a couple of a neighbour's squares cost
/// nothing. What this is looking for is a **country** — a seat inside
/// somebody's territory, where a new player would be unable to build without
/// paying the outside rate for every cell.
///
/// A quarter of the patch's own area, against a box that is four times that,
/// so it takes a real border to trip it.
const SPAWN_CROWDED: usize = (SPAWN_N * SPAWN_N / 4) as usize;

/// How many seats out from their own a player will look for somewhere emptier.
///
/// A bound rather than a sweep of the world: what is wanted is *near enough to
/// be in the same game and far enough to be nobody's*, and a search that ran
/// until it found perfect emptiness would put a latecomer half a map away from
/// everybody on any world that has been running a while.
const SPAWN_SEARCH: i32 = 64;

/// Which seat in the grid a player takes, as a square **spiral** out from the
/// origin.
///
/// The grid grows with the roster, which is the point. A fixed six-across grid
/// filled in reading order puts the first six players in a **line**, and a
/// line is the arrangement the layout exists to avoid: your only neighbours
/// are the two beside you, the ends can never reach each other, and the map is
/// a corridor. A spiral fills a square at every size — four players make a
/// 2x2, nine make a 3x3, sixteen a 4x4 — so however many turn up, everybody
/// has neighbours on more than one side.
///
/// A spiral rather than "lay out a grid for the players present", because a
/// seat must never move. Positions are a function of the player's number
/// alone, so a player's ground stays where it was put when the next person
/// joins, and the shape of the occupied region grows around them.
fn seat(n: i32) -> (i32, i32) {
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
fn patch_at(seat: (i32, i32)) -> (i32, i32) {
    (seat.0 * SPAWN_PITCH, seat.1 * SPAWN_PITCH)
}

/// Whether this patch is one this player has already been granted.
///
/// [`Cell::is_home`] never decays, so a patch handed out stays marked as long
/// as its owner holds it — which is what keeps a seat still. `grant` runs
/// again on every rejoin, and a spawn that moved would hand a returning player
/// a second patch somewhere else every time the world around their first one
/// changed.
fn already_granted(world: &World, (row, col): (i32, i32), player: PlayerId) -> bool {
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
fn crowding(world: &World, (row, col): (i32, i32), player: PlayerId) -> usize {
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
/// **One seat per number, and a side is a number**, so this takes a
/// [`PlayerId`] and nothing else. It used to take the whole roster's
/// allegiances as well, and map a player to their side's seat through
/// `seat_number` — which drew team ids and player numbers out of one 1..15
/// space and so seated an unaligned player 3 on top of team 3. There is no
/// mapping left to get wrong: everybody on a side already *is* that number.
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
/// is finite and has to be shared out, so the grid is spread over whatever
/// there is and **every number still gets its own square**, on a small world
/// as much as a large one.
///
/// Computed rather than searched for, so the answer never depends on what a
/// peer happens to hold. It does depend on the world's shape, which a client
/// cannot know until it is told — and that is why `Welcome` carries the spawn
/// rather than leaving the client to work it out and be wrong.
pub fn spawn_for(player: PlayerId, world: &World) -> (i32, i32) {
    let n = player.0 as i32;

    match world.size_in_cells() {
        None => {
            // Their own seat if it is still theirs, wherever the search below
            // put it last time -- a granted patch keeps its `HOME` marks, and
            // a spawn that moved would hand a returning player a second patch.
            let seats = || (0..SPAWN_SEARCH).map(|k| patch_at(seat(n - 1 + k)));
            if let Some(mine) = seats().find(|&at| already_granted(world, at, player)) {
                return mine;
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
/// **Sized by the roster first and the world second**, which is the fix for a
/// bug rather than a preference. It used to be sized by how many comfortable
/// pitches fit, and then the seat index was folded into that with `%` — so a
/// 128x128 world had four seats for fifteen numbers, players 1, 5, 9 and 13
/// all stood on one patch, and each new arrival claimed the last one's ground
/// out from under them. (`grant` claims dead ground whoever held it, and for
/// a good reason; see there. The two together meant a player could be left
/// holding the four squares under their own block.)
///
/// So this is the smallest grid that holds every number, never finer than the
/// world has whole patches for. Where there is room it is exactly what it
/// always was, because [`SPAWN_ACROSS`] still caps it; where there is not,
/// players are seated closer together rather than on top of each other. A
/// world with fewer patches than numbers cannot do even that, and
/// [`too_cramped_for_grants`] is what says so on the way in.
fn torus_grid(height: i32, width: i32) -> (i32, i32) {
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
/// **A client trusts its server about the shape of the world and should not
/// trust it about the size.** `Welcome` and `Resync` carry a `WorldKind`
/// straight off a socket, and a torus is allocated whole — so a server that
/// said `100000x100000`, by malice or by running an older build, took the
/// client's browser tab with it. Falling back to a boundless world means the
/// client goes on playing and disagrees with the server, which the checkpoint
/// will say out loud; the alternative is a page that closes itself.
pub fn sane_world(kind: crate::sim::WorldKind) -> World {
    match kind.checked() {
        Ok(kind) => kind.build(),
        Err(why) => {
            log::error!("the server named a world this client will not build ({why})");
            World::infinite_empty()
        }
    }
}

/// Every chunk a grant at this position touches, folded onto the chunks the
/// world actually has.
///
/// A patch is [`SPAWN_N`] cells and a chunk is sixteen, so a grant spans one
/// chunk at best and four at worst — and on a torus it may span four that are
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

    // **Once each.** A player coming back with a token is granted again by
    // `join_with`, and without this that hands them a fresh 12×12 patch and a
    // brand-new 2×2 block on top of whatever they had built — so disconnecting
    // and returning was a way to conjure a still life out of nothing, over and
    // over, for free.
    //
    // Asked of the world rather than remembered on the player, because the
    // mark is already there and is already the durable answer: `HOME` sits on
    // the **square**, survives the ground changing hands, and is written into
    // the save. A flag on the player would be a second copy of the same fact,
    // and one that a save from an older build would not have.
    //
    // **Which is also what makes a side share one platform**, and it costs
    // nothing to say so: a side is one number, so the second ally to arrive
    // asks about the ground the first was given and finds it already theirs.
    if already_granted(world, (row, col), player) {
        log::debug!("{player:?} is already granted at ({row}, {col}); not granting again");
        return;
    }

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
fn block_site(world: &World, player: PlayerId, row: i32, col: i32) -> Option<(i32, i32)> {
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

/// The prices themselves live with the rules, in [`crate::sim::rule`] —
/// "life costs one" is the same kind of statement as "a cell survives on two
/// or three", and somebody balancing the game should not have to look in two
/// files. This module names the actions and reads the numbers.
pub use crate::sim::{
    ICE_COST, LIFE_COST, MINE_COST, MINE_DRAIN, MINE_YIELD, RECLAIM, TURRET_COST,
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
                // Flat, wherever it is. A price that rose as influence
                // thinned went out with the shading that made it visible: a
                // cost the player cannot see is a cost they cannot play
                // around. Whether a placement is *allowed* still depends on
                // the square — `may_place` is that question, asked before
                // this one.
                placement.cost()
            })
            .sum::<i32>(),
        // What counts as "there" depends on what is being taken, since life
        // and ice are independent: removing ice from a living cell with no
        // pane on it is as much a no-op as erasing empty ground.
        Action::Erase { cells, placement } => cells
            .iter()
            .map(|&(row, col)| match world.cell_at(row, col) {
                Some(cell) if placement.remove_from(cell) == cell => 0,
                // A teammate's cell *is* your cell — one side, one number —
                // so tidying up beside an ally reclaims at your own rate
                // without anything here having to know a side exists.
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
        Stamped {
            tick: 0,
            player: PlayerId(1),
            seat: PlayerId(1),
            action: Action::Paint { cells, placement },
        }
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
            seat: PlayerId(1),
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
            world.set_cell_at(
                row,
                col,
                cell.with_player(player).with_level(crate::sim::bits::MAX_LEVEL),
            );
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
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 0)], Placement::Ice)),
            -Placement::Ice.cost()
        );
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
            seat: PlayerId(2),
            action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        apply(&mut world, &theirs);
        // Their pane, so their ground: laying over it is a change, and one
        // nobody else may make, since no influence of theirs reaches it.
        assert!(!may_place(&world, PlayerId(1), 0, 0), "not yours to build on");
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
            seat: PlayerId(1),
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
            seat: PlayerId(1),
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
            seat: PlayerId(1),
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
            seat: PlayerId(2),
            action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        apply(&mut world, &theirs);

        let mine = Stamped {
            tick: 0,
            player: PlayerId(1),
            seat: PlayerId(1),
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

    /// **Placing is confined to ground your own influence reaches**, at the
    /// placement's own price wherever that is. Both halves of the other
    /// arrangement went together: a price that rose as influence thinned, and
    /// permission to place anywhere for a multiple. Ten times a cell was no
    /// obstacle to anybody with a mine running, and a cost that varied across
    /// ground which all looks the same was one nobody could play around.
    #[test]
    fn placing_is_confined_to_ground_you_reach_and_costs_the_same_throughout() {
        let mut world = World::infinite_empty();
        let me = PlayerId(1);
        hold(&mut world, &[(0, 0)], me);

        // The middle of your ground and the thinnest edge of it: one price.
        world.set_cell_at(0, 1, Cell::DEAD.with_player(me).with_level(1));
        assert_eq!(value_delta(&world, &paint(vec![(0, 0)], Placement::Life)), -LIFE_COST);
        assert_eq!(value_delta(&world, &paint(vec![(0, 1)], Placement::Life)), -LIFE_COST);
        assert!(may_place(&world, me, 0, 0) && may_place(&world, me, 0, 1));

        // And a square nothing of yours reaches is not for sale at any price.
        assert!(!may_place(&world, me, 0, 5));
        assert_eq!(influence(&world, me, 0, 5), 0);
    }

    /// Somebody else's ground is not yours however strong their claim is:
    /// a square carries one owner, so two players' influence never sits on
    /// the same one.
    #[test]
    fn somebody_elses_influence_is_not_yours() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        hold(&mut world, &[(0, 0)], them);
        assert_eq!(influence(&world, them, 0, 0), crate::sim::bits::MAX_LEVEL);
        assert_eq!(influence(&world, me, 0, 0), 0);
        assert!(!may_place(&world, me, 0, 0));
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
                seat: them,
                action: Action::Paint { cells: vec![(0, 0)], placement: Placement::Life },
            },
        );
        let mine = Stamped {
            tick: 0,
            player: PlayerId(1),
            seat: PlayerId(1),
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
        assert!(may_place(&world, second, row, col));
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
        world.set_cell_at(
            middle.0,
            middle.1 + 1,
            Cell::DEAD.with_ice(true).with_player(PlayerId(1)),
        );

        grant(&mut world, second);

        let theirs = world.cell_at(middle.0, middle.1).unwrap();
        assert!(theirs.is_alive() && theirs.player() == PlayerId(1), "their life is untouched");
        let pane = world.cell_at(middle.0, middle.1 + 1).unwrap();
        assert!(pane.is_ice() && pane.player() == PlayerId(1), "and their pane");

        // The block went somewhere else in the patch rather than nowhere.
        let alive: Vec<(i32, i32)> = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| {
                world.cell_at(r, c).unwrap().player() == second
                    && world.cell_at(r, c).unwrap().is_alive()
            })
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

    /// The mark is on the **square**, not on what is standing on it, so the
    /// block does not rub out the `HOME` under its own four cells — which
    /// would leave the middle of a granted patch decaying like ordinary ground
    /// once the block died, with the ring around it permanent.
    #[test]
    fn the_block_stands_on_home_ground_like_the_rest_of_the_patch() {
        let mut world = World::infinite_empty();
        let me = PlayerId(1);
        grant(&mut world, me);
        let (row, col) = spawn_for(me, &world);

        let mut live = 0;
        for r in row..row + SPAWN_N {
            for c in col..col + SPAWN_N {
                let cell = world.cell_at(r, c).unwrap();
                assert!(cell.is_home(), "({r}, {c}) in the patch should be home ground");
                live += cell.is_alive() as usize;
            }
        }
        assert_eq!(live, 4, "and the block is on it");
    }

    /// Neighbouring grants are a patch apart plus the gap, and the gap is in
    /// chunks — which is the unit "how far away is my neighbour" is a question
    /// about. Pinned because the spacing is the one number a player feels
    /// before they have done anything.
    #[test]
    fn neighbouring_grants_are_a_gap_apart() {
        let world = World::infinite_empty();
        let (row, col) = spawn_for(PlayerId(1), &world);
        let (next_row, next_col) = spawn_for(PlayerId(2), &world);
        assert_eq!(next_row, row, "the first two are side by side");
        assert_eq!(next_col - col, SPAWN_PITCH);

        // What is between them is the gap: the pitch less the patch they each
        // stand on.
        assert_eq!(SPAWN_PITCH - SPAWN_N, SPAWN_GAP);
        assert_eq!(SPAWN_GAP, 3 * CHUNK_N as i32, "three chunks of no-man's-land");
    }

    /// **The grid grows with the roster.** Six seats filled in reading order
    /// put the first six players in a line, and a line is the arrangement the
    /// layout exists to avoid: your only neighbours are the two beside you and
    /// the ends can never reach each other. A spiral fills a square at every
    /// size, so however many turn up everybody has neighbours on more than one
    /// side.
    #[test]
    fn the_grid_is_a_square_at_every_size() {
        let world = World::infinite_empty();
        let seats = |n: u8| -> Vec<(i32, i32)> {
            (1..=n).map(|p| spawn_for(PlayerId(p), &world)).collect()
        };

        for (players, side) in [(4u8, 2), (9, 3), (16, 4), (25, 5)] {
            let spots = seats(players);
            let span =
                |v: Vec<i32>| (v.iter().max().unwrap() - v.iter().min().unwrap()) / SPAWN_PITCH + 1;
            assert_eq!(span(spots.iter().map(|s| s.0).collect()), side, "{players} players");
            assert_eq!(span(spots.iter().map(|s| s.1).collect()), side, "{players} players");

            // Filled, not just bounded: a square with holes in it is a
            // different arrangement from a square.
            let mut distinct = spots.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(distinct.len(), players as usize, "{players} seats should be distinct");
        }
    }

    /// A seat inside somebody's country is not a seat. A latecomer is put out
    /// where there is room rather than into ground they could not build on
    /// without paying the outside rate for every cell.
    #[test]
    fn a_seat_inside_somebody_elses_country_is_given_up() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(2), PlayerId(1));
        let wanted = spawn_for(me, &world);

        // Their ground over the whole of it and well past its edges.
        for r in wanted.0 - SPAWN_N..wanted.0 + 2 * SPAWN_N {
            for c in wanted.1 - SPAWN_N..wanted.1 + 2 * SPAWN_N {
                world.set_cell_at(r, c, Cell::DEAD.with_player(them));
            }
        }

        let moved = spawn_for(me, &world);
        assert_ne!(moved, wanted, "a seat buried in their country should be given up");
        assert_eq!(crowding(&world, moved, me), 0, "and the one taken instead should be nobody's");

        // But a couple of stray cells is not a country: `grant` claims dead
        // ground whoever held it and steps the block around anything alive, so
        // a seat with a few of somebody's squares in it is still a seat.
        let mut sparse = World::infinite_empty();
        let spot = spawn_for(me, &sparse);
        sparse.set_cell_at(spot.0 + 1, spot.1 + 1, Cell::alive(them));
        sparse.set_cell_at(spot.0 + 2, spot.1 + 2, Cell::DEAD.with_player(them));
        assert_eq!(spawn_for(me, &sparse), spot, "two cells should not move anybody");
    }

    /// **Crowded means held, not inhabited.**
    ///
    /// Territory *is* the owner field on dead squares, so a seat can be
    /// entirely somebody's country with not one living cell in it — which is
    /// what most of a country looks like most of the time, since life is
    /// sparse and the ground it claimed is not. A crowding check that counted
    /// life would call that seat empty and drop a latecomer into the middle of
    /// somebody's territory, where every square is owned and they can build
    /// nothing.
    #[test]
    fn a_seat_is_crowded_by_ground_even_with_nothing_alive_on_it() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(2), PlayerId(1));
        let at = spawn_for(me, &world);

        // Their ground, at full influence, and **nothing alive anywhere**.
        for r in at.0..at.0 + SPAWN_N {
            for c in at.1..at.1 + SPAWN_N {
                world.set_cell_at(
                    r,
                    c,
                    Cell::DEAD.with_player(them).with_level(crate::sim::bits::MAX_LEVEL),
                );
            }
        }
        assert!(world.live_cells().is_empty(), "the test is about ground, not life");

        assert!(
            crowding(&world, at, me) > SPAWN_CROWDED,
            "a seat full of somebody's territory read as empty because nothing stood on it"
        );
        assert_ne!(spawn_for(me, &world), at, "and so nobody was moved off it");

        // The converse, so this is a test about the owner field rather than
        // about any ground at all: the player's *own* territory does not
        // crowd them out of their own seat.
        let mut mine = World::infinite_empty();
        let seat = spawn_for(me, &mine);
        for r in seat.0..seat.0 + SPAWN_N {
            for c in seat.1..seat.1 + SPAWN_N {
                mine.set_cell_at(
                    r,
                    c,
                    Cell::DEAD.with_player(me).with_level(crate::sim::bits::MAX_LEVEL),
                );
            }
        }
        assert_eq!(crowding(&mine, seat, me), 0, "your own ground is not a crowd");
        assert_eq!(spawn_for(me, &mine), seat);
    }

    /// The other half of giving a crowded seat up: the cure must not be worse.
    ///
    /// An infinite plane has unlimited emptiness, so a search that simply
    /// walked until it found quiet would put a latecomer so far from everybody
    /// that they are alone in a multiplayer game. The seats are a **bounded**
    /// spiral — `SPAWN_SEARCH` of them, `SPAWN_PITCH` apart — so however
    /// crowded the world is, the furthest anybody lands is a distance that can
    /// be written down.
    #[test]
    fn a_crowded_world_does_not_fling_anybody_into_nowhere() {
        let mut world = World::infinite_empty();
        let me = PlayerId(2);
        let them = PlayerId(1);

        // Everything the search can reach, owned by somebody else. There is
        // no uncrowded seat at all, which is the worst case: it has to settle
        // for the emptiest rather than return one.
        let reach = SPAWN_PITCH * 8;
        for r in -reach..reach {
            for c in -reach..reach {
                world.set_cell_at(r, c, Cell::DEAD.with_player(them));
            }
        }

        let at = spawn_for(me, &world);
        let bound = SPAWN_PITCH * SPAWN_SEARCH;
        assert!(
            at.0.abs() <= bound && at.1.abs() <= bound,
            "spawned at {at:?}, beyond the {bound} the spiral can reach"
        );
        // And it is still a seat somebody could be granted: `grant` claims
        // dead ground whoever held it, so a crowded patch is buildable even
        // though it was not empty.
        grant(&mut world, me);
        assert!(may_place(&world, me, at.0, at.1), "granted ground nobody can build on");
    }

    /// A granted patch keeps its `HOME` marks and they never decay, so a
    /// spawn stays put however the world around it changes — `grant` runs
    /// again on every rejoin, and a seat that wandered would hand a returning
    /// player a second patch every time.
    #[test]
    fn no_two_grants_on_a_torus_are_neighbours() {
        // How far apart two positions are on a ring, which is the only measure
        // that means anything on a world you can walk off the edge of.
        let apart = |a: i32, b: i32, extent: i32| {
            let d = (a - b).abs();
            d.min(extent - d)
        };
        for chunks in [8, 12, 16, 24, 40] {
            let extent = chunks * CHUNK_N as i32;
            let world = World::toroidal(chunks, chunks);
            assert!(!too_cramped_for_grants(&world), "{chunks} chunks was called cramped");

            // **Every number, not as many as the grid felt like seating.**
            // This used to ask only about the seats a comfortable pitch fit,
            // and quietly accept that the numbers past that shared a patch.
            let spawns: Vec<(i32, i32)> =
                (1..=PlayerId::MAX).map(|n| spawn_for(PlayerId(n), &world)).collect();
            for (i, a) in spawns.iter().enumerate() {
                for b in &spawns[i + 1..] {
                    let (down, along) = (apart(a.0, b.0, extent), apart(a.1, b.1, extent));
                    // **Patches never overlap.** Two of them do overlap only
                    // if they are close on *both* axes, so one axis clearing a
                    // patch's width is the whole of it -- neighbours in a row
                    // share a row by construction.
                    assert!(
                        down >= SPAWN_N || along >= SPAWN_N,
                        "on a {chunks}-chunk torus {a:?} and {b:?} are {down} and {along} apart, \
                         and a patch is {SPAWN_N} wide"
                    );
                }
            }
        }
    }

    /// **A small torus seats everybody closer together rather than seating
    /// some of them on top of each other**, which is the trade this makes and
    /// the reverse of the one it used to.
    ///
    /// A 128-cell torus passes the cramped check and has room for four seats
    /// at a comfortable pitch and fifteen at a tight one. It used to take the
    /// four, fold the other eleven numbers onto them with `%`, and hand each
    /// new arrival a patch somebody was already standing on — and because
    /// `grant` claims dead ground whoever held it, the newcomer took all but
    /// the four squares under the last one's block. Players 1, 5, 9 and 13 all
    /// sat at (0, 64).
    #[test]
    fn a_small_torus_seats_everybody_closer_rather_than_twice_over() {
        let world = World::toroidal(8, 8);
        let seats: std::collections::HashSet<(i32, i32)> =
            (1..=PlayerId::MAX).map(|n| spawn_for(PlayerId(n), &world)).collect();
        assert_eq!(seats.len(), PlayerId::MAX as usize, "two numbers shared a patch");

        // And where there *is* room, the comfortable spacing is untouched.
        let roomy = World::toroidal(40, 40);
        let extent = 40 * CHUNK_N as i32;
        let (first, second) = (spawn_for(PlayerId(1), &roomy), spawn_for(PlayerId(2), &roomy));
        let along = (first.1 - second.1).abs().min(extent - (first.1 - second.1).abs());
        let down = (first.0 - second.0).abs().min(extent - (first.0 - second.0).abs());
        assert!(down.max(along) >= SPAWN_PITCH, "{first:?} and {second:?} on a roomy torus");
    }

    #[test]
    fn a_granted_seat_does_not_wander() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(2), PlayerId(1));
        let home = spawn_for(me, &world);
        grant(&mut world, me);

        // Their country arrives afterwards, all around and over the top of it.
        for r in home.0 - SPAWN_N..home.0 + 2 * SPAWN_N {
            for c in home.1 - SPAWN_N..home.1 + 2 * SPAWN_N {
                let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
                if !cell.is_home() {
                    world.set_cell_at(r, c, cell.with_player(them));
                }
            }
        }

        assert_eq!(spawn_for(me, &world), home, "their home is where it was granted");
    }

    /// Ground nobody holds prices as empty, which is what `apply` writes into
    /// it. The two must agree or a client would be charged for one thing and
    /// given another.
    #[test]
    fn unheld_ground_prices_as_empty() {
        let world = World::infinite_empty();
        let far = vec![(100_000, 100_000)];
        assert!(world.cell_at(far[0].0, far[0].1).is_none());
        // Nothing of anybody's reaches it, so nobody may build there. A
        // client cannot know what it does not hold, and reading unheld ground
        // as its own would predict a placement the server refuses.
        assert!(!may_place(&world, PlayerId(1), far[0].0, far[0].1));
        assert_eq!(influence(&world, PlayerId(1), far[0].0, far[0].1), 0);
    }
}
