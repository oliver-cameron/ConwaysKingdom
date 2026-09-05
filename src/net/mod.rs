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
pub mod jsonl;
pub mod keep;
pub mod kept;
#[cfg(not(target_arch = "wasm32"))]
pub mod link;
#[cfg(target_arch = "wasm32")]
pub mod link_web;
pub mod spawn;
#[cfg(target_arch = "wasm32")]
pub use link_web as link;

pub use auth::{PersonId, Secret};

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::sim::CHUNK_N;
use crate::sim::{Cell, Coord, Kind, PlayerId, World, WorldKind};

/// A chunk is identified by where it is. There is no separate id to allocate,
/// keep unique, or reconcile after a reconnect — two peers naming the same
/// coordinate mean the same chunk. On a toroidal world, fold with
/// [`crate::sim::World::canonical`] before comparing.
pub type ChunkId = Coord;

/// Generation number. The unit of lockstep: an action is applied *at* a tick,
/// so both sides apply it at the same point in the sequence.
pub type Tick = u64;

/// **What a room is**, for as long as its save file exists — as against what
/// it is *called*, which is typed, read aloud and may change.
///
/// A newtype because ids, names and codes are three strings about one room,
/// and passing one where another was meant is the failure the compiler can
/// catch for free. Always a legal [`room_name`], because it is also a
/// filename; the spelling is [`crate::server::rooms`]'s business.
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

/// This room's number, for the dice its world rolls.
///
/// Derived rather than sent: both peers know the room already, so a field
/// would be a third thing about it that could be wrong. From the id and not
/// the name, because renaming a room must not re-roll its dice.
///
/// FNV-1a for the reason [`crate::sim::World::digest`] gives: `DefaultHasher`
/// is not stable across Rust versions, so two builds would disagree about
/// every contested birth.
pub fn world_seed(room: &RoomId) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in room.as_str().as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// What a room is **called**: what a player reads in the list and types to
/// reach it. Unique on a server, so that typing one is unambiguous, and not
/// what anything durable keys off — see [`RoomId`].
pub type RoomName = String;

/// **A party**, for as long as anybody is in it. Issued by the server like a
/// [`RoomId`] and never shown; the name beside it is what people read.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PartyId(pub String);

impl PartyId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PartyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The room a client that names none is put in.
pub const DEFAULT_ROOM: &str = "main";

/// What a solitary world is called. Not asked for — a world nobody else can
/// reach needs none of a room's things — but it has one anyway, so the HUD has
/// something to read, [`world_seed`] has something to make dice from, and a
/// saved world has something to be filed under. One name, so one slot.
pub const SOLO_ROOM: &str = "solo";

/// The longest a room name may be. Short enough to read in a log line and in
/// the HUD, and long enough to be a word rather than a code.
pub const ROOM_NAME_MAX: usize = 24;

/// Normalise a room name, or say why it is not one.
///
/// Lowercased and narrow because **the name is also the save file's name**:
/// a case-insensitive filesystem would make `Lobby` and `lobby` two rooms on
/// one machine and one on another, and `../` in a name escapes the rooms
/// directory. Checked here as well as on the server so a client can refuse
/// the same name for the same reason.
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

/// What a challenge is played to, in squares held.
///
/// **Territory rather than a timer**, because a challenge is somebody asking
/// for a game rather than for an appointment: a match that ends when one of
/// them has built something is over when it is over, and one that ends at a
/// clock needs both of them to have agreed how long they have.
///
/// Five hundred is a few minutes of two people building at four generations a
/// second, and is what the make-a-world form already offers as its middle
/// answer.
pub const CHALLENGE_SQUARES: usize = 500;

/// The most sides a match may have.
///
/// **Every number there is**, which is what a side costs: a side *is* a
/// player, so making one takes a `PlayerId` out of the same pool the seats
/// come from — see [`PlayerId::MAX`].
///
/// This was `MAX / 2`, on the reasoning that every side also needs somebody
/// sitting on it, so half the numbers is the real ceiling. That arithmetic is
/// right and it was the wrong place to spend it. A form that refuses eight
/// teams is answering a question about *this* match — how many people are
/// coming — that it cannot know when the match is being described, and the
/// server already asks it at the moment it can be answered: `teams_are_fair`
/// will not blow the whistle on a side nobody is on.
///
/// So the cap here is the only thing that is true at this point, which is that
/// there are fifteen numbers. What you get for asking for more sides than you
/// have players is a match that will not start, and a refusal naming the empty
/// side — which is a better answer than a form saying "between 2 and 7" to
/// somebody who has eight friends.
pub const MAX_TEAMS: u8 = PlayerId::MAX;

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

/// The longest a player may call themselves, for the same reason a side's name
/// is bounded: it has to fit a lobby row, a standings row and a log line.
pub const PLAYER_NAME_MAX: usize = 24;

/// Clamp a name somebody joined under to something a server can hold.
///
/// **Clamped rather than refused**, which is where this differs from
/// [`room_name`] and [`team_name`]. Those two name a thing that has to be
/// found again, so a client that gets one wrong is told and asks differently.
/// A player's name is a label on a row: there is nothing to look it up by,
/// nobody is helped by being kept out of a game over it, and a `Join` is not a
/// message anybody retries.
///
/// **A length, and not a defence.** It was both: the stores were tab separated
/// and a name was the last field of a profile row, so a name carrying a newline
/// wrote a second row that read back as somebody else. The stores are
/// [`jsonl`] now and a value cannot reach the separator at all, which is the
/// difference between a format that is safe and one that every future field has
/// to remember to keep safe.
///
/// What is left is the honest job: a row has to fit a lobby, a standings line
/// and a log line, and "a string a client chose, of any length" fits none of
/// them. Control characters go because they do not draw, not because they
/// break anything.
pub fn player_name(raw: &str) -> String {
    raw.trim().chars().filter(|c| !c.is_control()).take(PLAYER_NAME_MAX).collect()
}

/// A side, as a lobby needs to show it.
///
/// **The id is a [`PlayerId`] because a side is one.** Everybody on it
/// places cells carrying that number, so "who is allied with whom" is never
/// a question the rules ask: two allies *are* the same player.
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

