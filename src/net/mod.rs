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
    /// After everything that was here before it, on purpose, and the two bot
    /// messages after it for the same reason. Postcard writes a variant as its
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

/// How many grants sit along one edge of a **torus's** grid. Six across is
/// thirty-six seats, comfortably over the fifteen a four-bit owner field holds.
///
/// Only the torus needs a fixed figure. Its ground is finite and has to be
/// divided whatever the roster turns out to be, so the grid is sized for the
/// worst case and spread over what there is. An infinite world has room, and
/// sizes its grid to the players who actually turned up — see [`seat`].
const SPAWN_ACROSS: i32 = 6;

/// How much of somebody else's ground in a seat makes it not worth taking.
///
/// A bar rather than "any at all": a few stray cells cost nothing, and what
/// this looks for is a *country* — a seat inside somebody's territory, where
/// a new player could not build.
const SPAWN_CROWDED: usize = (SPAWN_N * SPAWN_N / 4) as usize;

/// How many seats out from their own a player will look for somewhere emptier.
///
/// A bound rather than a sweep of the world: what is wanted is *near enough to
/// be in the same game and far enough to be nobody's*, and a search that ran
/// until it found perfect emptiness would put a latecomer half a map away from
/// everybody on any world that has been running a while.
const SPAWN_SEARCH: i32 = 64;

/// Which seat a player takes, as a square **spiral** out from the origin.
///
/// A spiral fills a square at every size, so however many turn up everybody
/// has neighbours on more than one side — a fixed grid filled in reading
/// order puts the first six players in a line, and a line is a corridor.
/// A function of the player's number alone, so a seat never moves.
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
const SPAWN_GAP: i32 = 48;

/// Centre to centre between neighbouring grants, in cells: a patch, plus the
/// ground between it and the next one.
const SPAWN_PITCH: i32 = SPAWN_N + SPAWN_GAP;

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
/// A patch is [`SPAWN_N`] cells and a chunk is [`CHUNK_N`], so a grant spans
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
mod tests {
    use super::*;

    /// **A name is a label, so it is clamped rather than refused.** What it is
    /// clamped to is a width a row can hold; keeping it out of the separator is
    /// [`jsonl`]'s job now and not this one.
    #[test]
    fn a_name_is_clamped_to_something_a_line_can_hold() {
        assert_eq!(player_name("  alice  "), "alice");
        assert_eq!(player_name("a\tb\nc"), "abc", "a name wrote its own field");
        assert_eq!(player_name(&"x".repeat(200)).chars().count(), PLAYER_NAME_MAX);
        // And nobody is kept out of a game over it, which is the difference
        // from `room_name` and `team_name`.
        assert_eq!(player_name(""), "");
    }

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

    /// **Priced before it is applied, or it is free.** Every placement is an
    /// idempotent setter, so once an action is down every cell it names
    /// already reads as what it would put there — which is exactly the test
    /// `value_delta` skips a cell on. Both sides of the wire depend on the
    /// order: the client's `Acted` arm applied first and charged nothing, so
    /// a teammate's spending never moved the purse it shares.
    #[test]
    fn pricing_an_action_after_applying_it_is_free() {
        let me = PlayerId(1);
        for placement in [Placement::Life, Placement::Factory, Placement::Turret, Placement::Ice] {
            let mut world = World::infinite_empty();
            let cells = vec![(0, 0), (0, 1)];
            hold(&mut world, &cells, me);

            let lay = paint(cells.clone(), placement);
            assert!(value_delta(&world, &lay) < 0, "{placement:?} costs nothing to lay");
            apply(&mut world, &lay);
            assert_eq!(value_delta(&world, &lay), 0, "{placement:?} priced after it was applied");

            if placement.can_be_taken() {
                let take = Stamped {
                    tick: 0,
                    player: me,
                    seat: me,
                    action: Action::Erase { cells: cells.clone(), placement },
                };
                assert!(value_delta(&world, &take) > 0, "{placement:?} reclaims nothing");
                apply(&mut world, &take);
                assert_eq!(value_delta(&world, &take), 0, "{placement:?} erased twice");
            }
        }
    }

