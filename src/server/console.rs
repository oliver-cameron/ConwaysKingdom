//! Typed commands on the server's own terminal.
//!
//! A room is declared on the command line and there was no way to make one
//! afterwards, so adding a world meant stopping the server, which disconnects
//! everybody in every *other* world to add one nobody is in yet. This is the
//! smaller lever: type a name, get a room.
//!
//! **Parsing and doing are here; reading is not.** [`run`] takes a line and a
//! [`Rooms`] and returns what to print, so the whole surface is a pure
//! function of a string and the worlds — which is the only way any of it can
//! be tested, since a terminal is not something a test has. The reading lives
//! in [`crate::server::ws`], on a thread of its own, because a blocking
//! `read_line` in an async loop is a task that cannot be cancelled.
//!
//! Answers go to **stdout**, not to the log. A log line is something that
//! happened; the answer to a question somebody typed is neither a warning nor
//! a record, and routing it through the logger would let a `--quiet` swallow
//! the reply to a command.

use crate::server::matches::{Phase, Victory};
use crate::server::rooms::Rooms;
use crate::sim::WorldKind;

/// What a command produced: something to print, and whether to stop.
pub struct Reply {
    pub lines: Vec<String>,
    /// The server should shut down, saving on the way out.
    pub stop: bool,
}

impl Reply {
    fn say(line: impl Into<String>) -> Self {
        Self { lines: vec![line.into()], stop: false }
    }

    fn lines(lines: Vec<String>) -> Self {
        Self { lines, stop: false }
    }

    fn nothing() -> Self {
        Self { lines: Vec::new(), stop: false }
    }
}

pub const HELP: &[(&str, &str)] = &[
    ("new NAME [ROWSxCOLS]", "make a room; wrapping if a size is given"),
    ("rooms", "what rooms there are, and who is in them"),
    ("match new NAME SHAPE [ROWSxCOLS] HOW N", "a match: infinite|toroidal, timer|territory"),
    ("match start NAME", "start that match's clock"),
    ("match dispatch", "start the one match that is waiting"),
    ("match", "what matches there are, and what they are doing"),
    ("stop", "save every room and shut down"),
    ("help", "this"),
];

/// One typed line.
///
/// `default_shape` is what the server was started with, so `new arena` makes
/// the same kind of world the command line asked for and `new arena 18x18`
/// overrides it. That is the whole of per-room shapes for now: enough to run
/// a wrapping world beside a boundless one, which a single `--torus` cannot.
pub fn run(line: &str, rooms: &mut Rooms, default_shape: WorldKind) -> Reply {
    let line = line.trim();
    // A bare newline is somebody pressing return, not a command. Answering it
    // with "unknown command" would fill the terminal with complaints about
    // nothing.
    if line.is_empty() {
        return Reply::nothing();
    }

    let mut words = line.split_whitespace();
    let command = words.next().unwrap_or_default();
    let rest: Vec<&str> = words.collect();

    match command {
        "help" | "?" => Reply::lines(
            HELP.iter().map(|(form, what)| format!("  {form:<22} {what}")).collect(),
        ),

        // Named for what it does to the process, not for what the person is
        // doing: `quit` reads as leaving, and there is nothing here to leave
        // -- the whole server goes with it, and everybody in every room.
        "stop" | "quit" | "exit" => Reply { lines: vec!["stopping".into()], stop: true },

        "rooms" | "ls" => {
            let listing = rooms.listing();
            Reply::lines(
                listing
                    .iter()
                    .map(|room| {
                        let here = if room.name == rooms.default_room() { " (default)" } else { "" };
                        format!(
                            "  {:<24} {:<22} {} online{here}",
                            room.name,
                            describe(room.world),
                            room.players
                        )
                    })
                    .collect(),
            )
        }

        "new" | "room" => {
            let Some(name) = rest.first() else {
                return Reply::say("new NAME [ROWSxCOLS] -- a room needs a name");
            };
            let shape = match rest.get(1) {
                None => default_shape,
                Some(size) => match crate::sim::parse_torus(size) {
                    Ok(shape) => shape,
                    Err(e) => return Reply::say(e),
                },
            };
            match rooms.create(name, shape) {
                Ok(name) => Reply::say(format!("made \"{name}\", {}", describe(shape))),
                Err(e) => Reply::say(e),
            }
        }

        "match" | "m" => match_command(&rest, rooms),

        other => Reply::say(format!("no command \"{other}\"; try help")),
    }
}