/// How much ground each player holds, by their number.
///
/// Here rather than on the server because an offline client is its own server
/// and owns the whole world; a connected one holds a viewport and takes the
/// server's figure instead. `granted` is [`Granted`].
pub fn holdings(world: &World, granted: Granted) -> [usize; PlayerId::COUNT] {
    let mut held = [0usize; PlayerId::COUNT];
    for (_, chunk) in world.stored() {
        for row in 0..crate::sim::CHUNK_N {
            for col in 0..crate::sim::CHUNK_N {
                let cell = chunk[(row, col)];
                if granted == Granted::Excluded && cell.is_home() {
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

/// Whether a count includes the patch everybody is granted on joining.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Granted {
    /// A score: a grant is not an achievement.
    Excluded,
    /// Ground: a grant is very much ground.
    Counted,
}

/// Everybody's holdings, most first, as the standings report them.
///
/// Shared so an offline client and a server produce the same rows in the same
/// order — the ordering is part of it, since rows that swapped places at a tie
/// would jump about on one and not the other.
pub fn standings(world: &World) -> Vec<Holding> {
    let scored = holdings(world, Granted::Excluded);
    let held = holdings(world, Granted::Counted);
    let mut rows: Vec<Holding> = held
        .iter()
        .enumerate()
        .skip(1)
        .filter(|&(_, &n)| n > 0)
        .map(|(id, &n)| Holding {
            who: PlayerId(id as u8),
            score: scored[id] as u32,
            ground: n as u32,
        })
        .collect();
    // Most first, and by number where two hold the same, so the order is the
    // same on every peer and rows do not swap places at a tie.
    rows.sort_by(|a, b| b.score.cmp(&a.score).then(a.who.cmp(&b.who)));
    rows
}

/// What one player holds, as the standings report it.
///
/// **Two numbers, because they answer two questions.** `score` is what a match
/// is won on and leaves out the patch everybody is granted on joining — a
/// grant is not an achievement. `ground` is every square they hold, which is
/// what a player is *shown*: a figure reading nought beside a screen of
/// squares plainly theirs is wrong however defensible the scoring rule is, and
/// it read nought for as long as somebody built only inside their own patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
    pub who: PlayerId,
    pub score: u32,
    pub ground: u32,
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
    /// factory's children are factories, and since a birth picks one of three parents
    /// at random the kind spreads through a mixed population rather than being
    /// handed down whole. What you are paying for is a **lineage**, not a
    /// cell — which is why it costs what ten cells of life cost.
    Factory,
    /// A living cell that claims ground at range, and a dead one running that
    /// backwards. Priced by the **emplacement**: a turret is not inherited, so
    /// it is bought once per cell, and one dies of loneliness in a generation —
    /// the smallest that works is four in a block.
    Turret,
    /// A living cell that counts down and then scrambles the ground around it.
    ///
    /// Priced by the **emplacement**, like a turret and for the same two
    /// reasons: it is not inherited, so it is bought once per cell, and one on
    /// its own dies of loneliness before its fuse burns. What it really costs
    /// is the pattern you have to build to keep it alive that long.
    Dynamite,
    /// A living cell that makes the ground round it step twice a generation.
    ///
    /// Priced by the **emplacement**, as a turret is and for the same two
    /// reasons: it is not inherited, so it is bought once per cell, and one on
    /// its own dies of loneliness in a generation. What it buys is a disc at
    /// twice the clock, with a ring at its edge that sees every other step.
    Overclock,
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
            // **The age goes with the kind, both ways.**
            //
            // Placed life is ordinary life: without `with_kind`, drawing over a
            // factory's corpse would hand you a free factory, since the kind is on
            // the cell and outlives the life that carried it. And without
            // `with_age` it hands you the *wear* instead — a factory's age is how
            // depleted its square is, and a plain cell has no such thing, so it
            // arrives as a number nothing will ever clear.
            //
            // It showed as an invisible cell. `Kind::NORMAL` is `Ages::Never`,
            // so the sheet has art for it at age nought and blank rows beneath;
            // drawing a cell over a worn factory put a live normal cell at age
            // five, which points at one of those blanks.
            Self::Life => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::NORMAL)
                .with_age(0),
            // **From nought**, as a dynamite is. A factory's age only resets when
            // it decays, so laying a fresh one over a corpse that had been
            // rotting would buy a factory already part way through its own.
            Self::Factory => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::FACTORY)
                .with_age(0),
            // `Ages::Never` too, so the same reasoning and the same reset.
            Self::Turret => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::TURRET)
                .with_age(0),
            // The same, and here it is a fuse somebody else's cell had
            // already half burnt.
            Self::Dynamite => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::DYNAMITE)
                .with_age(0),
            // `Ages::Never`, as a turret is.
            Self::Overclock => existing
                .with_alive(true)
                .with_player(player)
                .with_level(crate::sim::bits::MAX_LEVEL)
                .with_kind(Kind::OVERCLOCK)
                .with_age(0),
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
    /// **Held, not present.** Life and a factory are taken away by clearing the
    /// same bit, so "would removing it change anything" made a factory held over
    /// your own life read as already there and the click killed the cell
    /// instead of converting it. The owner is no part of it: somebody else's
    /// life is still life, priced at [`RECLAIM`].
    pub fn is_on(self, existing: Cell) -> bool {
        match self {
            Self::Life => existing.is_alive() && existing.kind() == Kind::NORMAL,
            Self::Factory => existing.is_alive() && existing.kind() == Kind::FACTORY,
            Self::Turret => existing.is_alive() && existing.kind() == Kind::TURRET,
            Self::Dynamite => existing.is_alive() && existing.kind() == Kind::DYNAMITE,
            Self::Overclock => existing.is_alive() && existing.kind() == Kind::OVERCLOCK,
            Self::Ice => existing.is_ice(),
        }
    }

    /// What one of these costs to put down. Life is cheap because a pencil
    /// lays tens of cells in a gesture; ice is dear because a wall that costs
    /// what a cell costs is not a decision. Placing and reclaiming life are
    /// both one, so rearranging your own board is free — the sink is
    /// mortality, since a cell that dies cannot be reclaimed. See
    /// [docs/game.md].
    pub const fn cost(self) -> i32 {
        match self {
            Self::Life => LIFE_COST,
            Self::Ice => ICE_COST,
            Self::Factory => FACTORY_COST,
            Self::Turret => TURRET_COST,
            Self::Dynamite => DYNAMITE_COST,
            Self::Overclock => OVERCLOCK_COST,
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
            // A factory is a live cell like any other, so taking it back is
            // taking back the life -- and at the reclaim rate, so a misplaced
            // one costs what it cost minus one. That is the commitment a factory
            // should carry without being a trap.
            Self::Factory => true,
            // As with a factory: it is a live cell, so taking it back is taking
            // back the life. A misplaced turret is dear, and a turret you
            // cannot pick up would make the fourth click of an emplacement a
            // trap rather than a decision.
            Self::Turret => true,
            // The same again, and it matters more here: a dynamite is a fuse
            // you have already lit, so being unable to put one out would make
            // a misplaced one a countdown you can only watch.
            Self::Dynamite => true,
            // A turret's case again: dear, and a live cell, so taking one
            // back is taking back the life.
            Self::Overclock => true,
            Self::Ice => false,
        }
    }

    /// Take this away and leave everything else alone — the inverse of
    /// [`Self::apply_to`]. Life and ice are independent, so clearing the cell
    /// outright would destroy a pane the player did not aim at. The owner
    /// stays, as it does when a cell dies of the rule.
    pub fn remove_from(self, existing: Cell) -> Cell {
        match self {
            // The kind stays on the corpse, as it does when a cell dies of the
            // rule: what is being taken back is the life.
            Self::Life | Self::Factory | Self::Turret | Self::Overclock => {
                existing.with_alive(false)
            }
            // And the fuse goes out with it. A corpse that kept its age would
            // be a bomb somebody could bring back to life at one generation
            // from going off.
            Self::Dynamite => existing.with_alive(false).with_age(0),
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

/// The most cells one action may name.
///
/// **Because the work is unbounded and the message is not.** A coordinate
/// pair is two bytes, so a frame the transport accepts holds tens of
/// millions — and an `Erase` over ground nobody holds prices at nothing, so
/// affordability is no bound. Every one is priced, applied, cloned into an
/// `Acted` and applied again by every client in the room.
pub const MOST_CELLS_AT_ONCE: usize = 4096;

impl Action {
    /// The cells this names, whichever it is.
    pub fn cells(&self) -> &[(i32, i32)] {
        match self {
            Self::Paint { cells, .. } | Self::Erase { cells, .. } => cells,
        }
    }
}

/// An action stamped with who did it and when, which is what makes replay on
/// another peer produce the same result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamped {
    pub tick: Tick,
    /// **The number the cells will carry**, which in a team match is the
    /// team's and not the sender's own.
    pub player: PlayerId,
    /// **Who sent it**, which is a different question once two clients can be
    /// one player: a client skips its own actions coming back, and skipping by
    /// `player` meant skipping a teammate's and never applying it. Equal to
    /// `player` outside a match.
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
    /// [`Self::stepping`] today and a different question: placing while
    /// gathering would be fair in generations and unfair in *time*, since
    /// holding the tick still does not hold a clock still.
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

/// **What the game does in this room**, as against what a match does.
///
/// Three switches a laboratory takes off and every other room leaves alone.
/// They belong to the room rather than to the client because a client that
/// decided for itself would predict placements the server refuses and resync
/// every time it drew — which is why a laboratory could only ever be offline,
/// and why it is a room now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// The world is stopped.
    ///
    /// The one control an experiment has that a world does not: everywhere
    /// else the server steps on its own clock and there is nothing to press.
    pub paused: bool,
    /// Placing is not confined to ground the player's influence reaches.
    pub place_anywhere: bool,
    /// Placing costs nothing.
    pub place_free: bool,
    /// **How fast the world runs, in generations a minute.**
    ///
    /// A rate rather than a span, and the unit is the point: 250 milliseconds
    /// a generation is four a second is **240 a minute**, which is a number
    /// people have intuition about and can halve and double meaningfully.
    /// Milliseconds were the same fact spelled so that nobody could tell 250
    /// was a round number.
    ///
    /// **Safe to change while a world is running**, which almost nothing here
    /// is. The dice are seeded by `seed::generation_seed(world, generation)` —
    /// the generation *number*, never a clock — so how fast generations arrive
    /// changes nothing any peer computes. Rate is not a rule.
    ///
    /// Zero would be [`Self::paused`] said twice, so it is not allowed: pausing
    /// keeps the rate it will resume at, and a stopped world is a state rather
    /// than a speed.
    #[serde(default = "default_bpm")]
    pub bpm: u16,
    /// Whether a client in this room may change the four above. True in an
    /// experiment and false everywhere else, which is the whole difference
    /// between a laboratory and a world.
    pub laboratory: bool,
}

/// **By hand, because a derived one would be stopped.** Every other field here
/// is a `false` that means "an ordinary world", and a rate's zero is not the
/// ordinary rate — it is no rate at all, which [`Rules::bpm`] does not allow
/// and which `Rules::default()` is used far too widely to be allowed to mean.
impl Default for Rules {
    fn default() -> Self {
        Self {
            paused: false,
            place_anywhere: false,
            place_free: false,
            bpm: DEFAULT_BPM,
            laboratory: false,
        }
    }
}

/// What a world runs at when nobody says: four generations a second.
///
/// The number the game was designed and balanced at, said in the unit the
/// control uses — see [`Rules::bpm`].
pub const DEFAULT_BPM: u16 = 240;

/// The slowest and fastest a room may be asked to run.
///
/// **A floor because a stopped world is `paused`**, and one generation a
/// minute is already "watch it think". The ceiling is what a server can
/// actually step: `examples/frametime` measures a generation at about 41
/// nanoseconds a cell, so four million cells is a sixth of a second and 360 a
/// minute is where a full torus stops keeping up. A client alone can go
/// faster than a server can serve, and one number for both is one fewer thing
/// to disagree about.
pub const SLOWEST_BPM: u16 = 1;
pub const FASTEST_BPM: u16 = 360;

fn default_bpm() -> u16 {
    DEFAULT_BPM
}

impl Rules {
    /// How long one generation lasts, in seconds.
    ///
    /// The conversion, in one place, so the client's clock and the server's
    /// cannot disagree about what a rate means.
    pub fn generation_span(&self) -> f32 {
        60.0 / self.bpm.clamp(SLOWEST_BPM, FASTEST_BPM) as f32
    }

    /// A rate a room may actually be set to, or why not.
    pub fn rate(asked: u16) -> Result<u16, String> {
        if !(SLOWEST_BPM..=FASTEST_BPM).contains(&asked) {
            return Err(format!(
                "a world runs between {SLOWEST_BPM} and {FASTEST_BPM} generations a minute"
            ));
        }
        Ok(asked)
    }
}

/// What kind of room this is: the one question the make-a-world form asks, and
/// the one distinction the room list has to show.
///
/// Derived rather than stored, because it is two facts the room already keeps
/// for other reasons — whether there is a way to win, and whether the rules
/// are yours to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoomKind {
    /// Steps forever, anybody may join, nobody wins.
    World,
    /// Won somehow. Gathers, runs, ends.
    Match,
    /// A laboratory: the clock is a control, and the game's two placing rules
    /// come off.
    Experiment,
}

