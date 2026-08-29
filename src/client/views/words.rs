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
    pub const ASKING: &str = "asking the server…";
    /// A server that answered, said once and quietly. The room list below it
    /// is the real answer; this is only here so that the moment of connecting
    /// is not silent.
    pub const REACHED: &str = "connected";
    pub const RETRY: &str = "try again";
    /// Reaching a server and asking it again are the same act from where the
    /// player stands, so they are one control whose meaning follows the state
    /// — which the hover text says.
    ///
    /// **Drawn rather than written**: it was `\u{21bb}` and rendered as a box,
    /// because no font is loaded anywhere in this client. See
    /// [`crate::client::views::icons::refresh`], and the same for the back
    /// arrow. A control that is one symbol has nothing left when the symbol is
    /// missing.
    pub const REFRESH_ASK: &str = "See what is on that server";
    pub const REFRESH_AGAIN: &str = "Ask that server again";
    /// The column of what is already here. "Worlds" rather than "Rooms",
    /// which is the machinery's word — a player joins a world.
    pub const ROOMS: &str = "Worlds here";
    /// An empty list is an invitation, not a failure: there is a form in the
    /// next column and this is the moment to point at it.
    pub const NO_ROOMS: &str = "None yet. Make the first one.";
    /// Waiting is a different thing from a server with nothing on it, and
    /// reads differently: one is a pause, the other is an invitation.
    pub const NOT_ASKED: &str = "No answer from that server yet.";
    pub const ALONE: &str = "Play alone";
    /// Out of a screen, by pointer. Escape does the same, and both exist
    /// because a phone has no escape key and a keyboard user should not have
    /// to reach for the mouse.
    pub const BACK: &str = "‹ back";
    /// What the same button says when you are already enrolled in a match.
    /// Starting a solitary game is never what pressing the only other button
    /// meant, so the press means the opposite instead.
    pub const BACK_TO_MATCH: &str = "Back to your match";
    pub const BACK_TO_MATCH_NOTE: &str = "It has not started. Nothing moves until it does.";
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

        /// A rating, said as a rating rather than as a bare number: five
        /// figures on a screen of other figures is one nobody can place.
        pub fn rating(rating: i32) -> String {
            format!("Rated {rating}")
        }

        /// The sign is the whole message, so it is always there -- `+0` never
        /// appears, because a result that moved nothing is not shown at all.
        pub fn rating_change(change: i32) -> String {
            format!("{change:+} from your last match")
        }
        pub const SETTINGS: &str = "Settings";
        pub const SETTINGS_HIDE: &str = "Close settings";

        pub mod settings {
            pub const KEY: &str = "Your player key";

            /// Native only. A path is a thing somebody can act on -- copy it,
            /// back it up, put it in a password manager -- and is a better
            /// answer to "where is my key" than a box of text.
            pub fn key_lives_at(path: &str) -> String {
                format!("Kept at {path}")
            }

            pub fn reveal(showing: bool) -> &'static str {
                if showing {
                    "Hide the key itself"
                } else {
                    "Show the key itself"
                }
            }
            /// Said plainly, because it is not the bargain people expect from
            /// something called a key. There is no account behind it, no
            /// address to send a reset to, and it is the same you on every
            /// server rather than one of them.
            pub const KEY_NOTE: &str =
                "An OpenSSH private key -- ssh-keygen reads it. Save it somewhere \
                 to play as yourself in another browser, or paste one in to \
                 become somebody else. Whoever has it is you, on every server, \
                 and nobody can give it back.";
            pub const KEY_TAKE: &str = "Use this key";
            pub const KEY_NONE: &str =
                "No key yet. One is made for you the first time you reach a server.";

            pub const FORGET: &str = "Forget everything";
            pub const FORGET_NOTE: &str =
                "Your key, your name, your record and every world you have played in.";

            pub const CONFIRM: &str = "Yes, do it";
            pub const CANCEL: &str = "No, leave it";
            pub const FORGET_ASK: &str = "Forget everything?";
            pub const FORGET_ASK_NOTE: &str =
                "Your key goes with it, and nobody -- including this server -- \
                 has a copy to give back. Everything you have ever held becomes \
                 somebody else's ground.";
            pub const KEY_ASK: &str = "Become somebody else?";
            pub const KEY_ASK_NOTE: &str =
                "The key you have now is replaced. Unless you have written it \
                 down somewhere, you cannot go back to being who you are.";
        }
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
        pub const JOIN: &str = "Join";
        /// Blowing the whistle, in the lobby, for whoever made the match.
        pub const START: &str = "Start the match";
        pub const START_NOTE: &str = "Everybody spawns together when you do.";
        pub const NOT_YOURS: &str = "Waiting for whoever made this match to start it.";
        pub const AT_CONSOLE: &str = "Waiting for the server to start it.";

        pub fn started_by(who: &str) -> String {
            format!("started by {who}")
        }
        /// Said on the HUD for the whole visit, because a spectator whose
        /// clicks do nothing needs to know why the first time rather than the
        /// fifth.
        pub const WATCHING: &str = "watching";
        pub const NO_SEAT: &str = "You are watching this world, not playing in it.";
    }

    /// Making a room. One label per decision, and a label appears only when
    /// the decision it belongs to is live — see [inspiration.md].
    ///
    /// [inspiration.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/inspiration.md#the-menu
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
        pub const TOGETHER: &str = "Played";
        pub const SOLO: &str = "Every player for themselves";
        pub const TEAMS: &str = "In teams";
        pub const SIDES: &str = "Teams";
        /// Teams are picked in the lobby, not here — said out loud, because a
        /// form that asks how many and never asks who reads as unfinished.
        pub const SIDES_NOTE: &str = "Who is on which team is settled in the lobby.";
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

        pub fn sides_range(least: u8, most: u8) -> String {
            format!("{SIDES}: between {least} and {most}")
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
        /// A world is made **on** a server, so there has to be one. Said at
        /// the point of pressing rather than by the form being absent.
        pub const NO_SERVER: &str = "Reach a server first — a world is made on one.";
        pub const CLEAR: &str = "Start again";
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
    /// The shape axis. Verbs, because they are how the cells get chosen
    /// rather than what ends up in them.
    pub const DRAW: &str = "Draw";
    pub const PANE: &str = "Pane";
    /// The key that puts the shape back to whatever the held material is
    /// usually wanted in, shown on the square rather than in a help screen —
    /// it is the one key on the bar that does something rather than selecting
    /// something.
    ///
    /// Written as the character it produces rather than as the chord that
    /// produces it: shift and backtick **is** tilde on every layout this is
    /// bound for, and `S\`` was two symbols asking to be decoded.
    pub const FLIP_KEY: &str = "~";

    pub const CAPTURE: &str = "Grab";
    /// The square that opens the library.
    pub const LIBRARY: &str = "Stamps";
}

