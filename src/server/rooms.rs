//! Several worlds behind one address.
//!
//! A room is a whole [`Server`] — one world, one player table, one tick — not
//! a channel inside a shared one. That is what "rooms" usually means, and it
//! is the simpler of the two things it could have meant: nothing in the
//! simulation has to learn that a world might be one of many, because a world
//! never is. What it costs is that territory, value, player numbers and the
//! rejoin token are all per room, and a player in two rooms is two players.
//!
//! ## Rooms are declared, not conjured
//!
//! Joining a name nobody has declared is refused, and the refusal names the
//! rooms that do exist. The alternative — creating a room for whoever asks —
//! makes a typo into a world: `loby` gets you an empty plane where you own
//! nothing, know nobody, and cannot tell that you are the only one who will
//! ever be there. Until there is a menu that lists what a server has, the
//! rejection *is* the list, which is why it carries the names.
//!
//! A room is declared by `--room NAME`, and by having a save file already:
//! every `<name>.ckw` in the rooms directory is a room, so a restart keeps
//! what a previous run was asked for without being asked again.
//!
//! ## One file per room
//!
//! The save format holds one world and its players, which is exactly one room,
//! so the file is unchanged and the directory is what grew. The room's name is
//! the file's name and is not written inside it — two places to keep one fact
//! is one too many, and the one that can be renamed by a person is the one
//! that has to win.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::net::{
    ClientMessage, Made, PartyId, PersonId, RoomId, RoomInfo, RoomName, Secret, ServerMessage,
    DEFAULT_ROOM,
};
use crate::server::matches::{Phase, Victory};
use crate::server::Server;
use crate::sim::{PlayerId, WorldKind};

/// Where a connected player is: which world, and who they are in it. Player
/// numbers are per room, so the number alone does not identify anybody.
pub type Seat = (RoomId, PlayerId);

/// The extension a room's world is saved under.
const SAVE_EXT: &str = "ckw";

/// A refusal, in the shape everything else here refuses in.
///
/// One line because a challenge fails in five ways and all of them are a
/// sentence for the person who pressed something — see
/// [`ServerMessage::Rejected`], which the menu already puts on screen.
fn refuse(why: &str) -> Vec<ServerMessage> {
    log::info!("refused a challenge: {why}");
    vec![ServerMessage::Rejected { reason: why.to_string() }]
}

/// A refusal that leaves the caller where they were — see
/// [`ServerMessage::NotDone`]. For what is asked of a person rather than of a
/// world, where a `Rejected` would close a door on somebody standing in a room.
fn not_done(why: &str) -> Vec<ServerMessage> {
    log::info!("would not: {why}");
    vec![ServerMessage::NotDone { reason: why.to_string() }]
}

/// Which socket a message came in on. Unique for as long as the process lives
/// and never reused, so a room's owner cannot become somebody else by a
/// counter wrapping onto a number a departed connection had.
pub type ConnectionId = u64;

/// Who is speaking.
///
/// Two things rather than one, because they answer different questions and
/// exist from different moments: the seat says which world a message belongs
/// to and appears only after a `Welcome`, while the connection exists from the
/// moment the socket opens — which is when a room can first be made, since
/// making one is what you do before there is a world to sit in.
#[derive(Clone)]
pub struct Caller {
    pub connection: ConnectionId,
    /// Which world, and who in it. `None` until this connection has joined.
    pub seat: Option<Seat>,
    /// Which world this connection is watching without a seat in it.
    ///
    /// Beside `seat` rather than folded into it, because they are answers to
    /// different questions: a seat says what this connection may *do*, and
    /// this says what it may *see*. A connection has at most one of them —
    /// watching a room and then joining it clears this — but the code that
    /// judges an action asks the first and the code that routes a read asks
    /// either, and one field would make those the same question.
    pub watching: Option<RoomId>,
    /// **Who this connection is**, once a `Welcome` has said so.
    ///
    /// Beside `seat` for the reason `watching` is: a person outlives a seat,
    /// so leaving a room clears the seat and not this. What it unlocks is the
    /// messages that are about a *person* rather than a world —
    /// [`ClientMessage::Keep`] is the one that writes, and a connection that
    /// has never joined has nowhere to put what it offers.
    pub person: Option<PersonId>,
}

impl Caller {
    /// A connection that has not joined anything.
    pub fn new(connection: ConnectionId) -> Self {
        Self { connection, seat: None, watching: None, person: None }
    }

    /// For tests, and for the console, which is nobody's socket.
    pub fn nobody() -> Self {
        Self::new(0)
    }

    pub fn sitting(connection: ConnectionId, seat: Seat) -> Self {
        Self { connection, seat: Some(seat), watching: None, person: None }
    }

    /// The same, for somebody this server knows the name of.
    pub fn known(connection: ConnectionId, who: PersonId) -> Self {
        Self { connection, seat: None, watching: None, person: Some(who) }
    }

    /// Which room's messages this connection may be routed to, seated or not.
    fn room(&self) -> Option<&RoomId> {
        self.seat.as_ref().map(|(room, _)| room).or(self.watching.as_ref())
    }
}

/// Who a client-made room belongs to. See [`Rooms::owner`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Person(PersonId),
    Seat(PlayerId),
}

/// **How a room made over the wire is reached.** The third is why this is not
/// a `bool`: a party's world is unlisted like a coded one and has no code,
/// because a code names nobody and a party is exactly a list of who. See
/// [`Rooms::may_enter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    /// In the listing, for anybody on this server.
    Listed,
    /// Unlisted, behind a code the server generates.
    Code,
    /// Unlisted, and its party's: members only.
    Party(PartyId),
}

/// What a private room is called when whoever made it named nothing. Nobody
/// browses for it, so a name is a courtesy rather than a requirement.
const UNNAMED: &str = "private game";

/// A short code that reaches a room the listing does not mention.
///
/// The thing you send somebody, rather than the thing you type. Room names are
/// typed, and typed names collide, are guessed, and have to be spelled out
/// over a phone; a code is generated, so it does neither.
///
/// A **credential, not an identity**. It is separate from [`crate::net::RoomId`]
/// so that it can be changed later without the room becoming a different room,
/// and separate from the name so that a private room can still be called
/// whatever its owner chose.
pub type Code = String;

/// How long a code is, and what it is spelled from.
///
/// Six characters, case-insensitive — which comes free, because
/// [`crate::net::room_name`] already folds case for every identifier here.
///
/// The alphabet leaves out `0`, `o`, `1`, `i` and `l`: those five are the
/// whole of why a code gets mistyped when it is read off one screen and typed
/// into another, or said out loud. That costs 1.3 bits — 31⁶ = 887,503,681
/// codes, or **29.7 bits**, against 36⁶ = 2,176,782,336 and 31.0 bits for the
/// full alphanumeric set — and it is a good trade, because the keyspace is not
/// what protects a private room.
///
/// It is worth being clear about what does. With [`MAX_MADE_ROOMS`] rooms in
/// play, a random guess finds one in about twenty-eight million, so the
/// defence is that guessing is not worth anybody's time; if it ever became
/// worth somebody's time the answer would be a limit on how fast a connection
/// may guess, not a longer code. A code is a **latch rather than a lock**.
pub const CODE_LEN: usize = 6;

/// What a code is spelled from: thirty-one characters, being the digits and
/// lowercase letters less the five confusable ones. Lowercase because
/// `room_name` folds case, so a code typed in capitals is the same code.
const CODE_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";

/// How many rooms a server will hold once clients are the ones making them.
///
/// A room costs a full simulation four times a second for as long as the
/// process lives, whether or not anybody is in it, so a server that makes one
/// for whoever asks is a server anybody can fill. This is the backstop rather
/// than the fix — see [auto-sleep] — and it counts only rooms made over the
/// wire: an operator declaring forty on the command line has made a decision,
/// and this is not the place to second-guess it.
///
/// [auto-sleep]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#making-rooms-from-the-client
pub const MAX_MADE_ROOMS: usize = 32;

pub struct Rooms {
    /// Sorted, so every listing — a log line, a rejection, a save sweep — is
    /// in the same order however the map was filled.
    ///
    /// Keyed by **id**, not by name. A name is typed and may be changed; an id
    /// is generated once and never is, so the seat a player holds, the token
    /// filed against it and the file it saves to all go on meaning the same
    /// room after a rename.
    rooms: BTreeMap<RoomId, Server>,
    dir: PathBuf,
    /// Where a client that names no room is put.
    default_room: RoomId,
    /// What each room is called. Separate from the map because a name is a
    /// label on a room rather than a fact the room keeps about itself, and
    /// because it has to be possible for two rooms to have swapped names
    /// without either becoming the other.
    names: BTreeMap<RoomId, RoomName>,
    /// Everybody this server has met, which is the one thing here that is not
    /// about a room. A person outlives the room they were last seen in, and a
    /// seat does not -- see [`crate::server::people`].
    people: crate::server::people::People,
    /// What each of them is rated here. Beside `people` and for the same
    /// reason: a person outlives every world on this server, and so does their
    /// number.
    profiles: crate::server::profiles::Profiles,
    /// What each of them has saved that **nobody else is shown** — their
    /// patterns and their diary. Beside `profiles` because it is filed the same
    /// way and for the same reason, and apart from it because the two answer
    /// opposite questions: one is what this server will vouch for, and this is
    /// what it merely holds. See [`crate::server::lockers`].
    lockers: crate::server::lockers::Lockers,
    /// What this server asks a client not to offer — see [`crate::net::Hidden`].
    /// A field rather than a `Config` this map borrows, because it is the only
    /// thing here a room list has to carry and `Rooms` is what answers one.
    pub hidden: crate::net::Hidden,
    /// **Messages waiting for somebody who is not listening yet.**
    ///
    /// A challenge is the first thing here addressed to a *person* rather than
    /// answered to whoever asked, and there is no channel for that: replies go
    /// back to the caller and broadcasts go to a room. Rather than build one,
    /// a challenge waits until its target is heard from — which is soon,
    /// because a client on the menu is asking for the room list and one in a
    /// world is checkpointing.
    ///
    /// The cost is honest: somebody who closed the tab is challenged the next
    /// time they open it, and somebody who never comes back never sees it. A
    /// challenge is an invitation rather than a notification, so arriving late
    /// is the right failure.
    waiting: BTreeMap<PersonId, Vec<ServerMessage>>,
    /// **Challenges standing against each person**, which is a different thing
    /// from the outbox above and was briefly the same one.
    ///
    /// A message in `waiting` is delivered *once*, the next time its target
    /// says anything. An invitation stands until it is answered — so keeping
    /// the challenge in the outbox meant that handing it over consumed it, and
    /// the `Answer` that came back a moment later found nothing to answer.
    ///
    /// One per person, so a challenge cannot be a way to fill somebody's
    /// screen and the room a decline names is the room they were shown.
    challenges: BTreeMap<PersonId, (PersonId, RoomId)>,
    /// What a room made after startup runs at, so one made at the console
    /// keeps the speed the command line asked for.
    default_bpm: u16,
    /// The code that reaches each private room.
    ///
    /// A credential, not an identity: it is separate from the id so that a
    /// code can be changed later without the room becoming a different room,
    /// and separate from the name so that a private room can still be called
    /// something its owner chose.
    codes: BTreeMap<RoomId, Code>,
    /// Rooms made over the wire, and which connection asked for each.
    ///
    /// Separate from `rooms` rather than a field on [`Server`], because it is
    /// a fact about how a room came to exist and not about the world in it —
    /// nothing in a save should change because a client rather than an
    /// operator typed the name. The connection is `None` for a room this
    /// process did not see made: it came back from `rooms.jsonl`, and a
    /// connection id means nothing after a restart while the cap still counts
    /// it.
    made: BTreeMap<RoomId, Option<ConnectionId>>,
    /// Whose room each client-made room is: who may start and end its match,
    /// and close it.
    ///
    /// **The person when this server knows one, and the seat when it does
    /// not.** A seat is a room's number for somebody and comes back on a
    /// rejoin, which is enough for a refresh; a person is who they are on this
    /// server, which is what "the room you made" means, and is the key that
    /// outlives the seat. The seat stands in for a client with no key, which
    /// is somebody this server will not remember.
    ///
    /// Recorded at `Create` when the maker has presented a key, and otherwise
    /// at the **creating connection's first join**, which is the first moment
    /// there is a seat to record. Saved in `rooms.jsonl` when it is a person;
    /// a seat is not, because a seat means nothing after a restart.
    owner: BTreeMap<RoomId, Owner>,
    /// The cap on `made`. [`MAX_MADE_ROOMS`] unless a flag says otherwise.
    max_made: usize,
    /// Rooms that are not in the listing, and the code that reaches each.
    ///
    /// A set rather than a map from code to room, because the code **is** the
    /// room's name: a generated name is already unique, already valid, and
    /// already what `Join` carries, so a second namespace to keep in step
    /// would be a second thing that can disagree. What is private about a
    /// private room is that [`Self::listing`] does not mention it.
    unlisted: BTreeSet<RoomId>,
    /// **Who may walk into each unlisted room by its id.**
    ///
    /// A code names nobody: whoever it is forwarded to gets in, and the room
    /// cannot tell. This names people — whoever was invited, whoever was
    /// challenged, and whoever once came in by the code, so a refresh on a
    /// room joined that way is not a refusal. It is what [`Self::may_enter`]
    /// asks, and it is saved, so an invitation given before a restart stands
    /// after it. The maker is not in it; the maker owns the room.
    admitted: BTreeMap<RoomId, BTreeSet<PersonId>>,
    /// Groups of people with worlds of their own. Beside `people` and
    /// `profiles` for the reason those are: a party outlives every room in
    /// it. See [`crate::server::parties`].
    parties: crate::server::parties::Parties,
}

impl Rooms {
    /// Open every room in `dir`, plus every one named in `declared`.
    ///
    /// The default room is the first name in `declared`, or [`DEFAULT_ROOM`],
    /// and is created if it does not exist — a server with no room at all can
    /// be connected to and not joined, which looks exactly like being broken.
    ///
    /// `fresh` ignores what is on disk, for all rooms at once. Per-room would
    /// need a way to name which, and the flag exists to start over.
    ///
    /// A save that cannot be read is an error rather than a silent reset: for
    /// one room as much as for one world, discarding it is the worst possible
    /// response to a bad read. The name of the file that failed is in the
    /// error, since with several of them "cannot read the world" no longer
    /// says which.
    pub fn open(
        dir: impl Into<PathBuf>,
        declared: &[String],
        shape: WorldKind,
        fresh: bool,
    ) -> std::io::Result<Self> {
        let dir = dir.into();
        let mut rooms = BTreeMap::new();
        // A room that came from a save or a flag takes its name as its id, so
        // an existing directory keeps working and `--room arena` still reaches
        // "arena" after a restart. Only rooms made over the wire get a
        // generated one; what the id buys there is that a room can be renamed
        // without every token filed against it going stale.
        let mut names: BTreeMap<RoomId, RoomName> = BTreeMap::new();

        if !fresh {
            for name in saved_in(&dir)? {
                let path = save_path(&dir, &RoomId(name.clone()));
                let server =
                    Server::load_or_new(&path, name.clone(), || shape.build()).map_err(|e| {
                        std::io::Error::new(
                            e.kind(),
                            format!("room \"{name}\" ({}): {e}", path.display()),
                        )
                    })?;
                rooms.insert(RoomId(name.clone()), server.seeded_by(&RoomId(name.clone())));
                names.insert(RoomId(name.clone()), name);
            }
        }

        let default_room = match declared.first() {
            Some(first) => crate::net::room_name(first)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            None => DEFAULT_ROOM.to_string(),
        };
        let default_room = RoomId(default_room);

        for raw in declared.iter().map(String::as_str).chain([default_room.as_str()]) {
            let name = crate::net::room_name(raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            let id = RoomId(name.clone());
            rooms
                .entry(id.clone())
                .or_insert_with(|| Server::named(name.clone(), shape.build()).seeded_by(&id));
            names.insert(id, name);
        }

        let people = crate::server::people::People::load(&people_path(&dir))?;
        let profiles = crate::server::profiles::Profiles::load(&profiles_path(&dir))?;
        let lockers = crate::server::lockers::Lockers::load(&stamps_path(&dir), &games_path(&dir))?;
        let mut parties = crate::server::parties::Parties::load(&parties_path(&dir))?;
        // A party's match is not saved and a room may have been deleted since;
        // whatever is not here is not the party's any more.
        parties.keep_rooms(|room| rooms.contains_key(room));
        if !people.is_empty() {
            log::info!("{} player key(s) known", people.len());
        }
        if !lockers.is_empty() {
            log::info!("{} locker(s) of patterns and diaries held", lockers.len());
        }
        if !parties.is_empty() {
            log::info!("{} part(ies) remembered", parties.len());
        }

        // **What a client-made room was, put back on it.** The world came
        // from its `.ckw` like any other; that it was a player's, whose, and
        // that it was private are facts about the map and not the world, so
        // they are in a table beside it. A row for a room that is not here is
        // a match, which is not saved, or a room since deleted -- dropped,
        // and the next write forgets it.
        let mut made = BTreeMap::new();
        let mut owner = BTreeMap::new();
        let mut codes = BTreeMap::new();
        let mut unlisted = BTreeSet::new();
        let mut admitted = BTreeMap::new();
        if !fresh {
            for row in load_meta(&meta_path(&dir))? {
                if !rooms.contains_key(&row.id) {
                    continue;
                }
                made.insert(row.id.clone(), None);
                if let Some(who) = row.owner {
                    owner.insert(row.id.clone(), Owner::Person(who));
                }
                if let Some(code) = row.code {
                    codes.insert(row.id.clone(), code);
                }
                if !row.admitted.is_empty() {
                    admitted.insert(row.id.clone(), row.admitted.into_iter().collect());
                }
                if row.unlisted {
                    unlisted.insert(row.id);
                }
            }
            if !made.is_empty() {
                log::info!("{} room(s) made by players remembered", made.len());
            }
        }

        Ok(Self {
            rooms,
            dir,
            default_room,
            names,
            people,
            profiles,
            lockers,
            hidden: crate::net::Hidden::default(),
            default_bpm: crate::net::DEFAULT_BPM,
            waiting: BTreeMap::new(),
            challenges: BTreeMap::new(),
            codes,
            made,
            owner,
            max_made: MAX_MADE_ROOMS,
            unlisted,
            admitted,
            parties,
        })
    }

    /// A single room, with nothing on disk behind it. What the tests want.
    pub fn just(server: Server) -> Self {
        let name = server.room().to_string();
        let id = RoomId(name.clone());
        Self {
            rooms: BTreeMap::from([(id.clone(), server)]),
            dir: PathBuf::new(),
            default_room: id.clone(),
            names: BTreeMap::from([(id, name)]),
            people: crate::server::people::People::new(),
            profiles: crate::server::profiles::Profiles::new(),
            lockers: crate::server::lockers::Lockers::new(),
            hidden: crate::net::Hidden::default(),
            default_bpm: crate::net::DEFAULT_BPM,
            waiting: BTreeMap::new(),
            challenges: BTreeMap::new(),
            codes: BTreeMap::new(),
            made: BTreeMap::new(),
            owner: BTreeMap::new(),
            max_made: MAX_MADE_ROOMS,
            unlisted: Default::default(),
            admitted: BTreeMap::new(),
            parties: crate::server::parties::Parties::new(),
        }
    }

    /// What this room is called, or its id if nothing has named it.
    pub fn name_of<'a>(&'a self, id: &'a RoomId) -> &'a str {
        self.names.get(id).map(String::as_str).unwrap_or_else(|| id.as_str())
    }

    /// The code that reaches this room, if it is private.
    pub fn code_of(&self, id: &RoomId) -> Option<&str> {
        self.codes.get(id).map(String::as_str)
    }

    /// Every room's id.
    pub fn ids(&self) -> impl Iterator<Item = &RoomId> {
        self.rooms.keys()
    }

    /// Every room's name, for a log line or a listing.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.rooms.keys().map(|id| self.name_of(id))
    }

