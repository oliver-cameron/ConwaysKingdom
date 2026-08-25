//! Every word the client puts on screen.
//!
//! One file, for the same reason `sim::rule` holds every number: a string a
//! player reads is a decision, and decisions are easier to get right when they
//! are next to each other than when they are scattered through the code that
//! happens to draw them. Changing what the game *says* should not mean reading
//! what it *does*.
//!
//! It is also where a translation would start, and where anybody can see the
//! whole voice of the thing at once — which is the only way to notice that one
//! screen says "server" and another says "host".
//!
//! Not log lines. Those are for whoever is running it and are written where
//! the thing they describe happens.

/// The screen before the game.
pub mod menu {
    pub const TITLE: &str = "Conway's Kingdom";
    pub const NAME: &str = "Name";
    pub const NAME_HINT: &str = "player";
    pub const SERVER: &str = "Server";
    pub const SERVER_HINT: &str = "ws://host:8080/ws";
    pub const LOOK: &str = "See what rooms are there";
    pub const ASKING: &str = "asking the server…";
    pub const REFRESH: &str = "refresh";
    pub const ROOMS: &str = "Rooms";
    pub const NO_ROOMS: &str = "this server has no rooms";
    pub const ALONE: &str = "Play alone";
    /// Out of a screen, by pointer. Escape does the same, and both exist
    /// because a phone has no escape key and a keyboard user should not have
    /// to reach for the mouse.
    pub const BACK: &str = "‹ back";
    pub const ALONE_NOTE: &str = "The rules are the same offline. Nobody else is.";
    pub const EMPTY_ROOM: &str = "empty";

    pub fn one_player() -> String {
        "1 player".into()
    }

    pub fn players(n: u32) -> String {
        format!("{n} players")
    }

    pub fn no_answer(address: &str) -> String {
        format!("no server answered at {address}")
    }

    pub fn not_an_address(address: &str) -> String {
        format!("{address} is not an address")
    }

    pub fn no_reply(address: &str) -> String {
        format!("{address} did not answer")
    }

    pub const LOST_CONNECTION: &str = "the connection went away";

    /// The home screen: who you are, what you have done, and the way in.
    pub mod home {
        pub const PLAY: &str = "Play";
        pub const WHO: &str = "You are";
        pub const RECORD: &str = "So far";
        /// A first visit has nothing to show, and five zeroes would say only
        /// that the game keeps score.
        pub const NOTHING_YET: &str = "Nothing played yet. That is what Play is for.";

        pub fn games(n: usize) -> String {
            if n == 1 {
                "1 world".into()
            } else {
                format!("{n} worlds")
            }
        }

        pub fn matches(won: usize, played: usize) -> String {
            format!("{won} of {played} matches won")
        }

        pub fn best(squares: u32) -> String {
            format!("{squares} squares at your largest")
        }

        pub fn generations(n: u64) -> String {
            format!("{n} generations lived through")
        }
    }

    /// Reaching a room that is not in the listing.
    pub mod code {
        pub const LABEL: &str = "Have a code?";
        pub const HINT: &str = "abc234";
        pub const GO: &str = "Go";
        /// What the server hands back after making a private room. The thing
        /// you send somebody, so it is worth saying that out loud.
        pub const MADE: &str = "Your code — send it to whoever is playing:";
    }

    /// Watching without a seat.
    pub mod watch {
        pub const WATCH: &str = "Watch";
        /// Said on the HUD for the whole visit, because a spectator whose
        /// clicks do nothing needs to know why the first time rather than the
        /// fifth.
        pub const WATCHING: &str = "watching";
        pub const NO_SEAT: &str = "You are watching this world, not playing in it.";
    }

