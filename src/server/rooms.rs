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

use crate::net::{ClientMessage, RoomInfo, RoomName, ServerMessage, DEFAULT_ROOM};
use crate::server::matches::{Phase, Victory};
use crate::server::Server;
use crate::sim::{PlayerId, WorldKind};

/// Where a connected player is: which world, and who they are in it. Player
/// numbers are per room, so the number alone does not identify anybody.
pub type Seat = (RoomName, PlayerId);

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
}

impl Caller {
    /// A connection that has not joined anything.
    pub fn new(connection: ConnectionId) -> Self {
        Self { connection, seat: None }
    }

    /// For tests, and for the console, which is nobody's socket.
    pub fn nobody() -> Self {
        Self::new(0)
    }

    pub fn sitting(connection: ConnectionId, seat: Seat) -> Self {
        Self { connection, seat: Some(seat) }
    }
}

/// How many rooms a server will hold once clients are the ones making them.
///
/// A room costs a full simulation four times a second for as long as the
/// process lives, whether or not anybody is in it, so a server that makes one
/// for whoever asks is a server anybody can fill. This is the backstop rather
/// than the fix — see [auto-sleep] — and it counts only rooms made over the
/// wire: an operator declaring forty on the command line has made a decision,
/// and this is not the place to second-guess it.
///
/// [auto-sleep]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#the-policy-which-is-the-actual-blocker
pub const MAX_MADE_ROOMS: usize = 32;

pub struct Rooms {
    /// Sorted, so every listing — a log line, a rejection, a save sweep — is
    /// in the same order however the map was filled.
    rooms: BTreeMap<RoomName, Server>,
    dir: PathBuf,
    /// Where a client that names no room is put.
    default_room: RoomName,
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
    made: BTreeMap<RoomName, ConnectionId>,
    /// The cap on `made`. [`MAX_MADE_ROOMS`] unless a flag says otherwise.
    max_made: usize,
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

        if !fresh {
            for name in saved_in(&dir)? {
                let path = save_path(&dir, &name);
                let server =
                    Server::load_or_new(&path, name.clone(), || shape.build()).map_err(|e| {
                        std::io::Error::new(
                            e.kind(),
                            format!("room \"{name}\" ({}): {e}", path.display()),
                        )
                    })?;
                rooms.insert(name, server);
            }
        }

        let default_room = match declared.first() {
            Some(first) => crate::net::room_name(first)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            None => DEFAULT_ROOM.to_string(),
        };

        for raw in declared.iter().map(String::as_str).chain([default_room.as_str()]) {
            let name = crate::net::room_name(raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            rooms.entry(name.clone()).or_insert_with(|| Server::named(name, shape.build()));
        }

        Ok(Self { rooms, dir, default_room, made: BTreeMap::new(), max_made: MAX_MADE_ROOMS })
    }

