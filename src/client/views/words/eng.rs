//! English.
//!
//! The words themselves, and the formatters that assemble the ones with a
//! number in them. A language is one `static` of string literals — nothing is
//! allocated and nothing is copied — plus its own implementations of the
//! handful of strings that are built rather than written.
//!
//! See [`super`] for why the formatters are re-exported from here rather than
//! living behind a trait: with one language a trait is fifty-three methods and
//! one implementor.

/// **What the clock's two keys do**, for the help screen's second column.
///
/// These used to be shared with the bar, on the reasoning that a key's name is
/// decided in one place — but a key's *name* is `help::keys` ("space", ".")
/// and these are what it does, and the bar had taken the wrong one of the
/// pair: the corner of a 44px square, where every other square carries one or
/// two characters, was printing "run, or stop running (alone, or in a
/// laboratory)" at 13px. The bar names its keys from `help::keys` now, and
/// draws the space bar because neither bundled face has a glyph for one.
const RUN_KEY: &str = "run, or stop running (alone, or in a laboratory)";
const STEP_KEY: &str = "one generation, and stay stopped";

/// English, as the whole set.
pub static WORDS: super::Words = super::Words {
    close: "Close",
    stopped: "stopped",
    menu: super::Menu {
        title: "Conway's Kingdom",
        name: "Name",
        name_hint: "player",
        server: "Server",
        server_hint: "ws://host:8080/ws",
        asking: "asking the server…",
        reached: "connected",
        retry: "try again",
        refresh_ask: "See what is on that server",
        refresh_again: "Ask that server again",
        rooms: "Worlds here",
        no_rooms: "None yet. Make the first one.",
        not_asked: "No answer from that server yet.",
        back: "‹ back",
        back_to_match: "Back to your match",
        back_to_match_note: "It has not started. Nothing moves until it does.",
        empty_room: "empty",
        lost_connection: "the connection went away",
        people: super::MenuPeople {
            title: "Players",
            note: "The best rated here, or type a name to find somebody.",
            hint: "a name",
            asking: "Asking the server…",
            nobody: "Nobody here by that name.",
            nobody_yet: "Nobody has played enough matches here to be rated yet.",
        },
        account: super::MenuAccount {
            title: "Your account",
            rated: "Rated here",
            unnamed: "No server has met you yet",
        },
        tutorial: super::MenuTutorial {
            run: "Run",
            stop: "Stop",
            step: "Step",
            show_me: "Draw it for me",
            clear: "Clear",
            lessons: &[
            (
                "Lorem ipsum: a factory pays when it turns over",
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Trace the outline to lay a blinker of factories, press Run, and watch the counter.",
            ),
            (
                "Lorem ipsum: and a block of them pays nothing",
                "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. The same four cells that never move are the same four cells that never earn.",
            ),
        ],
        },
        howto: super::MenuHowto {
            title: "How to play",
            note: "Conway's Game of Life, with owners. Five things the board does not tell you.",
            rules: &[
            (
                "You can only build where your influence already reaches.",
                "This is the first thing anybody runs into, and a refused click does not explain it. Territory is a field with sources: the patch you are granted is a spring that never runs dry, and your live cells feed it too. So you grow ground by growing life outward from what you have — not by clicking further away.",
            ),
            (
                "A factory pays when it turns over, not when you own it.",
                "The opposite of what owning a lot of factories suggests. A block of factories is a still life: it never gives birth, so it never pays anything at all. An oscillator earns every period and a gun earns forever. This one rule decides whether your economy works.",
            ),
            (
                "A turret is the other way round, so place it in fours.",
                "It works by standing still, and one on its own dies of loneliness in a generation. The block that is a factory's worst shape is a turret's best — four cells is the cheapest thing in Conway that never dies and never gives birth.",
            ),
            (
                "Ice cannot be taken back.",
                "It stops time over whatever it covers, and only life reaching it breaks it. A pane put down in the wrong place is a decision you live with, so it is worth thinking about before you spend on one.",
            ),
            (
                "A dynamite takes ground; it does not just make a mess.",
                "It burns down on a fuse you can watch — the last warning sprite is on screen for exactly one generation — and then scrambles a disc. What comes up alive is yours and what does not belongs to nobody, so a bomb breaks a country apart and leaves you some of the pieces. Ice stops the fuse, and a dynamite has to stay alive to go off at all.",
            ),
        ],
            tip_title: "Other people's patterns work here.",
            tip: "Life is exactly B3/S23 — an R-pentomino settles at generation 1103 with 116 cells, which is the figure in every book. So a glider is a glider, a gun is a gun, and fifty years of published patterns are things you can build here and expect to behave.",
        },
        conway: super::MenuConway {
            title: "About John Conway",
            body: "The rule underneath all of this is John Horton Conway's, from a \
             Sunday afternoon with counters on a Go board in 1970. He did not want to be \
             remembered for it, and said so often — it went round the world through Martin \
             Gardner's column and then stood in front of everything else he did for fifty \
             years. He died in April 2020. What he would rather you looked up:",
            work: &[
            (
                "Surreal numbers",
                "He thought them his best work. One construction, built out of nothing but \
                 games, that yields the reals and the ordinals and a great deal besides.",
                "https://en.wikipedia.org/wiki/Surreal_number",
            ),
            (
                "The Conway groups",
                "Three sporadic simple groups, pulled out of the Leech lattice — he set aside \
                 two long slots for the work and needed only the first.",
                "https://en.wikipedia.org/wiki/Conway_group",
            ),
            (
                "Monstrous moonshine",
                "His conjecture with Simon Norton connecting the Monster group to modular \
                 functions. Borcherds proved it and won a Fields Medal for it.",
                "https://en.wikipedia.org/wiki/Monstrous_moonshine",
            ),
            (
                "Combinatorial game theory",
                "Which he largely founded: On Numbers and Games, and Winning Ways with \
                 Berlekamp and Guy.",
                "https://en.wikipedia.org/wiki/Combinatorial_game_theory",
            ),
            (
                "The doomsday algorithm",
                "Working out the day of the week in your head. He delighted in it and \
                 practised it daily.",
                "https://en.wikipedia.org/wiki/Doomsday_rule",
            ),
        ],
        },
        home: super::MenuHome {
            play: "Play",
            who: "You are",
            record: "So far",
            profile: "Your profile",
            people: "Who else plays here",
            account: "Your account",
            howto: "How to play",
            settings_label: "Settings",
            settings_hide: "Close settings",
            nothing_yet: "Nothing played yet. That is what Play is for.",
            settings: super::MenuHomeSettings {
                key: "Your player key",
                key_note: "Thirty-two characters, and they are the whole of who you are. \
                 Save them somewhere to play as yourself in another browser, or \
                 paste somebody else's in to become them. Whoever has this is \
                 you, on every server that has met it, and nobody can give it \
                 back.",
                key_take: "Use this key",
                key_unseen: "No server has met this client yet, so none has named it.",
                key_none: "No key could be made or kept here, so this client is somebody new \
                 everywhere it goes.",
                forget: "Forget everything",
                forget_note: "Your key, your name, your record and every world you have played in.",
                confirm: "Yes, do it",
                cancel: "No, leave it",
                forget_ask: "Forget everything?",
                forget_ask_note: "Your key goes with it, and nobody -- including this server -- \
                 has a copy to give back. Everything you have ever held becomes \
                 somebody else's ground.",
                key_ask: "Become somebody else?",
                key_ask_note: "The key you have now is replaced. Unless you have written it \
                 down somewhere, you cannot go back to being who you are.",
            },
        },
        code: super::MenuCode {
            label: "Have a code?",
            hint: "abc234",
            go: "Go",
            made: "Your code — send it to whoever is playing:",
        },
        watch: super::MenuWatch {
            watch: "Watch",
            join: "Join",
            start: "Start the match",
            start_note: "Everybody spawns together when you do.",
            not_yours: "Waiting for whoever made this match to start it.",
            at_console: "Waiting for the server to start it.",
            watching: "watching",
            no_seat: "You are watching this world, not playing in it.",
        },
        make: super::MenuMake {
            alone: "Play",
            open: "New world",
            title: "A new world",
            name: "Name",
            name_hint: "arena",
            shape: "Shape",
            boundless: "Boundless",
            wrapping: "Wrapping",
            size: "Size",
            rows: "Rows",
            cols: "Columns",
            by: "x",
            size_note: "in chunks, each 16 cells square",
            together: "Played",
            solo: "Every player for themselves",
            teams: "In teams",
            sides: "Teams",
            sides_note: "Who is on which team is settled in the lobby.",
            private: "Who can find it",
            listed: "Anyone",
            unlisted: "By code",
            solo_access: "Play alone",
            solo_note: "No server, nobody else, nothing to join. The simulation is the \
             same one a match runs.",
            listed_note: "In the room list, for whoever is on this server.",
            unlisted_note: "Not listed. The server gives it a code to share, instead of a name.",
            kind: "Kind",
            world: "World",
            r#match: "Match",
            experiment: "Experiment",
            world_note: "Runs forever. Anybody may join at any time, and nobody wins.",
            match_note: "Gathers, then runs until somebody has won it.",
            experiment_note: "A laboratory. The same simulation, with the clock and the game's \
             placing rules yours to switch.",
            ends: "Ends",
            timer: "Timer",
            territory: "Territory",
            timer_note: "Most ground when the generations run out.",
            territory_note: "First to hold this many squares wins.",
            generations: "Generations",
            squares: "Squares",
            make: "Create",
            no_server: "Reach a server first — a world is made on one.",
            clear: "Reset",
            making: "creating…",
            match_waits: "A match gathers until the server starts it.",
        },
    },
    hotbar: super::Hotbar {
            step_key: ".",
            speed: "Speed",
            bpm_suffix: " a minute",
        purse: "purse",
        ground: "ground",
        tick: "tick",
        rating: "elo",
        life: "Life",
        factory: "Factory",
        turret: "Turret",
        ice: "Ice",
        dynamite: "Dynamite",
        draw: "Draw",
        pane: "Pane",
        pattern: "Stamp",
        run_hint: "Let the world run",
        stop_hint: "Hold the world still",
        step_hint: "One generation, and stay stopped",
        rules: "Rules",
        wipe_hint: "empty this laboratory",
        rules_hint: "What the game's rules are doing here",
        anywhere: "Place outside your territory",
        anywhere_note: "Off, you may only build where your own influence reaches, as in a game.",
        free: "Place without paying",
        free_note: "Off, everything costs what it costs and a purse can run out.",
        capture: "Grab",
        library: "Stamps",
        help: "?",
        help_hint: "Every key, on one screen",
    },
    stamps: super::Stamps {
        title: "Stamps",
        nothing_to_turn: "Hold a stamp to turn one",
        forget: "forget",
        none_yet: "Nothing kept yet.",
        how: "Grab and drag a box round your own life to take one, or draw one below.",
        draw: "Draw one",
        keep: "keep",
        clear: "clear",
        keep_name: "ok",
        rename_hint: "Click to rename",
        edit: "edit",
        edit_hint: "Open it on the pad. Keeping puts it back where it was.",
        on_bar: "bar",
        on_bar_hint: "Show it on the hotbar. Pin none and the bar is the newest ten.",
        bar_full: "the bar holds ten",
        editing: "editing",
        draw_how: "Click to lay a cell or lift it, drag to lay a run.",
        nothing_to_capture: "nothing of yours alive in there to capture",
        gone: "that stamp is gone",
    },
    clock: super::Clock {

    },
    profile: super::Profile {
        title: "Player",
        asking: "asking…",
        unknown: "this server has never met them.",
        you: "(you)",
        everywhere: "Everywhere you have played:",
        unrated: "No server has met you yet, so nobody has a number for you.",
        nobody: "Nobody in particular",
        here: "On this server:",
        rating_is: "Rating",
        matches_is: "Matches",
        won_is: "Won",
        best_is: "Most ground held",
        games_is: "Games",
        lived_is: "Generations lived",
    },
    lobby: super::Lobby {
        take_side: "Join this team",
        leave_side: "Leave this team",
        code: "Code to share",
        rename: "rename",
        keep_name: "ok",
        nobody_on_it: "nobody yet",
        waiting: "Waiting to start",
        finished: "Match over",
        nobody: "Nobody held any ground.",
        you: "you",
        you_won: "You won",
        how: "Nothing moves until whoever made the match starts it.",
    },
    hud: super::Hud {
        connected: "connected",
        offline: "offline",
        forfeit: "Give up",
        forfeit_hint: "Concede this match. Your team plays on if anyone is left on it.",
        gave_up: "you gave up",
        end_match: "End match",
        end_match_hint: "Call it off now. Whoever leads wins, and it is rated.",
        holding: "ground held",
        back: "\u{2190}",
        back_hint: "back to the menu",
        boundless: "boundless world",
        over_panel: "over panel",
        on_world: "on world",
        nothing_yet: "nothing yet",
        hints: &[
        "left click acts, left drag draws",
        "right, middle or space+drag to pan",
        "arrows or WASD to pan, shift to hurry",
        "wheel or pinch to zoom",
        "1–9 and 0 choose a stamp, shift+1–9 a tool",
        "one finger draws, two move the view",
        "escape abandons the drag in progress",
    ],
    },
    desync: super::Desync {
        settled: "in step",
        background: "in step, correcting",
        noticeable: "correcting often",
        alarming: "struggling to stay in step",
    },
    help: super::Help {
        title: "Keys",
        close: "close",
        dismiss: "Escape or ? closes this.",
        looking: "Looking about",
        building: "Building",
        getting_about: "Getting about",
        pan: "Move the view",
        pan_faster: "Move it faster",
        pan_by_hand: "Drag the world",
        zoom: "Zoom in and out",
        tools: "Pick a tool: life, factory, turret, ice",
        turn: "turn what you are holding",
        mirror: "mirror it, which no rotation can do",
        stamps: "Pick a stamp you have kept",
        drag: "Lay a run of cells, or a rectangle",
        the_clock: "The clock",
        play: RUN_KEY,
        step_one: STEP_KEY,
        server_keeps_time: "the server keeps time in a game; a laboratory's is yours",
        go_back: "back a screen",
        paused: "paused",
        wiped: "emptied",
        running: "running",
        walk: "Walk a list",
        choose: "Take what is picked",
        move_on: "Move between controls",
        back: "Back out: what you are drawing, then the screen",
        help: "This",
        keys: super::HelpKeys {
            pan_arrows: "arrows",
            pan_fast: "shift",
            pan_drag: "middle drag",
            zoom: "wheel / pinch",
            tools: "shift + a digit",
            stamps: "the digit row",
            turn: "R / shift + R",
            mirror: "F",
            drag: "drag",
            play: "space",
            step_one: ".",
            walk: "up / down",
            choose: "enter",
            move_on: "tab",
            back: "escape",
            help: "?",
        },
    },
    record: super::Record {
        nothing_yet: "Nothing played yet. That is what Play is for.",
        largest: "Largest territory, by game",
        form: "Recent",
        worlds: "worlds",
        matches_won: "matches won",
        largest_ever: "largest ever",
        generations: "generations",
        won: "won",
        lost: "lost",
        no_result: "no result",
    },
    refused: super::Refused {

    },
};

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
pub mod menu {
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
    /// The count under the room list, so the list says how much is behind it
    /// before anybody reads the names.
    pub fn rooms_here(rooms: usize, players: u32) -> String {
        let w = if rooms == 1 { "world" } else { "worlds" };
        let p = if players == 1 { "player" } else { "players" };
        format!("{rooms} {w}, {players} {p} online")
    }
    pub mod people {
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
    pub mod tutorial {
        pub fn purse(value: i32) -> String {
            format!("${value}")
        }
        pub fn generation(n: u64) -> String {
            format!("gen {n}")
        }
    }
    pub mod home {
        /// The sign is the whole message, so it is always there -- `+0` never
        /// appears, because a result that moved nothing is not shown at all.
        pub fn rating_change(change: i32) -> String {
            format!("{change:+} from your last match")
        }
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
        pub mod settings {
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
        }
    }
    pub mod watch {
        pub fn started_by(who: &str) -> String {
            format!("started by {who}")
        }
    }
    pub mod make {
        pub fn not_a_number_for(which: &str, text: &str) -> String {
            format!("{which}: \"{text}\" is not a number")
        }
        pub fn sides_range(least: u8, most: u8) -> String {
            format!(
                "{}: between {least} and {most}",
                crate::client::views::words::w().menu.make.sides
            )
        }
        pub fn out_of_range(which: &str, most: i32) -> String {
            format!("{which}: between 1 and {most}")
        }
        pub fn not_a_size(text: &str) -> String {
            format!("\"{text}\" is not a size; try 12x12")
        }
        pub fn not_a_number(text: &str) -> String {
            format!("\"{text}\" is not a number")
        }
    }
}
pub mod challenge {
    /// Said out loud when one arrives, because a panel nobody is looking at is
    /// a challenge nobody answers.
    pub fn asked(who: &str) -> String {
        format!("{who} wants a game")
    }