/// The library of captured patterns.
pub mod stamps {
    pub const TITLE: &str = "Stamps";
    /// Turning is a thing you do to a pattern, so with none held the key
    /// changes nothing on the screen — which looks like a key that does not
    /// work rather than one that had nothing to act on.
    pub const NOTHING_TO_TURN: &str = "Hold a stamp to turn one";
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
    /// **"Team", not "side".** They were the same word doing one job, and the
    /// game says team everywhere a player reads it.
    pub const TAKE_SIDE: &str = "Join this team";
    pub const LEAVE_SIDE: &str = "Leave this team";
    pub const CODE: &str = "Code to share";
    pub const RENAME: &str = "rename";
    pub const KEEP_NAME: &str = "ok";
    pub const NOBODY_ON_IT: &str = "nobody yet";

    /// Said rather than left to be noticed: the match will not start while
    /// somebody is unplaced, and a lobby that does not say who is the wrong
    /// place to find that out.
    pub fn not_picked(who: &str) -> String {
        format!("{who} has not picked a team")
    }

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
    /// **Drawn rather than written.** This was the arrow itself and came out
    /// as a box: no font is loaded anywhere in this client, so a glyph outside
    /// what egui bundles is tofu — and the one control whose whole job is to
    /// be recognised at a glance was a square. See
    /// [`crate::client::views::icons::back`]. Kept as a constant because the
    /// help screen still spells it in a line of text, where it is surrounded
    /// by words and reads.
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

/// Every key, on one screen, behind `?`.
pub mod help {
    pub const TITLE: &str = "Keys";
    pub const CLOSE: &str = "close";
    pub const DISMISS: &str = "Escape or ? closes this.";