impl RoomKind {
    pub fn of(victory: Option<Victory>, rules: &Rules) -> Self {
        match (victory, rules.laboratory) {
            (Some(_), _) => Self::Match,
            (None, true) => Self::Experiment,
            (None, false) => Self::World,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Match => "match",
            Self::Experiment => "experiment",
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

/// **Somebody in a room**, as everybody else in it is told.
///
/// Three things rather than the `(PlayerId, String)` this was: a number is a
/// seat in one world and a name is what they typed, and neither of those is
/// enough to look a [`Profile`] up — which is what a name in a lobby is *for*
/// being clicked on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub id: PlayerId,
    pub name: String,
    /// Who they are on this server. `None` for a client with no key, which is
    /// somebody the server will not remember and so has nothing to say about.
    pub who: Option<PersonId>,
    /// Whether the server is playing this seat — see `server::bot`. The one
    /// thing a client is told about a bot and all it needs: which rows get a
    /// word after the name and a control to take the seat away.
    pub bot: bool,
}

impl Seat {
    /// Name and fingerprint, or the bare name for somebody with no key —
    /// there is nothing to disambiguate them by, and a dangling separator
    /// would read as a fingerprint that failed to load.
    pub fn label(&self) -> String {
        match &self.who {
            Some(who) => label(&self.name, who),
            None => self.name.clone(),
        }
    }
}

/// How hard a bot plays.
///
/// Two dials rather than an algorithm — how often it acts, and what it will
/// do — and both are the server's; see `server::bot`. This is the word a
/// lobby picks and the wire carries. Spelled in lowercase where it is text —
/// the API and its JSON — which costs the socket nothing, since postcard
/// writes an index and never a name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Level {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];

    pub fn name(&self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }

    /// Read one as typed at a console or in a request.
    pub fn parse(word: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|l| l.name() == word.to_ascii_lowercase())
            .ok_or_else(|| format!("no level \"{word}\"; try easy, normal or hard"))
    }
}