    /// And the answer, either way — a decline reaches the person who asked
    /// rather than looking like a server that lost it.
    pub fn answered(who: &str, yes: bool) -> String {
        if yes {
            format!("{who} is coming")
        } else {
            format!("{who} said no")
        }
    }
}

pub mod hotbar {
    /// What a rate means, in the unit somebody actually feels it in.
    ///
    /// A slider in generations a minute is precise and hard to picture, so the
    /// line under it says the same number as a wait: 240 is four a second, 60
    /// is one, 12 is one every five seconds. The threshold is where "a second"
    /// stops being the useful half of the sentence.
    pub fn speed_note(bpm: u16) -> String {
        let per_second = bpm as f32 / 60.0;
        if per_second >= 1.0 {
            format!("{per_second:.0} a second")
        } else {
            format!("one every {:.0} seconds", 60.0 / bpm.max(1) as f32)
        }
    }
}

pub mod stamps {
    /// A sweep larger than the pad it would be edited on. Said as the size
    /// rather than as "too big", because the next thing somebody does is sweep
    /// again and they need the number to aim at.
    pub fn too_big(side: i32) -> String {
        format!("a stamp is at most {side} by {side}")
    }
    pub fn captured(name: &str, cells: usize) -> String {
        format!("captured {name} ({cells} cells)")
    }
    pub fn placed(name: &str, cells: usize, delta: i32) -> String {
        format!("stamped {name} ({cells} cells), {delta:+}")
    }
}
pub mod clock {
    fn clocked(seconds: u64) -> String {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }

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
    pub fn squares_left(target: u64, most: u64) -> String {
        format!("{most} of {target} squares")
    }
}
pub mod profile {
    /// **A figure, not a sentence.** These read "4 of 7 matches won" and sat in
    /// a stack, so the server's counts and this client's own were two
    /// paragraphs in two shapes and could not be compared by looking. They are
    /// rows under a heading now — the heading says what it is, so the value
    /// only has to say how much. A dash for nothing, because "0" and "none yet"
    /// are the same fact and one of them draws quieter.
    pub fn count(n: u64) -> String {
        if n == 0 {
            "—".to_string()
        } else {
            n.to_string()
        }
    }

