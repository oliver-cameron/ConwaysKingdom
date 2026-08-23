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
    pub const ROOMS: &str = "Rooms";
    pub const NO_ROOMS: &str = "this server has no rooms";
    pub const ALONE: &str = "Play alone";
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
    pub const NONE_YET: &str = "Nothing captured yet.";
    pub const HOW: &str = "Grab, then drag a box round your own life, to take another.";

    pub fn captured(name: &str, cells: usize) -> String {
        format!("captured {name} ({cells} cells)")
    }

    pub fn placed(name: &str, cells: usize, delta: i32) -> String {
        format!("stamped {name} ({cells} cells), {delta:+}")
    }

    pub const NOTHING_TO_CAPTURE: &str = "nothing of yours alive in there to capture";
    pub const GONE: &str = "that stamp is gone";
}

/// The panel in the corner.
pub mod hud {
    pub const CONNECTED: &str = "connected";
    pub const OFFLINE: &str = "offline";
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

/// What the world says back when it refuses something.
pub mod refused {
    pub fn not_your_territory(row: i32, col: i32) -> String {
        format!("({row}, {col}) is not your territory")
    }

    pub fn cells_not_yours(n: usize) -> String {
        format!("{n} of those cells are not your territory")
    }

    pub fn cannot_afford(cells: usize, costs: i32, have: i32) -> String {
        format!("{cells} cells costs {costs}, you have {have}")
    }
}
