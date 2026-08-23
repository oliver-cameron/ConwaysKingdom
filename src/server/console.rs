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

        other => Reply::say(format!("no command \"{other}\"; try help")),
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
        let listed: Vec<&str> =
            HELP.iter().map(|(form, _)| form.split_whitespace().next().unwrap()).collect();
        assert_eq!(listed, ["new", "rooms", "stop", "help"]);
        for word in &listed {
            let reply = run(word, &mut rooms, WorldKind::Infinite);
            assert!(
                !reply.lines.first().is_some_and(|l| l.contains("no command")),
                "help lists {word}, which the parser does not know"
            );
        }
    }
}