/// **What a server can vouch for about somebody**, which is only what
/// happened on it.
///
/// The line [player profiles] draws: anything another player is shown has to
/// be the server's, because client state is self-asserted and a rating you
/// keep is a rating you can type. So a name is the one thing here a client
/// chose, and it is shown as a name rather than as a fact — the fingerprint
/// beside it is the part that cannot be picked.
///
/// [player profiles]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#player-profiles
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub who: PersonId,
    /// What they last joined under here.
    pub name: String,
    pub rating: i32,
    /// Whether that number has been earned yet. An Elo from a fixed start
    /// means nothing until it has moved, so it is marked until it has — see
    /// `server::rating::PROVISIONAL_AFTER`.
    pub provisional: bool,
    /// Matches this server has settled for them.
    pub games: u32,
    /// **Where that rating has been**, oldest first, one point per settled
    /// match. A number on its own says nothing — only differences do — and the
    /// most useful comparison is with yourself a month ago.
    pub history: Vec<i32>,
    /// The most ground they have held at once, in squares.
    pub best: usize,
}

impl Profile {
    /// **Name and fingerprint**, which is how two players who picked one name
    /// are told apart.
    ///
    /// Nothing stops two people calling themselves alice, and an account
    /// system is what this game is deliberately not — so the name keeps its
    /// spelling and the id says which alice. Four characters is enough for a
    /// room of fifteen and is not meant to be enough for the world; the whole
    /// id is what identifies anybody absolutely.
    pub fn label(&self) -> String {
        label(&self.name, &self.who)
    }
}

/// The same, for anywhere a name and an id are to hand without a whole
/// profile behind them — a lobby row, a standings bar.
pub fn label(name: &str, who: &PersonId) -> String {
    format!("{name}·{}", who.short())
}

/// **What a room's match is doing, and who is in it.**
///
/// A struct rather than seven fields inline in [`ServerMessage::Match`],
/// because the client held a copy with the same seven fields in the same
/// meanings — one fact written down twice, and the two would have stopped
/// agreeing the first time either gained a field. The client holds this now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lobby {
    pub phase: MatchPhase,
    pub victory: Option<Victory>,
    /// Who is in the room. A lobby is the one screen where players are people
    /// rather than colours, so this is the one place a seat carries a name and
    /// a person as well as a number.
    pub players: Vec<Seat>,
    /// The sides this match has and who sits on them. Empty in a free-for-all.
    /// Also how a client learns **which number its own cells carry**: it finds
    /// its seat in a side and plays as that side's id.
    pub teams: Vec<Team>,
    /// Whose match this is: the player who may start it. A `PlayerId` rather
    /// than a flag, because this is broadcast and a flag would have to be true
    /// for one recipient and false for the rest. `None` for one the console
    /// made, which starts at the console.
    pub owner: Option<PlayerId>,
    /// Who blew the whistle, once somebody has. `None` before the start, and
    /// for a match the console started.
    pub started_by: Option<PlayerId>,
    /// The code that reaches this room, if it is private. Here as well as in
    /// the `Made` reply, because the reply is seen once and the code is what
    /// somebody reads out from the lobby while waiting.
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
    /// What the game is doing in it. With [`Self::victory`] this is what
    /// [`RoomKind`] reads, which is the one thing somebody picking a room off
    /// this list is actually choosing between.
    pub rules: Rules,
    /// Players connected right now, not players the room has ever seen. The
    /// second number is the one the world remembers and the wrong one to
    /// choose a room by.
    pub players: u32,
    pub world: WorldKind,
    /// Whose it is, by key, when a keyed player made it. As public as the id
    /// a lobby shows beside a name, and what lets a menu offer to close your
    /// own rooms and nobody else's. `None` for a room the console made and
    /// for one whose maker had no key.
    pub owner: Option<PersonId>,
}