/// `match`, and everything under it.
///
/// Its own function because it is a small vocabulary of its own rather than
/// one more verb, and because the parsing is the part worth reading: the shape
/// and the win condition are both two words, and telling them apart is what
/// the whole form turns on.
fn match_command(rest: &[&str], rooms: &mut Rooms) -> Reply {
    match rest.first().copied() {
        // Bare `match` lists them, the way bare `rooms` does. A verb that
        // needs an argument to do anything should say what there is when it
        // is given none.
        None | Some("ls") | Some("list") => {
            let listing = rooms.matches();
            if listing.is_empty() {
                return Reply::say("no matches; try match new infinite timer 2000");
            }
            Reply::lines(
                listing
                    .iter()
                    .map(|(name, phase, victory, players)| {
                        let how = victory.map(|v| v.describe()).unwrap_or_default();
                        let result = match phase {
                            Phase::Over { winner: Some(id), held, .. } => {
                                format!("  won by player {} with {held}", id.0)
                            }
                            Phase::Over { winner: None, .. } => "  nobody held anything".into(),
                            _ => String::new(),
                        };
                        format!("  {name:<12} {:<10} {players} in   {how}{result}", phase.name())
                    })
                    .collect(),
            )
        }

        Some("new") => {
            // name shape [size] how n -- the size is there only for a torus,
            // so the count of words is what says whether it was given.
            let parsed = match &rest[1..] {
                [name, shape, how, n] => Some((*name, parse_shape(shape, None), *how, *n)),
                [name, shape, size, how, n] => {
                    Some((*name, parse_shape(shape, Some(size)), *how, *n))
                }
                _ => None,
            };
            let Some((name, shape, how, n)) = parsed else {
                return Reply::say(
                    "match new NAME SHAPE [ROWSxCOLS] HOW N -- infinite|toroidal, timer|territory",
                );
            };
            let shape = match shape {
                Ok(shape) => shape,
                Err(e) => return Reply::say(e),
            };
            let victory = match Victory::parse(how, n) {
                Ok(v) => v,
                Err(e) => return Reply::say(e),
            };
            match rooms.new_match(name, shape, victory) {
                Ok(name) => Reply::lines(vec![
                    format!("made \"{name}\", {}, {}", describe(shape), victory.describe()),
                    format!("  gathering; nothing steps until `match start {name}`"),
                ]),
                Err(e) => Reply::say(e),
            }
        }

        Some("start") => match rest.get(1) {
            None => Reply::say("match start NAME -- or `match dispatch` if only one is waiting"),
            Some(name) => match rooms.start_match(name) {
                Ok(name) => Reply::say(format!("\"{name}\" is running; no more joining")),
                Err(e) => Reply::say(e),
            },
        },

        Some("dispatch" | "go") => match rooms.dispatch() {
            Ok(name) => Reply::say(format!("\"{name}\" is running; no more joining")),
            Err(e) => Reply::say(e),
        },

        Some(other) => Reply::say(format!("no match command \"{other}\"; try help")),
    }
}

/// `infinite`, or `toroidal` and a size.
///
/// A torus without a size is refused rather than given a default: how big a
/// wrapping world is, is the whole of what makes one match different from
/// another, and guessing it would make the important number the invisible one.
fn parse_shape(shape: &str, size: Option<&str>) -> Result<WorldKind, String> {
    match (shape, size) {
        ("infinite" | "boundless", None) => Ok(WorldKind::Infinite),
        ("infinite" | "boundless", Some(_)) => {
            Err("an infinite world has no size to give".into())
        }
        ("toroidal" | "torus" | "wrapping", Some(size)) => crate::sim::parse_torus(size),
        ("toroidal" | "torus" | "wrapping", None) => {
            Err("a wrapping world needs a size, as ROWSxCOLS".into())
        }
        (other, _) => Err(format!("no world shape \"{other}\"; try infinite or toroidal")),
    }
}