    /// Every room anybody may be told about, which is every room but the
    /// private ones. What a refusal names, and what a listing lists.
    pub fn public_names(&self) -> impl Iterator<Item = &str> {
        self.rooms.keys().filter(|id| !self.unlisted.contains(*id)).map(|id| self.name_of(id))
    }

    pub fn default_room(&self) -> &RoomId {
        &self.default_room
    }

    pub fn get(&self, room: &RoomId) -> Option<&Server> {
        self.rooms.get(room)
    }

    pub fn get_mut(&mut self, room: &RoomId) -> Option<&mut Server> {
        self.rooms.get_mut(room)
    }

    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }

    /// Which room a client asking for this one gets, or why it gets none.
    ///
    /// Three things can be typed and all three reach a room: its **id**, which
    /// is what a client that has seen the listing sends back; its **name**,
    /// which is what a person types on a command line or in a link; and its
    /// **code**, which is how a private room is reached. Tried in that order,
    /// and the order matters only if a name collides with another room's id,
    /// which is why ids generated for new rooms are not spelled like names
    /// people choose.
    ///
    /// The error is written to be read by a player: it says what they asked
    /// for and what is actually here. It names only rooms that are **listed** —
    /// it used to name every room, which with private ones in the map would
    /// hand out every code on the server to anybody who mistyped a name once.
    pub fn resolve(&self, asked: Option<&str>) -> Result<RoomId, String> {
        let Some(asked) = asked else {
            return Ok(self.default_room.clone());
        };
        // Folded the same way a room name is, so a code typed in capitals and
        // one typed in lowercase are the same code.
        let asked = crate::net::room_name(asked)?;
        let id = RoomId(asked.clone());
        if self.rooms.contains_key(&id) {
            return Ok(id);
        }
        if let Some((id, _)) = self.names.iter().find(|(_, name)| **name == asked) {
            return Ok(id.clone());
        }
        if let Some((id, _)) = self.codes.iter().find(|(_, code)| **code == asked) {
            return Ok(id.clone());
        }
        Err(self.not_here(&asked))
    }

    /// The refusal for a room that is not here — and for one that is and
    /// that this caller may not know about, which from where they stand is the
    /// same thing.
    fn not_here(&self, asked: &str) -> String {
        format!(
            "no room \"{asked}\" here; this server has {}",
            self.public_names().collect::<Vec<_>>().join(", ")
        )
    }

    /// **Whether this door opens for this caller.**
    ///
    /// A listed room opens for anybody. An unlisted one opens by its **code**,
    /// which is the latch a private room is meant to have; for the connection
    /// that made it in this process, which is a keyless maker's only way in;
    /// and for a key that owns it or is in [`Self::admitted`]. Anything else
    /// is refused as though the room were not here, because to somebody
    /// holding neither the code nor an invitation it is not — and the id
    /// alone, which every client that has been in the room has seen in its
    /// address bar, stops being a bearer credential.
    fn may_enter(
        &self,
        id: &RoomId,
        connection: ConnectionId,
        who: Option<&PersonId>,
        asked: Option<&str>,
    ) -> bool {
        if !self.unlisted.contains(id) || self.entered_by_code(id, asked) {
            return true;
        }
        if self.made.get(id) == Some(&Some(connection)) {
            return true;
        }
        let Some(who) = who else { return false };
        self.owned_by(id) == Some(who)
            || self.admitted.get(id).is_some_and(|in_| in_.contains(who))
            || self.parties.party_of(id).is_some_and(|party| self.parties.is_member(party, who))
    }

    /// Whether `asked` was this room's code, as against its id or its name.
    fn entered_by_code(&self, id: &RoomId, asked: Option<&str>) -> bool {
        let (Some(code), Some(asked)) = (self.codes.get(id), asked) else { return false };
        crate::net::room_name(asked).is_ok_and(|asked| *code == asked)
    }

    /// Let this person through this room's door from now on. Saved at once,
    /// for the reason an owner is.
    fn admit(&mut self, id: &RoomId, who: &PersonId) {
        if self.admitted.entry(id.clone()).or_default().insert(who.clone()) {
            self.save_meta();
        }
    }

    /// Decoded message in, replies out — the same contract as
    /// [`Server::handle`], with the room worked out first.
    ///
    /// A `Join` carries the room it wants and so needs no seat. Everything
    /// else is answered by the room the sender is already in, and a message
    /// from a connection that has not joined is dropped: it names no world,
    /// and answering it from the default room would let an unjoined client
    /// read a world it was never let into.
    ///
    /// A `Join` from a connection that already has a seat **leaves that seat
    /// first**, so joining twice is a room change rather than a leak. Without
    /// it the abandoned player stays marked online for as long as the process
    /// runs — and a player who is online cannot be returned to by their token,
    /// so the leak is not a stale flag, it is a locked-out player.
    ///
    /// Left only once the new room is certain. Leaving first and resolving
    /// after would take a player out of the room they were in to answer a
    /// request that was then refused, and their client, which learns where it
    /// is from the `Welcome` it never got, would go on believing it was still
    /// there.
    pub fn handle(&mut self, caller: &Caller, msg: ClientMessage) -> Vec<ServerMessage> {
        // **Anything held for this caller rides out with whatever they asked
        // for.** There is no channel to a person — see `Self::waiting` — so a
        // challenge waits until its target is heard from, which is soon: a
        // client on the menu is asking for the room list and one in a world is
        // checkpointing. Taken here, at the top, so every path answers with it
        // rather than the handful that remembered to.
        let mut out = self.deliver(caller);
        out.extend(self.answer_for(caller, msg));
        out
    }

    /// What this caller asked for, without what was waiting for them.
    fn answer_for(&mut self, caller: &Caller, msg: ClientMessage) -> Vec<ServerMessage> {
        // Answered without a seat, like `Join` and for the same reason: it
        // names no world. A player has to see the rooms before picking one,
        // and a room *is* a world, so asking from inside one is asking too
        // late.
        if let ClientMessage::Rooms = msg {
            return vec![ServerMessage::Rooms { rooms: self.listing(), hidden: self.hidden }];
        }
        // **Who, with no where.** The same meeting a `Join` does, without the
        // room: a client on the menu is somebody, and until it could say so
        // nothing filed against a person could reach it there. What was
        // waiting rides out with the answer rather than with the next thing
        // said, because `deliver` above ran before this connection had a name.
        if let ClientMessage::Hello { name, person } = &msg {
            let who = self.meet(person);
            self.profiles.met(&who, name);
            let mut out = self.deliver(&Caller::known(caller.connection, who.clone()));
            if let Some(profile) = self.profile_of(&who) {
                out.insert(0, ServerMessage::You(profile));
            }
            return out;
        }
        // Answered without a seat for a sharper version of the same reason: it
        // names a room that does not exist, so there is nowhere to have been
        // standing when it was sent.
        if let ClientMessage::Create { name, shape, victory, teams, private, laboratory, party } =
            msg
        {
            // A party's world is its members' to make: a room nobody in the
            // party asked for is a room they cannot close.
            let reach = match party {
                Some(party) => {
                    let member = caller
                        .person
                        .as_ref()
                        .is_some_and(|who| self.parties.is_member(&party, who));
                    if !member {
                        return vec![ServerMessage::Made(Err("you are not in that party".into()))];
                    }
                    Reach::Party(party)
                }
                None if private => Reach::Code,
                None => Reach::Listed,
            };
            let made =
                self.make(caller.connection, &name, shape, victory, teams, reach, laboratory);
            if let Ok(made) = &made {
                self.claim(&made.id, caller);
            }
            return vec![ServerMessage::Made(made)];
        }
        // Answered without a seat for the same reason `Rooms` is: a profile is
        // looked at from a lobby, from a standings bar and from a menu, and
        // only one of those is inside a room.
        if let ClientMessage::Profile { who } = &msg {
            return vec![ServerMessage::Profile(self.profile_of(who))];
        }
        // Answered without a seat for the same reason `Profile` is, and one
        // more: this is how somebody finds a person to look up in the first
        // place, and the menu is where they are standing when they do.
        if let ClientMessage::People { like } = &msg {
            return vec![ServerMessage::People {
                like: like.clone(),
                found: self.people_like(like),
            }];
        }
        // **Play me.** Answered without a seat, because you challenge somebody
        // from a profile panel or a list of who plays here and neither is
        // inside a room.
        if let ClientMessage::Challenge { who } = &msg {
            return self.challenge(caller, who);
        }
        if let ClientMessage::Answer { from, yes } = &msg {
            return self.answer(caller, from, *yes);
        }
        // Answered without a seat for the same reason again, and one of its
        // own: a library is edited *between* games, on a screen that is not
        // inside a room. What it does need is a person, because a locker is
        // filed against one -- a connection that has never joined has nowhere
        // to put what it is offering, and cannot say whose it is.
        if let ClientMessage::Keep(offered) = msg {
            match &caller.person {
                Some(who) => {
                    self.lockers.keep(who, offered);
                    self.save_lockers();
                }
                None => log::info!("a locker was offered by nobody, so there is nowhere for it"),
            }
            return Vec::new();
        }
        // Answered without a seat, and it has to be: a room will not close
        // while anybody is in it, so whoever closes one is on the menu.
        if let ClientMessage::Close { room } = &msg {
            return vec![ServerMessage::Closed(self.close(caller, room))];
        }
        // Judged here rather than by the room, because who may come in is a
        // fact about the map: a `Server` knows nothing of codes or listings.
        if let ClientMessage::Invite { who, room } = &msg {
            return self.invite(caller, who, room);
        }
        // **Parties**, which are lists of people and so are answered to a
        // person: a connection that has presented no key is on no list and
        // gets an empty one, which is true rather than a refusal.
        if let ClientMessage::Parties = &msg {
            return self.parties_for(caller);
        }
        if let ClientMessage::MakeParty { name } = &msg {
            return self.make_party(caller, name);
        }
        if let ClientMessage::InviteToParty { party, who } = &msg {
            return self.invite_to_party(caller, party, who);
        }
        if let ClientMessage::JoinParty { party } = &msg {
            return self.join_party(caller, party);
        }
        if let ClientMessage::LeaveParty { party } = &msg {
            return self.leave_party(caller, party);
        }
        // Admitted at any generation, and that is the point rather than an
        // oversight: **no late joining is a rule about players.** Somebody
        // turning up at generation four hundred is exactly what watching is
        // for, so this asks only whether the room is here.
        if let ClientMessage::Watch { room } = &msg {
            let door = self.resolve(Some(room.as_str())).and_then(|id| {
                if self.may_enter(
                    &id,
                    caller.connection,
                    caller.person.as_ref(),
                    Some(room.as_str()),
                ) {
                    Ok(id)
                } else {
                    Err(self.not_here(room.as_str()))
                }
            });
            return match door {
                Ok(id) => {
                    let name = self.name_of(&id).to_string();
                    let server = self.rooms.get(&id).expect("resolve only returns rooms here");
                    log::info!("connection {} is watching \"{name}\" ({id})", caller.connection);
                    vec![ServerMessage::Watching {
                        room: id.clone(),
                        name,
                        tick: server.tick(),
                        world: server.world().kind(),
                        rules: server.rules(),
                    }]
                }
                Err(reason) => vec![ServerMessage::Rejected { reason }],
            };
        }
        // Blowing the whistle on a match. Judged here because this is the only
        // thing that knows who made a room -- a `Server` is one room and has
        // no idea how it came to exist.
        if let ClientMessage::Start = msg {
            let Some((room, player)) = caller.seat.as_ref() else {
                return vec![ServerMessage::NotStarted { reason: "you are not in a match".into() }];
            };
            // Whoever made it. Anybody may join a gathering match; if anybody
            // could also start it, the person who set it up could not wait for
            // their friends to arrive.
            //
            // A room the console made has no owner, so nobody may start it
            // from a client -- which is right: it is the operator's match, and
            // `match start` is theirs.
            if !self.owns(room, caller, *player) {
                return vec![ServerMessage::NotStarted {
                    reason: match self.owner.get(room) {
                        Some(_) => "only whoever made this match can start it".into(),
                        None => "this match is the server's; it starts at the console".into(),
                    },
                }];
            }
            let room = room.clone();
            let server = self.rooms.get_mut(&room).expect("a seat names a room that is here");
            return match server.start_match(Some(*player)) {
                Ok(()) => {
                    log::info!(
                        "connection {} started match \"{}\"",
                        caller.connection,
                        self.name_of(&room)
                    );
                    // Nothing to reply: `start_match` sets `lobby_changed`, so
                    // the next step broadcasts the new phase to everybody in
                    // the room -- including whoever pressed it.
                    Vec::new()
                }
                Err(reason) => vec![ServerMessage::NotStarted { reason }],
            };
        }
        // Calling it off. Judged here for the same reason `Start` is: this is
        // the only thing that knows who made a room.
        if let ClientMessage::EndMatch = msg {
            let Some((room, player)) = caller.seat.as_ref() else {
                return vec![ServerMessage::NotStarted { reason: "you are not in a match".into() }];
            };
            if !self.owns(room, caller, *player) {
                return vec![ServerMessage::NotStarted {
                    reason: match self.owner.get(room) {
                        Some(_) => "only whoever started this match can end it".into(),
                        None => "this match is the server's; it ends at the console".into(),
                    },
                }];
            }
            let room = room.clone();
            let server = self.rooms.get_mut(&room).expect("a seat names a room that is here");
            return match server.end_match() {
                // Nothing to reply: `end_match` sets `lobby_changed`, so the
                // next step broadcasts the result to everybody in the room,
                // including whoever pressed it.
                Ok(()) => {
                    log::info!(
                        "connection {} ended match \"{}\"",
                        caller.connection,
                        self.name_of(&room)
                    );
                    Vec::new()
                }
                Err(reason) => vec![ServerMessage::NotStarted { reason }],
            };
        }
        // Giving up a seat without closing the socket. Handled here because a
        // seat is this map's business — a `Server` is told who left, it does
        // not find out.
        if let ClientMessage::Leave = msg {
            if let Some(seat) = caller.seat.as_ref() {
                log::info!("{:?} left room \"{}\"", seat.1, self.name_of(&seat.0));
                self.leave(seat);
            }
            return Vec::new();
        }
        let seat = caller.seat.as_ref();
        if let ClientMessage::Join { room, person, name: joining_as, .. } = &msg {
            let asked = room.clone();
            let joining_as = joining_as.clone();
            // **Who, before where.** People are a server's table and a room is
            // one world on it, so this is settled once here rather than
            // fifteen times by fifteen rooms each with their own idea of who
            // somebody is. A refusal is a refusal to join at all: a client
            // that offered a key meant to be somebody, and putting them in as
            // a stranger instead would be answering a different question.
            let who = person.as_ref().map(|secret| self.meet(secret));
            // **And whether the door opens for them**, which is the map's
            // question and not the room's -- see `may_enter`.
            let asked_for = asked.as_ref().map(RoomId::as_str);
            let door = self.resolve(asked_for).and_then(|id| {
                if self.may_enter(&id, caller.connection, who.as_ref(), asked_for) {
                    Ok(id)
                } else {
                    Err(self.not_here(asked_for.unwrap_or_default()))
                }
            });
            return match door {
                Ok(name) => {
                    if let Some(seat) = seat {
                        log::info!("{:?} is leaving room \"{}\" for \"{name}\"", seat.1, seat.0);
                        self.leave(seat);
                    }
                    // A `Server` is one room and knows only what it is
                    // called; ids are this map's business, so the id and the
                    // name are stamped on the way out rather than passed in.
                    // Keeping `Server` ignorant of ids is what stops a room
                    // needing to be told its own identity to answer a join.
                    let room_name = self.name_of(&name).to_string();
                    let mut out = self
                        .rooms
                        .get_mut(&name)
                        .expect("resolve only returns rooms that are here")
                        .handle(None, who.as_ref(), msg);
                    // A maker who presented no key at `Create` is recorded at
                    // their first join, which is the first moment there is a
                    // seat to record. By person if they have one now -- see
                    // `Owner`.
                    if self.made.get(&name) == Some(&Some(caller.connection)) {
                        if let Some(ServerMessage::Welcome { you, .. }) =
                            out.iter().find(|m| matches!(m, ServerMessage::Welcome { .. }))
                        {
                            match &who {
                                Some(person) => self.claim_for(&name, person),
                                None => {
                                    self.owner.entry(name.clone()).or_insert(Owner::Seat(*you));
                                }
                            }
                        }
                    }
                    let owner = self.owner_seat(&name);
                    let code = self.codes.get(&name).cloned();
                    // What this server has to say about them, which a room
                    // cannot know: a profile outlives every room here. A join
                    // is also when a name is taken, so a profile can be looked
                    // at before anybody has finished a match.
                    let profile = who.as_ref().and_then(|who| {
                        self.profiles.met(who, &joining_as);
                        self.profile_of(who)
                    });
                    for reply in &mut out {
                        if let ServerMessage::Welcome {
                            room, name: called, profile: ours, ..
                        } = reply
                        {
                            *room = name.clone();
                            *called = room_name.clone();
                            ours.clone_from(&profile);
                        }
                        stamp(reply, owner, code.clone());
                    }
                    // **And what this server merely holds for them**, which is
                    // the other half of a profile and goes only to its owner.
                    // Sent even when it is empty, because an empty one is what
                    // tells a client to offer what it is carrying -- see
                    // `ServerMessage::Yours`.
                    //
                    // Only on a join that was *allowed*: this room resolved,
                    // and the room itself can still refuse -- a match already
                    // under way, or a person already sitting here in another
                    // tab. Handing a locker to a connection that was turned
                    // away would have that tab replace the library of the one
                    // holding the seat.
                    let welcomed = out.iter().any(|m| matches!(m, ServerMessage::Welcome { .. }));
                    if let (Some(who), true) = (&who, welcomed) {
                        out.push(ServerMessage::Yours(self.lockers.of(who)));
                        // **Somebody who came in by the code is in from now
                        // on.** Their address bar says the id, and a refresh
                        // rejoins by it; a door that shut behind them would
                        // make a refresh a refusal.
                        if self.entered_by_code(&name, asked_for) {
                            self.admit(&name, who);
                        }
                    }
                    out
                }
                Err(reason) => {
                    log::info!("refused a join for {asked:?}: {reason}");
                    vec![ServerMessage::Rejected { reason }]
                }
            };
        }

        // A watcher is routed like a player with no number. `Server::handle`
        // already takes `Option<PlayerId>` and already answers a `Subscribe`
        // from nobody with the chunks it asked for, so reading works out of
        // the box — and everything that *acts* is refused for want of an id
        // rather than for want of a check somebody has to remember to write.
        let Some(room) = caller.room() else {
            log::debug!("a message from a connection in no room; dropped");
            return Vec::new();
        };
        let id = seat.map(|(_, id)| *id);
        match self.rooms.get_mut(room) {
            // Not a `Join`: those are answered above, where the person was
            // settled. Everything else is asked by a seat and needs no name.
            Some(server) => server.handle(id, None, msg),
            // Only reachable if a room could go away under a seated player,
            // which nothing does yet. Said out loud rather than ignored,
            // because the symptom would be one client silently going deaf.
            None => {
                log::warn!("{id:?} is in room \"{room}\", which is not here");
                Vec::new()
            }
        }
    }

    /// Everything the rooms want said **now** rather than at the next step,
    /// with the room it belongs to. See [`ServerMessage::Acted`].
    pub fn take_announcements(&mut self) -> Vec<(RoomId, ServerMessage)> {
        let owners = self.owner_seats();
        let codes = self.codes.clone();
        self.rooms
            .iter_mut()
            .flat_map(|(id, server)| {
                let (owner, code) = (owners.get(id).copied().flatten(), codes.get(id).cloned());
                server.take_announcements().into_iter().map(move |mut m| {
                    stamp(&mut m, owner, code.clone());
                    (id.clone(), m)
                })
            })
            .collect()
    }

    /// Advance every room one generation, and say which room each reply
    /// belongs to. A `Step` is only meaningful to the clients in its own
    /// world, so the room travels with it as far as the connection that
    /// decides whether to send it on.
    pub fn step(&mut self, dt: std::time::Duration) -> Vec<(RoomId, ServerMessage)> {
        // Cloned so the stamp below can read them while the rooms are borrowed
        // mutably. Sixteen bytes and a short string per room, once a tick.
        let owners = self.owner_seats();
        let codes = self.codes.clone();
        // **Each room on its own clock.** They shared one, on the reasoning
        // that a room with its own rate would be a second thing to tell a
        // client and a second way to disagree about what a tick is — which was
        // true until the rate became one of the room's `Rules`, and a `Welcome`
        // has carried those all along. So the ticker is a *fine* one now and
        // each room banks time against its own rate, which is exactly what the
        // client already does with `World::update`.
        let mut out: Vec<(RoomId, ServerMessage)> = self
            .rooms
            .iter_mut()
            .flat_map(|(id, server)| {
                let (owner, code) = (owners.get(id).copied().flatten(), codes.get(id).cloned());
                server.owe(dt).into_iter().map(move |mut m| {
                    stamp(&mut m, owner, code.clone());
                    (id.clone(), m)
                })
            })
            .collect();

        // **Rated here rather than in the room that ended.** A rating is a
        // fact about a person and a person outlives every world on this
        // server, so the table belongs beside `people` and not inside a
        // `Server` -- which is one room and, in the case of a match, one that
        // is about to stop existing.
        //
        // On the generation it was decided, once: `just_decided` is true only
        // while the tick that ended it is the current one, so a match cannot
        // be rated twice by sitting there over.
        for (id, server) in &self.rooms {
            if !server.just_decided() {
                continue;
            }
            let finishers = server.finishers();
            let moved = self.profiles.settle(&finishers);
            if moved.is_empty() {
                continue;
            }
            for (who, change) in &moved {
                log::info!("{who} is now rated {} ({change:+})", self.profiles.rating_of(who));
            }
            // Told, not left to be found on the next join: the screen somebody
            // is looking at when a match ends is the one this belongs on.
            out.extend(finishers.iter().filter_map(|f| {
                let change = moved.iter().find(|(who, _)| *who == f.who)?.1;
                Some((
                    id.clone(),
                    ServerMessage::Rated {
                        who: f.who.clone(),
                        rating: self.profiles.rating_of(&f.who),
                        change,
                    },
                ))
            }));
            self.save_profiles();
        }
        out
    }

    pub fn leave(&mut self, (room, id): &Seat) {
        if let Some(server) = self.rooms.get_mut(room) {
            server.leave(*id);
        }
    }

    /// **What every room here runs at**, for rooms nobody has said anything
    /// about — see [`crate::net::Rules::bpm`]. A laboratory may still be set
    /// to something else once it exists; this is the answer it starts from.
    pub fn run_at(&mut self, bpm: u16) {
        for server in self.rooms.values_mut() {
            server.set_rate(bpm);
        }
        self.default_bpm = bpm;
    }

    /// **Make a match for two and hold it for somebody.**
    ///
    /// A room like any other once it exists — private, two sides, and the
    /// challenger in it. What makes it a challenge is only that the server is
    /// holding the way in for one named person, and will tell them the next
    /// time it hears from them.
    fn challenge(&mut self, caller: &Caller, who: &PersonId) -> Vec<ServerMessage> {
        let Some(from) = caller.person.clone() else {
            return refuse("a challenge comes from somebody, and this client has no key");
        };
        if from == *who {
            return refuse("you cannot challenge yourself");
        }
        if !self.people.knows(who) {
            return refuse("this server has never met them");
        }
        if self.challenges.contains_key(who) {
            return refuse("they already have a challenge waiting");
        }

        // Named for the two of them, so the room list -- which does not show
        // it, being private -- and a log line both say what it is.
        let name = format!("{}-v-{}", from.short(), who.short());
        let made = match self.make(
            caller.connection,
            &name,
            WorldKind::Infinite,
            Some(Victory::Territory { squares: crate::net::CHALLENGE_SQUARES }),
            Some(2),
            Reach::Code,
            false,
        ) {
            Ok(made) => made,
            Err(why) => return refuse(&why),
        };

        self.claim_for(&made.id, &from);
        // The room is held for them: the id in the `Challenged` is their way
        // in, and the door has to know it.
        self.admit(&made.id, who);

        let Some(theirs) = self.profile_of(&from) else {
            return refuse("this server has nothing to say about you yet");
        };
        // Recorded *and* queued: the record is what an answer looks up and
        // stands until then, the message is delivered once.
        self.challenges.insert(who.clone(), (from.clone(), made.id.clone()));
        self.waiting
            .entry(who.clone())
            .or_default()
            .push(ServerMessage::Challenged { from: theirs, room: made.id.clone() });
        log::info!("{from} challenged {who} in room {}", made.id);
        vec![ServerMessage::Made(Ok(made))]
    }

    /// **Yes or no, back to whoever asked.**
    ///
    /// A yes is the room, which the challenger already holds — the answer is
    /// worth sending anyway, because "they are coming" is the thing you are
    /// waiting to hear. A no is the same message with no room in it, so a
    /// refusal reaches somebody rather than looking like a server that lost it.
    fn answer(&mut self, caller: &Caller, from: &PersonId, yes: bool) -> Vec<ServerMessage> {
        let Some(me) = caller.person.clone() else {
            return refuse("an answer comes from somebody, and this client has no key");
        };
        // Taken whichever way it is answered: it has been decided, and one
        // left standing would be offered again.
        let Some((asked_by, _)) = self.challenges.get(&me) else {
            return refuse("there is no challenge to answer");
        };
        if asked_by != from {
            return refuse("there is no challenge from them to answer");
        }
        let room = self.challenges.remove(&me).map(|(_, room)| room);

        let Some(mine) = self.profile_of(&me) else {
            return refuse("this server has nothing to say about you yet");
        };
        let told = ServerMessage::Answered { who: mine, room: yes.then(|| room.clone()).flatten() };
        self.waiting.entry(from.clone()).or_default().push(told);
        log::info!("{me} answered {from}'s challenge: {}", if yes { "yes" } else { "no" });
        match (yes, room) {
            // Handed straight back, so accepting is one press: what a client
            // does with it is the `Join` it would have made anyway.
            (true, Some(room)) => vec![ServerMessage::Challenged {
                from: self.profile_of(from).expect("challenged by somebody this server knows"),
                room,
            }],
            _ => Vec::new(),
        }
    }

    /// **Bring somebody in by name.**
    ///
    /// Anybody seated in a private room may, and the invitation waits in the
    /// outbox the way a challenge does. What it changes on the room is one
    /// entry in [`Self::admitted`], which is what lets the person it names
    /// join by the room's id with no code — so an invitation names a person
    /// where a code names nobody, and that is the whole of what it adds.
    fn invite(&mut self, caller: &Caller, who: &PersonId, room: &RoomId) -> Vec<ServerMessage> {
        let Some(from) = caller.person.clone() else {
            return not_done("an invitation comes from somebody, and this client has no key");
        };
        if caller.seat.as_ref().map(|(here, _)| here) != Some(room) {
            return not_done("you can only invite somebody into a room you are in");
        }
        if !self.unlisted.contains(room) {
            return not_done("anybody can join that room from the list");
        }
        if self.parties.party_of(room).is_some() {
            return not_done("this world is its party's; ask them into the party");
        }
        if from == *who {
            return not_done("you are already here");
        }
        if !self.people.knows(who) {
            return not_done("this server has never met them");
        }
        let Some(theirs) = self.profile_of(&from) else {
            return not_done("this server has nothing to say about you yet");
        };
        self.admit(room, who);
        let name = self.name_of(room).to_string();
        self.waiting.entry(who.clone()).or_default().push(ServerMessage::Invited {
            from: theirs,
            room: room.clone(),
            name,
        });
        log::info!("{from} invited {who} into {room}");
        Vec::new()
    }

    /// **Who this secret is, written down if it is new.**
    ///
    /// A secret this server has not seen is a person it has not met, not an
    /// impostor: the client made it, nothing was issued, and the answer is to
    /// issue an id and remember the pairing. There is nothing here that can
    /// fail to check out, which is what a signature bought and what a single
    /// server does not need — see `net::auth`. A person met for the first
    /// time reaches disk at once, because a rating earned by somebody this
    /// server forgets on a restart is worse than none.
    fn meet(&mut self, secret: &Secret) -> PersonId {
        let (who, new) = self.people.meet(secret);
        if new {
            self.save_people();
        }
        who
    }

    /// **Everything held for this caller**, taken as it is handed over.
    ///
    /// Appended to whatever they asked for, because there is no channel to a
    /// person — see [`Self::waiting`]. Any message will do: a client on the
    /// menu is asking for the room list every few seconds and one in a world
    /// is checkpointing.
    fn deliver(&mut self, caller: &Caller) -> Vec<ServerMessage> {
        let Some(who) = &caller.person else { return Vec::new() };
        self.waiting.remove(who).unwrap_or_default()
    }

    /// Write the lockers, and say so if they will not go.
    ///
    /// Not fatal, for the reason [`Self::save_people`] gives: a server that
    /// cannot write is a bad day rather than a reason to refuse everybody
    /// entry. Loud, because the symptom otherwise is somebody's library
    /// quietly not being there next time.
    fn save_lockers(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = self.lockers.save(&stamps_path(&self.dir), &games_path(&self.dir)) {
            log::error!("could not write the lockers: {e}");
        }
    }

    /// Write the parties, and say so if they will not go. Not fatal, for the
    /// reason the tables are not; loud, because the symptom otherwise is a
    /// group finding their worlds gone from under them.
    fn save_parties(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let path = parties_path(&self.dir);
        if let Err(e) = self.parties.save(&path) {
            log::error!("saving the parties to {}: {e}", path.display());
        }
    }

    /// Whether this person is in a room here right now. One server answering
    /// its own members about each other — see [`crate::net::Member::online`].
    fn is_online(&self, who: &PersonId) -> bool {
        self.rooms
            .values()
            .any(|s| s.players().any(|p| p.online && p.person.as_deref() == Some(who.as_str())))
    }

    /// One room as a listing shows it, whichever listing.
    fn room_info(&self, id: &RoomId, server: &Server) -> RoomInfo {
        RoomInfo {
            id: id.clone(),
            name: self.name_of(id).to_string(),
            phase: server.phase().clone(),
            victory: server.victory(),
            players: server.players().filter(|p| p.online).count() as u32,
            world: server.world().kind(),
            rules: server.rules(),
            owner: self.owned_by(id).cloned(),
        }
    }

    /// **The parties this caller is in**, with their people and their worlds —
    /// which is a different answer for everybody who asks, and the reason it
    /// is not the room list.
    fn parties_for(&self, caller: &Caller) -> Vec<ServerMessage> {
        let Some(who) = &caller.person else {
            return vec![ServerMessage::Parties { parties: Vec::new() }];
        };
        let parties = self
            .parties
            .of(who)
            .map(|(id, party)| crate::net::PartyInfo {
                id: id.clone(),
                name: party.name.clone(),
                members: party
                    .members
                    .iter()
                    .map(|who| crate::net::Member {
                        who: who.clone(),
                        name: self.profiles.of(who).name,
                        online: self.is_online(who),
                    })
                    .collect(),
                rooms: party
                    .rooms
                    .iter()
                    .filter_map(|room| Some(self.room_info(room, self.rooms.get(room)?)))
                    .collect(),
            })
            .collect();
        vec![ServerMessage::Parties { parties }]
    }

    /// Make a party with the caller as its first member, and answer with the
    /// listing that now has it.
    fn make_party(&mut self, caller: &Caller, name: &str) -> Vec<ServerMessage> {
        let Some(who) = caller.person.clone() else {
            return not_done("a party is made by somebody, and this client has no key");
        };
        match self.parties.make(name, &who) {
            Ok(id) => {
                self.save_parties();
                log::info!("{who} made party {id} \"{name}\"");
                self.parties_for(caller)
            }
            Err(why) => not_done(&why),
        }
    }

    /// Ask somebody into a party. Recorded as standing and queued once, the
    /// way a challenge is — see [`Self::challenges`] for why those are two.
    fn invite_to_party(
        &mut self,
        caller: &Caller,
        party: &PartyId,
        who: &PersonId,
    ) -> Vec<ServerMessage> {
        let Some(from) = caller.person.clone() else {
            return not_done("an invitation comes from somebody, and this client has no key");
        };
        if from == *who {
            return not_done("you are already in it");
        }
        if !self.people.knows(who) {
            return not_done("this server has never met them");
        }
        let Some(theirs) = self.profile_of(&from) else {
            return not_done("this server has nothing to say about you yet");
        };
        match self.parties.invite(party, &from, who) {
            Ok(name) => {
                self.save_parties();
                self.waiting.entry(who.clone()).or_default().push(ServerMessage::PartyInvite {
                    from: theirs,
                    party: party.clone(),
                    name,
                });
                log::info!("{from} asked {who} into party {party}");
                Vec::new()
            }
            Err(why) => not_done(&why),
        }
    }

    /// Take a standing invitation, and answer with the listing.
    fn join_party(&mut self, caller: &Caller, party: &PartyId) -> Vec<ServerMessage> {
        let Some(who) = caller.person.clone() else {
            return not_done("a party is joined by somebody, and this client has no key");
        };
        match self.parties.join(party, &who) {
            Ok(()) => {
                self.save_parties();
                log::info!("{who} joined party {party}");
                self.parties_for(caller)
            }
            Err(why) => not_done(&why),
        }
    }

    /// Leave, and answer with the listing that no longer has it. The last one
    /// out takes the party with them; its worlds stay, unlisted and their
    /// maker's, and close the way any room closes.
    fn leave_party(&mut self, caller: &Caller, party: &PartyId) -> Vec<ServerMessage> {
        let Some(who) = caller.person.clone() else {
            return not_done("this client has no key, and so is in no party");
        };
        match self.parties.leave(party, &who) {
            Ok(emptied) => {
                self.save_parties();
                log::info!(
                    "{who} left party {party}{}",
                    if emptied { ", which is gone with them" } else { "" }
                );
                self.parties_for(caller)
            }
            Err(why) => not_done(&why),
        }
    }

    /// Write what is known about the client-made rooms, and say so if it will
    /// not go. Not fatal, for the reason the tables are not; loud, because the
    /// symptom otherwise is a private world coming back listed and codeless.
    fn save_meta(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let rows = self.made.keys().map(|id| MetaRow {
            v: META_VERSION,
            id: id.clone(),
            owner: self.owned_by(id).cloned(),
            code: self.codes.get(id).cloned(),
            unlisted: self.unlisted.contains(id),
            admitted: self
                .admitted
                .get(id)
                .map(|in_| in_.iter().cloned().collect())
                .unwrap_or_default(),
        });
        let path = meta_path(&self.dir);
        if let Err(e) = std::fs::write(&path, crate::net::jsonl::write(rows)) {
            log::error!("saving the rooms table to {}: {e}", path.display());
        }
    }

    /// Write the people table, and say so if it will not go.
    ///
    /// Not fatal, deliberately: a server that cannot write its table is a
    /// server where new keys are forgotten on a restart, which is a bad day
    /// rather than a reason to refuse everybody entry. It is loud, because the
    /// symptom otherwise is players quietly becoming strangers.
    fn save_people(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let path = people_path(&self.dir);
        if let Err(e) = self.people.save(&path) {
            log::error!("saving the people table to {}: {e}", path.display());
        }
    }

    /// Write the ratings table, and say so if it will not go. Not fatal, for
    /// the reason `save_people` is not: a server that cannot write a number is
    /// a bad day rather than a reason to refuse everybody entry.
    fn save_profiles(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
        let path = profiles_path(&self.dir);
        if let Err(e) = self.profiles.save(&path) {
            log::error!("saving the ratings to {}: {e}", path.display());
        }
    }

    /// What somebody is rated here, which is what a `Welcome` carries.
    /// What this server has to say about somebody, as a client is told it.
    ///
    /// `None` for an id it never issued, which is a real answer rather than a
    /// failure: a client can ask about anybody, and "not here" is what it
    /// should get for a name it made up.
    pub fn profile_of(&self, who: &crate::net::PersonId) -> Option<crate::net::Profile> {
        if !self.people.knows(who) {
            return None;
        }
        let row = self.profiles.of(who);
        Some(crate::net::Profile {
            who: who.clone(),
            name: row.name.clone(),
            rating: row.rating(),
            provisional: row.provisional(),
            games: row.games,
            history: row.history.clone(),
            best: row.best,
        })
    }

    /// The people this server can vouch for whose name matches, or the best
    /// rated when nothing is asked.
    ///
    /// Only people it has actually **met** — `people.knows` is the same gate
    /// `profile_of` uses, so a fingerprint that reached the ratings table
    /// without ever joining cannot appear here either.
    pub fn people_like(&self, like: &str) -> Vec<crate::net::Profile> {
        self.profiles
            .search(like, crate::net::PEOPLE_MOST)
            .into_iter()
            .filter_map(|who| self.profile_of(&who))
            .collect()
    }

    pub fn rating_of(&self, who: &crate::net::PersonId) -> i32 {
        self.profiles.rating_of(who)
    }

    /// Save every room. One failure does not stop the others: a room that
    /// cannot be written is one world lost, and giving up would lose the rest
    /// as well.
    pub fn save(&self) -> std::io::Result<()> {
        if self.dir.as_os_str().is_empty() {
            return Ok(());
        }
        self.save_people();
        self.save_profiles();
        self.save_lockers();
        self.save_meta();
        self.save_parties();
        let mut first_error = None;
        for (id, server) in &self.rooms {
            // A match is an event rather than a world to keep: it has an end,
            // and a half-finished one restored into a server that has
            // forgotten it was a match would run on forever with nobody able
            // to win it. Losing it on a restart is the honest outcome.
            if !matches!(server.phase(), crate::server::matches::Phase::Open) {
                continue;
            }
            let path = save_path(&self.dir, id);
            if let Err(e) = server.save(&path) {
                log::error!("saving room \"{}\" to {}: {e}", self.name_of(id), path.display());
                first_error.get_or_insert(e);
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Make a room while the server is running.
    ///
    /// The shape is per room, which the command line cannot say: `--torus`
    /// applies to every room a run creates, so a server could not offer a
    /// wrapping world and a boundless one side by side. Made here it can.
    ///
    /// **Saved at once**, before anything is in it. A room that existed only
    /// in memory until the next periodic save would vanish on a crash, and
    /// the person who made it would have no way to tell whether it had ever
    /// been real — worse than a failure, which at least says so.
    ///
    /// An existing name is refused rather than reopened. "Create" that
    /// sometimes means "and empty it" is one keystroke from destroying a world
    /// somebody is standing in.
    pub fn create(&mut self, name: &str, shape: WorldKind) -> Result<RoomId, String> {
        let shape = shape.checked()?;
        let name = crate::net::room_name(name)?;
        // A room made at the console takes its name as its id, so the
        // directory on disk stays readable and `--room arena` reaches the same
        // room after a restart. Only rooms made over the wire get a generated
        // id — that is where a rename has to survive.
        let id = RoomId(name.clone());
        if self.rooms.contains_key(&id) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let server = Server::named(name.clone(), shape.build()).seeded_by(&id);
        let path = save_path(&self.dir, &id);
        if !self.dir.as_os_str().is_empty() {
            server.save(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        log::info!("created room \"{name}\"");
        self.rooms.insert(id.clone(), server);
        self.names.insert(id.clone(), name);
        Ok(id)
    }

    /// Make a match: a room with a beginning, an end and a winner.
    ///
    /// Named like any other room, because a match **is** a room and that is
    /// the name people type to join it, the name `match start` takes, and the
    /// name it is listed under. A generated one would be a second vocabulary
    /// for the same thing.
    ///
    /// An existing name is refused rather than reopened, for the reason
    /// [`Self::create`] refuses one: "make" that sometimes means "and empty
    /// it" is one keystroke from destroying a world somebody is standing in.
    ///
    /// Not saved, so unlike [`Self::create`] there is no file to fail on.
    pub fn new_match(
        &mut self,
        name: &str,
        shape: WorldKind,
        victory: Victory,
    ) -> Result<RoomId, String> {
        let shape = shape.checked()?;
        let name = crate::net::room_name(name)?;
        let id = RoomId(name.clone());
        if self.rooms.contains_key(&id) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let mut server = Server::named(name.clone(), shape.build()).seeded_by(&id);
        server.make_match(victory);
        log::info!("made match \"{name}\": {}", victory.describe());
        self.rooms.insert(id.clone(), server);
        self.names.insert(id.clone(), name);
        Ok(id)
    }

    /// Make a room because a client asked for one.
    ///
    /// The same two calls the console makes, behind the cap and the owner
    /// record that the console does not need. `victory` is the whole of the
    /// difference between a world and a match, which is why this is one
    /// function where the console has two commands: the console is a
    /// vocabulary people type and reads better split, and this is one message.
    ///
    /// Refusals are the wording [`Self::create`] and [`Self::new_match`]
    /// already produce, so a client is told the same thing an operator would
    /// be — including that a name is taken, which is the common case and the
    /// one worth reading.
    pub fn make(
        &mut self,
        by: ConnectionId,
        name: &str,
        shape: WorldKind,
        victory: Option<Victory>,
        teams: Option<u8>,
        reach: Reach,
        laboratory: bool,
    ) -> Result<Made, String> {
        // Checked before the name is, so a server that is full says so rather
        // than arguing about a name it was never going to use. Counted over
        // rooms made this way and not over every room: an operator who
        // declared forty has made a decision.
        if self.made.len() >= self.max_made {
            return Err(format!(
                "this server is holding {} rooms made by players, which is all it will",
                self.made.len()
            ));
        }
        // **The shape, before anything is built out of it.** It arrives on a
        // socket from a connection that has not joined anything, and a torus
        // is allocated whole -- so `rows: 0` reached an `assert!` and
        // `100000x100000` overflowed the multiply that sizes the allocation.
        // Either one killed the simulation task, which owns every room in the
        // process. See `WorldKind::checked`.
        let shape = shape.checked()?;
        // A name is still a name on a private room. What used to happen here
        // was that the code *became* the name, which conflated a credential
        // with an identity: the code could never be changed, and somebody
        // making a game for four friends could not call it anything.
        let name = crate::net::room_name(name).or_else(|e| {
            // A private room may go unnamed, since nobody browses for it.
            if reach != Reach::Listed && name.trim().is_empty() {
                Ok(UNNAMED.to_string())
            } else {
                Err(e)
            }
        })?;
        if self.names.values().any(|n| *n == name) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let id = self.free_id()?;
        let mut server = Server::named(name.clone(), shape.build()).seeded_by(&id);
        if let Some(victory) = victory {
            server.make_match(victory);
        }
        // **A laboratory is a room like any other**, which is the whole point:
        // the clock and the two placing rules are things this room does, so
        // several people can be in one and there is nothing offline about it.
        // A way to win takes precedence, because a match with the rules off is
        // not a match.
        if laboratory && victory.is_none() {
            server.make_laboratory();
        }
        // **A world may have teams.** What a team is, is people playing as one
        // player, and that is worth having without a result to win: a world
        // with two teams is two shared kingdoms rather than fifteen small
        // ones. Made before anybody joins either way, because a team takes a
        // number out of the same pool the seats do.
        if let Some(n) = teams {
            server.make_teams(n)?;
        }
        let path = save_path(&self.dir, &id);
        if victory.is_none() && !self.dir.as_os_str().is_empty() {
            server.save(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        self.rooms.insert(id.clone(), server);
        self.names.insert(id.clone(), name.clone());
        let code = match &reach {
            Reach::Listed => None,
            Reach::Code => {
                self.unlisted.insert(id.clone());
                let code = self.free_code()?;
                self.codes.insert(id.clone(), code.clone());
                Some(code)
            }
            // No code: the party is the list of who may come in.
            Reach::Party(party) => {
                self.unlisted.insert(id.clone());
                self.parties.attach(party, &id);
                self.save_parties();
                None
            }
        };
        self.made.insert(id.clone(), Some(by));
        self.save_meta();
        log::info!(
            "connection {by} made {} room \"{name}\" ({id}){}",
            match &reach {
                Reach::Listed => "an open".to_string(),
                Reach::Code => "a private".to_string(),
                Reach::Party(party) => format!("party {party}'s"),
            },
            code.as_ref().map(|c| format!(", code {c}")).unwrap_or_default()
        );
        Ok(Made { id, name, code })
    }

    /// An id no room is using.
    ///
    /// Spelled `r-` and a code, so a generated id can never collide with a
    /// name somebody chose — `room_name` forbids nothing about a leading `r-`,
    /// but nobody types one, and [`Self::resolve`] tries ids before names.
    fn free_id(&self) -> Result<RoomId, String> {
        (0..10)
            .map(|_| RoomId(format!("r-{}", code())))
            .find(|id| !self.rooms.contains_key(id))
            .ok_or_else(|| "could not find a free room id".to_string())
    }

    /// A code no room is using.
    ///
    /// Retried rather than assumed unique: a collision is vanishingly
    /// unlikely and silently reopening somebody else's private room would be
    /// the worst possible way to find out it was not impossible. Giving up
    /// after a few tries rather than looping, because a server that cannot
    /// find a free code in ten attempts has something wrong with it that a
    /// tighter loop will not fix.
    fn free_code(&self) -> Result<Code, String> {
        (0..10)
            .map(|_| code())
            .find(|c| !self.codes.values().any(|used| used == c))
            .ok_or_else(|| "could not find a free code".to_string())
    }

    /// Whether this room is kept out of the listing.
    pub fn is_unlisted(&self, id: &RoomId) -> bool {
        self.unlisted.contains(id)
    }

    /// Whether this caller, sitting in `seat`, is who this room belongs to.
    ///
    /// Asked of the person first: a key presented on any socket is the maker,
    /// whichever seat it was given. The seat is the answer only for a maker
    /// this server never knew, and then a seat is all there is to ask.
    fn owns(&self, room: &RoomId, caller: &Caller, seat: PlayerId) -> bool {
        match self.owner.get(room) {
            Some(Owner::Person(who)) => caller.person.as_ref() == Some(who),
            Some(Owner::Seat(theirs)) => *theirs == seat,
            None => false,
        }
    }

    /// The owner as the seat they hold in the room, which is what a lobby
    /// shows: a client compares it with its own number. `None` for a room with
    /// no owner, and for a keyed owner who has no seat there yet.
    fn owner_seat(&self, room: &RoomId) -> Option<PlayerId> {
        match self.owner.get(room)? {
            Owner::Seat(seat) => Some(*seat),
            Owner::Person(who) => self.rooms.get(room)?.seat_of(who),
        }
    }

    /// [`Self::owner_seat`] for every room at once, taken before the rooms
    /// are borrowed mutably to be stepped.
    fn owner_seats(&self) -> BTreeMap<RoomId, Option<PlayerId>> {
        self.rooms.keys().map(|id| (id.clone(), self.owner_seat(id))).collect()
    }

    /// Which connection asked for this room, if a client did and this process
    /// saw it.
    pub fn made_by(&self, id: &RoomId) -> Option<ConnectionId> {
        self.made.get(id).copied().flatten()
    }

    /// Whose room this is, by key, if a keyed person made it.
    pub fn owned_by(&self, id: &RoomId) -> Option<&PersonId> {
        match self.owner.get(id)? {
            Owner::Person(who) => Some(who),
            Owner::Seat(_) => None,
        }
    }

    /// Record the maker of a room they have just made, if they have a key.
    ///
    /// At `Create` rather than at the first join, because with a `Hello` the
    /// person is known before there is a seat -- and a maker who makes a room
    /// and then closes the tab has still made it.
    fn claim(&mut self, id: &RoomId, caller: &Caller) {
        if let Some(who) = &caller.person {
            self.claim_for(id, who);
        }
    }

    /// The same, for a person named directly. Written to disk at once: a room
    /// whose owner is lost on a restart is a room nobody can close.
    fn claim_for(&mut self, id: &RoomId, who: &PersonId) {
        if self.owner.contains_key(id) {
            return;
        }
        self.owner.insert(id.clone(), Owner::Person(who.clone()));
        self.save_meta();
    }

    /// How many rooms clients have made, and how many they may.
    pub fn made_count(&self) -> (usize, usize) {
        (self.made.len(), self.max_made)
    }

    /// Override the cap, from `--max-rooms`.
    pub fn cap_made(&mut self, max: usize) {
        self.max_made = max;
    }

    /// Start a named match's clock.
    pub fn start_match(&mut self, name: &str) -> Result<RoomId, String> {
        let id = self.resolve(Some(name))?;
        let server = self.rooms.get_mut(&id).expect("resolve only returns rooms that are here");
        server.start_match(None)?;
        log::info!("match \"{}\" started at tick {}", server.room(), server.tick());
        Ok(id)
    }

    /// Start the one match that is waiting.
    ///
    /// A convenience for the common case, and it refuses rather than guesses
    /// when there is more than one: starting the wrong match is not something
    /// that can be taken back.
    pub fn dispatch(&mut self) -> Result<RoomId, String> {
        let waiting: Vec<RoomId> = self
            .rooms
            .iter()
            .filter(|(_, s)| matches!(s.phase(), Phase::Gathering))
            .map(|(id, _)| id.clone())
            .collect();
        match waiting.as_slice() {
            [] => Err("no match is waiting to start".into()),
            [only] => self.start_match(only.as_str()),
            several => Err(format!(
                "{} matches are waiting; name one of {}",
                several.len(),
                several.iter().map(|id| self.name_of(id)).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Remove a room and the file it was saved to.
    ///
    /// **Refused while anybody is in it.** Deleting a world somebody is
    /// standing in is the one thing here that cannot be taken back, and the
    /// difference between "nobody is in it" and "nobody was in it a moment
    /// ago" is a question the person typing can answer and this cannot.
    ///
    /// The default room is refused too: `resolve(None)` sends every client
    /// that names no room to it, so a server without one has nowhere to put
    /// anybody.
    pub fn delete(&mut self, name: &str) -> Result<RoomId, String> {
        let id = self.resolve(Some(name))?;
        let name = self.name_of(&id).to_string();
        if id == self.default_room {
            return Err(format!(
                "\"{name}\" is the default room; every client that names none goes there"
            ));
        }
        let server = self.rooms.get(&id).expect("resolve only returns rooms that are here");
        let here = server.players().filter(|p| p.online).count();
        if here > 0 {
            return Err(format!("{here} still in \"{name}\""));
        }
        self.rooms.remove(&id);
        if !self.dir.as_os_str().is_empty() {
            let path = save_path(&self.dir, &id);
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("removed room \"{name}\" but not {}: {e}", path.display());
                }
            }
        }
        // Forgotten here too, or a server that made and deleted its cap's
        // worth of rooms would refuse to make another while holding none.
        self.made.remove(&id);
        self.owner.remove(&id);
        self.unlisted.remove(&id);
        self.codes.remove(&id);
        self.admitted.remove(&id);
        self.names.remove(&id);
        self.save_meta();
        self.parties.detach(&id);
        self.save_parties();
        log::info!("deleted room \"{name}\"");
        Ok(id)
    }

    /// **Close a room from a client**, behind the owner check.
    ///
    /// Routed to [`Self::delete`], which refuses while anybody is in it and
    /// refuses the default room, so this is something done from the menu once
    /// everybody has left. A room the console made is the operator's and
    /// closes there. A seat-keyed owner can never pass, and the arm is here
    /// so the refusal is the true one: the seat is theirs only while they sit
    /// in it, and a room with them in it will not close — so closing needs a
    /// key.
    fn close(&mut self, caller: &Caller, room: &RoomId) -> Result<RoomId, String> {
        let id = self.resolve(Some(room.as_str()))?;
        match self.owner.get(&id) {
            Some(Owner::Person(who)) if caller.person.as_ref() == Some(who) => {}
            Some(Owner::Seat(seat)) if caller.seat.as_ref() == Some(&(id.clone(), *seat)) => {}
            Some(_) => return Err("only whoever made this room can close it".into()),
            None => return Err("this room is the server's; it closes at the console".into()),
        }
        let closed = self.delete(id.as_str())?;
        log::info!("connection {} closed room {closed}", caller.connection);
        Ok(closed)
    }

    /// Stop or start a world.
    pub fn set_asleep(&mut self, name: &str, asleep: bool) -> Result<RoomId, String> {
        let id = self.resolve(Some(name))?;
        let server = self.rooms.get_mut(&id).expect("resolve only returns rooms that are here");
        server.set_asleep(asleep)?;
        log::info!("room \"{}\" is {}", self.name_of(&id), if asleep { "asleep" } else { "awake" });
        Ok(id)
    }

    /// Every room that is not a match, and whether it is running.
    pub fn worlds(&self) -> Vec<(&str, WorldKind, usize, bool)> {
        self.rooms
            .iter()
            .filter(|(_, s)| matches!(s.phase(), Phase::Open))
            .map(|(name, s)| {
                (
                    name.as_str(),
                    s.world().kind(),
                    s.players().filter(|p| p.online).count(),
                    s.is_asleep(),
                )
            })
            .collect()
    }

    /// Every match, and what it is doing.
    pub fn matches(&self) -> Vec<(&str, &Phase, Option<Victory>, usize)> {
        self.rooms
            .iter()
            .filter(|(_, s)| !matches!(s.phase(), Phase::Open))
            .map(|(name, s)| {
                (name.as_str(), s.phase(), s.victory(), s.players().filter(|p| p.online).count())
            })
            .collect()
    }

    /// Every room, as a menu needs to see it.
    ///
    /// In name order, because `rooms` is a `BTreeMap` and a listing that
    /// reordered itself between two requests would be a menu whose buttons
    /// move under the pointer.
    /// Every room anybody may see, which is every room but the private ones.
    ///
    /// Filtered here rather than at the call site because this is the only
    /// listing there is: the menu shows what comes back, the console prints
    /// it, and a refusal names it. One place that decides what is public is
    /// one place to be wrong.
    /// Every room, private ones included, with whether each is unlisted.
    ///
    /// For the **console** and nothing else. Whoever is running the server can
    /// already read the save directory and the log, so hiding a room from them
    /// would be theatre — and an operator who cannot see a room cannot delete
    /// one that is being misused.
    pub fn everything(&self) -> Vec<(RoomInfo, bool)> {
        self.rooms
            .iter()
            .map(|(id, server)| (self.room_info(id, server), self.unlisted.contains(id)))
            .collect()
    }

    pub fn listing(&self) -> Vec<RoomInfo> {
        self.rooms
            .iter()
            .filter(|(id, _)| !self.unlisted.contains(*id))
            .map(|(id, server)| self.room_info(id, server))
            .collect()
    }

    /// How many players are connected right now, across every room.
    pub fn online(&self) -> usize {
        self.rooms.values().map(|s| s.players().filter(|p| p.online).count()).sum()
    }
}

/// Put what only this map knows onto a message on its way out.
///
/// **One place**, called from both paths, and that is the point rather than
/// tidiness: it was two, the broadcast one was lost in an unrelated edit, and
/// the symptom was that whoever made a match was never told it was theirs to
/// start — so the button never appeared and the match could not be started at
/// all. A `Server` is one room and knows neither who asked for it nor what
/// code reaches it; those are facts about the map it sits in.
fn stamp(msg: &mut ServerMessage, owner: Option<PlayerId>, code: Option<Code>) {
    if let ServerMessage::Match(lobby) = msg {
        lobby.owner = owner;
        lobby.code = code;
    }
}

/// One code, from [`CODE_ALPHABET`].
///
/// Each character is drawn from its own `RandomState`, hashed together with
/// the position. `RandomState::new()` reuses one process-wide key and varies a
/// counter, so consecutive draws are **correlated** — taking one `u64` and
/// dividing it down, which is what this used to do and what [`new_token`] in
/// `server::mod` still does, spreads that correlation across every character
/// of one code. Hashing the counter breaks it up.
///
/// Still not a cryptographic generator, and it does not need to be: see
/// [`CODE_LEN`] for what a code is and is not. What this buys is that two
/// codes minted in the same second are not neighbours.
///
/// [`new_token`]: crate::server
fn code() -> Code {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    (0..CODE_LEN)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_usize(i);
            let n = h.finish() % CODE_ALPHABET.len() as u64;
            CODE_ALPHABET[n as usize] as char
        })
        .collect()
}

/// The format of a row in `rooms.jsonl`.
const META_VERSION: u8 = 1;

/// **What is known about a client-made room that its world does not hold.**
///
/// One row per room in [`Rooms::made`], beside the `.ckw` files and in the
/// shape of the other tables -- see [`crate::net::jsonl`]. Whose it is, what
/// code reaches it and whether the listing mentions it are facts about the
/// map a room sits in, not about the world, so a save of the world could not
/// carry them; without this a private world came back from a restart listed,
/// codeless and nobody's.
///
/// The owner is a person or nothing. A seat is not written, because a seat
/// means nothing after a restart.
#[derive(Serialize, Deserialize)]
struct MetaRow {
    v: u8,
    id: RoomId,
    owner: Option<PersonId>,
    code: Option<Code>,
    unlisted: bool,
    /// Sorted, so two saves of one table are the same bytes.
    #[serde(default)]
    admitted: Vec<PersonId>,
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join("rooms.jsonl")
}

/// Read the rows back. A table that is not there is an empty one, and a row
/// this build cannot read is skipped, as with every table here.
fn load_meta(path: &Path) -> std::io::Result<Vec<MetaRow>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(crate::net::jsonl::read::<MetaRow>(&text, "the rooms file")
            .into_iter()
            .filter(|row| row.v == META_VERSION)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Where the people table lives: beside the rooms rather than inside one,
/// because a person is not a room's business and outlives every world here.
///
/// A plain extension, so `ls` on the directory says which file is which and
/// `saved_in` -- which looks for `.ckw` -- never mistakes it for a world.
fn people_path(dir: &Path) -> PathBuf {
    dir.join("people.jsonl")
}

fn profiles_path(dir: &Path) -> PathBuf {
    dir.join("profiles.jsonl")
}

/// The patterns each person has saved. Beside the profiles and not inside
/// them, because nobody else is ever shown one — see
/// [`crate::server::lockers`].
fn stamps_path(dir: &Path) -> PathBuf {
    dir.join("stamps.jsonl")
}

/// And what each of them has played, which is theirs in the same way.
fn games_path(dir: &Path) -> PathBuf {
    dir.join("games.jsonl")
}

/// The parties, which are about several people at once and so are neither a
/// room's nor a person's — see [`crate::server::parties`].
fn parties_path(dir: &Path) -> PathBuf {
    dir.join("parties.jsonl")
}

fn save_path(dir: &Path, room: &RoomId) -> PathBuf {
    dir.join(format!("{room}.{SAVE_EXT}"))
}

/// Room names with a save file already in `dir`.
///
/// A missing directory is no rooms rather than an error — a server started
/// somewhere new has no saves and that is the ordinary case. A file whose name
/// is not a room name is skipped with a warning: it is somebody else's file,
/// or a room named before the rules were, and refusing to start over it would
/// take the whole server down for one stray name.
fn saved_in(dir: &Path) -> std::io::Result<Vec<RoomName>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut names = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some(SAVE_EXT) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        match crate::net::room_name(stem) {
            // Compared against the stem as written, not just validated: a name
            // that only becomes legal after normalising would be a room whose
            // file is called one thing and whose players call it another, and
            // the next save would write a second file beside the first.
            Ok(name) if name == stem => names.push(name),
            _ => log::warn!(
                "{} is not a room name, so it is not a room; ignoring it",
                path.display()
            ),
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole generation's worth of time, so one call to `Rooms::step` is one
    /// generation whatever rate a room is set to — see `Server::owe`.
    fn a_generation() -> std::time::Duration {
        std::time::Duration::from_secs_f32(crate::net::Rules::default().generation_span())
    }
    use crate::sim::World;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ck-rooms-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_declared_room_exists_and_an_undeclared_one_does_not() {
        let rooms = Rooms::open(
            temp_dir("declared"),
            &["lobby".into(), "arena".into()],
            WorldKind::Infinite,
            true,
        )
        .unwrap();

        assert_eq!(rooms.names().collect::<Vec<_>>(), ["arena", "lobby"]);
        assert_eq!(rooms.default_room().as_str(), "lobby", "the first declared is the default");
        assert_eq!(rooms.resolve(None).unwrap().as_str(), "lobby");
        assert_eq!(
            rooms.resolve(Some("ARENA")).unwrap().as_str(),
            "arena",
            "names fold to lowercase"
        );

        // A typo is refused, and the refusal says what is actually here --
        // which, with no menu yet, is the only way a player finds out.
        let why = rooms.resolve(Some("loby")).unwrap_err();
        assert!(why.contains("arena") && why.contains("lobby"), "{why}");
    }

    #[test]
    fn a_name_that_could_escape_the_directory_is_not_a_room() {
        let rooms = Rooms::open(temp_dir("escape"), &[], WorldKind::Infinite, true).unwrap();
        for bad in ["../elsewhere", "a/b", "", "with space", &"x".repeat(64)] {
            assert!(rooms.resolve(Some(bad)).is_err(), "{bad:?} should not be a room");
        }
    }

    /// The whole point of separate worlds: what happens in one is not visible
    /// in the other, and a player number means nothing without its room.
    #[test]
    fn two_rooms_are_two_worlds() {
        let mut rooms =
            Rooms::open(temp_dir("two"), &["a".into(), "b".into()], WorldKind::Infinite, true)
                .unwrap();

        let a = rooms.get_mut(&RoomId::from("a")).unwrap().join("alice").unwrap();
        let b = rooms.get_mut(&RoomId::from("b")).unwrap().join("bob").unwrap();
        assert_eq!((a, b), (PlayerId(1), PlayerId(1)), "numbers are per room");

        assert_eq!(rooms.get(&RoomId::from("a")).unwrap().player_count(), 1);
        assert_eq!(rooms.get(&RoomId::from("b")).unwrap().player_count(), 1);

        // Alice's ground is in her world and nowhere else. Both players hold
        // number one, so a shared world would have them standing on it
        // together and this would pass for the wrong reason -- hence the
        // second player's own room being checked for emptiness too.
        rooms.get_mut(&RoomId::from("a")).unwrap().step();
        let (row, col) = crate::net::spawn_for(a, rooms.get(&RoomId::from("a")).unwrap().world());
        assert!(rooms.get(&RoomId::from("a")).unwrap().world().cell_at(row, col).is_some());
        assert_eq!(
            rooms.get(&RoomId::from("b")).unwrap().world().generation,
            0,
            "stepping one room does not step the other"
        );
    }

    #[test]
    fn every_room_steps_and_says_which_it_was() {
        let mut rooms =
            Rooms::open(temp_dir("step"), &["a".into(), "b".into()], WorldKind::Infinite, true)
                .unwrap();
        let stepped = rooms.step(a_generation());
        let names: Vec<&str> = stepped.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(names, ["a", "b"], "one Step per room, each labelled");
        assert_eq!(rooms.get(&RoomId::from("a")).unwrap().tick(), 1);
        assert_eq!(rooms.get(&RoomId::from("b")).unwrap().tick(), 1);
    }

    /// A room is a file, so a restart finds it without being told again.
    #[test]
    fn a_saved_room_comes_back_without_being_declared() {
        let dir = temp_dir("saved");
        {
            let mut rooms = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
            rooms.get_mut(&RoomId::from("kept")).unwrap().join("alice").unwrap();
            rooms.step(a_generation());
            rooms.save().unwrap();
        }

        let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        assert!(back.get(&RoomId::from("kept")).is_some(), "the file is the declaration");
        assert_eq!(back.get(&RoomId::from("kept")).unwrap().tick(), 1, "and it kept its tick");
        assert!(
            back.get(&RoomId::from(DEFAULT_ROOM)).is_some(),
            "a server always has somewhere to put a client that named no room"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--fresh` is for starting over, so it must not leave one room's save
    /// standing while the rest begin again.
    #[test]
    fn fresh_ignores_every_room_on_disk() {
        let dir = temp_dir("fresh");
        {
            let mut rooms = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
            rooms.step(a_generation());
            rooms.save().unwrap();
        }
        let back = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
        assert_eq!(back.get(&RoomId::from("kept")).unwrap().tick(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `Join` carries its own room, so it needs no seat; anything else from
    /// a connection that has not joined names no world and is dropped rather
    /// than answered out of the default room.
    #[test]
    fn a_message_from_nobody_is_answered_only_if_it_is_a_join() {
        let mut rooms =
            Rooms::open(temp_dir("route"), &["hall".into()], WorldKind::Infinite, true).unwrap();

        let replies = rooms.handle(
            &Caller::nobody(),
            ClientMessage::Join {
                name: "alice".into(),
                room: Some(RoomId::from("hall")),
                person: None,
            },
        );
        let [ServerMessage::Welcome { room, world, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        assert_eq!(room.as_str(), "hall", "the welcome names the room it let you into");
        assert_eq!(*world, WorldKind::Infinite);

        assert!(
            rooms
                .handle(&Caller::nobody(), ClientMessage::Subscribe { chunks: vec![(0, 0)] })
                .is_empty(),
            "an unjoined connection may not read a world"
        );

        // And a `Hello`, which names no world either: it says who is asking,
        // and is answered with who this server takes them to be.
        let replies = rooms.handle(
            &Caller::nobody(),
            ClientMessage::Hello { name: "alice".into(), person: Secret::new().unwrap() },
        );
        let [ServerMessage::You(profile)] = &replies[..] else {
            panic!("expected to be told who we are, got {replies:?}");
        };
        assert_eq!(profile.name, "alice", "the name rode with the hello");
        assert!(rooms.people.knows(&profile.who), "a hello is a meeting");
    }

    /// **A person on the menu is somebody.** A `Hello` names a person with no
    /// seat, and whatever was waiting for them rides out with the answer rather
    /// than with the next room list — so a challenge reaches a client that has
    /// opened the page and joined nothing.
    #[test]
    fn a_hello_names_a_person_and_hands_over_what_is_waiting() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (a, b) = (Secret::new().unwrap(), Secret::new().unwrap());
        let hello =
            |s: &Secret| ClientMessage::Hello { name: "somebody".into(), person: s.clone() };

        // Both met by saying hello and nothing else: no room was ever joined.
        let out = rooms.handle(&Caller::new(1), hello(&a));
        let [ServerMessage::You(a_profile)] = &out[..] else { panic!("{out:?}") };
        let out = rooms.handle(&Caller::new(2), hello(&b));
        let [ServerMessage::You(b_profile)] = &out[..] else { panic!("{out:?}") };
        let (a_id, b_id) = (a_profile.who.clone(), b_profile.who.clone());
        assert_ne!(a_id, b_id);

        // The same secret is the same person, said twice.
        let out = rooms.handle(&Caller::new(3), hello(&a));
        let [ServerMessage::You(again)] = &out[..] else { panic!("{out:?}") };
        assert_eq!(again.who, a_id, "a second hello renamed somebody");

        rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Challenge { who: b_id.clone() },
        );

        // A fresh socket for b, which has said nothing yet: the hello is the
        // first word, and the challenge comes back with it.
        let out = rooms.handle(&Caller::new(4), hello(&b));
        assert!(matches!(&out[..], [ServerMessage::You(_), ..]), "{out:?}");
        let told = out.iter().find_map(|m| match m {
            ServerMessage::Challenged { from, .. } => Some(from.who.clone()),
            _ => None,
        });
        assert_eq!(told, Some(a_id), "the challenge did not ride out with the hello: {out:?}");
    }

    /// The menu's whole reason for being able to show anything. Asked before
    /// joining, because a room is a world and picking one after you are in it
    /// is picking too late.
    #[test]
    fn the_rooms_can_be_listed_without_joining_one() {
        let mut rooms = Rooms::open(
            temp_dir("listing"),
            &["lobby".into(), "arena".into()],
            WorldKind::Toroidal { rows: 4, cols: 4 },
            true,
        )
        .unwrap();
        rooms.get_mut(&RoomId::from("arena")).unwrap().join("alice").unwrap();

        let replies = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
        let [ServerMessage::Rooms { rooms: listed, .. }] = &replies[..] else {
            panic!("expected a listing, got {replies:?}");
        };
        assert_eq!(
            listed.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["arena", "lobby"],
            "in one order, so the buttons do not move under the pointer"
        );
        assert_eq!(listed[0].players, 1, "connected now");
        assert_eq!(listed[1].players, 0);
        assert_eq!(listed[0].world, WorldKind::Toroidal { rows: 4, cols: 4 });

        // A player who left is not a player who is there.
        rooms.get_mut(&RoomId::from("arena")).unwrap().leave(PlayerId(1));
        let [ServerMessage::Rooms { rooms: listed, .. }] =
            &rooms.handle(&Caller::nobody(), ClientMessage::Rooms)[..]
        else {
            panic!("expected a listing");
        };
        assert_eq!(listed[0].players, 0);
    }

    /// Joining twice on one connection is a room change, not a second player
    /// left standing in the first room — where, being marked online, they
    /// could never be returned to by their token.
    #[test]
    fn joining_again_leaves_the_room_it_came_from() {
        let mut rooms =
            Rooms::open(temp_dir("move"), &["a".into(), "b".into()], WorldKind::Infinite, true)
                .unwrap();

        let join = |room: &str| ClientMessage::Join {
            name: "alice".into(),
            room: Some(room.into()),
            person: None,
        };

        let replies = rooms.handle(&Caller::nobody(), join("a"));
        let [ServerMessage::Welcome { you, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        let seat: Seat = ("a".into(), *you);

        rooms.handle(&Caller::sitting(1, seat.clone()), join("b"));
        assert!(
            !rooms.get(&RoomId::from("a")).unwrap().players().any(|p| p.online),
            "nobody is left standing in the room she left"
        );
        assert!(rooms.get(&RoomId::from("b")).unwrap().players().any(|p| p.online));

        // A refused change leaves her where she was. Her client learns where
        // it is from the Welcome it will not get, so anything else would have
        // the two disagreeing about which world she is in.
        let seat: Seat = ("b".into(), PlayerId(1));
        let replies = rooms.handle(&Caller::sitting(1, seat.clone()), join("nowhere"));
        assert!(matches!(replies[..], [ServerMessage::Rejected { .. }]));
        assert!(
            rooms.get(&RoomId::from("b")).unwrap().players().any(|p| p.online),
            "a refused join must not empty the room she was in"
        );
    }

    /// The shape of the world reaches the client, which is the only way it can
    /// build one that folds at the same place the server's does.
    #[test]
    fn a_welcome_from_a_wrapping_room_says_so() {
        let mut rooms = Rooms::just(Server::named("ring", World::toroidal_empty(4, 6)));
        let replies = rooms.handle(
            &Caller::nobody(),
            ClientMessage::Join { name: "alice".into(), room: None, person: None },
        );
        let [ServerMessage::Welcome { world, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        assert_eq!(*world, WorldKind::Toroidal { rows: 4, cols: 6 });
    }

    /// The whole of client-made rooms, in one exchange: ask, get a name back,
    /// join that name. Making does not seat you, so the second half is the
    /// same `Join` the room list sends.
    #[test]
    fn a_client_can_make_a_room_and_then_join_it() {
        let mut rooms =
            Rooms::open(temp_dir("made"), &["hall".into()], WorldKind::Infinite, true).unwrap();
        let me = Caller::new(7);

        let replies = rooms.handle(
            &me,
            ClientMessage::Create {
                // Typed with a capital and a space around it, because that is
                // what a text field hands you and the name that comes back is
                // the one that has to be joined.
                name: "  Arena  ".into(),
                shape: WorldKind::Toroidal { rows: 4, cols: 6 },
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: None,
            },
        );
        let [ServerMessage::Made(Ok(made))] = &replies[..] else {
            panic!("expected a room, got {replies:?}");
        };
        assert_eq!(made.name, "arena", "trimmed and lowercased, the way it will be shown");
        assert_eq!(made.code, None, "an open room needs no code");
        assert_eq!(rooms.made_by(&made.id), Some(7), "and it remembers who asked");

        let made_id = made.id.clone();
        let replies = rooms.handle(
            &me,
            ClientMessage::Join { name: "alice".into(), room: Some(made_id.clone()), person: None },
        );
        let [ServerMessage::Welcome { room, name, world, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        assert_eq!(*room, made_id, "joined by the id it was given");
        assert_eq!(name, "arena", "and told what it is called");
        assert_eq!(*world, WorldKind::Toroidal { rows: 4, cols: 6 }, "the shape it asked for");
    }

    /// A win condition is the whole of the difference between a world and a
    /// match, so one message makes either.
    #[test]
    fn a_victory_makes_a_match_and_no_victory_makes_a_world() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let me = Caller::new(1);

        rooms.make(1, "plain", WorldKind::Infinite, None, None, Reach::Listed, false).unwrap();
        rooms
            .make(
                1,
                "cup",
                WorldKind::Infinite,
                Some(Victory::Territory { squares: 500 }),
                None,
                Reach::Listed,
                false,
            )
            .unwrap();

        let [ServerMessage::Rooms { rooms: listed, .. }] =
            &rooms.handle(&me, ClientMessage::Rooms)[..]
        else {
            panic!("expected a listing");
        };
        let find = |name: &str| listed.iter().find(|r| r.name == name).expect(name).clone();
        assert_eq!(find("plain").phase, Phase::Open, "a world is open and stays open");
        assert_eq!(find("plain").victory, None);
        assert_eq!(find("cup").phase, Phase::Gathering, "a match waits for a whistle");
        assert_eq!(find("cup").victory, Some(Victory::Territory { squares: 500 }));
    }

    /// A name already taken is refused in the client's own words, because "there
    /// is already a room called that" is the common failure and the one a player
    /// can act on.
    #[test]
    fn a_name_that_is_taken_is_refused_and_nothing_is_made() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let before = rooms.len();

        let replies = rooms.handle(
            &Caller::new(2),
            ClientMessage::Create {
                name: "hall".into(),
                shape: WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: None,
            },
        );
        let [ServerMessage::Made(Err(why))] = &replies[..] else {
            panic!("expected a refusal, got {replies:?}");
        };
        assert!(why.contains("hall"), "the refusal names the room: {why}");
        assert_eq!(rooms.len(), before, "and nothing was made");
        assert_eq!(rooms.made_by(&RoomId::from("hall")), None, "an existing room gets no owner");
    }

    /// The backstop. A server anybody can fill is a server that steps a
    /// simulation four times a second for nobody, once per room, forever.
    #[test]
    fn the_cap_is_on_rooms_players_made_and_not_on_the_operators() {
        let mut rooms = Rooms::open(
            temp_dir("cap"),
            &["one".into(), "two".into(), "three".into()],
            WorldKind::Infinite,
            true,
        )
        .unwrap();
        rooms.cap_made(2);
        assert_eq!(rooms.len(), 3, "three declared, none of them counted");

        assert!(rooms.make(1, "a", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
        assert!(rooms.make(1, "b", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
        let (made, cap) = rooms.made_count();
        assert_eq!((made, cap), (2, 2));

        let refused =
            rooms.make(1, "c", WorldKind::Infinite, None, None, Reach::Listed, false).unwrap_err();
        assert!(refused.contains('2'), "the refusal says how many: {refused}");
        assert!(rooms.get(&RoomId::from("c")).is_none(), "and made none");

        // Deleting one frees a slot, or a server that had made and deleted its
        // cap's worth would refuse for ever while holding nothing.
        rooms.delete("a").unwrap();
        assert_eq!(rooms.made_count().0, 1);
        assert!(rooms.make(1, "c", WorldKind::Infinite, None, None, Reach::Listed, false).is_ok());
    }

    /// A private room is reachable by its code and mentioned nowhere else —
    /// including in the refusal a mistyped name gets back, which used to name
    /// every room on the server and would have handed out every code.
    #[test]
    fn a_private_room_is_reachable_by_code_and_named_nowhere() {
        let mut rooms =
            Rooms::open(temp_dir("private"), &["hall".into()], WorldKind::Infinite, true).unwrap();

        let made = rooms
            .make(3, "friends-only", WorldKind::Infinite, None, None, Reach::Code, false)
            .unwrap();
        let code = made.code.clone().expect("a private room gets a code");
        assert_eq!(code.len(), CODE_LEN);
        assert_ne!(code, made.id.as_str(), "a code is a credential, not an identity");
        assert_eq!(made.name, "friends-only", "a private room keeps the name it was given");
        assert_ne!(code, made.name, "and the code is not that name");
        assert!(rooms.is_unlisted(&made.id));

        let listing = rooms.listing();
        let listed: Vec<&str> = listing.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(listed, ["hall"], "the listing does not mention it");

        // The code still joins, which is the whole point of having one.
        assert_eq!(rooms.resolve(Some(&code)).unwrap(), made.id);
        assert_eq!(rooms.resolve(Some(made.id.as_str())).unwrap(), made.id);

        // And a wrong name is refused without naming it.
        let refused = rooms.resolve(Some("nowhere")).unwrap_err();
        assert!(refused.contains("hall"));
        assert!(!refused.contains(&code), "the refusal leaked a code: {refused}");
        assert!(!refused.contains(made.id.as_str()), "or an id: {refused}");
    }

    /// **A restart keeps what a client-made room was.** The world came back
    /// from its file already; the code, the unlisting and the owner did not,
    /// so a private world reopened listed, codeless and nobody's.
    #[test]
    fn a_restart_keeps_a_private_rooms_code_unlisting_and_owner() {
        let dir = temp_dir("meta");
        let key = Secret::new().unwrap();
        let (id, code, who) = {
            let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
            let out = rooms.handle(
                &Caller::new(1),
                ClientMessage::Hello { name: "maker".into(), person: key.clone() },
            );
            let [ServerMessage::You(profile)] = &out[..] else { panic!("{out:?}") };
            let who = profile.who.clone();
            let out = rooms.handle(
                &Caller::known(1, who.clone()),
                ClientMessage::Create {
                    name: "den".into(),
                    shape: WorldKind::Infinite,
                    victory: None,
                    teams: None,
                    private: true,
                    laboratory: false,
                    party: None,
                },
            );
            let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
            // Owned at `Create`, before anybody has joined: the hello named
            // the maker, so there was somebody to record.
            assert_eq!(rooms.owned_by(&made.id), Some(&who), "a keyed maker owns it at once");
            rooms.save().unwrap();
            (made.id.clone(), made.code.clone().expect("a code"), who)
        };

        let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        assert!(back.get(&id).is_some(), "the world did not come back");
        assert!(back.is_unlisted(&id), "it came back listed");
        assert_eq!(back.code_of(&id), Some(code.as_str()), "it came back codeless");
        assert_eq!(back.owned_by(&id), Some(&who), "it came back nobody's");
        assert_eq!(back.made_count().0, 1, "it came back outside the cap");
        assert_eq!(back.made_by(&id), None, "a connection outlived the process");
        assert_eq!(back.resolve(Some(&code)).unwrap(), id, "the code stopped working");
        let refused = back.resolve(Some("nowhere")).unwrap_err();
        assert!(!refused.contains(&code) && !refused.contains(id.as_str()), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A keyless maker's room keeps its code and its unlisting and has no
    /// owner to keep, because a seat means nothing after a restart — and a
    /// room deleted before the restart leaves no row to count against the cap.
    #[test]
    fn a_seat_owner_is_not_saved_and_a_deleted_room_leaves_no_row() {
        let dir = temp_dir("meta-seat");
        let id = {
            let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
            let made =
                rooms.make(4, "den", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
            let out = rooms.handle(
                &Caller::new(4),
                ClientMessage::Join {
                    name: "maker".into(),
                    room: Some(made.id.clone()),
                    person: None,
                },
            );
            let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
            assert_eq!(rooms.owner.get(&made.id), Some(&Owner::Seat(*you)), "owned by seat");
            let gone = rooms
                .make(4, "gone", WorldKind::Infinite, None, None, Reach::Listed, false)
                .unwrap();
            rooms.delete(gone.id.as_str()).unwrap();
            rooms.save().unwrap();
            made.id
        };

        let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        assert!(back.is_unlisted(&id) && back.code_of(&id).is_some(), "privacy was lost");
        assert_eq!(back.owner.get(&id), None, "a seat outlived the process");
        assert_eq!(back.made_count().0, 1, "a deleted room was counted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Whoever made a room can close it from the menu, and nobody else can.**
    /// Not while anybody is in it, the maker included; not a room the console
    /// made; and the listing says whose each room is, so a menu offers the
    /// door only on your own.
    #[test]
    fn only_whoever_made_a_room_can_close_it_and_only_once_it_is_empty() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (a_id, b_id) = two_people(&mut rooms);
        let out = rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Create {
                name: "den".into(),
                shape: WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: None,
            },
        );
        let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
        let den = made.id.clone();
        let close = |room: &RoomId| ClientMessage::Close { room: room.clone() };

        // The listing says whose it is, which is how a menu knows to offer it.
        let mine = rooms.listing().into_iter().find(|r| r.id == den).expect("listed");
        assert_eq!(mine.owner, Some(a_id.clone()));

        // Somebody else, and a room nobody made.
        let out = rooms.handle(&Caller::known(2, b_id), close(&den));
        let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
        assert!(why.contains("whoever made"), "{why}");
        let out = rooms.handle(&Caller::known(1, a_id.clone()), close(&RoomId::from("hall")));
        let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
        assert!(why.contains("console"), "{why}");

        // The maker, from inside: refused, and the reason is the room being
        // occupied rather than the key being wrong.
        let out = rooms.handle(
            &Caller::new(1),
            ClientMessage::Join { name: "maker".into(), room: Some(den.clone()), person: None },
        );
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let mut inside = Caller::sitting(1, (den.clone(), *you));
        inside.person = Some(a_id.clone());
        let out = rooms.handle(&inside, close(&den));
        let [ServerMessage::Closed(Err(why))] = &out[..] else { panic!("{out:?}") };
        assert!(why.contains("still in"), "{why}");
        assert!(rooms.get(&den).is_some(), "an occupied room was closed");

        // And once they have left, it goes.
        rooms.handle(&inside, ClientMessage::Leave);
        let out = rooms.handle(&Caller::known(1, a_id), close(&den));
        assert!(matches!(&out[..], [ServerMessage::Closed(Ok(id))] if *id == den), "{out:?}");
        assert!(rooms.get(&den).is_none(), "the room is still here");
        assert_eq!(rooms.made_count().0, 0, "and it still counts against the cap");
    }

    /// Somebody met by hello, with the secret still in hand for a join.
    fn met(rooms: &mut Rooms, n: u64) -> (Secret, PersonId) {
        let key = Secret::new().unwrap();
        let out = rooms.handle(
            &Caller::new(n),
            ClientMessage::Hello { name: format!("p{n}"), person: key.clone() },
        );
        let [ServerMessage::You(profile), ..] = &out[..] else { panic!("{out:?}") };
        (key, profile.who.clone())
    }

    fn join_as(key: &Secret, room: &str) -> ClientMessage {
        ClientMessage::Join {
            name: "somebody".into(),
            room: Some(RoomId::from(room)),
            person: Some(key.clone()),
        }
    }

    fn private_room(rooms: &mut Rooms, by: &Caller) -> Made {
        let out = rooms.handle(
            by,
            ClientMessage::Create {
                name: "den".into(),
                shape: WorldKind::Infinite,
                victory: None,
                teams: None,
                private: true,
                laboratory: false,
                party: None,
            },
        );
        let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
        made.clone()
    }

    /// **An invitation names a person, where a code names nobody.** The id of
    /// a private room stops being a way in on its own: it opens for its maker,
    /// for whoever was invited, and by the code — and somebody who came in by
    /// the code is in from then on, so a refresh is not a refusal.
    #[test]
    fn an_invitation_admits_the_person_it_names_and_the_id_alone_admits_nobody() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (ka, a) = met(&mut rooms, 1);
        let (kb, b) = met(&mut rooms, 2);
        let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
        let (den, code) = (made.id.clone(), made.code.expect("a code"));

        // A stranger with the id is told what anybody mistyping a name is
        // told, and no more: the refusal echoes what they typed and names the
        // listed rooms, and the code is in neither.
        let out = rooms.handle(&Caller::known(2, b.clone()), join_as(&kb, den.as_str()));
        let [ServerMessage::Rejected { reason }] = &out[..] else { panic!("{out:?}") };
        assert!(reason.contains("no room"), "{reason}");
        assert!(!reason.contains(&code), "the refusal leaked a code: {reason}");
        let out = rooms.handle(&Caller::new(9), ClientMessage::Watch { room: den.clone() });
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "a stranger watched: {out:?}"
        );

        // The maker, by id: the key that made it.
        let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, den.as_str()));
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let mut inside = Caller::sitting(1, (den.clone(), *you));
        inside.person = Some(a.clone());

        // Invited, and told so with the next thing they say -- with the room's
        // name, which they have never been listed.
        let out =
            rooms.handle(&inside, ClientMessage::Invite { who: b.clone(), room: den.clone() });
        assert!(out.is_empty(), "{out:?}");
        let out = rooms.handle(&Caller::known(2, b.clone()), ClientMessage::Rooms);
        let told = out.iter().find_map(|m| match m {
            ServerMessage::Invited { from, room, name } => {
                Some((from.who.clone(), room.clone(), name.clone()))
            }
            _ => None,
        });
        assert_eq!(told, Some((a.clone(), den.clone(), "den".into())), "{out:?}");

        // And now the id is a way in for them.
        let out = rooms.handle(&Caller::known(2, b.clone()), join_as(&kb, den.as_str()));
        assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");

        // Codes stay: a third person, by the code, and from then on by the id.
        let (kc, c) = met(&mut rooms, 3);
        let out = rooms.handle(&Caller::known(3, c.clone()), join_as(&kc, &code));
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        rooms.handle(&Caller::sitting(3, (den.clone(), *you)), ClientMessage::Leave);
        let out = rooms.handle(&Caller::known(3, c), join_as(&kc, den.as_str()));
        assert!(
            matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
            "a refresh refused: {out:?}"
        );
    }

    /// The five ways an invitation will not go, each a sentence that leaves the
    /// asker where they were rather than back on the menu.
    #[test]
    fn an_invitation_nobody_can_use_is_refused_where_it_was_asked() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (ka, a) = met(&mut rooms, 1);
        let (_, b) = met(&mut rooms, 2);
        let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
        let den = made.id.clone();
        let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, den.as_str()));
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let mut inside = Caller::sitting(1, (den.clone(), *you));
        inside.person = Some(a.clone());
        let why = |out: &[ServerMessage]| match out {
            [ServerMessage::NotDone { reason }] => reason.clone(),
            other => panic!("not a refusal in place: {other:?}"),
        };
        let invite = |who: &PersonId, room: &RoomId| ClientMessage::Invite {
            who: who.clone(),
            room: room.clone(),
        };

        let mut keyless = inside.clone();
        keyless.person = None;
        assert!(why(&rooms.handle(&keyless, invite(&b, &den))).contains("no key"));
        assert!(why(&rooms.handle(&Caller::known(1, a.clone()), invite(&b, &den)))
            .contains("a room you are in"));
        assert!(why(&rooms.handle(&inside, invite(&b, &RoomId::from("hall"))))
            .contains("a room you are in"));
        assert!(why(&rooms.handle(&inside, invite(&a, &den))).contains("already here"));
        let stranger = PersonId("nobody-here".into());
        assert!(why(&rooms.handle(&inside, invite(&stranger, &den))).contains("never met"));
        assert!(
            rooms.admitted.get(&den).is_none_or(|in_| in_.is_empty()),
            "a refusal admitted somebody"
        );
    }

    fn party_room(rooms: &mut Rooms, by: &Caller, party: &PartyId) -> Result<Made, String> {
        let out = rooms.handle(
            by,
            ClientMessage::Create {
                name: "den".into(),
                shape: WorldKind::Infinite,
                victory: None,
                teams: None,
                private: false,
                laboratory: false,
                party: Some(party.clone()),
            },
        );
        let [ServerMessage::Made(made)] = &out[..] else { panic!("{out:?}") };
        made.clone()
    }

    /// The parties in an answer, or a panic that says what came instead.
    fn parties_in(out: &[ServerMessage]) -> Vec<crate::net::PartyInfo> {
        out.iter()
            .find_map(|m| match m {
                ServerMessage::Parties { parties } => Some(parties.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no party listing in {out:?}"))
    }

    /// **A party is a private set of worlds its members see and nobody else
    /// does.** A member sees the party's world in the party listing and not in
    /// the room list; a non-member sees neither and cannot join it by id; an
    /// invitation reaches the person it names and lets them in; leaving takes
    /// the worlds with it.
    #[test]
    fn a_party_is_a_private_set_of_worlds_only_its_members_see_or_join() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (ka, a) = met(&mut rooms, 1);
        let (kb, b) = met(&mut rooms, 2);
        let (_, c) = met(&mut rooms, 3);
        let me = Caller::known(1, a.clone());
        let them = Caller::known(2, b.clone());

        // Nobody has presented a key: on no list, and told so truthfully.
        let out = rooms.handle(&Caller::new(9), ClientMessage::Parties);
        assert!(parties_in(&out).is_empty());

        let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
        let listed = parties_in(&out);
        assert_eq!(listed.len(), 1, "making a party did not list it: {out:?}");
        let party = listed[0].id.clone();
        assert_eq!(listed[0].name, "friday");
        assert_eq!(listed[0].members.len(), 1, "the maker is its first member");
        assert_eq!(listed[0].members[0].who, a);

        // A world of the party's: no code, not in the room list, in the party's.
        let made = party_room(&mut rooms, &me, &party).expect("a member may make one");
        assert_eq!(made.code, None, "a party's world has a code");
        assert!(rooms.is_unlisted(&made.id));
        assert!(!rooms.listing().iter().any(|r| r.id == made.id), "it is in the room list");
        let mine = parties_in(&rooms.handle(&me, ClientMessage::Parties));
        assert_eq!(mine[0].rooms.len(), 1, "the party does not list its world");
        assert_eq!(mine[0].rooms[0].id, made.id);

        // A non-member sees nothing and gets in nowhere -- not by making a
        // world for it, not by the id, and not by watching.
        assert!(party_room(&mut rooms, &them, &party).is_err(), "a stranger made a party world");
        assert!(parties_in(&rooms.handle(&them, ClientMessage::Parties)).is_empty());
        let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
        assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
        let out = rooms.handle(&them, ClientMessage::Watch { room: made.id.clone() });
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "a stranger watched: {out:?}"
        );
        // And a member cannot hand the door to a person by a room invitation,
        // which would be a way round the party.
        let out = rooms.handle(&me, join_as(&ka, made.id.as_str()));
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let mut inside = Caller::sitting(1, (made.id.clone(), *you));
        inside.person = Some(a.clone());
        let out = rooms.handle(&inside, ClientMessage::Invite { who: c, room: made.id.clone() });
        let [ServerMessage::NotDone { reason }] = &out[..] else { panic!("{out:?}") };
        assert!(reason.contains("party"), "{reason}");

        // An invitation reaches the person it names, rides out with their next
        // word, and is the only way in; the party then lists them both.
        let out = rooms.handle(&them, ClientMessage::JoinParty { party: party.clone() });
        assert!(matches!(&out[..], [ServerMessage::NotDone { .. }]), "joined uninvited: {out:?}");
        let out = rooms
            .handle(&inside, ClientMessage::InviteToParty { party: party.clone(), who: b.clone() });
        assert!(out.is_empty(), "{out:?}");
        let out = rooms.handle(&them, ClientMessage::Rooms);
        let asked = out.iter().find_map(|m| match m {
            ServerMessage::PartyInvite { from, party, name } => {
                Some((from.who.clone(), party.clone(), name.clone()))
            }
            _ => None,
        });
        assert_eq!(asked, Some((a.clone(), party.clone(), "friday".into())), "{out:?}");
        let out = rooms.handle(&them, ClientMessage::JoinParty { party: party.clone() });
        let theirs = parties_in(&out);
        assert_eq!(theirs.len(), 1);
        assert_eq!(theirs[0].members.len(), 2);
        assert!(theirs[0].members.iter().any(|m| m.who == a && m.online), "a is in the world");
        assert!(theirs[0].members.iter().any(|m| m.who == b && !m.online));
        let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
        assert!(
            matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
            "a member refused: {out:?}"
        );

        // Leaving loses the worlds, which a code could never express.
        let out = rooms.handle(&them, ClientMessage::LeaveParty { party: party.clone() });
        assert!(parties_in(&out).is_empty(), "left and still listed");
        let seat = (made.id.clone(), PlayerId(2));
        rooms.handle(&Caller::sitting(2, seat), ClientMessage::Leave);
        let out = rooms.handle(&them, join_as(&kb, made.id.as_str()));
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "the door stayed open: {out:?}"
        );

        // The last one out takes the party; its world stays its maker's.
        let out = rooms.handle(&me, ClientMessage::LeaveParty { party });
        assert!(parties_in(&out).is_empty());
        assert!(rooms.parties.is_empty(), "an empty party stayed");
        assert!(rooms.get(&made.id).is_some(), "the world went with the party");
        assert_eq!(rooms.owned_by(&made.id), Some(&a));
    }

    /// **A party survives a restart** with its people, its standing
    /// invitations and its worlds, and a world of its is still members-only.
    #[test]
    fn a_party_survives_a_restart() {
        let dir = temp_dir("parties");
        let (ka, kb, kc) = (Secret::new().unwrap(), Secret::new().unwrap(), Secret::new().unwrap());
        let hello = |rooms: &mut Rooms, n: u64, key: &Secret| -> PersonId {
            let out = rooms.handle(
                &Caller::new(n),
                ClientMessage::Hello { name: format!("p{n}"), person: key.clone() },
            );
            let [ServerMessage::You(profile), ..] = &out[..] else { panic!("{out:?}") };
            profile.who.clone()
        };
        let (party, den, b) = {
            let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
            let a = hello(&mut rooms, 1, &ka);
            let b = hello(&mut rooms, 2, &kb);
            hello(&mut rooms, 3, &kc);
            let me = Caller::known(1, a.clone());
            let out = rooms.handle(&me, ClientMessage::MakeParty { name: "friday".into() });
            let party = parties_in(&out)[0].id.clone();
            let den = party_room(&mut rooms, &me, &party).unwrap().id;
            rooms
                .handle(&me, ClientMessage::InviteToParty { party: party.clone(), who: b.clone() });
            rooms.save().unwrap();
            (party, den, b)
        };

        let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        let a = hello(&mut back, 4, &ka);
        let mine = parties_in(&back.handle(&Caller::known(4, a), ClientMessage::Parties));
        assert_eq!(mine.len(), 1, "the party was lost");
        assert_eq!(mine[0].id, party);
        assert_eq!(mine[0].rooms.iter().map(|r| &r.id).collect::<Vec<_>>(), [&den]);

        // The invitation stood, and the door is still the party's.
        let out = back.handle(&Caller::known(5, b.clone()), ClientMessage::JoinParty { party });
        assert_eq!(parties_in(&out).len(), 1, "the invitation was lost: {out:?}");
        let out = back.handle(&Caller::known(5, b), join_as(&kb, den.as_str()));
        assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");
        let out = back.handle(&Caller::new(6), join_as(&kc, den.as_str()));
        assert!(matches!(&out[..], [ServerMessage::Rejected { .. }]), "a stranger got in: {out:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **An invitation given before a restart stands after it**, because the
    /// door is in `rooms.jsonl` beside the code.
    #[test]
    fn an_invitation_survives_a_restart() {
        let dir = temp_dir("admitted");
        let kb = Secret::new().unwrap();
        let den = {
            let mut rooms = Rooms::open(&dir, &["hall".into()], WorldKind::Infinite, true).unwrap();
            let (ka, a) = met(&mut rooms, 1);
            let out = rooms.handle(
                &Caller::new(2),
                ClientMessage::Hello { name: "b".into(), person: kb.clone() },
            );
            let [ServerMessage::You(theirs)] = &out[..] else { panic!("{out:?}") };
            let b = theirs.who.clone();
            let made = private_room(&mut rooms, &Caller::known(1, a.clone()));
            let out = rooms.handle(&Caller::known(1, a.clone()), join_as(&ka, made.id.as_str()));
            let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
            let mut inside = Caller::sitting(1, (made.id.clone(), *you));
            inside.person = Some(a);
            rooms.handle(&inside, ClientMessage::Invite { who: b, room: made.id.clone() });
            rooms.save().unwrap();
            made.id
        };

        let mut back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        let out = back.handle(&Caller::new(5), join_as(&kb, den.as_str()));
        assert!(
            matches!(&out[..], [ServerMessage::Welcome { .. }, ..]),
            "the door forgot them: {out:?}"
        );
        let out = back.handle(&Caller::new(6), join_as(&Secret::new().unwrap(), den.as_str()));
        assert!(
            matches!(&out[..], [ServerMessage::Rejected { .. }]),
            "and let a stranger in: {out:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whoever is running the server can read the save directory anyway, and
    /// an operator who cannot see a room cannot delete one being misused.
    #[test]
    fn the_console_sees_private_rooms_and_the_wire_does_not() {
        let mut rooms =
            Rooms::open(temp_dir("console-sees"), &["hall".into()], WorldKind::Infinite, true)
                .unwrap();
        let made = rooms.make(3, "", WorldKind::Infinite, None, None, Reach::Code, false).unwrap();
        assert!(made.code.is_some(), "a private room gets a code");

        let everything = rooms.everything();
        let found = everything.iter().find(|(r, _)| r.id == made.id).expect("the console sees it");
        assert!(found.1, "and knows it is private");
        assert_eq!(everything.len(), 2);

        assert_eq!(rooms.listing().len(), 1, "the wire sees only the open one");

        // What the console actually prints, since that is the thing being
        // claimed. Both rooms, and the private one said to be private.
        let printed = crate::server::console::run("rooms", &mut rooms, WorldKind::Infinite);
        let text = printed.lines.join("\n");
        assert!(text.contains("hall"), "{text}");
        assert!(text.contains(made.id.as_str()), "the console shows the id: {text}");
        assert!(text.contains("private"), "{text}");
        assert!(
            text.contains(made.code.as_ref().unwrap().as_str()),
            "and the code, which is why an operator looks a private room up: {text}"
        );
    }

    /// A code is read off one screen and typed into another. The five
    /// characters that make that go wrong are not in it.
    #[test]
    fn a_code_has_nothing_confusable_in_it() {
        for _ in 0..500 {
            let c = code();
            assert_eq!(c.len(), CODE_LEN);
            assert!(!c.contains(['0', 'o', '1', 'i', 'l']), "confusable character in {c}");
            assert!(crate::net::room_name(&c).is_ok(), "a code must be a legal room name: {c}");
        }
    }

    /// A player can start the match they made, and nobody else can — not
    /// another player in it, and not somebody who only joined.
    #[test]
    fn only_whoever_made_a_match_can_start_it() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let ours = Caller::new(5);
        let theirs = Caller::new(6);

        let made = rooms
            .make(
                5,
                "cup",
                WorldKind::Infinite,
                Some(Victory::Timer { generations: 50 }),
                None,
                Reach::Listed,
                false,
            )
            .unwrap();
        let join = |name: &str| ClientMessage::Join {
            name: name.into(),
            room: Some(made.id.clone()),
            person: None,
        };

        // Nobody owns it until the maker joins: the owner is a PlayerId, and
        // there is no player until somebody has one.
        let out = rooms.handle(&ours, join("owner"));
        let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };
        let owner = *you;

        let mut sitting = Caller::sitting(6, (made.id.clone(), PlayerId(0)));
        let out = rooms.handle(&theirs, join("guest"));
        let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };
        sitting.seat = Some((made.id.clone(), *you));
        assert_ne!(*you, owner, "two players, not one");

        // The guest cannot.
        let out = rooms.handle(&sitting, ClientMessage::Start);
        let [ServerMessage::NotStarted { reason }] = &out[..] else {
            panic!("a guest started somebody else's match: {out:?}");
        };
        assert!(reason.contains("made"), "{reason}");
        assert_eq!(*rooms.get(&made.id).unwrap().phase(), Phase::Gathering);

        // The owner can — and from a **different connection**, because a
        // reconnect gets a new socket and the same player, and losing your own
        // match to a refresh would be the obvious way for this to be wrong.
        let reconnected = Caller::sitting(99, (made.id.clone(), owner));
        let out = rooms.handle(&reconnected, ClientMessage::Start);
        assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
        assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
        assert_eq!(
            rooms.get(&made.id).unwrap().started_by(),
            Some(owner),
            "and it remembers who blew it"
        );
    }

    /// **A match is its maker's by person, not by seat.** A seat is a room's
    /// number for somebody and a key is who they are on this server, so the
    /// whistle answers to the key wherever it is presented from, and to
    /// nobody holding the seat without it.
    #[test]
    fn a_keyed_maker_owns_their_match_from_any_seat() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let made = rooms
            .make(
                5,
                "cup",
                WorldKind::Infinite,
                Some(Victory::Timer { generations: 50 }),
                None,
                Reach::Listed,
                false,
            )
            .unwrap();
        let key = Secret::new().unwrap();
        let out = rooms.handle(
            &Caller::new(5),
            ClientMessage::Join {
                name: "maker".into(),
                room: Some(made.id.clone()),
                person: Some(key),
            },
        );
        let Some(ServerMessage::Welcome { you, profile: Some(profile), .. }) =
            out.iter().find(|m| matches!(m, ServerMessage::Welcome { .. }))
        else {
            panic!("{out:?}")
        };
        let (seat, who) = (*you, profile.who.clone());
        // The lobby names the maker's seat, which is how a client knows the
        // whistle is its own to blow. It goes out with the next step, as every
        // lobby does.
        let broadcast = rooms.step(std::time::Duration::from_secs(1));
        let named = broadcast.iter().find_map(|(_, m)| match m {
            ServerMessage::Match(lobby) => Some(lobby.owner),
            _ => None,
        });
        assert_eq!(named, Some(Some(seat)), "the lobby did not name the maker: {broadcast:?}");

        // Somebody in the maker's seat without the maker's key: a stranger.
        let mut impostor = Caller::sitting(9, (made.id.clone(), seat));
        let out = rooms.handle(&impostor, ClientMessage::Start);
        assert!(
            matches!(&out[..], [ServerMessage::NotStarted { .. }]),
            "a seat started a keyed match: {out:?}"
        );
        impostor.person = Some(PersonId("nobody".into()));
        let out = rooms.handle(&impostor, ClientMessage::Start);
        assert!(
            matches!(&out[..], [ServerMessage::NotStarted { .. }]),
            "the wrong key started it: {out:?}"
        );
        assert_eq!(*rooms.get(&made.id).unwrap().phase(), Phase::Gathering);

        // The maker's key on a new socket, given a seat the room never handed
        // out: the whistle is theirs anyway.
        let mut maker = Caller::sitting(99, (made.id.clone(), PlayerId(7)));
        maker.person = Some(who);
        let out = rooms.handle(&maker, ClientMessage::Start);
        assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
        assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
    }

    /// The whole flow a client actually walks: make a match, join it, and be
    /// told — by the broadcast every client in the room gets — that it is
    /// yours to start. The owner check has a unit test; this is about whether
    /// the answer ever *reaches* the person who has to press the button.
    #[test]
    fn the_maker_of_a_match_is_told_it_is_theirs_to_start() {
        let mut rooms =
            Rooms::open(temp_dir("told"), &["hall".into()], WorldKind::Infinite, true).unwrap();
        let me = Caller::new(12);

        let made = rooms
            .handle(
                &me,
                ClientMessage::Create {
                    name: "cup".into(),
                    shape: WorldKind::Infinite,
                    victory: Some(Victory::Timer { generations: 200 }),
                    teams: None,
                    private: false,
                    laboratory: false,
                    party: None,
                },
            )
            .into_iter()
            .find_map(|m| match m {
                ServerMessage::Made(Ok(made)) => Some(made),
                _ => None,
            })
            .expect("a room");

        let welcomed = rooms.handle(
            &me,
            ClientMessage::Join { name: "maker".into(), room: Some(made.id.clone()), person: None },
        );
        let you = welcomed
            .iter()
            .find_map(|m| match m {
                ServerMessage::Welcome { you, .. } => Some(*you),
                _ => None,
            })
            .expect("a welcome");

        // The lobby reaches everybody by broadcast rather than in the reply,
        // because a lobby full of people all changing sides needs one message
        // to all of them. A gathering match does not advance its world, and it
        // still has to produce this — which is the thing most likely to have
        // been got wrong.
        let broadcast = rooms.step(a_generation());
        let owner = broadcast
            .iter()
            .find_map(|(_, m)| match m {
                ServerMessage::Match(lobby) => Some(lobby.owner),
                _ => None,
            })
            .expect("a gathering match still broadcasts its lobby");
        assert_eq!(owner, Some(you), "the maker was not told the match is theirs");

        // And pressing it works from the seat that broadcast named.
        let out = rooms.handle(&Caller::sitting(12, (made.id.clone(), you)), ClientMessage::Start);
        assert!(out.is_empty(), "the whistle answers by broadcast, got {out:?}");
        assert!(matches!(rooms.get(&made.id).unwrap().phase(), Phase::Running { .. }));
    }

    /// A room the console made is the operator's, and starts at the console.
    #[test]
    fn a_match_nobody_made_cannot_be_started_from_a_client() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        rooms.new_match("cup", WorldKind::Infinite, Victory::Timer { generations: 50 }).unwrap();
        let id = RoomId::from("cup");

        let out = rooms.handle(
            &Caller::new(3),
            ClientMessage::Join { name: "someone".into(), room: Some(id.clone()), person: None },
        );
        let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };

        let out = rooms.handle(&Caller::sitting(3, (id.clone(), *you)), ClientMessage::Start);
        let [ServerMessage::NotStarted { reason }] = &out[..] else { panic!("{out:?}") };
        assert!(reason.contains("console"), "{reason}");
    }

    /// **A join hands back the locker this server holds**, and an empty one is
    /// how a client knows to offer what it is carrying — which is what makes a
    /// library follow somebody to a server they have never played on, with no
    /// two servers ever talking to each other.
    #[test]
    fn a_join_hands_back_the_locker_and_an_empty_one_asks_for_it() {
        use crate::net::kept::{Kept, Stamp};

        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let me = Secret::new().unwrap();
        let join = || ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: Some(me.clone()),
        };

        // Nothing held yet, so the answer is empty and the client seeds it.
        let out = rooms.handle(&Caller::new(1), join());
        let [.., ServerMessage::Yours(kept)] = &out[..] else { panic!("{out:?}") };
        assert!(kept.is_empty(), "a locker appeared from nowhere");

        let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let who = profile.clone().expect("no profile was issued").who;

        let mut library =
            Kept { stamps: vec![Stamp::trimmed(vec![(0, 0), (1, 1)])], games: vec![] };
        library.stamps[0].name = "corner".into();
        let caller = Caller::known(1, who.clone());
        assert!(rooms.handle(&caller, ClientMessage::Keep(library.clone())).is_empty());

        // And the next join gets it back. Seat given up first, or the second
        // connection is the same person arriving twice and is refused.
        let seat = (RoomId::from("hall"), crate::sim::PlayerId(1));
        rooms.handle(&Caller::sitting(1, seat), ClientMessage::Leave);
        let out = rooms.handle(&Caller::new(2), join());
        let [.., ServerMessage::Yours(kept)] = &out[..] else { panic!("{out:?}") };
        assert_eq!(kept.stamps.len(), 1, "the library did not come back");
        assert_eq!(kept.stamps[0].name, "corner");
    }

    /// **What a server asks a client not to offer travels with the room list**,
    /// because that is the first thing a menu asks any server — so the answer
    /// is known before the menu draws anything.
    ///
    /// A request rather than a rule, and it cannot be anything else: the
    /// client is somebody else's and every screen it hides is still compiled
    /// into it. What it is for is copy nobody has written yet.
    #[test]
    fn a_room_list_says_what_this_server_would_rather_not_be_offered() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));

        let out = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
        let [ServerMessage::Rooms { hidden, .. }] = &out[..] else { panic!("{out:?}") };
        assert_eq!(*hidden, crate::net::Hidden::default(), "a fresh server hides nothing");

        rooms.hidden.hide("howto").expect("howto is a name");
        let out = rooms.handle(&Caller::nobody(), ClientMessage::Rooms);
        let [ServerMessage::Rooms { hidden, .. }] = &out[..] else { panic!("{out:?}") };
        assert!(hidden.howto, "the server's answer did not reach the list");
    }

    /// A name this build does not know is refused rather than quietly hiding
    /// nothing, or `--hide howtoo` starts a server that ignores the flag.
    #[test]
    fn a_screen_this_build_has_no_name_for_is_refused() {
        let mut hidden = crate::net::Hidden::default();
        let why = hidden.hide("howtoo").expect_err("a typo was accepted");
        assert!(why.contains("howto"), "the refusal does not say what the names are: {why}");
        assert_eq!(hidden, crate::net::Hidden::default(), "a refused name changed something");
    }

    /// **A challenge is a room made and held for one named person**, and it
    /// reaches them on the next thing they say — there is no channel to a
    /// person, so it waits.
    #[test]
    fn a_challenge_makes_a_room_and_reaches_the_person_it_names() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (a, b) = (Secret::new().unwrap(), Secret::new().unwrap());
        let join = |s: &Secret| ClientMessage::Join {
            name: "somebody".into(),
            room: Some(RoomId::from("hall")),
            person: Some(s.clone()),
        };
        // Both have to be somebody this server has met.
        let out = rooms.handle(&Caller::new(1), join(&a));
        let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let a_id = profile.clone().expect("a profile").who;
        rooms.handle(
            &Caller::sitting(1, (RoomId::from("hall"), crate::sim::PlayerId(1))),
            ClientMessage::Leave,
        );
        let out = rooms.handle(&Caller::new(2), join(&b));
        let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let b_id = profile.clone().expect("a profile").who;

        let out = rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Challenge { who: b_id.clone() },
        );
        let [ServerMessage::Made(Ok(made))] = &out[..] else { panic!("{out:?}") };
        let room = made.id.clone();
        assert!(made.code.is_some(), "a challenge is not in the listing");

        // Nothing reaches them until they say something -- and then it does,
        // riding out with whatever they asked for.
        let out = rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
        let told = out.iter().find_map(|m| match m {
            ServerMessage::Challenged { from, room } => Some((from.clone(), room.clone())),
            _ => None,
        });
        let (from, told_room) = told.expect("the challenge never arrived");
        assert_eq!(from.who, a_id, "it came from the wrong person");
        assert_eq!(told_room, room);

        // Taken as it is handed over, so it is not shown twice.
        let out = rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
        assert!(!out.iter().any(|m| matches!(m, ServerMessage::Challenged { .. })), "shown twice");

        // Yes, and the answer reaches the person who asked.
        let out = rooms.handle(
            &Caller::known(2, b_id.clone()),
            ClientMessage::Answer { from: a_id.clone(), yes: true },
        );
        assert!(out.iter().any(|m| matches!(m, ServerMessage::Challenged { .. })), "{out:?}");

        let out = rooms.handle(&Caller::known(1, a_id), ClientMessage::Rooms);
        let answered = out.iter().find_map(|m| match m {
            ServerMessage::Answered { who, room } => Some((who.who.clone(), room.clone())),
            _ => None,
        });
        let (who, room_back) = answered.expect("the answer never arrived");
        assert_eq!(who, b_id);
        assert_eq!(room_back, Some(room), "a yes did not name the room");
    }

    /// **A no reaches somebody**, because the point of asking is finding out
    /// and silence cannot be told from not having seen it.
    #[test]
    fn a_decline_reaches_the_person_who_asked() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (a_id, b_id) = two_people(&mut rooms);

        rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Challenge { who: b_id.clone() },
        );
        rooms.handle(&Caller::known(2, b_id.clone()), ClientMessage::Rooms);
        rooms.handle(
            &Caller::known(2, b_id.clone()),
            ClientMessage::Answer { from: a_id.clone(), yes: false },
        );

        // Searched rather than positioned: what is waiting is handed over
        // *before* what was asked for, so an answer arrives in front of the
        // room list it rode out with.
        let out = rooms.handle(&Caller::known(1, a_id), ClientMessage::Rooms);
        let answered = out.iter().find_map(|m| match m {
            ServerMessage::Answered { who, room } => Some((who.who.clone(), room.clone())),
            _ => None,
        });
        let (who, room) = answered.unwrap_or_else(|| panic!("no answer: {out:?}"));
        assert_eq!(who, b_id);
        assert!(room.is_none(), "a no named a room to join");
    }

    /// The five ways it will not go, each a sentence somebody can act on.
    #[test]
    fn a_challenge_nobody_can_answer_is_refused_with_a_reason() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let (a_id, b_id) = two_people(&mut rooms);
        let why = |out: &[ServerMessage]| match out {
            [ServerMessage::Rejected { reason }] => reason.clone(),
            other => panic!("not a refusal: {other:?}"),
        };

        // A client with no key is nobody, so there is nowhere to answer to.
        let out = rooms.handle(&Caller::new(9), ClientMessage::Challenge { who: b_id.clone() });
        assert!(why(&out).contains("no key"), "{:?}", why(&out));

        // Yourself.
        let out = rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Challenge { who: a_id.clone() },
        );
        assert!(why(&out).contains("yourself"));

        // Somebody this server has never met.
        let stranger = crate::net::PersonId("nobody-here".into());
        let out = rooms
            .handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: stranger });
        assert!(why(&out).contains("never met"));

        // And twice over, so a challenge cannot fill somebody's screen.
        rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Challenge { who: b_id.clone() },
        );
        let out =
            rooms.handle(&Caller::known(1, a_id.clone()), ClientMessage::Challenge { who: b_id });
        assert!(why(&out).contains("already"), "{:?}", why(&out));

        // An answer to nothing.
        let out = rooms.handle(
            &Caller::known(1, a_id.clone()),
            ClientMessage::Answer { from: a_id, yes: true },
        );
        assert!(why(&out).contains("no challenge"), "{:?}", why(&out));
    }

    /// Two people this server has met, each having left the room again.
    fn two_people(rooms: &mut Rooms) -> (PersonId, PersonId) {
        let hall = RoomId::from("hall");
        let mut meet = |n: u64| {
            let key = Secret::new().unwrap();
            let out = rooms.handle(
                &Caller::new(n),
                ClientMessage::Join {
                    name: "somebody".into(),
                    room: Some(hall.clone()),
                    person: Some(key),
                },
            );
            let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else {
                panic!("{out:?}")
            };
            let (seat, who) = (*you, profile.clone().expect("a profile").who);
            rooms.handle(&Caller::sitting(n, (hall.clone(), seat)), ClientMessage::Leave);
            who
        };
        (meet(1), meet(2))
    }

    /// **A locker is nobody's to offer without a name.** `Keep` writes a
    /// client's own words to this server's disk, so a connection that has
    /// never joined has nowhere to put them and cannot say whose they are.
    #[test]
    fn a_locker_offered_by_nobody_is_dropped() {
        use crate::net::kept::{Kept, Stamp};

        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let offered = Kept { stamps: vec![Stamp::trimmed(vec![(0, 0)])], games: Vec::new() };
        assert!(rooms.handle(&Caller::new(9), ClientMessage::Keep(offered)).is_empty());
        assert!(rooms.lockers.is_empty(), "a nameless client filled a locker");
    }

    /// **A join that was refused is handed nothing.** The room can still turn
    /// somebody away after this map has resolved it — a match under way, or a
    /// person already sitting here in another tab — and a second tab that was
    /// given a locker would go on to replace the library of the one holding
    /// the seat.
    #[test]
    fn a_refused_join_is_handed_no_locker() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let me = Secret::new().unwrap();
        let join = || ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: Some(me.clone()),
        };
        assert!(rooms
            .handle(&Caller::new(1), join())
            .iter()
            .any(|m| { matches!(m, ServerMessage::Yours(_)) }));

        let out = rooms.handle(&Caller::new(2), join());
        let [ServerMessage::Rejected { .. }] = &out[..] else { panic!("{out:?}") };
    }

    /// **The same secret is the same person**, on a second connection and
    /// after a restart. That is the whole of what an identity has to do: the
    /// server issues an id the first time it sees a secret and gives the same
    /// one back for ever after, so a rating filed against it does not move.
    #[test]
    fn one_secret_is_one_person() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let hall = RoomId::from("hall");
        let me = Secret::new().unwrap();
        let join = |secret: Option<Secret>| ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: secret,
        };
        let named = |rooms: &Rooms, you: &crate::sim::PlayerId| {
            rooms.get(&hall).unwrap().players().find(|p| p.id == *you).unwrap().person.clone()
        };

        let out = rooms.handle(&Caller::new(1), join(Some(me.clone())));
        let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let ours = profile.clone().expect("no profile was issued");
        let first = ours.who.clone();
        assert_eq!(ours.name, "alice", "the name a join was made under");
        assert!(ours.provisional, "a first join has no result behind it");
        assert_eq!(named(&rooms, you).as_deref(), Some(first.as_str()));

        // Away, and back: the same secret finds the same seat and the same
        // name. Nothing was presented and nothing was reissued.
        rooms.handle(&Caller::sitting(1, (hall.clone(), *you)), ClientMessage::Leave);
        let out = rooms.handle(&Caller::new(2), join(Some(me)));
        let [ServerMessage::Welcome { you, profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        assert_eq!(profile.as_ref().map(|p| &p.who), Some(&first), "one secret was two people");
        assert_eq!(named(&rooms, you).as_deref(), Some(first.as_str()));

        // And somebody else's secret is somebody else.
        let out = rooms.handle(&Caller::new(3), join(Some(Secret::new().unwrap())));
        let [ServerMessage::Welcome { profile, .. }, ..] = &out[..] else { panic!("{out:?}") };
        assert_ne!(profile.as_ref().map(|p| &p.who), Some(&first), "two secrets were one person");

        // And so is who else plays here, for the same reason and one more:
        // this is how you find a person to look up in the first place, and the
        // menu is where you are standing when you do.
        let asked = rooms.handle(&Caller::nobody(), ClientMessage::People { like: "ali".into() });
        let [ServerMessage::People { like, found }] = &asked[..] else { panic!("{asked:?}") };
        assert_eq!(like, "ali", "the query comes back, so a stale answer can be dropped");
        assert!(found.iter().any(|p| p.who == first), "alice is not in a search for ali");
        assert!(found.iter().all(|p| p.name.to_lowercase().contains("ali")));

        // Nobody the server has never met, however the ratings table got their
        // fingerprint into it.
        let asked = rooms.handle(&Caller::nobody(), ClientMessage::People { like: "zzz".into() });
        let [ServerMessage::People { found, .. }] = &asked[..] else { panic!("{asked:?}") };
        assert!(found.is_empty(), "found somebody who is not here: {found:?}");

        // And what a server says about somebody is answerable from outside
        // every room, because that is where it is looked at from.
        let asked = rooms.handle(&Caller::nobody(), ClientMessage::Profile { who: first.clone() });
        let [ServerMessage::Profile(Some(found))] = &asked[..] else { panic!("{asked:?}") };
        assert_eq!(found.who, first);
        assert_eq!(found.label(), format!("alice·{}", first.short()));

        // Somebody this server never issued is "not here" rather than a
        // failure: a client may ask about anything.
        let none = rooms.handle(
            &Caller::nobody(),
            ClientMessage::Profile { who: crate::net::PersonId("nobody".into()) },
        );
        assert!(matches!(&none[..], [ServerMessage::Profile(None)]), "{none:?}");
    }

    /// A client with no key plays. It is nobody the server will remember,
    /// which is the honest outcome for a browser that cannot keep one rather
    /// than a reason to refuse to let anybody in.
    #[test]
    fn a_client_with_no_key_still_plays() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let out = rooms.handle(
            &Caller::new(1),
            ClientMessage::Join {
                name: "alice".into(),
                room: Some(RoomId::from("hall")),
                person: None,
            },
        );
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let seat =
            rooms.get(&RoomId::from("hall")).unwrap().players().find(|p| p.id == *you).unwrap();
        assert_eq!(seat.person, None);
    }

    /// **Leaving frees the seat, and the person still brings you back.**
    ///
    /// Going back to the menu used to send nothing at all, so the player
    /// stayed online: the room went on counting them, and the way back — which
    /// only returns you to a player who is *not* online — found them online
    /// and made a new one instead. Leave and come back three times and a room
    /// with one person in it said three.
    #[test]
    fn leaving_frees_the_seat_and_the_person_still_comes_back() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let hall = RoomId::from("hall");
        let me = Caller::new(3);
        let secret = Secret::new().unwrap();
        let join = || ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: Some(secret.clone()),
        };

        let out = rooms.handle(&me, join());
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        let first = *you;
        assert_eq!(rooms.get(&hall).unwrap().players().filter(|p| p.online).count(), 1);

        // Back to the menu, still connected.
        rooms.handle(&Caller::sitting(3, (hall.clone(), first)), ClientMessage::Leave);
        assert_eq!(
            rooms.get(&hall).unwrap().players().filter(|p| p.online).count(),
            0,
            "the room still counts somebody who left"
        );

        // And back in: the same player, not a new one. Nothing was presented —
        // the secret this client already had is the whole of the way back.
        let out = rooms.handle(&me, join());
        let [ServerMessage::Welcome { you, .. }, ..] = &out[..] else { panic!("{out:?}") };
        assert_eq!(*you, first, "coming back made a second player");
        assert_eq!(
            rooms.get(&hall).unwrap().players().filter(|p| p.online).count(),
            1,
            "one person, however many times they have come and gone"
        );

        // Three times over, which is what the listing was counting.
        for _ in 0..3 {
            rooms.handle(&Caller::sitting(3, (hall.clone(), first)), ClientMessage::Leave);
            rooms.handle(&me, join());
        }
        assert_eq!(rooms.listing()[0].players, 1, "the room list counted the comings and goings");
    }

    /// **A person is not two players.** Somebody who has carried their secret
    /// to a second machine and joined from both is told so, rather than being
    /// handed a stranger's seat.
    ///
    /// This is the one place a person is stricter than the token it replaced.
    /// A token said which *seat*, so two tabs sharing one were honestly two
    /// players and the second quietly got a new number — four hundred
    /// generations into a match, if that is when they arrived.
    #[test]
    fn one_person_cannot_be_in_a_room_twice() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let secret = Secret::new().unwrap();
        let join = || ClientMessage::Join {
            name: "alice".into(),
            room: Some(RoomId::from("hall")),
            person: Some(secret.clone()),
        };
        let out = rooms.handle(&Caller::new(1), join());
        assert!(matches!(&out[..], [ServerMessage::Welcome { .. }, ..]), "{out:?}");

        let out = rooms.handle(&Caller::new(2), join());
        let [ServerMessage::Rejected { reason }] = &out[..] else { panic!("{out:?}") };
        assert!(reason.contains("already"), "{reason}");
        assert_eq!(
            rooms.get(&RoomId::from("hall")).unwrap().players().count(),
            1,
            "a refused join took a seat anyway"
        );
    }

    /// Late to a match, and what happens now: the join is **refused** and the
    /// client is told why. It is not turned into a watch.
    ///
    /// Deliberate rather than missing. A `Join` that quietly became a `Watch`
    /// would put a player into a world they cannot act in without their having
    /// asked for that, and the two are answered by different messages —
    /// `Welcome` carries a player number, a purse and a spawn, and `Watching`
    /// carries none of them. A client that asked to play and got a `Watching`
    /// back would have to discover it had no seat by trying to use one.
    ///
    /// So the server refuses and says so, and the **client** offers the watch:
    /// the room list has a Watch button on every room, and the refusal names
    /// the reason beside it. That keeps "you cannot play in this" and "would
    /// you like to watch it" two separate answers, which is what they are.
    #[test]
    fn joining_a_running_match_is_refused_and_watching_it_is_not() {
        let mut rooms = Rooms::just(Server::named("cup", World::infinite_empty()));
        {
            let server = rooms.get_mut(&RoomId::from("cup")).unwrap();
            server.make_match(Victory::Timer { generations: 100 });
            server.join("early").unwrap();
            server.start_match(None).unwrap();
        }
        for _ in 0..40 {
            rooms.step(a_generation());
        }

        let late = Caller::new(11);
        let replies = rooms.handle(
            &late,
            ClientMessage::Join {
                name: "late".into(),
                room: Some(RoomId::from("cup")),
                person: None,
            },
        );
        let [ServerMessage::Rejected { reason }] = &replies[..] else {
            panic!("expected a refusal, got {replies:?}");
        };
        assert!(reason.contains("cup"), "the refusal names the room: {reason}");

        // And the same connection may watch it, at the same generation, which
        // is the whole distinction: no late joining is a rule about players.
        let replies = rooms.handle(&late, ClientMessage::Watch { room: RoomId::from("cup") });
        let [ServerMessage::Watching { tick, .. }] = &replies[..] else {
            panic!("a late connection may still watch, got {replies:?}");
        };
        assert_eq!(*tick, 40, "and sees the world where it actually is");
    }

    /// The whole reason a spectator is not "a player with the actions taken
    /// away": a seat is one of fifteen, and a match under way admits no new
    /// players at all. Neither of those should keep somebody from watching.
    #[test]
    fn watching_needs_no_seat_and_no_room_in_the_roster() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        // Fill every seat there is. `PlayerId::MAX` is four bits of cell.
        for n in 0..PlayerId::MAX {
            rooms
                .get_mut(&RoomId::from("hall"))
                .unwrap()
                .join(format!("player{n}"))
                .unwrap_or_else(|e| panic!("seat {n}: {e}"));
        }
        assert!(
            rooms.get_mut(&RoomId::from("hall")).unwrap().join("one-too-many").is_err(),
            "the room is full, which is the situation being tested"
        );

        let replies =
            rooms.handle(&Caller::new(9), ClientMessage::Watch { room: RoomId::from("hall") });
        let [ServerMessage::Watching { room, world, .. }] = &replies[..] else {
            panic!("a full room still admits a watcher, got {replies:?}");
        };
        assert_eq!(room.as_str(), "hall");
        assert_eq!(*world, WorldKind::Infinite);
    }

    /// A watcher reads and does not act. Both halves matter: one that could
    /// not read would be watching nothing, and one that could act would be a
    /// player who never took a seat.
    #[test]
    fn a_watcher_is_sent_chunks_and_changes_nothing() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let seated = rooms.get_mut(&RoomId::from("hall")).unwrap().join("alice").unwrap();
        let watcher = Caller {
            connection: 4,
            seat: None,
            watching: Some(RoomId::from("hall")),
            person: None,
        };

        let before = rooms.get(&RoomId::from("hall")).unwrap().world().digest();

        // Reads.
        let replies = rooms.handle(&watcher, ClientMessage::Subscribe { chunks: vec![(0, 0)] });
        assert!(
            replies.iter().any(|m| matches!(m, ServerMessage::ChunkData { .. })),
            "a watcher gets the chunks it asks for, got {replies:?}"
        );

        // And does not act. The action names a seated player, which is the
        // stronger version of the test: it is refused for coming from a
        // connection with no seat, not for naming a player who is not here.
        rooms.handle(
            &watcher,
            ClientMessage::Act(crate::net::Stamped {
                tick: rooms.get(&RoomId::from("hall")).unwrap().tick(),
                player: seated,
                seat: seated,
                action: crate::net::Action::Paint {
                    cells: vec![(0, 0)],
                    placement: crate::net::Placement::Life,
                },
            }),
        );
        rooms.step(a_generation());
        assert_eq!(
            rooms.get(&RoomId::from("hall")).unwrap().world().digest(),
            {
                let mut clean = Rooms::just(Server::named("hall", World::infinite_empty()));
                clean.get_mut(&RoomId::from("hall")).unwrap().join("alice").unwrap();
                clean.step(a_generation());
                clean.get(&RoomId::from("hall")).unwrap().world().digest()
            },
            "a watcher put something in the world"
        );
        let _ = before;
    }

    /// A room made while the server runs is a room, on disk immediately, and
    /// the shape is its own rather than the one the server was started with.
    #[test]
    fn a_room_can_be_made_while_the_server_is_running() {
        let dir = temp_dir("create");
        let mut rooms = Rooms::open(&dir, &[], WorldKind::Infinite, true).unwrap();
        assert_eq!(rooms.names().collect::<Vec<_>>(), [DEFAULT_ROOM]);

        let made = rooms.create("Arena", WorldKind::Toroidal { rows: 4, cols: 4 }).unwrap();
        assert_eq!(made.as_str(), "arena", "names fold to lowercase, as they do on a join");
        assert_eq!(rooms.names().collect::<Vec<_>>(), ["arena", DEFAULT_ROOM]);
        assert_eq!(
            rooms.get(&RoomId::from("arena")).unwrap().world().kind(),
            WorldKind::Toroidal { rows: 4, cols: 4 },
            "its own shape, not the one the server was started with"
        );
        assert_eq!(
            rooms.resolve(Some("arena")).unwrap(),
            RoomId::from("arena"),
            "and joinable at once"
        );

        // On disk before anything is in it, so a crash does not take a room
        // somebody was told they had made.
        assert!(dir.join("arena.ckw").exists());

        // A name that is not one, and a name already taken, are both refused
        // rather than silently doing something else.
        assert!(rooms.create("../escape", WorldKind::Infinite).is_err());
        let taken = rooms.create("arena", WorldKind::Infinite).unwrap_err();
        assert!(taken.contains("already"), "{taken}");
        assert_eq!(
            rooms.get(&RoomId::from("arena")).unwrap().world().kind(),
            WorldKind::Toroidal { rows: 4, cols: 4 },
            "and the refusal left the existing world alone"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not a room must not stop the server, and must not become
    /// a room under a name nobody typed.
    #[test]
    fn a_stray_file_is_ignored_rather_than_opened() {
        let dir = temp_dir("stray");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.join("Mixed Case.ckw"), b"not a world").unwrap();

        let rooms = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        assert_eq!(rooms.names().collect::<Vec<_>>(), [DEFAULT_ROOM]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A world that cannot be read is an error naming the room, not a silent
    /// reset and not an error naming nothing.
    #[test]
    fn a_corrupt_room_is_an_error_that_says_which_room() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.ckw"), b"not a world file at all").unwrap();

        let Err(e) = Rooms::open(&dir, &[], WorldKind::Infinite, false) else {
            panic!("a corrupt room must not be opened");
        };
        assert!(e.to_string().contains("broken"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