/// One person in a party, as its other members are told.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub who: PersonId,
    /// What they last joined under here.
    pub name: String,
    /// Whether they are in a room on this server right now. One server
    /// answering its own members about each other; nothing is reported
    /// anywhere else, which is the line [presence] draws.
    ///
    /// [presence]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#friends-searching-and-inviting-somebody-in-particular
    pub online: bool,
}

/// **A party, as one of its members sees it**: who is in it, and which worlds
/// are its own. Answered only to a member, which is the first thing on the
/// wire whose answer depends on who is asking — and why it is not the room
/// list, which stays one answer for everybody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyInfo {
    pub id: PartyId,
    pub name: String,
    pub members: Vec<Member>,
    /// Its worlds, in the shape the room list uses, so a row here joins the
    /// way a row there does. Unlisted everywhere else.
    pub rooms: Vec<RoomInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Asking to play. See [docs/networking.md].
    Join {
        name: String,
        /// Which world. `None` takes the server's default. A room is a separate
        /// world, so player numbers, value and territory are all per room — a
        /// player in two rooms is two players.
        room: Option<RoomId>,
        /// Who is asking, as against which seat they want back. The client's
        /// [`Secret`], which the server exchanges for a [`PersonId`]. `None` from a
        /// client that could not make one, which the server will not remember.
        ///
        /// It crosses the wire, which is a single-server bargain and has to change
        /// before there are two — see [`crate::net::auth::person`].
        person: Option<Secret>,
    },
    /// What this player did, and when they believe it happened.
    Act(Stamped),
    /// The chunks the client now needs, because its viewport moved.
    ///
    /// **A fetch, whatever it is called.** Nothing is kept: a change reaches a
    /// client as the `Step` for the generation it happened in, so there is no
    /// push for a subscription to select from.
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
    /// What does this server say about this person?
    ///
    /// Answerable **without a seat**, like [`Self::Rooms`] and for the same
    /// reason: a profile is looked at from a lobby, from a standings bar, and
    /// from a menu, and only one of those is inside a room.
    Profile { who: PersonId },
    /// **Who else plays here.** Names this server has met, filtered by `like`,
    /// with an empty `like` answering the best rated — which is the
    /// leaderboard, and is why it is one message rather than two.
    ///
    /// Answerable **without a seat**, like [`Self::Rooms`] and
    /// [`Self::Profile`] and for the same reason: you look somebody up from a
    /// menu as often as from a lobby.
    ///
    /// It must not become a way to enumerate everybody a server has met, so
    /// the answer is capped at [`PEOPLE_MOST`] either way.
    People { like: String },
    /// Watch a room without taking a seat in it.
    ///
    /// A spectator is a connection with a room and no `PlayerId`, not a player
    /// with the actions taken away: a seat is one of fifteen, and **no late
    /// joining is a rule about players**, so the refusal has to tell the two
    /// apart. See [server.md](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md).
    Watch { room: RoomId },
    /// Give up this seat, without closing the connection.
    ///
    /// Not the same as the socket closing, which already frees one. Without it a
    /// client that went back to the menu stayed online, so the room went on
    /// counting them and their token found them online and issued a new player.
    Leave,
    /// Take a side, or leave the one you are on. Only while **gathering**:
    /// changing sides mid-match would hand your ground to the people you were
    /// fighting. Any side, with no balance check — that is at `Start`.
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
    /// Sent with no room, because the seat already says which. Refused unless
    /// this connection made it: anybody may join a gathering match, and if
    /// anybody could start it the person who set it up could not wait.
    Start,
    /// Make a room that is not here yet, answerable without a seat.
    ///
    /// One message rather than two, because a world and a match differ by
    /// whether there is a way to win and by nothing else. **Making a room does
    /// not put you in it**: the answer is a name, and the client sends `Join`,
    /// which is the same `Join` the room list sends.
    Create {
        /// What to call it. Validated by [`room_name`] on both sides — here so the
        /// menu can refuse without a round trip, and again on the server because
        /// nothing a client says about a filename is trusted. The **id** is the
        /// server's to choose and is never sent.
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
        /// Make it a laboratory: the clock is a control and the two placing
        /// rules can be taken off. With `victory` this is the whole of what
        /// [`RoomKind`] the form asked for — a match has a way to win, an
        /// experiment has this, and a world has neither.
        laboratory: bool,
        /// **A party's world.** Unlisted, with no code, and open to that
        /// party's members and nobody else; it appears in their
        /// [`ServerMessage::Parties`] and nowhere else. Refused from anybody
        /// not in the party, and `private` is beside the point when this is
        /// set.
        party: Option<PartyId>,
    },
    /// Change what the game is doing in this room. Refused unless the room is
    /// a laboratory, because everywhere else these are the rules of the game.
    ///
    /// The whole set rather than one switch, so a client cannot half-apply a
    /// change and the answer is one broadcast rather than three.
    SetRules(Rules),
    /// One generation, in a room that is stopped.
    ///
    /// Its own message rather than an unpause and a pause, which would be two
    /// round trips and a world that ran for however long they took.
    StepOnce,
    /// **Empty this laboratory.** Refused anywhere else, for the reason
    /// [`Self::SetRules`] is: everywhere but a laboratory these are the rules
    /// of the game, and a world somebody can wipe is not one anybody would
    /// build in.
    ///
    /// The tick is kept. A generation is a number two peers agree on, and
    /// starting it over would be a world at tick nought that half the room is
    /// still at tick nine hundred in — the ground is what is being cleared,
    /// not the clock.
    Wipe,
    /// **Play me.** A match for two, made and handed to somebody by name.
    ///
    /// The server makes a private match with two sides, puts the challenger in
    /// it, and holds the room for whoever was named — see
    /// [`ServerMessage::Challenged`]. It is a room like any other once it
    /// exists: the answer is a `Join`, the whistle is the whistle, and nothing
    /// about the match knows it began as a challenge.
    ///
    /// **Answerable without a seat.** You challenge somebody from a profile
    /// panel or a list of who plays here, and neither is inside a room.
    Challenge { who: PersonId },
    /// **Yes or no**, to the person who asked.
    ///
    /// A decline is worth sending rather than letting a challenge rot: the
    /// point of asking somebody is finding out, and silence is the one answer
    /// that cannot be told from not having seen it.
    Answer { from: PersonId, yes: bool },
    /// **Here is my locker; keep it.**
    ///
    /// After everything that was here before it, on purpose, and everything
    /// after it is there for the same reason. Postcard writes a variant as its
    /// index, so appending is the one change that leaves every other message
    /// where it was — see [`codec::PROTOCOL`], which is what says so when it
    /// does not. The patterns and the diary this client
    /// now holds, replacing whatever the server had — see [`kept`].
    ///
    /// Whole rather than a change, because both are small and replacing is one
    /// meaning with no merge behind it. Clamped by the server before it is
    /// stored: this is the one message a client uses to write its own words to
    /// somebody else's disk.
    ///
    /// Answerable **without a seat**. A library is edited between games and the
    /// screen that edits it is not inside a room.
    Keep(kept::Kept),
    /// **Seat a player the server plays**, on this side or on whichever the
    /// lobby would put anybody — see `server::bot`.
    ///
    /// Any **seated** player may, while the room admits anybody: a spectator
    /// has no standing in a lobby, and once a match is running a seat arriving
    /// is the late joining `Join` refuses. The answer is the next `Match`, or
    /// a [`ServerMessage::NotStarted`] with the reason, because a press in a
    /// lobby that does nothing and explains nothing reads as a broken lobby.
    AddBot { team: Option<PlayerId>, level: Level },
    /// Take one out again, by seat, under the same rules.
    RemoveBot { seat: PlayerId },
    /// **This is who I am**, said before any room is named.
    ///
    /// A `Join` carries the secret and so a seat has always come with a person;
    /// nothing else did, so a client sitting on the menu was nobody — it heard
    /// no challenge queued for it, and could not be answered anything filed
    /// against a person. This presents the secret on its own, and the answer is
    /// [`ServerMessage::You`]. The name rides with it for the reason it rides on
    /// a `Join`: it is the one thing a profile takes a client's word for, and a
    /// person met here and nowhere else would otherwise have none.
    Hello { name: String, person: Secret },
    /// **Close a room you made.** Answered with [`ServerMessage::Closed`].
    ///
    /// Names the room, because it is sent from the menu and not from inside:
    /// a room is refused closing while anybody is in it, the one closing it
    /// included, so it is something you do after everybody has left. Refused
    /// unless the key presented is the one that made it.
    Close { room: RoomId },
    /// **Bring somebody in by name**, to the private room this connection is
    /// sitting in.
    ///
    /// Anybody seated there may. What it changes on the room is that the
    /// person named may join it by its id with no code, from now on — an
    /// invitation names a person where a code names nobody, which is the
    /// whole of what it adds. Delivered as [`ServerMessage::Invited`] the way
    /// a challenge is, the next time they are heard from. It does not expire;
    /// what would carry an "until" is a signed invitation, which is not
    /// built.
    Invite { who: PersonId, room: RoomId },
    /// **Which parties am I in**, and what is in them. Answered with
    /// [`ServerMessage::Parties`], and answered empty to a connection that has
    /// presented no key: a party is a list of people, and nobody is on no
    /// list.
    ///
    /// A second message beside [`Self::Rooms`] rather than a filter on it,
    /// because a party listing wants different *contents* — who is in it, who
    /// is online, which of its worlds are running — and because the room list
    /// is the one message a client sends before it is anybody, and stays so.
    Parties,
    /// Make a party, with the asker as its first member.
    MakeParty { name: String },
    /// Ask somebody into a party this connection is in. Delivered as
    /// [`ServerMessage::PartyInvite`], the way a challenge is, and stands until
    /// they take it or the party is gone.
    InviteToParty { party: PartyId, who: PersonId },
    /// Take a standing invitation. Refused without one: a party is not a room
    /// with a code, and its id admits nobody.
    JoinParty { party: PartyId },
    /// Leave, and with it lose the way into its worlds — which is the thing a
    /// code could never express and the reason a party is a list of people.
    /// The last one out takes the party with them.
    LeaveParty { party: PartyId },
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
        /// **What this server says about you**, which is what it calls you
        /// and what you have done here.
        ///
        /// `None` for a client that offered no secret: it is nobody this
        /// server remembers, and a dashboard showing it the starting rating
        /// would be inventing one. Two fields before this — a `PersonId` and a
        /// bare `i32` — and the `i32` was exactly that invention.
        profile: Option<Profile>,
        /// What this player has to spend. Sent because a returning player has a
        /// value already and the client cannot know it — assuming the starting
        /// figure left the two disagreeing from the first frame.
        value: i32,
        /// Which room this is. Named back rather than assumed, because the client
        /// may have asked for none.
        room: RoomId,
        /// What that room is called, for the HUD. Sent beside the id rather
        /// than looked up, because a client that has joined by code has never
        /// seen the listing and so has no name for where it is.
        name: RoomName,
        /// The shape of the world, so the client builds the same one. Not something
        /// it can derive: nothing it can see says whether the ground ends, and a
        /// client that guessed infinite against a toroidal server folded no
        /// coordinates and disagreed the moment anything crossed the seam.
        world: WorldKind,
        /// What the game is doing here. Sent rather than assumed for the same
        /// reason the shape is: nothing a client can see says whether placing
        /// is free, and one that guessed would price every action wrongly.
        rules: Rules,
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
    /// Watching: a `Welcome` with no player, because a spectator has no number,
    /// no purse and no spawn, and sending zeroes would draw all three.
    Watching {
        room: RoomId,
        name: RoomName,
        tick: Tick,
        world: WorldKind,
        rules: Rules,
    },
    /// The rules of this room changed, to everybody in it at once.
    ///
    /// Broadcast rather than answered to whoever asked, because a laboratory
    /// is a room several people are in and a clock that stopped for one of
    /// them would be two worlds.
    Rules(Rules),
    /// The answer to [`ClientMessage::Create`]: what the room is called, or why
    /// there is not one. The name is sent back because [`room_name`] lowercases
    /// and trims, so what was typed and what the room is called differ.
    Made(Result<Made, String>),
    /// One generation happened. `tick` is the generation the world is on
    /// **after** it, and `actions` is what was applied on the way.
    ///
    /// **The server is the clock.** A step is a pure function of state and
    /// tick, so two peers stay identical only while they step at the same
    /// ticks; a client keeping its own drifted within seconds. See
    /// [networking.md](https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/networking.md).
    Step {
        tick: Tick,
        actions: Vec<Stamped>,
    },
    /// Where a player's opening ground was laid, and whose it is.
    ///
    /// **A grant is a change to the world nobody was told about.** A match
    /// grants everybody at the whistle, long after every client has subscribed,
    /// so their chunks do not change hands and nothing re-fetches them. A
    /// `Resync` naming the same chunks goes out beside it.
    Spawned {
        player: PlayerId,
        at: (i32, i32),
    },
    /// The answer to [`ClientMessage::Profile`]. `None` is somebody this
    /// server has never met, which is a real answer and not a failure.
    Profile(Option<Profile>),
    /// The answer to [`ClientMessage::People`], with the query that produced
    /// it.
    ///
    /// **The query comes back** so a client can drop an answer to something it
    /// no longer asks. A search box moves faster than a round trip, so replies
    /// arrive out of order with respect to typing, and one that overwrote the
    /// list would show results for a prefix the box no longer holds.
    People {
        like: String,
        found: Vec<Profile>,
    },
    /// Somebody's rating here, and what the match just finished moved it by.
    /// Broadcast, because a result is a comparison and the interesting half is
    /// what happened to everybody else.
    Rated {
        who: PersonId,
        rating: i32,
        change: i32,
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
    /// One action, the moment the server took it, rather than in the `Step` for
    /// the generation it belongs to — which was a wait of half a generation, or
    /// 125 ms at four a second, on a link that costs four.
    ///
    /// The `Step` still carries it, because a broadcast can be dropped. A client
    /// applies whichever reaches it first and ignores the other.
    Acted(Stamped),
    /// What the match in this room is doing, and who is in it. Sent on joining
    /// and whenever it changes: a lobby has to be right rather than eventually
    /// right. Names as well as numbers, since a lobby is the one screen where
    /// players are people rather than colours.
    Match(Lobby),
    /// Who holds how much ground, most first.
    ///
    /// From the server because a client holds the chunks it subscribed to, so
    /// counting locally would score its own screen. Granted ground is left out:
    /// `HOME` never decays, so it would be points for having turned up.
    /// Broadcast on a cadence, being a pass over the world.
    Standing {
        tick: Tick,
        held: Vec<Holding>,
    },
    /// What this player actually has to spend, in reply to a `Checkpoint`.
    /// Manufacture made value depend on births anywhere in the world, and a client
    /// holds a viewport — so its figure drifts down for as long as it plays and
    /// nothing else would correct it.
    Purse {
        value: i32,
    },
    /// The rooms this server has, in the order **it** lists them, so two players
    /// looking at one menu see one list.
    Rooms {
        rooms: Vec<RoomInfo>,
        /// And what this server would rather a client did not offer — see
        /// [`Hidden`]. It rides here because this is the first thing a menu
        /// asks any server, so the answer is known before anything is drawn.
        #[serde(default)]
        hidden: Hidden,
    },
    /// **Somebody has challenged you**, and the room is already there.
    ///
    /// Carries who, so the panel that shows it can show a face and a rating
    /// rather than an id, and the room, so accepting is a `Join` and nothing
    /// else. Delivered on the next thing this client says: a challenge waits
    /// in the server's hands until its target is heard from, which is what
    /// lets somebody be challenged while they are on the menu.
    Challenged {
        from: Profile,
        room: RoomId,
    },
    /// **They answered.** `None` for a decline, so a refusal reaches the
    /// person who asked instead of looking like a server that lost the
    /// message.
    Answered {
        who: Profile,
        room: Option<RoomId>,
    },
    /// **Something went off**, and where.
    ///
    /// Broadcast to the room, because a blast is a thing that *happened*
    /// rather than a thing the world is: the cells before and the cells after
    /// are both just cells, so a client watching the board sees a disc quietly
    /// become different and reads it as a glitch. Nothing in the simulation
    /// reads this back — it is for whatever is drawing.
    ///
    /// A list rather than one, because a blob is one bomb and a chain sets off
    /// several in the same generation. See [`crate::sim::Blast`].
    Blasts(Vec<crate::sim::Blast>),
    /// **What this server holds for you**, sent on joining — see [`kept`].
    ///
    /// Only ever to the person it belongs to. It is the one thing on a profile
    /// nobody else is shown, which is what lets the server hold it without
    /// vouching for it.
    ///
    /// An **empty** one from a server that has met you is how a client knows to
    /// offer what it is carrying: that is what makes a library follow somebody
    /// to a server they have never played on, with no two servers ever talking
    /// to each other. The cost is one honest edge — somebody who threw their
    /// last pattern away and then joins from a second machine gets it back,
    /// because "empty" and "emptied" look the same from here. A tombstone would
    /// tell them apart and is more machinery than the mistake is worth.
    Yours(kept::Kept),
    /// **Who this server says you are**, in answer to [`ClientMessage::Hello`].
    ///
    /// Its own message rather than a [`Self::Profile`], because the socket
    /// reads it: this is the one reply that tells a connection which person it
    /// carries before a `Welcome` has, and a profile you *looked up* must not
    /// be mistaken for that. Anything queued for this person rides out with it.
    You(Profile),
    /// The answer to [`ClientMessage::Close`]: the room that is gone, or why
    /// it is not. A refusal leaves you where you were, with a reason to read.
    Closed(Result<RoomId, String>),
    /// **Somebody holds a door open for you**: a private room you may now
    /// join by its id. Who, so the panel can show a face rather than a
    /// fingerprint; the name, because a room you were never listed has no name
    /// on your screen yet.
    Invited {
        from: Profile,
        room: RoomId,
        name: RoomName,
    },
    /// **It would not do that, and here is why.** For an invitation and the
    /// party verbs, where [`Self::Rejected`] would be wrong: that one closes a
    /// door on a connection, and this leaves you exactly where you were with a
    /// sentence to read, the way [`Self::NotStarted`] does for a whistle. A
    /// challenge still refuses with `Rejected`.
    NotDone {
        reason: String,
    },
    /// The parties this connection's person is in, each with its people and
    /// its worlds. The answer to [`ClientMessage::Parties`], and to making,
    /// joining or leaving one, since what changed is the listing.
    Parties {
        parties: Vec<PartyInfo>,
    },
    /// **Somebody asks you into their party.** Who, the party, and what it is
    /// called, since a party you are not in is one you have never been shown.
    PartyInvite {
        from: Profile,
        party: PartyId,
        name: String,
    },
}