    /// A single room, with nothing on disk behind it. What the tests want.
    pub fn just(server: Server) -> Self {
        let name = server.room().to_string();
        Self {
            rooms: BTreeMap::from([(name.clone(), server)]),
            dir: PathBuf::new(),
            default_room: name,
            made: BTreeMap::new(),
            max_made: MAX_MADE_ROOMS,
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.rooms.keys().map(String::as_str)
    }

    pub fn default_room(&self) -> &str {
        &self.default_room
    }

    pub fn get(&self, room: &str) -> Option<&Server> {
        self.rooms.get(room)
    }

    pub fn get_mut(&mut self, room: &str) -> Option<&mut Server> {
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
    pub fn resolve(&self, asked: Option<&str>) -> Result<RoomName, String> {
        let Some(asked) = asked else {
            return Ok(self.default_room.clone());
        };
        let name = crate::net::room_name(asked)?;
        if self.rooms.contains_key(&name) {
            return Ok(name);
        }
        Err(format!(
            "no room \"{name}\" here; this server has {}",
            self.names().collect::<Vec<_>>().join(", ")
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
        if let ClientMessage::Create { name, shape, victory } = msg {
            return vec![ServerMessage::Made(self.make(caller.connection, &name, shape, victory))];
        }
        let seat = caller.seat.as_ref();
        if let ClientMessage::Join { room, .. } = &msg {
            let asked = room.clone();
            return match self.resolve(asked.as_deref()) {
                Ok(name) => {
                    if let Some(seat) = seat {
                        log::info!("{:?} is leaving room \"{}\" for \"{name}\"", seat.1, seat.0);
                        self.leave(seat);
                    }
                    self.rooms
                        .get_mut(&name)
                        .expect("resolve only returns rooms that are here")
                        .handle(None, msg)
                }
                Err(reason) => {
                    log::info!("refused a join for {asked:?}: {reason}");
                    vec![ServerMessage::Rejected { reason }]
                }
            };
        }

        let Some((room, id)) = seat else {
            log::debug!("a message from a connection that has not joined; dropped");
            return Vec::new();
        };
        match self.rooms.get_mut(room) {
            Some(server) => server.handle(Some(*id), msg),
            // Only reachable if a room could go away under a seated player,
            // which nothing does yet. Said out loud rather than ignored,
            // because the symptom would be one client silently going deaf.
            None => {
                log::warn!("{id:?} is seated in room \"{room}\", which is not here");
                Vec::new()
            }
        }
    }

    /// Advance every room one generation, and say which room each reply
    /// belongs to. A `Step` is only meaningful to the clients in its own
    /// world, so the room travels with it as far as the connection that
    /// decides whether to send it on.
    pub fn step(&mut self) -> Vec<(RoomName, ServerMessage)> {
        self.rooms
            .iter_mut()
            .flat_map(|(name, server)| server.step().into_iter().map(move |m| (name.clone(), m)))
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
        for (name, server) in &self.rooms {
            // A match is an event rather than a world to keep: it has an end,
            // and a half-finished one restored into a server that has
            // forgotten it was a match would run on forever with nobody able
            // to win it. Losing it on a restart is the honest outcome.
            if !matches!(server.phase(), crate::server::matches::Phase::Open) {
                continue;
            }
            let path = save_path(&self.dir, name);
            if let Err(e) = server.save(&path) {
                log::error!("saving room \"{name}\" to {}: {e}", path.display());
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
    pub fn create(&mut self, name: &str, shape: WorldKind) -> Result<RoomName, String> {
        let name = crate::net::room_name(name)?;
        if self.rooms.contains_key(&name) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let server = Server::named(name.clone(), shape.build());
        let path = save_path(&self.dir, &name);
        if !self.dir.as_os_str().is_empty() {
            server.save(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        log::info!("created room \"{name}\"");
        self.rooms.insert(name.clone(), server);
        Ok(name)
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
    ) -> Result<RoomName, String> {
        let name = crate::net::room_name(name)?;
        if self.rooms.contains_key(&name) {
            return Err(format!("there is already a room called \"{name}\""));
        }
        let mut server = Server::named(name.clone(), shape.build());
        server.make_match(victory);
        log::info!("made match \"{name}\": {}", victory.describe());
        self.rooms.insert(name.clone(), server);
        Ok(name)
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
    ) -> Result<RoomName, String> {
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
        let name = match victory {
            Some(victory) => self.new_match(name, shape, victory)?,
            None => self.create(name, shape)?,
        };
        self.made.insert(name.clone(), by);
        log::info!("connection {by} made room \"{name}\"");
        Ok(name)
    }

    /// Which connection asked for this room, if a client did.
    pub fn made_by(&self, name: &str) -> Option<ConnectionId> {
        self.made.get(name).copied()
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
    pub fn start_match(&mut self, name: &str) -> Result<RoomName, String> {
        let name = crate::net::room_name(name)?;
        let server = self
            .rooms
            .get_mut(&name)
            .ok_or_else(|| format!("there is no room called \"{name}\""))?;
        server.start_match()?;
        log::info!("match \"{name}\" started at tick {}", server.tick());
        Ok(name)
    }

    /// Start the one match that is waiting.
    ///
    /// A convenience for the common case, and it refuses rather than guesses
    /// when there is more than one: starting the wrong match is not something
    /// that can be taken back.
    pub fn dispatch(&mut self) -> Result<RoomName, String> {
        let waiting: Vec<RoomName> = self
            .rooms
            .iter()
            .filter(|(_, s)| matches!(s.phase(), Phase::Gathering))
            .map(|(name, _)| name.clone())
            .collect();
        match waiting.as_slice() {
            [] => Err("no match is waiting to start".into()),
            [only] => self.start_match(only),
            several => Err(format!(
                "{} matches are waiting; name one of {}",
                several.len(),
                several.join(", ")
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
    pub fn delete(&mut self, name: &str) -> Result<RoomName, String> {
        let name = crate::net::room_name(name)?;
        if name == self.default_room {
            return Err(format!(
                "\"{name}\" is the default room; every client that names none goes there"
            ));
        }
        let server =
            self.rooms.get(&name).ok_or_else(|| format!("there is no room called \"{name}\""))?;
        let here = server.players().filter(|p| p.online).count();
        if here > 0 {
            return Err(format!("{here} still in \"{name}\""));
        }
        self.rooms.remove(&name);
        if !self.dir.as_os_str().is_empty() {
            let path = save_path(&self.dir, &name);
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("removed room \"{name}\" but not {}: {e}", path.display());
                }
            }
        }
        // Forgotten here too, or a server that made and deleted its cap's
        // worth of rooms would refuse to make another while holding none.
        self.made.remove(&name);
        log::info!("deleted room \"{name}\"");
        Ok(name)
    }

    /// Stop or start a world.
    pub fn set_asleep(&mut self, name: &str, asleep: bool) -> Result<RoomName, String> {
        let name = crate::net::room_name(name)?;
        let server = self
            .rooms
            .get_mut(&name)
            .ok_or_else(|| format!("there is no room called \"{name}\""))?;
        server.set_asleep(asleep)?;
        log::info!("room \"{name}\" is {}", if asleep { "asleep" } else { "awake" });
        Ok(name)
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
    pub fn listing(&self) -> Vec<RoomInfo> {
        self.rooms
            .iter()
            .map(|(name, server)| RoomInfo {
                name: name.clone(),
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

fn save_path(dir: &Path, room: &str) -> PathBuf {
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
        assert_eq!(rooms.default_room(), "lobby", "the first declared is the default");
        assert_eq!(rooms.resolve(None).unwrap(), "lobby");
        assert_eq!(rooms.resolve(Some("ARENA")).unwrap(), "arena", "names fold to lowercase");

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

        let a = rooms.get_mut("a").unwrap().join("alice").unwrap();
        let b = rooms.get_mut("b").unwrap().join("bob").unwrap();
        assert_eq!((a, b), (PlayerId(1), PlayerId(1)), "numbers are per room");

        assert_eq!(rooms.get("a").unwrap().player_count(), 1);
        assert_eq!(rooms.get("b").unwrap().player_count(), 1);

        // Alice's ground is in her world and nowhere else. Both players hold
        // number one, so a shared world would have them standing on it
        // together and this would pass for the wrong reason -- hence the
        // second player's own room being checked for emptiness too.
        rooms.get_mut("a").unwrap().step();
        let (row, col) = crate::net::spawn_for(a, rooms.get("a").unwrap().world());
        assert!(rooms.get("a").unwrap().world().cell_at(row, col).is_some());
        assert_eq!(
            rooms.get("b").unwrap().world().generation,
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
        assert_eq!(rooms.get("a").unwrap().tick(), 1);
        assert_eq!(rooms.get("b").unwrap().tick(), 1);
    }

    /// A room is a file, so a restart finds it without being told again.
    #[test]
    fn a_saved_room_comes_back_without_being_declared() {
        let dir = temp_dir("saved");
        {
            let mut rooms = Rooms::open(&dir, &["kept".into()], WorldKind::Infinite, true).unwrap();
            rooms.get_mut("kept").unwrap().join("alice").unwrap();
            rooms.step();
            rooms.save().unwrap();
        }

        let back = Rooms::open(&dir, &[], WorldKind::Infinite, false).unwrap();
        assert!(back.get("kept").is_some(), "the file is the declaration");
        assert_eq!(back.get("kept").unwrap().tick(), 1, "and it kept its tick");
        assert!(
            back.get(DEFAULT_ROOM).is_some(),
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
        assert_eq!(back.get("kept").unwrap().tick(), 0);
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
            ClientMessage::Join { name: "alice".into(), token: None, room: Some("hall".into()) },
        );
        let [ServerMessage::Welcome { room, world, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        assert_eq!(room, "hall", "the welcome names the room it let you into");
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
        rooms.get_mut("arena").unwrap().join("alice").unwrap();

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
        rooms.get_mut("arena").unwrap().leave(PlayerId(1));
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
            !rooms.get("a").unwrap().players().any(|p| p.online),
            "nobody is left standing in the room she left"
        );
        assert!(rooms.get("b").unwrap().players().any(|p| p.online));

        // A refused change leaves her where she was. Her client learns where
        // it is from the Welcome it will not get, so anything else would have
        // the two disagreeing about which world she is in.
        let seat: Seat = ("b".into(), PlayerId(1));
        let replies = rooms.handle(&Caller::sitting(1, seat.clone()), join("nowhere"));
        assert!(matches!(replies[..], [ServerMessage::Rejected { .. }]));
        assert!(
            rooms.get("b").unwrap().players().any(|p| p.online),
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
            },
        );
        let [ServerMessage::Made(Ok(name))] = &replies[..] else {
            panic!("expected a name, got {replies:?}");
        };
        assert_eq!(name, "arena", "the server names the room it actually made");
        assert_eq!(rooms.made_by("arena"), Some(7), "and remembers who asked");

        let name = name.clone();
        let replies = rooms.handle(
            &me,
            ClientMessage::Join { name: "alice".into(), token: None, room: Some(name) },
        );
        let [ServerMessage::Welcome { room, world, .. }] = &replies[..] else {
            panic!("expected a welcome, got {replies:?}");
        };
        assert_eq!(room, "arena");
        assert_eq!(*world, WorldKind::Toroidal { rows: 4, cols: 6 }, "the shape it asked for");
    }

    /// A win condition is the whole of the difference between a world and a
    /// match, so one message makes either.
    #[test]
    fn a_victory_makes_a_match_and_no_victory_makes_a_world() {
        let mut rooms = Rooms::just(Server::named("hall", World::infinite_empty()));
        let me = Caller::new(1);

        rooms.make(1, "plain", WorldKind::Infinite, None).unwrap();
        rooms
            .make(1, "cup", WorldKind::Infinite, Some(Victory::Territory { squares: 500 }))
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
            },
        );
        let [ServerMessage::Made(Err(why))] = &replies[..] else {
            panic!("expected a refusal, got {replies:?}");
        };
        assert!(why.contains("hall"), "the refusal names the room: {why}");
        assert_eq!(rooms.len(), before, "and nothing was made");
        assert_eq!(rooms.made_by("hall"), None, "an existing room gets no owner");
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

        assert!(rooms.make(1, "a", WorldKind::Infinite, None).is_ok());
        assert!(rooms.make(1, "b", WorldKind::Infinite, None).is_ok());
        let (made, cap) = rooms.made_count();
        assert_eq!((made, cap), (2, 2));

        let refused = rooms.make(1, "c", WorldKind::Infinite, None).unwrap_err();
        assert!(refused.contains('2'), "the refusal says how many: {refused}");
        assert!(rooms.get("c").is_none(), "and made none");

        // Deleting one frees a slot, or a server that had made and deleted its
        // cap's worth would refuse for ever while holding nothing.
        rooms.delete("a").unwrap();
        assert_eq!(rooms.made_count().0, 1);
        assert!(rooms.make(1, "c", WorldKind::Infinite, None).is_ok());
    }

    /// A room made while the server runs is a room, on disk immediately, and
    /// the shape is its own rather than the one the server was started with.
    #[test]
    fn a_room_can_be_made_while_the_server_is_running() {
        let dir = temp_dir("create");
        let mut rooms = Rooms::open(&dir, &[], WorldKind::Infinite, true).unwrap();
        assert_eq!(rooms.names().collect::<Vec<_>>(), [DEFAULT_ROOM]);

        let made = rooms.create("Arena", WorldKind::Toroidal { rows: 4, cols: 4 }).unwrap();
        assert_eq!(made, "arena", "names fold to lowercase, as they do on a join");
        assert_eq!(rooms.names().collect::<Vec<_>>(), ["arena", DEFAULT_ROOM]);
        assert_eq!(
            rooms.get("arena").unwrap().world().kind(),
            WorldKind::Toroidal { rows: 4, cols: 4 },
            "its own shape, not the one the server was started with"
        );
        assert_eq!(rooms.resolve(Some("arena")).unwrap(), "arena", "and joinable at once");

        // On disk before anything is in it, so a crash does not take a room
        // somebody was told they had made.
        assert!(dir.join("arena.ckw").exists());

        // A name that is not one, and a name already taken, are both refused
        // rather than silently doing something else.
        assert!(rooms.create("../escape", WorldKind::Infinite).is_err());
        let taken = rooms.create("arena", WorldKind::Infinite).unwrap_err();
        assert!(taken.contains("already"), "{taken}");
        assert_eq!(
            rooms.get("arena").unwrap().world().kind(),
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
