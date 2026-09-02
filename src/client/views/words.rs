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
    pub const ALONE: &str = "Play Solo";
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
    /// Who else plays here, and the leaderboard, which is the same list with
    /// nothing typed into the box.
    pub mod people {
        pub const TITLE: &str = "Players";
        pub const NOTE: &str = "The best rated here, or type a name to find somebody.";
        pub const HINT: &str = "a name";
        pub const ASKING: &str = "Asking the server…";
        pub const NOBODY: &str = "Nobody here by that name.";
        /// Not the same sentence as [`NOBODY`]. An empty board means the
        /// server has met nobody it is sure about yet, which is a fact about
        /// the server rather than about what was typed.
        pub const NOBODY_YET: &str = "Nobody has played enough matches here to be rated yet.";

        /// A provisional rating is marked rather than hidden: the number is
        /// real and the server is not yet sure of it.
        pub fn rating(rating: i32, provisional: bool) -> String {
            if provisional {
                format!("{rating}?")
            } else {
                rating.to_string()
            }
        }

        pub fn capped(most: usize) -> String {
            format!("The first {most}. Type a name to narrow it.")
        }
    }

    /// You, as a page: the name, the rating, the record and the key.
    pub mod account {
        pub const TITLE: &str = "Your account";
        pub const RATED: &str = "Rated here";
    }

    /// What somebody who has just arrived cannot work out by clicking. Every
    /// entry is a rule people lose to before they learn it, in the order they
    /// bite, and the argument for each is in docs/game.md.
    pub mod howto {
        pub const TITLE: &str = "How to play";
        pub const NOTE: &str =
            "Conway's Game of Life, with owners. Five things the board does not tell you.";

        pub const RULES: &[(&str, &str)] = &[
            (
                "You can only build where your influence already reaches.",
                "This is the first thing anybody runs into, and a refused click does not                  explain it. Territory is a field with sources: the patch you are granted                  is a spring that never runs dry, and your live cells feed it too. So you                  grow ground by growing life outward from what you have — not by clicking                  further away.",
            ),
            (
                "A mine pays when it turns over, not when you own it.",
                "The opposite of what owning a lot of mines suggests. A block of mines is                  a still life: it never gives birth, so it never pays anything at all. An                  oscillator earns every period and a gun earns forever. This one rule                  decides whether your economy works.",
            ),
            (
                "A turret is the other way round, so place it in fours.",
                "It works by standing still, and one on its own dies of loneliness in a                  generation. The block that is a mine's worst shape is a turret's best —                  four cells is the cheapest thing in Conway that never dies and never                  gives birth.",
            ),
            (
                "Ice cannot be taken back.",
                "It stops time over whatever it covers, and only life reaching it breaks                  it. A pane put down in the wrong place is a decision you live with, so                  it is worth thinking about before you spend on one.",
            ),
            (
                "A payload takes ground; it does not just make a mess.",
                "It burns down on a fuse you can watch — the last warning sprite is on                  screen for exactly one generation — and then scrambles a disc. What comes                  up alive is yours and what does not belongs to nobody, so a bomb breaks a                  country apart and leaves you some of the pieces. Ice stops the fuse, and                  a payload has to stay alive to go off at all.",
            ),
        ];

        pub const TIP_TITLE: &str = "Other people's patterns work here.";
        pub const TIP: &str =
            "Life is exactly B3/S23 — an R-pentomino settles at generation 1103 with 116              cells, which is the figure in every book. So a glider is a glider, a gun is a              gun, and fifty years of published patterns are things you can build here and              expect to behave.";
    }

    pub mod home {
        pub const PLAY: &str = "Play";
        pub const WHO: &str = "You are";
        pub const RECORD: &str = "So far";

        /// The sign is the whole message, so it is always there -- `+0` never
        /// appears, because a result that moved nothing is not shown at all.
        pub fn rating_change(change: i32) -> String {
            format!("{change:+} from your last match")
        }
        pub const PROFILE: &str = "Your profile";
        pub const PEOPLE: &str = "Who else plays here";
        pub const ACCOUNT: &str = "Your account";
        pub const HOWTO: &str = "How to play";
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
            /// A key is made at startup, so this is the store refusing it or
            /// there being no entropy to make one from — and the consequence
            /// is worth stating rather than the absence.
            /// A server issues the name it calls you, so there is nothing to
            /// show until one has.
            pub const KEY_UNSEEN: &str = "No server has met this client yet, so none has named it.";
            pub const KEY_NONE: &str =
                "No key could be made or kept here, so this client is somebody new \
                 everywhere it goes.";

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
    pub mod alone {
        pub const TITLE: &str = "Play alone";
        /// Said once, at the top, rather than beside every field: the whole
        /// page is about a world nobody else is in.
        pub const NOTE: &str =
            "No server, no other players. The simulation is the same one a match runs.";
    }

    pub mod make {
        /// The action, when there is no server to ask. The same form, so the
        /// same questions; a different place for the answer to go.
        pub const ALONE: &str = "Play alone";

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
        /// Between the two numbers, so a size reads as `12x12` — which is how
        /// a size is written and what `--torus` takes. Two boxes labelled
        /// "Rows" and "Columns" said, over two lines, what one character says.
        pub const BY: &str = "x";
        /// On hover rather than on a line of its own: worth knowing once, and
        /// worth no space after that.
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
        /// **The first question, because it decides the rest.** It used to
        /// be implied by "ends: never", which told a world and a laboratory
        /// apart not at all — the laboratory was not a room.
        pub const KIND: &str = "Kind";
        pub const WORLD: &str = "World";
        pub const MATCH: &str = "Match";
        pub const EXPERIMENT: &str = "Experiment";
        pub const WORLD_NOTE: &str = "Runs forever. Anybody may join at any time, and nobody wins.";
        pub const MATCH_NOTE: &str = "Gathers, then runs until somebody has won it.";
        /// Says what it is *for*. The switches themselves are in the room, on
        /// the bar, because they are things this world does rather than things
        /// it was made with.
        pub const EXPERIMENT_NOTE: &str =
            "A laboratory. The same simulation, with the clock and the game's \
             placing rules yours to switch.";

        pub const ENDS: &str = "Ends";
        pub const TIMER: &str = "Timer";
        pub const TERRITORY: &str = "Territory";
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
    /// The four figures on the bar. One word each, lower case: they label a
    /// number rather than heading a section, and a capital would make each of
    /// them look like the start of something.
    pub const PURSE: &str = "purse";
    /// What you hold, which is territory. "held" was the field's own name and
    /// said nothing about what was being held.
    pub const GROUND: &str = "ground";
    pub const TICK: &str = "tick";
    pub const RATING: &str = "elo";

    pub const LIFE: &str = "Life";
    pub const MINE: &str = "Mine";
    pub const TURRET: &str = "Turret";
    pub const ICE: &str = "Ice";
    pub const PAYLOAD: &str = "Payload";
    /// The square that takes a stamp. Short, because it sits in a 44px box.
    /// The shape axis. Verbs, because they are how the cells get chosen
    /// rather than what ends up in them.
    pub const DRAW: &str = "Draw";
    pub const PANE: &str = "Pane";
    /// What the shape square says while a stamp is held: the axis is the same
    /// one, so it shows what is on it rather than going blank.
    pub const PATTERN: &str = "Stamp";
    /// The key that puts the shape back to whatever the held material is
    /// usually wanted in, shown on the square rather than in a help screen —
    /// it is the one key on the bar that does something rather than selecting
    /// something.
    ///
    /// The clock section. Words rather than glyphs, because the sheet has no
    /// art for any of them and a triangle painted by hand is a decision about
    /// a whole icon set — see `icons::back`, which is the precedent and the
    /// argument for eventually doing it.
    pub const RUN_HINT: &str = "Let the world run";
    pub const STOP_HINT: &str = "Hold the world still";
    pub const STEP_HINT: &str = "One generation, and stay stopped";
    /// The panel's own heading. The square that opens it is an icon.
    pub const RULES: &str = "Rules";
    /// Said as what it does to the world rather than as "reset", which reads
    /// like putting something back the way it was.
    pub const WIPE_HINT: &str = "empty this laboratory";
    pub const RULES_HINT: &str = "What the game's rules are doing here";
    pub const ANYWHERE: &str = "Place outside your territory";
    pub const ANYWHERE_NOTE: &str =
        "Off, you may only build where your own influence reaches, as in a game.";
    pub const FREE: &str = "Place without paying";
    pub const FREE_NOTE: &str = "Off, everything costs what it costs and a purse can run out.";
    /// The keys these squares teach, which live with the rest of the key
    /// labels — `help::keys` is where a key's name is decided, and a second
    /// spelling here would be a square and a key list disagreeing.
    pub use super::help::keys::{PLAY as RUN_KEY, STEP_ONE as STEP_KEY};

    pub const CAPTURE: &str = "Grab";
    /// The square that opens the library.
    pub const LIBRARY: &str = "Stamps";
    /// The character, not the chord: it is bound by what it types, so the
    /// label is right on every layout.
    pub const HELP: &str = "?";
    pub const HELP_HINT: &str = "Every key, on one screen";
}

/// The library of captured patterns.
pub mod stamps {
    pub const TITLE: &str = "Stamps";
    /// Turning is a thing you do to a pattern, so with none held the key
    /// changes nothing on the screen — which looks like a key that does not
    /// work rather than one that had nothing to act on.
    pub const NOTHING_TO_TURN: &str = "Hold a stamp to turn one";
    pub const FORGET: &str = "forget";
    pub const NONE_YET: &str = "Nothing kept yet.";
    pub const HOW: &str = "Grab and drag a box round your own life to take one, or draw one below.";
    pub const DRAW: &str = "Draw one";
    pub const KEEP: &str = "keep";
    pub const CLEAR: &str = "clear";
    /// The library survives a session, so a stamp is worth naming.
    pub const KEEP_NAME: &str = "ok";
    pub const RENAME_HINT: &str = "Click to rename";
    pub const EDIT: &str = "edit";
    pub const EDIT_HINT: &str = "Open it on the pad. Keeping puts it back where it was.";
    pub const ON_BAR: &str = "bar";
    pub const ON_BAR_HINT: &str = "Show it on the hotbar. Pin none and the bar is the newest ten.";
    pub const BAR_FULL: &str = "the bar holds ten";
    /// Editing one rather than drawing a new one, so `keep` means replace.
    pub const EDITING: &str = "editing";
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
    /// A stopped world, and where it stopped. The generation is the point — a
    /// paused board with no number on it cannot be stepped *to* anywhere, and
    /// stepping to somewhere is what a laboratory is for.
    pub fn paused_at(generation: u64) -> String {
        format!("paused  ·  generation {generation}")
    }

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

/// **The way out of a panel**, and there is one of them.
///
/// Three modules each declared this, which is three places for one control to
/// come to be called three things. Every panel is drawn by
/// [`super::panel`] now, so there is one button as well as one word.
pub const CLOSE: &str = "Close";

/// A rating, said as a rating rather than as a bare number: five figures on a
/// screen of other figures is one nobody can place.
///
/// Up here rather than under [`menu::home`] because two screens show it — the
/// home screen and a profile — and one wording is what stops them drifting
/// into saying the same number two ways.
pub fn rating(rating: i32) -> String {
    format!("Rated {rating}")
}

/// **A rating that has not been earned yet**, and how far off it is.
///
/// Said with the count rather than as the bare word, because "provisional" on
/// its own is a label somebody has to already know the meaning of, and the
/// number is the whole of what it means.
///
/// Shown on the home screen and on a profile, and **not on the bar**: the mark
/// exists so a rating read as a *claim* is not taken for one it is not, and
/// the bar is your own readout of your own number rather than a comparison.
pub fn provisional(games: u32) -> String {
    format!("provisional · {games} {} so far", if games == 1 { "match" } else { "matches" })
}

/// What a server says about somebody.
pub mod profile {
    pub const TITLE: &str = "Player";
    /// Asked for, and not answered yet. Its own line rather than an empty
    /// panel, because a wait and a blank look the same and only one of them is
    /// worth waiting through.
    pub const ASKING: &str = "asking…";
    /// A real answer, and it says which kind of nothing it is: this server has
    /// never met them, as against not having replied.
    pub const UNKNOWN: &str = "this server has never met them.";
    pub const YOU: &str = "(you)";
    /// **Your own diary**, against the server's count above it. The two
    /// headings are what make the two numbers readable rather than a
    /// contradiction.
    pub const EVERYWHERE: &str = "Everywhere you have played:";
    /// Not "unrated", which reads as a judgement. No server has met you, so
    /// there is nobody to have an opinion.
    pub const UNRATED: &str = "No server has met you yet, so nobody has a number for you.";
    /// A client with no key, which is a browser that cannot keep one.
    pub const NOBODY: &str = "Nobody in particular";

    pub fn played(games: usize, won: usize) -> String {
        let g = if games == 1 { "game" } else { "games" };
        match won {
            0 => format!("{games} {g}"),
            1 => format!("{games} {g}, 1 match won"),
            n => format!("{games} {g}, {n} matches won"),
        }
    }
    /// **On the panel, once.** A server can only speak for what happened on
    /// it, and a screen that did not say so would read as a record of a person
    /// rather than of a visit.
    pub const HERE: &str = "On this server:";

    pub fn matches(n: u32) -> String {
        match n {
            0 => "No matches finished yet".to_string(),
            1 => "1 match finished".to_string(),
            n => format!("{n} matches finished"),
        }
    }

    /// The **most** ever held, not the last: a profile says what somebody has
    /// managed, so a bad match after a good one does not erase the good one.
    pub fn best(squares: usize) -> String {
        match squares {
            0 => "No ground held yet".to_string(),
            1 => "1 square at their largest".to_string(),
            n => format!("{n} squares at their largest"),
        }
    }
}

/// What kind of room this is, for the list somebody picks from.
///
/// A world says only its shape, because "world" is what every row on the list
/// is until it says otherwise. The other two are the exceptions and so are the
/// ones worth a word.
pub fn room_kind(kind: crate::net::RoomKind) -> Option<&'static str> {
    use crate::net::RoomKind::*;
    match kind {
        World => None,
        Match => Some("match"),
        Experiment => Some("laboratory"),
    }
}

/// A laboratory whose clock is stopped, which is the one thing about one that
/// is worth knowing before you go in.
pub const STOPPED: &str = "stopped";

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

    /// What the last match did to it, kept beside the number for as long as
    /// the number is on screen. A rating is a comparison, and a comparison
    /// with nothing to compare against is a score.
    pub fn rating_change(change: i32) -> String {
        format!("{change:+}")
    }

    /// Giving up, which is not the same as leaving: the back arrow beside this
    /// walks out of the room and gives up the seat, and somebody losing a
    /// match should be able to concede it rather than vanish from it.
    pub const FORFEIT: &str = "Give up";
    pub const FORFEIT_HINT: &str =
        "Concede this match. Your team plays on if anyone is left on it.";
    pub const GAVE_UP: &str = "you gave up";
    /// Only for whoever started it, which is the same person and the same
    /// reasoning as the whistle.
    pub const END_MATCH: &str = "End match";
    pub const END_MATCH_HINT: &str = "Call it off now. Whoever leads wins, and it is rated.";
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
/// How a room is won, in a sentence.
///
/// Here rather than in the lobby that shows it, because the creation form
/// shows it too — and a helper one screen borrows from another is the one
/// thing keeping two screens in the same module.
pub fn describe(victory: crate::net::Victory) -> String {
    match victory {
        crate::net::Victory::Timer { generations } => lobby::timer(generations),
        crate::net::Victory::Territory { squares } => lobby::territory(squares),
    }
}

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
    /// **So a glider is one stamp and not four.** Turning is held rather than
    /// saved, so it changes nothing in the library.
    pub const TURN: &str = "turn what you are holding";
    pub const MIRROR: &str = "mirror it, which no rotation can do";
    pub const STAMPS: &str = "Pick a stamp you have kept";
    pub const DRAG: &str = "Lay a run of cells, or a rectangle";
    /// **The clock, and it is yours only when you are alone.** Connected, a
    /// generation happens when the server says one did — see
    /// `networking.md` — so these say what they did rather than doing
    /// nothing, which is the difference between a rule and a broken key.
    pub const THE_CLOCK: &str = "The clock";
    pub const PLAY: &str = "run, or stop running (alone, or in a laboratory)";
    pub const STEP_ONE: &str = "one generation, and stay stopped";
    /// Said when either is pressed in a game — which is every room but a
    /// laboratory, where the clock belongs to whoever is in it.
    pub const SERVER_KEEPS_TIME: &str = "the server keeps time in a game; a laboratory's is yours";
    pub const GO_BACK: &str = "back a screen";
    pub const PAUSED: &str = "paused";
    pub const WIPED: &str = "emptied";
    pub const RUNNING: &str = "running";
    pub fn stepped_to(generation: u64) -> String {
        format!("stepped to generation {generation}")
    }

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
        /// The pan cluster as it prints on *this* keyboard, plus the arrows.
        ///
        /// Four letters rather than the word "WASD", which is a name for a
        /// shape on the board and only spells itself on one layout — on Dvorak
        /// the same four keys print `,aoe`.
        pub fn pan(cluster: &str) -> String {
            format!("{cluster} / arrows")
        }

        /// What to say before anybody has pressed one of them and there is
        /// nothing to report: the arrows do the same job and are the same
        /// everywhere, so they are the honest half of the answer.
        pub const PAN_ARROWS: &str = "arrows";

        /// The tool row, as whatever shift and the first four digits print
        /// here. Bound by position, so the label is the keyboard's answer and
        /// not the one a US layout would have given.
        pub fn with_shift(row: &str) -> String {
            format!("shift + {row}")
        }

        pub const PAN_FAST: &str = "shift";
        /// **Middle drag only.** Space held used to do this too, which is
        /// the convention in a drawing tool and is the weaker claim on the
        /// key: panning also has the walk cluster and the arrows, and a pause
        /// has nowhere else obvious to live. The cost is real and worth
        /// knowing — a trackpad with no middle button has no drag-to-pan.
        pub const PAN_DRAG: &str = "middle drag";
        pub const ZOOM: &str = "wheel / pinch";
        /// **Only reached if a key has no label at all**, which since the
        /// digit row is seeded means never. Kept, and said without a count,
        /// because the shifted row grows when a square is added to the bar and
        /// a number here would go stale the way `shift + 1-4` did — it named
        /// four keys for a row that had run to six.
        pub const TOOLS: &str = "shift + a digit";
        pub const STAMPS: &str = "the digit row";
        /// The unshifted half of the key `~` is on, so it is one press — and
        /// `~` is a dead key on the Spanish, Portuguese and Nordic layouts,
        /// which produces no text at all and left the shape reset unreachable
        /// there. `~` still works; this is what the square says.
        pub const TURN: &str = "R / shift + R";
        pub const MIRROR: &str = "F";
        pub const DRAG: &str = "drag";
        /// Return, and a full stop. Golly's, because somebody who wants a
        /// pause button has almost certainly used Golly.
        pub const PLAY: &str = "space";
        /// **Two spellings, because the key is genuinely different.** On a
        /// Mac, back is `cmd+[` and the browser already does it; everywhere
        /// else `ctrl+[` is bound here because nothing else claims it. Naming
        /// one of them would name the wrong key for most of whoever is
        /// reading, which is the failure a key list exists to prevent.
        pub fn back_key(mac: bool) -> &'static str {
            if mac {
                "\u{2318} ["
            } else {
                "ctrl + ["
            }
        }
        pub const STEP_ONE: &str = ".";
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