    /// Life and a factory are different things to hold, so a click holding one
    /// over the other replaces the kind rather than killing the cell — which
    /// is what `is_on` answers and what `remove_from` could not, since both
    /// are taken away by clearing the same bit.
    #[test]
    fn a_factory_held_over_life_is_not_already_there() {
        let me = PlayerId(1);
        let life = Placement::Life.apply_to(Cell::DEAD, me);
        let factory = Placement::Factory.apply_to(Cell::DEAD, me);

        assert!(Placement::Life.is_on(life), "life is what is on a living cell");
        assert!(!Placement::Factory.is_on(life), "so a factory held over it places");
        assert!(Placement::Factory.is_on(factory));
        assert!(!Placement::Life.is_on(factory), "and life held over a factory places");

        // And placing is what converts, at the price of what is being laid.
        let mut world = World::infinite_empty();
        apply(&mut world, &paint(vec![(0, 0)], Placement::Life));
        assert_eq!(
            value_delta(&world, &paint(vec![(0, 0)], Placement::Factory)),
            -Placement::Factory.cost(),
            "converting life to a factory costs what a factory costs"
        );
        apply(&mut world, &paint(vec![(0, 0)], Placement::Factory));
        assert_eq!(world.cell_at(0, 0).unwrap().kind(), Kind::FACTORY);
        assert!(world.cell_at(0, 0).unwrap().is_alive(), "and leaves the cell living");
    }

    /// An overclocker is a machine placed in fours for a turret's reasons, and
    /// it is put down, recognised and taken back the way a turret is.
    #[test]
    fn an_overclocker_is_placed_and_taken_back_like_a_turret() {
        assert!(OVERCLOCK_COST > FACTORY_COST, "an overclocker does not inherit, so it costs more");

        let mut world = World::infinite_empty();
        let block = vec![(0, 0), (0, 1), (1, 0), (1, 1)];
        hold(&mut world, &block, PlayerId(1));
        assert_eq!(
            value_delta(&world, &paint(block.clone(), Placement::Overclock)),
            -4 * OVERCLOCK_COST,
            "an emplacement is four of them"
        );
        apply(&mut world, &paint(block.clone(), Placement::Overclock));
        for &(row, col) in &block {
            let cell = world.cell_at(row, col).unwrap();
            assert!(cell.is_alive() && cell.kind() == Kind::OVERCLOCK);
            assert!(Placement::Overclock.is_on(cell), "the square holds what was placed");
            assert!(!Placement::Turret.is_on(cell), "and not the other machine");
            let taken = Placement::Overclock.remove_from(cell);
            assert!(!taken.is_alive() && taken.player() == PlayerId(1));
        }
    }

    /// A turret is bought once per cell forever, where a factory is bought once
    /// per lineage — so it is dearer than a factory, and the price to read is the
    /// **emplacement**: one turret dies of loneliness, and the smallest one
    /// that works is a block of four.
    #[test]
    fn a_turret_is_priced_per_cell_and_placed_in_fours() {
        assert!(TURRET_COST > FACTORY_COST, "a turret does not inherit, so it costs more");

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
        // exactly as life over a factory does.
        let placed = world.cell_at(0, 0).unwrap();
        assert!(Placement::Turret.is_on(placed));
        assert!(!Placement::Life.is_on(placed));
        assert!(!Placement::Factory.is_on(placed));
    }