/// Screens this server asks a client not to offer.
///
/// **A request, not a permission.** Nothing here is enforced and none of it
/// could be: the client is somebody else's, every screen it hides is still
/// compiled into it, and a page it draws anyway costs this server nothing. It
/// is a server saying "my players should not be sent to that", which is a
/// different thing from a rule.
///
/// What it is *for* is copy that is not finished. The how-to page is
/// placeholder prose — `words::MenuTutorial` says so itself — and a public
/// server should be able to stop handing that to newcomers without waiting for
/// somebody to write it. Offline and playing alone it is always shown: there
/// is no server to have an opinion, and a page nobody else sees is nobody
/// else's problem.
///
/// A struct rather than a `bool` because the next screen with unfinished words
/// on it should be a field here rather than a second message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hidden {
    /// The how-to page, and the practice patches on it.
    pub howto: bool,
}

impl Hidden {
    /// Read a name off a command line, or say what the names are.
    ///
    /// Named rather than numbered so `--hide howto` reads as what it does, and
    /// so a server started with a name this build does not know is told rather
    /// than quietly hiding nothing.
    pub fn hide(&mut self, what: &str) -> Result<(), String> {
        match what {
            "howto" => self.howto = true,
            _ => return Err(format!("nothing here is called {what:?}; there is: howto")),
        }
        Ok(())
    }
}

