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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::net::{ClientMessage, Made, RoomId, RoomInfo, RoomName, ServerMessage, DEFAULT_ROOM};
use crate::server::matches::{Phase, Victory};
use crate::server::Server;
use crate::sim::{PlayerId, WorldKind};

/// Where a connected player is: which world, and who they are in it. Player
/// numbers are per room, so the number alone does not identify anybody.
pub type Seat = (RoomId, PlayerId);

/// The extension a room's world is saved under.
const SAVE_EXT: &str = "ckw";

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
}

impl Caller {
    /// A connection that has not joined anything.
    pub fn new(connection: ConnectionId) -> Self {
        Self { connection, seat: None, watching: None }
    }

    /// For tests, and for the console, which is nobody's socket.
    pub fn nobody() -> Self {
        Self::new(0)
    }

    pub fn sitting(connection: ConnectionId, seat: Seat) -> Self {
        Self { connection, seat: Some(seat), watching: None }
    }

    /// Which room's messages this connection may be routed to, seated or not.
    fn room(&self) -> Option<&RoomId> {
        self.seat.as_ref().map(|(room, _)| room).or(self.watching.as_ref())
    }
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
    /// operator typed the name, and nothing here survives a restart for the
    /// same reason a connection does not.
    ///
    /// Nothing reads the owner yet. What recording it buys is that "close what
    /// you opened" and "you have three open already" are both answerable later
    /// without a migration, and that the log line for a room that appeared
    /// says who asked for it.
    made: BTreeMap<RoomId, ConnectionId>,
    /// Whose match each client-made room is: the player who may start it.
    ///
    /// A `PlayerId` and not the connection that asked, because a connection
    /// does not survive a reconnect and the person does — a rejoin token
    /// brings somebody back to the same number, so ownership recorded this way
    /// survives a refresh, which is exactly when somebody would otherwise find
    /// they could no longer start their own match.
    ///
    /// Set on the **creating connection's first join**, which is the moment
    /// there is a player to record. A room whose maker never joined has none,
    /// and so cannot be started from a client at all.
    owner: BTreeMap<RoomId, PlayerId>,
    /// The cap on `made`. [`MAX_MADE_ROOMS`] unless a flag says otherwise.
    max_made: usize,
    /// Rooms that are not in the listing, and the code that reaches each.
    ///
    /// A set rather than a map from code to room, because the code **is** the
    /// room's name: a generated name is already unique, already valid, and
    /// already what `Join` carries, so a second namespace to keep in step
    /// would be a second thing that can disagree. What is private about a
    /// private room is that [`Self::listing`] does not mention it.
    unlisted: std::collections::BTreeSet<RoomId>,
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
                rooms.insert(RoomId(name.clone()), server);
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
            rooms.entry(id.clone()).or_insert_with(|| Server::named(name.clone(), shape.build()));
            names.insert(id, name);
        }

        Ok(Self {
            rooms,
            dir,
            default_room,
            names,
            codes: BTreeMap::new(),
            made: BTreeMap::new(),
            owner: BTreeMap::new(),
            max_made: MAX_MADE_ROOMS,
            unlisted: Default::default(),
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
            codes: BTreeMap::new(),
            made: BTreeMap::new(),
            owner: BTreeMap::new(),
            max_made: MAX_MADE_ROOMS,
            unlisted: Default::default(),
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
    /// The error is written to be read by a player: it says what they asked
    /// for and what is actually here, because with no menu yet this reply is
    /// the only way anybody finds out what a server holds.
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
        Err(format!(
            "no room \"{asked}\" here; this server has {}",
            self.public_names().collect::<Vec<_>>().join(", ")
        ))
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
        // Answered without a seat, like `Join` and for the same reason: it
        // names no world. A player has to see the rooms before picking one,
        // and a room *is* a world, so asking from inside one is asking too
        // late.
        if let ClientMessage::Rooms = msg {
            return vec![ServerMessage::Rooms { rooms: self.listing() }];
        }
        // Answered without a seat for a sharper version of the same reason: it
        // names a room that does not exist, so there is nowhere to have been
        // standing when it was sent.
        if let ClientMessage::Create { name, shape, victory, teams, private } = msg {
            return vec![ServerMessage::Made(self.make(
                caller.connection,
                &name,
                shape,
                victory,
                teams,
                private,
            ))];
        }
        // Admitted at any generation, and that is the point rather than an
        // oversight: **no late joining is a rule about players.** Somebody
        // turning up at generation four hundred is exactly what watching is
        // for, so this asks only whether the room is here.
        if let ClientMessage::Watch { room } = &msg {
            return match self.resolve(Some(room.as_str())) {
                Ok(id) => {
                    let name = self.name_of(&id).to_string();
                    let server = self.rooms.get(&id).expect("resolve only returns rooms here");
                    log::info!("connection {} is watching \"{name}\" ({id})", caller.connection);
                    vec![ServerMessage::Watching {
                        room: id.clone(),
                        name,
                        tick: server.tick(),
                        world: server.world().kind(),
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
            if self.owner.get(room) != Some(player) {
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
        let seat = caller.seat.as_ref();
        if let ClientMessage::Join { room, .. } = &msg {
            let asked = room.clone();
            return match self.resolve(asked.as_ref().map(RoomId::as_str)) {
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
                        .handle(None, msg);
                    // The creator's first join is where a room's owner is
                    // recorded: it is the first moment there is a player to
                    // record, and a `PlayerId` survives the reconnect that a
                    // connection id does not.
                    if self.made.get(&name) == Some(&caller.connection) {
                        if let Some(ServerMessage::Welcome { you, .. }) =
                            out.iter().find(|m| matches!(m, ServerMessage::Welcome { .. }))
                        {
                            self.owner.entry(name.clone()).or_insert(*you);
                        }
                    }
                    let owner = self.owner.get(&name).copied();
                    let code = self.codes.get(&name).cloned();
                    for reply in &mut out {
                        if let ServerMessage::Welcome { room, name: called, .. } = reply {
                            *room = name.clone();
                            *called = room_name.clone();
                        }
                        stamp(reply, owner, code.clone());
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
            Some(server) => server.handle(id, msg),
            // Only reachable if a room could go away under a seated player,
            // which nothing does yet. Said out loud rather than ignored,
            // because the symptom would be one client silently going deaf.
            None => {
                log::warn!("{id:?} is in room \"{room}\", which is not here");
                Vec::new()
            }
        }
    }

    /// Advance every room one generation, and say which room each reply
    /// belongs to. A `Step` is only meaningful to the clients in its own
    /// world, so the room travels with it as far as the connection that
    /// decides whether to send it on.
    pub fn step(&mut self) -> Vec<(RoomId, ServerMessage)> {
        // Cloned so the stamp below can read them while the rooms are borrowed
        // mutably. Sixteen bytes and a short string per room, once a tick.
        let owners = self.owner.clone();
        let codes = self.codes.clone();
        self.rooms
            .iter_mut()
            .flat_map(|(id, server)| {
                let (owner, code) = (owners.get(id).copied(), codes.get(id).cloned());
                server.step().into_iter().map(move |mut m| {
                    stamp(&mut m, owner, code.clone());
                    (id.clone(), m)
                })
            })
            .collect()
    }

    pub fn leave(&mut self, (room, id): &Seat) {
        if let Some(server) = self.rooms.get_mut(room) {
            server.leave(*id);
        }
    }

    /// Save every room. One failure does not stop the others: a room that
    /// cannot be written is one world lost, and giving up would lose the rest
    /// as well.
    pub fn save(&self) -> std::io::Result<()> {
        if self.dir.as_os_str().is_empty() {
            return Ok(());
        }
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
        let name = crate::net::room_name(name)?;
        // A room made at the console takes its name as its id, so the
        // directory on disk stays readable and `--room arena` reaches the same
        // room after a restart. Only rooms made over the wire get a generated
        // id — that is where a rename has to survive.
        let id = RoomId(name.clone());
        if self.rooms.contains_key(&id) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let server = Server::named(name.clone(), shape.build());
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
        let name = crate::net::room_name(name)?;
        let id = RoomId(name.clone());
        if self.rooms.contains_key(&id) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let mut server = Server::named(name.clone(), shape.build());
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
        private: bool,
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
        // A name is still a name on a private room. What used to happen here
        // was that the code *became* the name, which conflated a credential
        // with an identity: the code could never be changed, and somebody
        // making a game for four friends could not call it anything.
        let name = crate::net::room_name(name).or_else(|e| {
            // A private room may go unnamed, since nobody browses for it.
            if private && name.trim().is_empty() {
                Ok(UNNAMED.to_string())
            } else {
                Err(e)
            }
        })?;
        if self.names.values().any(|n| *n == name) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let id = self.free_id()?;
        let mut server = Server::named(name.clone(), shape.build());
        if let Some(victory) = victory {
            server.make_match(victory);
            // Sides only on a match. A team is a way of deciding a result, and
            // a world has none — so a world asked for teams is a world with a
            // field nobody could ever read.
            if let Some(n) = teams {
                server.make_teams(n)?;
            }
        } else if teams.is_some() {
            return Err("only a match has teams".into());
        }
        let path = save_path(&self.dir, &id);
        if victory.is_none() && !self.dir.as_os_str().is_empty() {
            server.save(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        self.rooms.insert(id.clone(), server);
        self.names.insert(id.clone(), name.clone());
        let code = if private {
            self.unlisted.insert(id.clone());
            let code = self.free_code()?;
            self.codes.insert(id.clone(), code.clone());
            Some(code)
        } else {
            None
        };
        self.made.insert(id.clone(), by);
        log::info!(
            "connection {by} made {} room \"{name}\" ({id}){}",
            if private { "a private" } else { "an open" },
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

    /// Which connection asked for this room, if a client did.
    pub fn made_by(&self, id: &RoomId) -> Option<ConnectionId> {
        self.made.get(id).copied()
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
        self.names.remove(&id);
        log::info!("deleted room \"{name}\"");
        Ok(id)
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
            .map(|(id, server)| {
                (
                    RoomInfo {
                        id: id.clone(),
                        name: self.name_of(id).to_string(),
                        phase: server.phase().clone(),
                        victory: server.victory(),
                        players: server.players().filter(|p| p.online).count() as u32,
                        world: server.world().kind(),
                    },
                    self.unlisted.contains(id),
                )
            })
            .collect()
    }

    pub fn listing(&self) -> Vec<RoomInfo> {
        self.rooms
            .iter()
            .filter(|(id, _)| !self.unlisted.contains(*id))
            .map(|(id, server)| RoomInfo {
                id: id.clone(),
                name: self.name_of(id).to_string(),
                phase: server.phase().clone(),
                victory: server.victory(),
                players: server.players().filter(|p| p.online).count() as u32,
                world: server.world().kind(),
            })
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
    if let ServerMessage::Match { owner: whose, code: reachable, .. } = msg {
        *whose = owner;
        *reachable = code;
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
        let stepped = rooms.step();
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
            rooms.step();
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
            rooms.step();
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
                token: None,
                room: Some(RoomId::from("hall")),
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
        let [ServerMessage::Rooms { rooms: listed }] = &replies[..] else {
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
        let [ServerMessage::Rooms { rooms: listed }] =
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
            token: None,
            room: Some(room.into()),
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
            ClientMessage::Join { name: "alice".into(), token: None, room: None },
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
            ClientMessage::Join { name: "alice".into(), token: None, room: Some(made_id.clone()) },
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

        rooms.make(1, "plain", WorldKind::Infinite, None, None, false).unwrap();
        rooms
            .make(
                1,
                "cup",
                WorldKind::Infinite,
                Some(Victory::Territory { squares: 500 }),
                None,
                false,
            )
            .unwrap();

        let [ServerMessage::Rooms { rooms: listed }] = &rooms.handle(&me, ClientMessage::Rooms)[..]
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

        assert!(rooms.make(1, "a", WorldKind::Infinite, None, None, false).is_ok());
        assert!(rooms.make(1, "b", WorldKind::Infinite, None, None, false).is_ok());
        let (made, cap) = rooms.made_count();
        assert_eq!((made, cap), (2, 2));

        let refused = rooms.make(1, "c", WorldKind::Infinite, None, None, false).unwrap_err();
        assert!(refused.contains('2'), "the refusal says how many: {refused}");
        assert!(rooms.get(&RoomId::from("c")).is_none(), "and made none");

        // Deleting one frees a slot, or a server that had made and deleted its
        // cap's worth would refuse for ever while holding nothing.
        rooms.delete("a").unwrap();
        assert_eq!(rooms.made_count().0, 1);
        assert!(rooms.make(1, "c", WorldKind::Infinite, None, None, false).is_ok());
    }

    /// A private room is reachable by its code and mentioned nowhere else —
    /// including in the refusal a mistyped name gets back, which used to name
    /// every room on the server and would have handed out every code.
    #[test]
    fn a_private_room_is_reachable_by_code_and_named_nowhere() {
        let mut rooms =
            Rooms::open(temp_dir("private"), &["hall".into()], WorldKind::Infinite, true).unwrap();

        let made = rooms.make(3, "friends-only", WorldKind::Infinite, None, None, true).unwrap();
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

    /// Whoever is running the server can read the save directory anyway, and
    /// an operator who cannot see a room cannot delete one being misused.
    #[test]
    fn the_console_sees_private_rooms_and_the_wire_does_not() {
        let mut rooms =
            Rooms::open(temp_dir("console-sees"), &["hall".into()], WorldKind::Infinite, true)
                .unwrap();
        let made = rooms.make(3, "", WorldKind::Infinite, None, None, true).unwrap();
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
        let mine = Caller::new(5);
        let theirs = Caller::new(6);

        let made = rooms
            .make(
                5,
                "cup",
                WorldKind::Infinite,
                Some(Victory::Timer { generations: 50 }),
                None,
                false,
            )
            .unwrap();
        let join = |name: &str| ClientMessage::Join {
            name: name.into(),
            token: None,
            room: Some(made.id.clone()),
        };

        // Nobody owns it until the maker joins: the owner is a PlayerId, and
        // there is no player until somebody has one.
        let out = rooms.handle(&mine, join("owner"));
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
            ClientMessage::Join { name: "maker".into(), token: None, room: Some(made.id.clone()) },
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
        let broadcast = rooms.step();
        let owner = broadcast
            .iter()
            .find_map(|(_, m)| match m {
                ServerMessage::Match { owner, .. } => Some(*owner),
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
            ClientMessage::Join { name: "someone".into(), token: None, room: Some(id.clone()) },
        );
        let [ServerMessage::Welcome { you, .. }] = &out[..] else { panic!("{out:?}") };

        let out = rooms.handle(&Caller::sitting(3, (id.clone(), *you)), ClientMessage::Start);
        let [ServerMessage::NotStarted { reason }] = &out[..] else { panic!("{out:?}") };
        assert!(reason.contains("console"), "{reason}");
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
            rooms.step();
        }

        let late = Caller::new(11);
        let replies = rooms.handle(
            &late,
            ClientMessage::Join {
                name: "late".into(),
                token: None,
                room: Some(RoomId::from("cup")),
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
        let watcher = Caller { connection: 4, seat: None, watching: Some(RoomId::from("hall")) };

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
                action: crate::net::Action::Paint {
                    cells: vec![(0, 0)],
                    placement: crate::net::Placement::Life,
                },
            }),
        );
        rooms.step();
        assert_eq!(
            rooms.get(&RoomId::from("hall")).unwrap().world().digest(),
            {
                let mut clean = Rooms::just(Server::named("hall", World::infinite_empty()));
                clean.get_mut(&RoomId::from("hall")).unwrap().join("alice").unwrap();
                clean.step();
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