    /// A corpse holds no life for either placement to take, whatever kind it
    /// kept — which is what stops a click over a dead factory handing out a free
    /// one instead of charging for it.
    #[test]
    fn a_dead_mine_holds_neither_life_nor_a_mine() {
        let corpse = Placement::Factory.apply_to(Cell::DEAD, PlayerId(1)).with_alive(false);
        assert_eq!(corpse.kind(), Kind::FACTORY);
        assert!(!Placement::Factory.is_on(corpse));
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

        let ours = Stamped {
            tick: 0,
            player: PlayerId(1),
            seat: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Ice },
        };
        assert_eq!(value_delta(&world, &ours), -1);
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
    /// obstacle to anybody with a factory running, and a cost that varied across
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
        assert_eq!(reach(&world, me, 0, 5), 0);
    }

    /// Somebody else's ground is not yours however strong their claim is:
    /// a square carries one owner, so two players' influence never sits on
    /// the same one.
    #[test]
    fn somebody_elses_influence_is_not_yours() {
        let mut world = World::infinite_empty();
        let (me, them) = (PlayerId(1), PlayerId(2));
        hold(&mut world, &[(0, 0)], them);
        assert_eq!(reach(&world, them, 0, 0), crate::sim::bits::MAX_LEVEL);
        assert_eq!(reach(&world, me, 0, 0), 0);
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
        let ours = Stamped {
            tick: 0,
            player: PlayerId(1),
            seat: PlayerId(1),
            action: Action::Erase { cells: vec![(0, 0)], placement: Placement::Life },
        };
        assert_eq!(value_delta(&world, &ours), -RECLAIM);
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
        let ours = (row..row + SPAWN_N)
            .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
            .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == second)
            .count();
        assert_eq!(ours, (SPAWN_N * SPAWN_N) as usize, "the whole patch is theirs");

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
            let ours = (row..row + SPAWN_N)
                .flat_map(|r| (col..col + SPAWN_N).map(move |c| (r, c)))
                .filter(|&(r, c)| world.cell_at(r, c).unwrap().player() == PlayerId(id))
                .count();
            assert_eq!(
                ours,
                (SPAWN_N * SPAWN_N) as usize,
                "player {id} did not get a whole square"
            );
        }
    }

    /// **No world a client or server can make is too small any more**, and
    /// that is worth a test rather than a deletion.
    ///
    /// The smallest torus there is, is one chunk. At sixteen cells a side that
    /// was 256 cells and could not seat everybody, so `too_cramped_for_grants`
    /// had a case to answer; at sixty-four it is 4096, which holds a five by
    /// five grid of patches against a ceiling of fifteen players. The guard is
    /// still right and is now unreachable from outside, which is the state to
    /// know about — if either number moves back it starts mattering again, and
    /// this is what would notice.
    #[test]
    fn no_world_anybody_can_make_is_too_small_to_go_round() {
        let smallest = World::toroidal_empty(1, 1);
        assert!(
            !too_cramped_for_grants(&smallest),
            "the smallest world there is cannot seat everybody"
        );
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
        // In cells, and stated as a distance rather than as a count of chunks
        // — the number was always about how far a glider travels, and tying it
        // to `CHUNK_N` quadrupled it the day a chunk grew.
        assert_eq!(SPAWN_GAP, 48, "forty-eight cells of no-man's-land");
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
        let mut ours = World::infinite_empty();
        let seat = spawn_for(me, &ours);
        for r in seat.0..seat.0 + SPAWN_N {
            for c in seat.1..seat.1 + SPAWN_N {
                ours.set_cell_at(
                    r,
                    c,
                    Cell::DEAD.with_player(me).with_level(crate::sim::bits::MAX_LEVEL),
                );
            }
        }
        assert_eq!(crowding(&ours, seat, me), 0, "your own ground is not a crowd");
        assert_eq!(spawn_for(me, &ours), seat);
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
        // A quarter the chunks a side for the same worlds, since a chunk grew
        // fourfold on the edge — and the largest is the cap, which is the size
        // this most wants to be checked at.
        for chunks in [2, 3, 4, 6, 10] {
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
        let roomy = World::toroidal(10, 10);
        // The world's own extent, not a number repeated from the line above
        // — which is how this came to describe a world four times the size of
        // the one it was measuring, and wrapped every distance the wrong way.
        let extent = roomy.size_in_cells().expect("a torus has a size").1;
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
        assert_eq!(reach(&world, PlayerId(1), far[0].0, far[0].1), 0);
    }
}