    /// Matches won out of matches finished, which is one figure and not two:
    /// either number alone says nothing anybody wants.
    pub fn won_of(won: usize, played: usize) -> String {
        if played == 0 {
            "—".to_string()
        } else {
            format!("{won} of {played}")
        }
    }

    /// The **most** ever held, not the last: a profile says what somebody has
    /// managed, so a bad match after a good one does not erase the good one.
    pub fn squares(n: u64) -> String {
        if n == 0 {
            "—".to_string()
        } else {
            format!("{n}")
        }
    }
}
pub mod lobby {
    /// Said rather than left to be noticed: the match will not start while
    /// somebody is unplaced, and a lobby that does not say who is the wrong
    /// place to find that out.
    pub fn not_picked(who: &str) -> String {
        format!("{who} has not picked a team")
    }
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
pub mod hud {
    /// What the last match did to it, kept beside the number for as long as
    /// the number is on screen. A rating is a comparison, and a comparison
    /// with nothing to compare against is a score.
    pub fn rating_change(change: i32) -> String {
        format!("{change:+}")
    }
}
pub mod desync {
    /// The reading itself, for the developer panel: the rate, and how much
    /// has ever been put right.
    pub fn reading(rate: f64, total: u64) -> String {
        format!("desync {rate:.1}/12s, {total} chunks corrected")
    }
}
pub mod help {
    pub fn stepped_to(generation: u64) -> String {
        format!("stepped to generation {generation}")
    }
    pub mod keys {
        /// The pan cluster as it prints on *this* keyboard, plus the arrows.
        ///
        /// Four letters rather than the word "WASD", which is a name for a
        /// shape on the board and only spells itself on one layout — on Dvorak
        /// the same four keys print `,aoe`.
        pub fn pan(cluster: &str) -> String {
            format!("{cluster} / arrows")
        }
        /// The tool row, as whatever shift and the first four digits print
        /// here. Bound by position, so the label is the keyboard's answer and
        /// not the one a US layout would have given.
        pub fn with_shift(row: &str) -> String {
            format!("shift + {row}")
        }
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
    }
}
pub mod record {
    /// One game, as a tooltip reads it. Named parts rather than a template,
    /// because the order of them is a decision: what it was, then how big you
    /// got, then how long it took.
    pub fn a_game(room: &str, squares: u32, generations: u64, outcome: &str) -> String {
        format!("{room} · {squares} squares · {generations} generations · {outcome}")
    }
}
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