/// How many people one [`ClientMessage::People`] may answer with.
///
/// A cap and not a page: what this is for is finding somebody and seeing who
/// is at the top, and neither wants paging. It is also the whole of what keeps
/// the message from being a way to read out everybody a server has ever met.
pub const PEOPLE_MOST: usize = 25;

/// How much of `player`'s influence reaches this square, nought to
/// [`crate::sim::bits::MAX_LEVEL`].
///
/// A lookup rather than a sum: a square carries one owner, so the contest
/// was settled when it last worked itself out. Nought for a chunk this peer
/// does not hold, which is the honest answer — guessing would let a client
/// predict a cheaper price than the server charges.
pub fn reach(world: &World, player: PlayerId, row: i32, col: i32) -> u8 {
    world.cell_at(row, col).filter(|c| c.player() == player).map(|c| c.influence()).unwrap_or(0)
}

/// Whether `player` may put something down here: **only where their own
/// influence reaches.**
///
/// Placing anywhere for a multiple of the price was tried and is out — it
/// made the map somewhere you bought your way into rather than grew into.
/// Safe in a way it was not before levels, because granted ground is a
/// *source*: a player whose life has gone out still has a live gradient
/// around their patch. See [docs/game.md#where-you-may-build].
pub fn may_place(world: &World, player: PlayerId, row: i32, col: i32) -> bool {
    reach(world, player, row, col) > 0
}