    /// Making a room. One label per decision, and a label appears only when
    /// the decision it belongs to is live — see [planned.md].
    ///
    /// [planned.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#the-screen-and-where-it-is-borrowed-from
    pub mod make {
        /// Opens the form. Says "world" rather than "room" because that is
        /// what you get and what the game calls it everywhere else; "room" is
        /// the machinery's word.
        pub const OPEN: &str = "New world";
        pub const TITLE: &str = "A new world";
        pub const NAME: &str = "Name";
        pub const NAME_HINT: &str = "arena";
        pub const SHAPE: &str = "Shape";
        pub const BOUNDLESS: &str = "Boundless";
        pub const WRAPPING: &str = "Wrapping";
        pub const SIZE: &str = "Size";
        /// Two fields, because a size is two numbers. Naming them separately
        /// is also what lets an error say which one is wrong.
        pub const ROWS: &str = "Rows";
        pub const COLS: &str = "Columns";
        /// Chunks, not cells. Said out loud because the number is small and
        /// would otherwise read as a tiny world.
        pub const SIZE_NOTE: &str = "in chunks, each 16 cells square";
        pub const PRIVATE: &str = "Who can find it";
        pub const LISTED: &str = "Anyone";
        pub const UNLISTED: &str = "By code";
        pub const LISTED_NOTE: &str = "In the room list, for whoever is on this server.";
        /// The name field is ignored for a private room, and a field being
        /// quietly discarded is worse than one that is not there.
        pub const UNLISTED_NOTE: &str =
            "Not listed. The server gives it a code to share, instead of a name.";

        pub fn not_a_number_for(which: &str, text: &str) -> String {
            format!("{which}: \"{text}\" is not a number")
        }

        pub fn out_of_range(which: &str, most: i32) -> String {
            format!("{which}: between 1 and {most}")
        }
        pub const ENDS: &str = "Ends";
        pub const NEVER: &str = "Never";
        pub const TIMER: &str = "Timer";
        pub const TERRITORY: &str = "Territory";
        /// A world is the ordinary case and a match is the one with a
        /// condition on it, so "never" is a legal answer rather than a
        /// separate question about which of the two this is.
        pub const NEVER_NOTE: &str = "A world with no end. Anybody may join at any time.";
        pub const TIMER_NOTE: &str = "Most ground when the generations run out.";
        pub const TERRITORY_NOTE: &str = "First to hold this many squares wins.";
        pub const GENERATIONS: &str = "Generations";
        pub const SQUARES: &str = "Squares";
        pub const MAKE: &str = "Make it";
        pub const CANCEL: &str = "Cancel";
        pub const MAKING: &str = "making it…";
        /// A match does not start on its own, so somebody about to make one
        /// should know that before they make it rather than after.
        pub const MATCH_WAITS: &str = "A match gathers until the server starts it.";

        pub fn not_a_size(text: &str) -> String {
            format!("\"{text}\" is not a size; try 12x12")
        }

        pub fn not_a_number(text: &str) -> String {
            format!("\"{text}\" is not a number")
        }
    }

    /// The count under the room list, so the list says how much is behind it
    /// before anybody reads the names.
    pub fn rooms_here(rooms: usize, players: u32) -> String {
        let w = if rooms == 1 { "world" } else { "worlds" };
        let p = if players == 1 { "player" } else { "players" };
        format!("{rooms} {w}, {players} {p} online")
    }
}

/// The bar along the bottom.
pub mod hotbar {
    pub const LIFE: &str = "Life";
    pub const MINE: &str = "Mine";
    pub const TURRET: &str = "Turret";
    pub const ICE: &str = "Ice";
    /// The square that takes a stamp. Short, because it sits in a 44px box.
    pub const CAPTURE: &str = "Grab";
    /// The square that opens the library.
    pub const LIBRARY: &str = "Stamps";
}

/// The library of captured patterns.
pub mod stamps {
    pub const TITLE: &str = "Stamps";
    pub const CLOSE: &str = "Close";
    pub const FORGET: &str = "forget";
    pub const NONE_YET: &str = "Nothing kept yet.";
    pub const HOW: &str = "Grab and drag a box round your own life to take one, or draw one below.";
    pub const DRAW: &str = "Draw one";
    pub const KEEP: &str = "keep";
    pub const CLEAR: &str = "clear";
    pub const DRAW_HOW: &str = "Click to lay a cell or lift it, drag to lay a run.";

    pub fn captured(name: &str, cells: usize) -> String {
        format!("captured {name} ({cells} cells)")
    }

    pub fn placed(name: &str, cells: usize, delta: i32) -> String {
        format!("stamped {name} ({cells} cells), {delta:+}")
    }

    pub const NOTHING_TO_CAPTURE: &str = "nothing of yours alive in there to capture";
    pub const GONE: &str = "that stamp is gone";
}