    pub const LOOKING: &str = "Looking about";
    pub const BUILDING: &str = "Building";
    pub const GETTING_ABOUT: &str = "Getting about";

    pub const PAN: &str = "Move the view";
    pub const PAN_FASTER: &str = "Move it faster";
    pub const PAN_BY_HAND: &str = "Drag the world";
    pub const ZOOM: &str = "Zoom in and out";
    pub const TOOLS: &str = "Pick a tool: life, mine, turret, ice";
    /// The shape axis has one key and it goes to the default; the other shape
    /// is a click away on the bar. See `hotbar::Held::defaulted`.
    pub const SHAPE: &str = "back to the usual shape";
    /// **So a glider is one stamp and not four.** Turning is held rather than
    /// saved, so it changes nothing in the library.
    pub const TURN: &str = "turn what you are holding";
    pub const MIRROR: &str = "mirror it, which no rotation can do";
    pub const STAMPS: &str = "Pick a stamp you have kept";
    pub const DRAG: &str = "Lay a run of cells, or a rectangle";

    pub const WALK: &str = "Walk a list";
    pub const CHOOSE: &str = "Take what is picked";
    pub const MOVE_ON: &str = "Move between controls";
    /// One key, one meaning: back out of the innermost thing. It was listed
    /// twice — once for abandoning a drawing and once for leaving a screen —
    /// which is two answers to one question. It is a ladder, and saying so is
    /// shorter than saying it twice.
    pub const BACK: &str = "Back out: what you are drawing, then the screen";
    pub const HELP: &str = "This";

    /// The keycaps themselves. Spelled the way a keyboard is read rather than
    /// the way winit names them — nobody has a key called `ArrowLeft`.
    pub mod keys {
        pub const PAN_KEYS: &str = "WASD / arrows";
        pub const PAN_FAST: &str = "shift";
        pub const PAN_DRAG: &str = "space or middle drag";
        pub const ZOOM: &str = "wheel / pinch";
        pub const TOOLS: &str = "shift + 1-4";
        pub const STAMPS: &str = "1-9";
        pub const SHAPE: &str = "~";
        pub const TURN: &str = "R / shift + R";
        pub const MIRROR: &str = "F";
        pub const DRAG: &str = "drag";
        pub const WALK: &str = "up / down";
        pub const CHOOSE: &str = "enter";
        pub const MOVE_ON: &str = "tab";
        pub const BACK: &str = "escape";
        pub const HELP: &str = "?";
    }
}

/// The record on the home screen.
pub mod record {
    pub const NOTHING_YET: &str = "Nothing played yet. That is what Play is for.";
    pub const LARGEST: &str = "Largest territory, by game";
    pub const FORM: &str = "Recent";
    pub const WORLDS: &str = "worlds";
    pub const MATCHES_WON: &str = "matches won";
    pub const LARGEST_EVER: &str = "largest ever";
    pub const GENERATIONS: &str = "generations";
    pub const WON: &str = "won";
    pub const LOST: &str = "lost";
    pub const NO_RESULT: &str = "no result";

    /// One game, as a tooltip reads it. Named parts rather than a template,
    /// because the order of them is a decision: what it was, then how big you
    /// got, then how long it took.
    pub fn a_game(room: &str, squares: u32, generations: u64, outcome: &str) -> String {
        format!("{room} · {squares} squares · {generations} generations · {outcome}")
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