fn describe(world: WorldKind) -> String {
    match world {
        WorldKind::Infinite => "boundless".to_string(),
        WorldKind::Toroidal { rows, cols } => format!("{rows}x{cols} chunks, wrapping"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Server;
    use crate::sim::World;

    fn rooms() -> Rooms {
        Rooms::just(Server::named("main", World::infinite_empty()))
    }

    fn out(line: &str, rooms: &mut Rooms) -> String {
        run(line, rooms, WorldKind::Infinite).lines.join("\n")
    }

    #[test]
    fn a_room_is_made_by_naming_it() {
        let mut rooms = rooms();
        assert!(out("new arena", &mut rooms).contains("arena"));
        assert!(rooms.get("arena").is_some());
        assert_eq!(rooms.get("arena").unwrap().world().kind(), WorldKind::Infinite);

        // A size makes it wrap, which a single --torus on the command line
        // cannot do for one room and not another.
        out("new ring 4x6", &mut rooms);
        assert_eq!(
            rooms.get("ring").unwrap().world().kind(),
            WorldKind::Toroidal { rows: 4, cols: 6 }
        );
    }

    /// Every way of getting it wrong says what was wrong, and changes nothing.
    #[test]
    fn a_bad_command_is_answered_rather_than_obeyed() {
        let mut rooms = rooms();
        let before: Vec<String> = rooms.names().map(str::to_string).collect();

        assert!(out("new", &mut rooms).contains("needs a name"));
        assert!(out("new ../escape", &mut rooms).contains("letters"));
        assert!(out("new arena sideways", &mut rooms).contains("ROWSxCOLS"));
        assert!(out("frobnicate", &mut rooms).contains("no command"));
        assert!(out("new main", &mut rooms).contains("already"));

        assert_eq!(rooms.names().collect::<Vec<_>>(), before, "nothing was made");
    }

    /// Pressing return is not a command, and answering it would fill the
    /// terminal with complaints about nothing.
    #[test]
    fn an_empty_line_says_nothing() {
        let mut rooms = rooms();
        for quiet in ["", "   ", "\t"] {
            let reply = run(quiet, &mut rooms, WorldKind::Infinite);
            assert!(reply.lines.is_empty(), "{quiet:?}");
            assert!(!reply.stop);
        }
    }

    #[test]
    fn stopping_is_the_only_thing_that_stops() {
        let mut rooms = rooms();
        for word in ["stop", "quit", "exit"] {
            assert!(run(word, &mut rooms, WorldKind::Infinite).stop, "{word}");
        }
        for word in ["help", "rooms", "new arena", "", "nonsense"] {
            assert!(!run(word, &mut rooms, WorldKind::Infinite).stop, "{word}");
        }
    }

    #[test]
    fn the_listing_says_which_room_a_client_naming_none_gets() {
        let mut rooms = rooms();
        out("new arena", &mut rooms);
        let listing = out("rooms", &mut rooms);
        assert!(listing.contains("main") && listing.contains("(default)"));
        assert!(listing.contains("arena") && listing.contains("boundless"));
        assert_eq!(listing.matches("(default)").count(), 1, "exactly one is the default");
    }

    /// Every command in `help` is a command, and every command is in `help`.
    /// A menu that lists something the parser does not know is worse than no
    /// menu, and one that omits a command hides it for good.
    #[test]
    fn help_and_the_parser_agree() {
        let mut rooms = rooms();
        let mut listed: Vec<&str> =
            HELP.iter().map(|(form, _)| form.split_whitespace().next().unwrap()).collect();
        // `match` has a vocabulary under it, so it earns several lines and is
        // one command. Deduplicated rather than listed once, because the
        // subcommands are what somebody reading help needs to see.
        listed.dedup();
        assert_eq!(listed, ["new", "rooms", "match", "stop", "help"]);
        for word in &listed {
            let reply = run(word, &mut rooms, WorldKind::Infinite);
            assert!(
                !reply.lines.first().is_some_and(|l| l.contains("no command")),
                "help lists {word}, which the parser does not know"
            );
        }
    }

    /// The whole `match` vocabulary, as somebody would type it.
    #[test]
    fn a_match_is_made_started_and_listed() {
        let mut rooms = rooms();

        assert!(out("match", &mut rooms).contains("no matches"));

        let made = out("match new arena infinite timer 2000", &mut rooms);
        assert!(made.contains("arena"), "{made}");
        assert!(made.contains("2000 generations"), "{made}");
        assert!(made.contains("gathering"), "{made}");

        // A gathering match does not step, which is what makes the opening
        // drawn rather than raced.
        let before = rooms.get("arena").unwrap().tick();
        rooms.step();
        assert_eq!(rooms.get("arena").unwrap().tick(), before, "gathering holds still");

        assert!(out("match", &mut rooms).contains("gathering"));
        assert!(out("match dispatch", &mut rooms).contains("running"));
        assert!(out("match dispatch", &mut rooms).contains("no match is waiting"));

        rooms.step();
        assert_eq!(rooms.get("arena").unwrap().tick(), before + 1, "running steps");
        assert!(out("match", &mut rooms).contains("running"));
    }

    /// A torus needs its size and an infinite world will not take one: how big
    /// a wrapping world is, is most of what makes one match different from
    /// another, so guessing it would hide the important number.
    #[test]
    fn a_match_needs_a_shape_it_can_actually_build() {
        let mut rooms = rooms();
        assert!(out("match new a toroidal timer 10", &mut rooms).contains("needs a size"));
        assert!(out("match new a infinite 18x18 timer 10", &mut rooms).contains("no size to give"));
        assert!(out("match new a spherical timer 10", &mut rooms).contains("no world shape"));
        assert!(out("match new a infinite vibes 10", &mut rooms).contains("no win condition"));
        assert!(out("match new a infinite timer 0", &mut rooms).contains("over already"));

        let made = out("match new a toroidal 18x18 territory 500", &mut rooms);
        assert!(made.contains("first to 500 squares"), "{made}");
        assert!(made.contains("wrapping"), "{made}");
    }

    /// A match is a room, so it is named like one and cannot take a name a
    /// room already has — "make" that sometimes means "and empty it" is one
    /// keystroke from destroying a world somebody is standing in.
    #[test]
    fn a_match_is_named_and_will_not_take_a_name_in_use() {
        let mut rooms = rooms();
        assert!(out("match new infinite timer 10", &mut rooms).contains("match new NAME"));

        assert!(out("match new arena infinite timer 10", &mut rooms).contains("arena"));
        let clash = out("match new arena infinite timer 10", &mut rooms);
        assert!(clash.contains("already a room called"), "{clash}");
        let over_a_room = out("match new main infinite timer 10", &mut rooms);
        assert!(over_a_room.contains("already a room called"), "{over_a_room}");

        // And the name is the one you join by and the one `start` takes.
        assert!(rooms.get("arena").is_some());
        assert!(out("match start arena", &mut rooms).contains("running"));
    }

    /// Two waiting matches make `dispatch` ambiguous, and starting the wrong
    /// one is not something that can be taken back.
    #[test]
    fn dispatch_refuses_to_guess_between_two() {
        let mut rooms = rooms();
        out("match new dawn infinite timer 10", &mut rooms);
        out("match new dusk infinite timer 20", &mut rooms);
        let answer = out("match dispatch", &mut rooms);
        assert!(answer.contains("2 matches are waiting"), "{answer}");
        assert!(answer.contains("dawn") && answer.contains("dusk"), "{answer}");
        assert!(out("match start dusk", &mut rooms).contains("running"));
        assert!(out("match dispatch", &mut rooms).contains("running"), "one left, so no guess");
    }
}