/// How much of a match is left.
pub mod clock {
    pub fn generations_left(generations: u64, seconds: u64) -> String {
        if generations == 0 {
            return "time".into();
        }
        format!("{generations} left  ·  {}", clocked(seconds))
    }

    /// Minutes and seconds, because "224 seconds" is a number somebody has to
    /// do arithmetic on to know whether to hurry.
    fn clocked(seconds: u64) -> String {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

    pub fn squares_left(target: u64, most: u64) -> String {
        format!("{most} of {target} squares")
    }
}

/// The screen before a match starts, and the word for what one is doing.
pub fn phase(phase: &crate::net::MatchPhase) -> &'static str {
    use crate::net::MatchPhase::*;
    match phase {
        Open => "room",
        Gathering => "waiting to start",
        Running { .. } => "under way",
        Over { .. } => "finished",
    }
}

/// The screen before a match starts.
pub mod lobby {
    pub const WAITING: &str = "Waiting to start";
    pub const FINISHED: &str = "Match over";
    pub const NOBODY: &str = "Nobody held any ground.";
    pub const YOU: &str = "you";
    pub const YOU_WON: &str = "You won";
    pub const HOW: &str = "Nothing moves until whoever made the match starts it.";

    pub fn who(n: usize) -> String {
        match n {
            0 => "Nobody here yet".into(),
            1 => "1 player here".into(),
            n => format!("{n} players here"),
        }
    }

    pub fn held(n: usize) -> String {
        format!("{n} squares")
    }

    pub fn timer(generations: u64) -> String {
        format!("most ground after {generations} generations")
    }

    pub fn territory(squares: usize) -> String {
        format!("first to {squares} squares")
    }
}

/// The panel in the corner.
pub mod hud {
    pub const CONNECTED: &str = "connected";
    pub const OFFLINE: &str = "offline";
    pub const HOLDING: &str = "ground held";
    /// The arrow out. A glyph rather than the word, because it sits beside a
    /// player's name in a row that is already full.
    pub const BACK: &str = "\u{2190}";
    pub const BACK_HINT: &str = "back to the menu";
    pub const BOUNDLESS: &str = "boundless world";
    pub const OVER_PANEL: &str = "over panel";
    pub const ON_WORLD: &str = "on world";
    pub const NOTHING_YET: &str = "nothing yet";

    /// The hint lines, in the order they are shown.
    ///
    /// A list rather than a run of `ui.small` calls, so what the game claims
    /// you can do is one thing to read and one thing to keep true.
    pub const HINTS: &[&str] = &[
        "left click acts, left drag draws",
        "right, middle or space+drag to pan",
        "arrows or WASD to pan, shift to hurry",
        "wheel or pinch to zoom",
        "1–9 and 0 choose a stamp, shift+1–9 a tool",
        "one finger draws, two move the view",
        "escape abandons the drag in progress",
    ];
}

/// The disagreement counter, which is quiet almost all of the time.
pub mod desync {
    /// The connection has slipped before and is settled now. Worth saying,
    /// because a rate back at nought and a link that has never slipped look
    /// identical and are not the same thing.
    pub const SETTLED: &str = "in step";
    /// Ticking over. Prediction costs this and always has.
    pub const BACKGROUND: &str = "in step, correcting";
    pub const NOTICEABLE: &str = "correcting often";
    pub const ALARMING: &str = "struggling to stay in step";

    /// The reading itself, for the developer panel: the rate, and how much
    /// has ever been put right.
    pub fn reading(rate: f64, total: u64) -> String {
        format!("desync {rate:.1}/12s, {total} chunks corrected")
    }
}

/// What the world says back when it refuses something.
pub mod refused {
    /// A match that has not started, or one that is decided. Said rather than
    /// silently ignored: a click that does nothing looks exactly like a click
    /// that never arrived.
    pub fn not_your_territory(row: i32, col: i32) -> String {
        format!("nothing of yours reaches ({row}, {col})")
    }

    pub fn cells_not_yours(n: usize) -> String {
        format!("{n} of those cells are out of your reach")
    }

    pub fn not_started() -> &'static str {
        "nothing can be placed until the match starts"
    }

    pub fn cannot_afford(cells: usize, costs: i32, have: i32) -> String {
        format!("{cells} cells costs {costs}, you have {have}")
    }
}