/// The same question under a room's [`Rules`], which is what both sides
/// actually ask.
///
/// **The rule and the switch that takes it off are read together or not at
/// all.** Asked separately, a client predicts a placement the server refuses
/// and resyncs the moment it draws; this is the one call, so a laboratory
/// takes the rule off everywhere it is asked rather than at three of the four
/// call sites.
pub fn may_place_under(world: &World, player: PlayerId, row: i32, col: i32, rules: &Rules) -> bool {
    rules.place_anywhere || may_place(world, player, row, col)
}

/// Seating and grants are [`spawn`]'s, and are named here as they always were:
/// where a player starts is asked from the client, the server and the tests
/// alike, and moving it into a file should not move it in anybody's `use`.
pub use spawn::{grant, grant_chunks, sane_world, spawn_for, too_cramped_for_grants, SPAWN_N};

/// The prices themselves live with the rules, in [`crate::sim::rule`] —
/// "life costs one" is the same kind of statement as "a cell survives on two
/// or three", and somebody balancing the game should not have to look in two
/// files. This module names the actions and reads the numbers.
pub use crate::sim::{
    DYNAMITE_COST, FACTORY_COST, FACTORY_DRAIN, FACTORY_YIELD, ICE_COST, LIFE_COST, OVERCLOCK_COST,
    RECLAIM, TURRET_COST,
};

/// What a generation's tally is worth to one player.
///
/// Here rather than in `sim` because it is a price, and the rule should not
/// know prices — it counts births and deaths and this says what they are worth.
pub fn earnings(earned: &crate::sim::Takings, player: PlayerId) -> i32 {
    let at = player.0 as usize;
    earned.born[at] as i32 * FACTORY_YIELD - earned.upkeep[at] as i32 * FACTORY_DRAIN
}

/// What an action is worth to the player who did it.
///
/// Must be read **before** the action is applied, since it depends on what
/// is there now. Shared by client and server, because two implementations
/// of what something costs are two ways to disagree about who can afford
/// what.
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

/// What an action costs under a room's [`Rules`], which in a laboratory is
/// nothing. The pricing half of [`may_place_under`], and shared for the same
/// reason.
pub fn price_under(world: &World, stamped: &Stamped, rules: &Rules) -> i32 {
    if rules.place_free {
        0
    } else {
        value_delta(world, stamped)
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
mod tests;
