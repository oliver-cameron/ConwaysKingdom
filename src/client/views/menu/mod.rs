//! The screen before the game.
//!
//! Play alone, or reach a server and pick one of its worlds. Until this
//! existed the only way to reach a server was `--ws` on a command line, which
//! a phone does not have and a browser only gets by being served from the
//! server it will talk to — so a room, which is a world you have to choose
//! before you can be in it, could be chosen by nobody without a terminal.
//!
//! Read-only about the world, like every view here: it holds what the player
//! has typed and what the server has said, and returns what was chosen. It
//! opens no sockets and sends no messages. The client above it does that,
//! which is what keeps "what a menu looks like" and "what connecting means"
//! two separate things.
//!
//! **A room list is a request, not a guess.** A room is a whole separate
//! world, so a name that does not exist is not a mistyped filter, it is
//! nowhere — and a client cannot know what a server has without asking. So the
//! list arrives from `ServerMessage::Rooms` and the menu shows nothing until
//! it does, rather than offering a name that might be there.

mod alone;
pub mod draft;
mod home;
mod play;
mod settings;

pub use draft::{Draft, Ends, Kind, Shape, Together};

use home::home;
use play::play;

use crate::client::views::theme::Theme;
use crate::client::views::words::menu as words;
use crate::net::{RoomId, RoomInfo, RoomName, Victory};
use crate::sim::WorldKind;

/// How long the typing has to stop before the address is taken as finished.
///
/// Long enough that `ws://127.0.0.1:8080/ws` is one address and not twenty on
/// the way to one, and short enough that somebody who has stopped typing does
/// not wonder whether anything is going to happen. Enter and leaving the field
/// both beat it, so this is only the answer for somebody who typed an address
/// and then looked at it.
const SETTLE: f64 = 0.7;

/// How often a refused address is tried again.
///
/// Slow, because the thing being waited for is a person starting a server in
/// another window, and a client hammering a port that is not listening is a
/// log full of nothing. Slower than the room list refreshes, for the same
/// reason: that one is a question the server answers, and this one is a
/// question about whether there is a server.
const RETRY_EVERY: f64 = 4.0;

/// What the menu is doing, which is mostly what it is waiting for.
#[derive(Clone, PartialEq, Eq)]
pub enum Stage {
    /// Nothing has been asked of any server yet.
    Idle,
    /// A socket is open, or opening, and the room list has been asked for.
    /// The address is kept so a failure can name it.
    Asking,
    /// The server answered. Pick one.
    ///
    /// `note` is why you are looking at this list rather than at a world —
    /// normally nothing, and after a refusal the refusal. Kept beside the list
    /// rather than replaced by it: "no room \"nowhere\" here" and the names of
    /// the rooms that are here are two halves of one answer, and showing only
    /// the second leaves the player wondering whether they mistyped or the
    /// server moved.
    Choosing { rooms: Vec<RoomInfo>, note: Option<String> },
    /// Something went wrong, or the server said no. Shown until the next try.
    Failed(String),
}

/// Which of the menu's screens is showing.
///
/// Two, and the depth stops there. Clash Royale's interface almost never goes
/// more than one level down and disguises it where it does, which is the whole
/// argument for this being a page rather than a stack: home is who you are and
/// what you have done, play is where you go, and there is nothing under either.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Your name, your record, and the way in.
    Home,
    /// A server, its rooms, a code, and the form that makes one — a world,
    /// a match or a laboratory, which is [`Kind`] on that form rather than a
    /// page of its own.
    Play,
    /// Describing a world to play in on your own.
    ///
    /// **A page rather than a button.** "Play alone" went straight into a
    /// world built from whatever the command line had said, so a solitary game
    /// could not be a small torus or have a way to win, and the form that asks
    /// those questions was only reachable by going to the server screen and
    /// finding it under a room list nobody had asked for.
    Alone,
}

pub struct Menu {
    pub name: String,
    /// Where to connect. Fixed on the web — the page came from a server, so
    /// that is the server — and typed natively, where there is no page to
    /// have come from.
    pub address: String,
    pub stage: Stage,
    /// Which screen is showing.
    pub page: Page,
    /// When the address was last typed into, so the reaching can wait for the
    /// typing to stop. `None` once it has been acted on.
    pub typed_at: Option<f64>,
    /// When the last refusal was shown, so it can be tried again without
    /// anybody having to notice and click.
    pub failed_at: Option<f64>,
    /// The address already asked about, so a settled field is reached for
    /// once rather than every frame after it settles.
    pub attempted: Option<String>,
    /// Which room in the list is picked out, so its actions can live inside it
    /// rather than beside every row.
    ///
    /// A row of buttons on every entry makes the list twice as tall and twice
    /// as busy to read, and most of those buttons belong to rooms nobody is
    /// looking at. One selection, and Join and Watch appear in it.
    pub selected: Option<RoomId>,
    /// A code typed in to reach a room that is not in the listing.
    ///
    /// Beside the room list rather than instead of it: a code is how you reach
    /// somebody's private game, and the list is how you find a public one.
    /// They are two ways in, not two versions of one.
    pub code: String,
    /// What this client has played before, read once when the menu opens.
    ///
    /// The games rather than only their totals, because the home screen draws
    /// the shape of them and not just the count. Read once rather than every
    /// frame: it comes out of `localStorage` or a file, and neither wants
    /// touching sixty times a second for something that changes when a game
    /// ends.
    pub games: Vec<crate::client::record::Game>,
    pub record: crate::client::record::Summary,
    /// Whether the settings at the foot of the home screen are open.
    ///
    /// Shut every time the menu is built, because it is not a preference: it
    /// is a drawer somebody opened once to do a thing, and a client that
    /// remembered it open would put the key back where it used to be.
    /// What this client is rated on the server it last reached, and what the
    /// last result moved it by.
    ///
    /// `None` until a server has said. That is not the same as the starting
    /// number: somebody who has never connected has no rating rather than an
    /// average one, and showing them 1200 would be inventing it.
    ///
    /// Passed in rather than read, like everything else on this screen: the
    /// menu holds what it was told and what was typed, and a rating is neither
    /// of this client's business to work out nor kept anywhere it could look.
    pub rating: Option<crate::client::session::Rating>,
    pub advanced: bool,
    /// Whether the secret half is on screen. Shut on every build of the menu,
    /// because a secret nobody is looking at should not be one anybody can
    /// type over.
    pub revealed: bool,
    /// What is waiting to be confirmed, if anything.
    ///
    /// Held on the menu rather than answered where it is asked, because the
    /// question is drawn over every screen and the press that raises it is in
    /// a column half way down one.
    pub asking: Option<Ask>,
    /// This client's key, shown so it can be copied and editable so another
    /// one can be pasted over it.
    ///
    /// **One field for both jobs**, because they are one question — who is
    /// this browser — and two fields side by side, one showing a key and one
    /// taking one, would be two answers to it. Read once when the menu opens,
    /// like the record beside it: it changes when a server says so, and not
    /// sixty times a second.
    pub key: String,
    /// The world being described, if the form is open.
    ///
    /// On the menu rather than inside [`Stage::Choosing`] because the room
    /// list asks itself again every few seconds and replaces that stage
    /// wholesale — a half-filled form living in it would be wiped mid-typing,
    /// three seconds at a time.
    pub draft: Option<Draft>,
}

/// Whether a world ends, and how. Three answers to one question rather than a
/// separate "world or match?": a room with no end is the ordinary case, and a
/// match is the one with a condition on it.
/// What the player chose this frame, if anything.
pub enum Chose {
    Nothing,
    /// Ask the server what rooms it has, again. The list refreshes itself
    /// every few seconds; this is for the moment somebody has just made a room
    /// on the other screen and does not want to wait out the interval.
    Refresh,
    /// Play with no server at all, on whatever world the command line asked
    /// for. The simulation is deterministic, so this is a whole game rather
    /// than a broken one — just a solitary one.
    Offline,
    /// Play alone, on a world described here.
    ///
    /// **The make-a-world form, pointed somewhere else.** Its questions —
    /// how big, does it end, how — are the same questions whether or not a
    /// server is going to hold the answer, and asking them twice in two forms
    /// would be two places for "boundless" to mean something slightly
    /// different. What a server adds is a name, a listing and other people,
    /// which is exactly what the form hides when there is nobody to ask.
    Alone {
        shape: WorldKind,
        /// `None` is a sandbox, which is what playing alone has always been.
        victory: Option<Victory>,
    },
    /// Reach this server and ask what rooms it has.
    Connect(String),
    /// Join this room on the server already reached. An **id**, not a name:
    /// what the listing sent back, or what was typed into the code field.
    Join(RoomId),
    /// Put the form back to its defaults. It is a column now rather than
    /// something opened, so there is nothing to shut — what a press here means
    /// is "start this description again".
    Clear,
    /// Back into the world already behind this menu, without joining anything.
    Resume,
    /// Forget the key, the record, the name and every room's token. There is
    /// no way back from this, which is why it is asked about first.
    ResetEverything,
    /// Be the person this key names, from the next join onwards.
    ///
    /// Raw rather than parsed, unlike [`Self::Create`] — the menu checks that
    /// it *reads* as a key before offering the press, and what happens to a
    /// key that reads but is not this server's is a refusal from the server,
    /// which is not a thing a view can find out.
    UseKey(String),
    /// Make this world on the server already reached, then join it. Parsed
    /// rather than raw: what the player typed became what they chose in
    /// [`Draft::parse`], which is the menu's whole job.
    Create {
        name: RoomName,
        shape: WorldKind,
        victory: Option<Victory>,
        /// `None` is a free-for-all; `Some(n)` is a match played in n sides.
        teams: Option<u8>,
        private: bool,
        /// Make it a laboratory: the clock is a control, and the game's two
        /// placing rules can be switched off inside it. Never true beside a
        /// `victory` — a match with the rules off is not a match.
        laboratory: bool,
    },
    /// Watch this room without taking a seat in it.
    Watch(RoomId),
}

impl Menu {
    /// Opens on what was last used, so the common case is one click.
    ///
    /// The address is only a default on native. In a browser the page came
    /// from the server, so a remembered address from some other visit would be
    /// pointing the client away from the machine that served it.
    pub fn new(default_address: String, on_web: bool) -> Self {
        let address = if on_web {
            default_address
        } else {
            crate::net::keep::server().unwrap_or(default_address)
        };
        let key = String::new();
        Self {
            name: crate::net::keep::name().unwrap_or_else(|| "player".to_string()),
            address,
            stage: Stage::Idle,
            page: Page::Home,
            typed_at: None,
            failed_at: None,
            attempted: None,
            selected: None,
            code: String::new(),
            games: crate::client::record::games(),
            record: crate::client::record::Summary::of(&crate::client::record::games()),
            key,
            rating: None,
            advanced: false,
            revealed: false,
            asking: None,
            draft: None,
        }
    }

    /// Open the make-a-world form already describing a kind of room.
    ///
    /// `/experiments` is this, and so is the back button landing on it: a
    /// laboratory is a kind on that form rather than a page, so a link that
    /// asks for one asks for the form with an answer already given.
    pub fn describe(&mut self, kind: Kind) {
        self.page = Page::Play;
        self.draft.get_or_insert_with(Default::default).kind = kind;
    }

    /// The menu, opened on a sentence saying why the client is looking at it.
    ///
    /// For the client that was told where to go and could not get there: a
    /// link into a room, or `--ws` on a command line. Those used to fall
    /// through to a solo world with nothing said.
    pub fn failed(default_address: String, on_web: bool, why: String) -> Self {
        Self { stage: Stage::Failed(why), ..Self::new(default_address, on_web) }
    }
}

/// Draw it, and say what was chosen. Returns the rectangle it covered, so the
/// world behind it does not also take the click.
/// A question the menu has raised and not had answered.
///
/// Both of these destroy a key, and a key is the one thing in this client that
/// nobody anywhere holds a second copy of.
#[derive(Clone, PartialEq, Eq)]
pub enum Ask {
    /// Forget everything, including who this client is.
    Forget,
    /// Replace this client's key with the one that was pasted in.
    UseKey(String),
}

/// What the client already is, which the menu cannot see for itself.
///
/// The menu holds what was typed and what the server said; whether there is a
/// world behind it, and whether that world is a match you are enrolled in, are
/// facts about the client. Passed in rather than reached for, which is what
/// keeps this module able to read and not to act.
#[derive(Clone, Copy, Default)]
pub struct Where {
    /// Seconds since the client started, which is what the address field
    /// measures a pause in typing against.
    pub now: f64,
    /// The socket is derived from the page's origin, so the address is shown
    /// rather than typed.
    pub on_web: bool,
    /// There is a world to go back to, and it is a match that has not started.
    /// "Play alone" becomes "back to your match", because starting a solitary
    /// game while enrolled in one is never what the press meant.
    pub waiting_in_a_match: bool,
}

pub fn show(ctx: &egui::Context, theme: &Theme, menu: &mut Menu, at: Where) -> super::Shown<Chose> {
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    // **Two ways out of a text field, because there have to be two.**
    //
    // egui takes the keyboard while a field has focus, so the app's own escape
    // ladder never sees the key — a selection you cannot clear, in a field you
    // cannot leave, was the whole of the complaint. Escape is the first way.
    //
    // The second is pressing somewhere else, which is what anybody does before
    // they think to reach for a key. egui gives focus up when a press lands on
    // another *widget*; a press on the panel between them lands on nothing and
    // leaves the field looking as though it still has the keyboard, with the
    // highlight painted where it was.
    //
    // Both end the same way: surrender the focus **and collapse the cursor
    // range**. Surrendering alone leaves the selection drawn, which is the
    // half of it that reads as broken.
    let focused = ctx.memory(|mem| mem.focused());
    if let Some(id) = focused {
        let escaped = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Where the field is, so a press inside it can be left alone: clearing
        // on every press would make a field impossible to click into.
        let field = ctx.read_response(id).map(|r| r.rect);
        let elsewhere = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|at| field.is_none_or(|rect| !rect.contains(at)))
        });
        if escaped || elsewhere {
            if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                state.cursor.set_char_range(None);
                egui::TextEdit::store_state(ctx, id, state);
            }
            ctx.memory_mut(|mem| mem.surrender_focus(id));
            ctx.memory_mut(|mem| mem.stop_text_input());
        }
    }

    // The window, in points, which is what `Views` handed egui — not physical
    // pixels, so this reads the same on a hidpi display as on any other.
    let screen = ctx.content_rect();
    let width = theme.panel_width(screen.width());

    // **The menu is the screen, so it is drawn as the screen.** It was a card
    // floating in the middle of a much larger dark field, which reads as
    // cramped however wide the card is: the eye takes the border as the edge
    // of the thing, and everything outside it as space the game is refusing to
    // use. Filling the window and letting the *content* be a column inside it
    // is the same information without the fence around it.
    //
    // No stroke, for the same reason. A border is what tells you where one
    // surface ends and another begins, and at the edge of the window there is
    // nothing on the other side of it.
    //
    // [game.md](../../../docs/game.md#the-menu) has said the menu has the
    // screen to itself since it stopped being a corner panel; this is that
    // sentence being true.
    //
    // **Not at `Order::Background`, which it used to be and which bought
    // nothing.** There is nothing behind the menu to be behind: the world is
    // not drawn on this screen at all — `GameApp::showing_world` is false
    // for `Screen::Menu`, so `draw_calls` returns an empty list and the frame
    // is a clear and this panel. Background was the one attribute separating
    // the single panel that has been reported blank from the several that are
    // not: the HUD is a `Window`, and the hotbar, the clock, the lobby and the
    // stamp library are all `Area`s at the default `Order::Middle`. An
    // attribute that is unique to the thing that fails, and that nothing
    // depends on, does not get to stay.
    let area = egui::Area::new("menu".into())
        .fixed_pos(screen.min)
        // A fade is for something arriving over something else. This is the
        // screen, and a fade that does not finish is a screen with nothing on
        // it — which is the shape of the fault being chased.
        .fade_in(false)
        // Nothing here is a floating window, and `Area::new` makes one movable
        // by default. `fixed_pos` re-pins it every frame so a drag never went
        // anywhere visible, which is not the same as it not happening: the
        // whole screen was answering a drag with a move nobody asked for.
        .movable(false)
        .show(ctx, |ui| {
            // **An area does not bound its content, so the bounds are stated
            // here.** `Area` builds its `Ui` from the size it was *measured*
            // at on the previous frame — and on the first frame there is no
            // previous one, so it uses egui's default area size and runs a
            // sizing pass. A `ScrollArea` works out its viewport from the room
            // it is given, so inside an area it is working from a rectangle
            // that has nothing to do with the window, and
            // `auto_shrink([false, false])` then tells it to fill exactly
            // that. The screen is what this panel is, so the screen is what it
            // is given, before anything inside it asks.
            ui.set_max_size(screen.size());
            egui::Frame::new().fill(theme.palette.surface).show(ui, |ui| {
                ui.set_min_size(screen.size());
                ui.set_max_size(screen.size());
                // Scrolled, because filling the screen does not make the
                // screen taller: a server with a dozen rooms and the form
                // beside them still runs off the bottom of a laptop, and
                // content that cannot be reached is worse than content that
                // looks cramped.
                // Told how tall rather than left to work it out: a scroll
                // area with no viewport it can trust is one that clips its
                // content against a rectangle it invented.
                egui::ScrollArea::vertical()
                    .max_height(screen.height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            // A column inside the window rather than the window
                            // itself. A room list stretched across two thousand
                            // points is a list nobody can follow from a name to
                            // the count beside it, and prose that wide is
                            // unreadable — the screen is not cramped, so the
                            // content need not sprawl to prove it.
                            ui.add_space(m.margin * 2.0);
                            ui.allocate_ui(egui::vec2(width, screen.height()), |ui| {
                                ui.set_width(width);
                                ui.spacing_mut().item_spacing.y = m.item_spacing;
                                match menu.page {
                                    Page::Home => chose = home(ui, theme, menu, at),
                                    Page::Play => chose = play(ui, theme, menu, at),
                                    Page::Alone => chose = alone::show(ui, theme, menu),
                                }
                            });
                            ui.add_space(m.margin * 2.0);
                        });
                    });
            });
        });

    // **Over everything, and drawn last.** What is being confirmed changes
    // what this client *is* rather than what it is looking at, so it is not a
    // row of buttons half way down a column somebody is already scrolling
    // past. A press below it does nothing while it is up, because it covers
    // the screen and takes the pointer.
    if let Some(ask) = menu.asking.clone()
        && let Some(answer) = settings::confirm(ctx, theme, &ask)
    {
        menu.asking = None;
        if answer {
            chose = match ask {
                Ask::Forget => Chose::ResetEverything,
                Ask::UseKey(key) => Chose::UseKey(key),
            };
        }
    }

    super::Shown::new(area.response.rect, chose)
}

/// Who you are, what you have done, and the way in.
///
/// **One accent-coloured control**, and it is Play. Everything else on this
/// screen is something you read rather than something you press, which is the
/// hierarchy Clash Royale gets right: one thing you are meant to do next, in
/// How a world's shape reads to somebody choosing between two of them.
pub fn describe(world: WorldKind) -> String {
    match world {
        WorldKind::Infinite => "boundless".to_string(),
        WorldKind::Toroidal { rows, cols } => format!("{rows}×{cols} chunks, wrapping"),
    }
}

/// Players connected now. "1 player" rather than "1 players", because the menu
/// is the first thing anybody reads.
pub(super) fn players(n: u32) -> String {
    match n {
        0 => words::EMPTY_ROOM.to_string(),
        1 => words::one_player(),
        n => words::players(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::views::theme::Theme;

    #[test]
    fn a_room_reads_as_something_to_choose_between() {
        assert_eq!(players(0), "empty");
        assert_eq!(players(1), "1 player", "not \"1 players\"");
        assert_eq!(players(4), "4 players");
        assert_eq!(describe(WorldKind::Infinite), "boundless");
        assert_eq!(describe(WorldKind::Toroidal { rows: 6, cols: 8 }), "6×8 chunks, wrapping");
    }

    /// What was typed becomes what was chosen, and a field that does not
    /// apply is not read — a size left over from a wrapping world does not
    /// refuse a boundless one.
    /// **The offline form plays offline.** It produced a `Create` whatever the
    /// button said, so pressing "Play alone" asked a server that was not there
    /// — `Chose::Alone` existed and nothing in the tree ever built one.
    #[test]
    fn the_form_with_no_server_plays_the_world_here() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Alone;
        assert!(
            probe(&mut menu, at(1.0, false), |_, chose| matches!(chose, Chose::Alone { .. })),
            "the solitary form asked a server instead of playing"
        );
    }

    #[test]
    fn a_draft_becomes_a_room_or_says_what_is_wrong() {
        let world = Draft { name: "  Arena ".into(), ..Draft::default() };
        let made = world.parse().unwrap();
        assert_eq!(made.name, "arena", "trimmed and lowercased, the way the server will name it");
        assert_eq!((made.shape, made.victory, made.teams), (WorldKind::Infinite, None, None));

        let torus = Draft {
            name: "ring".into(),
            shape: Shape::Wrapping,
            rows: "6".into(),
            cols: "8".into(),
            ..Draft::default()
        };
        assert_eq!(torus.parse().unwrap().shape, WorldKind::Toroidal { rows: 6, cols: 8 });

        let cup = Draft {
            name: "cup".into(),
            kind: Kind::Match,
            ends: Ends::Territory,
            target: "500".into(),
            // Left behind by a change of mind, and never read, because the
            // shape is boundless.
            rows: "nonsense".into(),
            ..Draft::default()
        };
        assert_eq!(cup.parse().unwrap().victory, Some(Victory::Territory { squares: 500 }));
        assert_eq!(cup.parse().unwrap().teams, None, "a match is solo unless asked otherwise");

        // Teams on a match, and on a world too: a team is people playing as
        // one player, which is worth having without a result to win.
        let sided = Draft {
            name: "cup".into(),
            kind: Kind::Match,
            ends: Ends::Timer,
            together: Together::Teams,
            team_count: "3".into(),
            ..Draft::default()
        };
        assert_eq!(sided.parse().unwrap().teams, Some(3));
        let world_with_teams = Draft {
            name: "hall".into(),
            together: Together::Teams,
            team_count: "2".into(),
            ..Draft::default()
        };
        let made = world_with_teams.parse().unwrap();
        assert_eq!(made.victory, None, "a world still never ends");
        assert_eq!(made.teams, Some(2), "and may still be played in teams");
        let too_many = Draft {
            name: "cup".into(),
            kind: Kind::Match,
            ends: Ends::Timer,
            together: Together::Teams,
            team_count: "99".into(),
            ..Draft::default()
        };
        assert!(too_many.parse().is_err());

        let unnamed = Draft { name: "  ".into(), ..Draft::default() };
        assert!(unnamed.parse().is_err(), "a room needs a name");
        let bad = Draft { name: "arena!".into(), ..Draft::default() };
        assert!(bad.parse().is_err(), "and one the filesystem can hold");
        let sizeless = Draft {
            name: "ring".into(),
            shape: Shape::Wrapping,
            rows: "big".into(),
            ..Draft::default()
        };
        let why = sizeless.parse().unwrap_err();
        assert!(why.contains(words::make::ROWS), "the error says which field: {why}");
        assert!(!why.contains(words::make::COLS), "and not the one that is fine: {why}");

        // A torus is allocated whole, so an enormous side is a client that
        // asks its own machine for gigabytes and stops.
        let huge = Draft {
            name: "ring".into(),
            shape: Shape::Wrapping,
            cols: "100000".into(),
            ..Draft::default()
        };
        assert!(huge.parse().is_err());

        // A private room takes the server's code for a name, so there is
        // nothing here to refuse.
        let coded = Draft { name: String::new(), private: true, ..Draft::default() };
        assert!(coded.parse().is_ok(), "a private room needs no typed name");
        let endless = Draft {
            name: "cup".into(),
            kind: Kind::Match,
            ends: Ends::Timer,
            target: "0".into(),
            ..Draft::default()
        };
        assert!(endless.parse().is_err(), "a match of zero is over already");
    }

    /// Two thousand generations and two thousand squares are not the same
    /// order of thing, so the number follows the condition rather than being
    /// carried across it.
    #[test]
    fn the_target_follows_the_win_condition() {
        let mut draft = Draft::default();
        assert_eq!(draft.ends, Ends::Timer, "a match is timed unless asked otherwise");
        assert_eq!(draft.target, crate::net::DEFAULT_TIMER.to_string());
        draft.retarget(Ends::Territory);
        assert_eq!(draft.target, crate::net::DEFAULT_TERRITORY.to_string());

        // Typed over, and then the same condition chosen again: nothing
        // happens, or every frame would reset the field under the cursor.
        draft.target = "42".into();
        draft.retarget(Ends::Territory);
        assert_eq!(draft.target, "42", "only a change of condition changes it");
    }

    /// The reaching has no button, so the thing that must not go wrong is
    /// asking more than once for one address. `ws://127.0.0.1:8080/ws` passes
    /// through twenty addresses on its way to being one, and a pause fires
    /// every frame after it settles until something says otherwise.
    #[test]
    fn one_address_is_asked_about_once() {
        // What `server_field` does with `attempted`, in the one place it is
        // worth pinning: the guard, not the drawing.
        fn settle(attempted: &mut Option<String>, asks: &mut u32, address: &str) {
            if !address.is_empty() && attempted.as_deref() != Some(address) {
                *attempted = Some(address.to_string());
                *asks += 1;
            }
        }
        let mut attempted: Option<String> = None;
        let mut asks = 0;

        // Typing settles, then the pause keeps firing while nothing changes.
        settle(&mut attempted, &mut asks, "ws://host:8080/ws");
        settle(&mut attempted, &mut asks, "ws://host:8080/ws");
        settle(&mut attempted, &mut asks, "ws://host:8080/ws");
        assert_eq!(asks, 1, "one address, one socket");

        // A different address is a different question.
        settle(&mut attempted, &mut asks, "ws://elsewhere:9000/ws");
        assert_eq!(asks, 2);

        // And back again is too — somebody who mistyped and corrected it is
        // asking about the first address for the first time since.
        settle(&mut attempted, &mut asks, "ws://host:8080/ws");
        assert_eq!(asks, 3);

        // An empty field asks nothing rather than asking about "".
        settle(&mut attempted, &mut asks, "");
        assert_eq!(asks, 3);
    }

    /// Run the menu for one frame in a headless egui and say what it chose.
    ///
    /// The class of bug this exists for has now bitten twice: a control that
    /// is unreachable on one platform, or in one state, which no amount of
    /// reading the drawing code reliably catches. egui runs perfectly well
    /// with no window, so the menu can simply be *asked*.
    /// One pass, because one pass is one frame and the menu changes its own
    /// state as it decides — a second would run against a menu that had
    /// already acted, and report that it did nothing.
    fn one_frame(menu: &mut Menu, at: Where) -> Chose {
        let ctx = egui::Context::default();
        frame(&ctx, 0.0, menu, at, Vec::new()).0
    }

    fn at(now: f64, on_web: bool) -> Where {
        Where { now, on_web, waiting_in_a_match: false }
    }

    /// Run a frame with these events and say what the menu chose.
    fn frame(
        ctx: &egui::Context,
        clock: f64,
        menu: &mut Menu,
        at: Where,
        events: Vec<egui::Event>,
    ) -> (Chose, egui::Rect) {
        let theme = Theme::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            time: Some(clock),
            focused: true,
            events,
            ..Default::default()
        });
        let shown = show(ctx, &theme, menu, at);
        let (chose, rect) = (shown.did, shown.rect);
        // Cleared, not dropped — see docs/gotchas.md.
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
        (chose, rect.unwrap_or(egui::Rect::NOTHING))
    }

    /// The two halves of a press, as **two frames**.
    ///
    /// Not one. A click is a press and then a release, and egui decides one
    /// happened by watching the pointer across frames — put both in a single
    /// frame's events and nothing is clicked, which is a fault in the test and
    /// reads exactly like a fault in the screen. That mistake cost the first
    /// version of this harness, which accused the menu of being dead.
    fn down(at: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    }

    fn up(at: egui::Pos2) -> Vec<egui::Event> {
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]
    }

    /// The menu fills the window rather than floating a card in the middle of
    /// it, so its rectangle is the window — which is also what tells the
    /// client that no press on this screen belongs to the world behind.
    #[test]
    fn the_menu_is_the_whole_screen() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        let ctx = egui::Context::default();
        let (_, rect) = frame(&ctx, 0.0, &mut menu, at(0.0, false), Vec::new());
        // The harness hands egui a 1200x800 window.
        assert!(rect.width() >= 1200.0, "the menu left the sides of the screen: {rect:?}");
        assert!(rect.height() >= 800.0, "and the top or the bottom: {rect:?}");
    }

    /// **The home screen with a record on it.**
    ///
    /// The chart, the form strip and the tiles are only drawn when there is
    /// something to draw — so a client that has never played never runs any of
    /// it, and a client that has runs all of it on every frame of the menu.
    /// That is a real difference between two machines with the same build, and
    /// exactly the shape of a fault that follows the machine rather than the
    /// commit.
    #[test]
    fn the_home_screen_draws_a_record_it_actually_has() {
        use crate::client::record::{Game, Outcome, Summary};
        use crate::sim::WorldKind;

        let games: Vec<Game> = (0..40)
            .map(|i| Game {
                room: format!("room-{i}"),
                world: if i % 2 == 0 {
                    WorldKind::Infinite
                } else {
                    WorldKind::Toroidal { rows: 6, cols: 8 }
                },
                generations: i as u64 * 137,
                // Including nought, which is the case the chart has to give a
                // stub to rather than scale away.
                best: if i % 7 == 0 { 0 } else { i as u32 * 31 },
                outcome: match i % 3 {
                    0 => Outcome::Won,
                    1 => Outcome::Lost,
                    _ => Outcome::Played,
                },
            })
            .collect();

        for (w, h) in [(1200.0, 800.0), (420.0, 700.0), (64.0, 64.0)] {
            let mut menu = Menu::new("ws://host:8080/ws".into(), false);
            menu.record = Summary::of(&games);
            menu.games = games.clone();
            assert!(menu.record.any(), "the record must be non-empty to test it");

            let ctx = egui::Context::default();
            let theme = Theme::default();
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
                time: Some(0.0),
                focused: true,
                ..Default::default()
            });
            let rect = show(&ctx, &theme, &mut menu, at(0.0, false)).rect;
            let mut out = ctx.end_pass();
            out.textures_delta.clear();
            assert!(
                rect.is_some_and(|r| r.width() > 1.0),
                "the home screen drew nothing at {w}x{h} with a record on it"
            );
        }
    }

    /// **A canvas has no size on its first frames**, and a browser's is the
    /// worst case: winit's `inner_size` starts at zero, so the surface is
    /// configured 1x1 before any resize observation lands — see
    /// docs/gotchas.md. Whatever the menu does then, it must not be nothing.
    #[test]
    fn the_menu_survives_a_screen_with_no_size() {
        for (w, h) in [(0.0, 0.0), (1.0, 1.0), (64.0, 48.0), (320.0, 200.0)] {
            let mut menu = Menu::new("ws://host:8080/ws".into(), false);
            let ctx = egui::Context::default();
            let theme = Theme::default();
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h))),
                time: Some(0.0),
                focused: true,
                ..Default::default()
            });
            let rect = show(&ctx, &theme, &mut menu, at(0.0, false)).rect;
            let mut out = ctx.end_pass();
            out.textures_delta.clear();

            let rect = rect.unwrap_or(egui::Rect::NOTHING);
            assert!(
                rect.width() > 1.0 && rect.height() > 1.0,
                "at {w}x{h} the menu drew nothing at all: {rect:?}"
            );
        }
    }

    /// **Is this screen clickable at all?**
    ///
    /// Written after a report that buttons had stopped working, which no
    /// amount of reading the drawing code settles: a widget can be laid out
    /// perfectly and still be unreachable, covered by something drawn after
    /// it, or sitting where the pointer is not.
    ///
    /// The buttons on these screens run the full width of the panel, so the
    /// centre line crosses every one of them. Walk it, press at each step, and
    /// see whether anything answers.
    fn probe(menu: &mut Menu, at: Where, mut answered: impl FnMut(&Menu, &Chose) -> bool) -> bool {
        // **One context for the whole walk.** egui interacts against widgets
        // it has already seen, and a fresh context has seen nothing — a probe
        // that made one per press would be testing a screen's first frame over
        // and over, which is never a frame anybody clicks on.
        let ctx = egui::Context::default();
        let mut clock = 0.0;
        let mut tick = |menu: &mut Menu, events| {
            clock += 1.0 / 60.0;
            frame(&ctx, clock, menu, at, events)
        };

        let (_, rect) = tick(menu, Vec::new());
        assert!(rect.width() > 1.0, "the menu drew nothing to press: {rect:?}");

        // Four columns of probe rather than the centre line. The centre line
        // was enough while every control ran the full width of a card; on a
        // two-column screen it runs down the **gap between them** and finds
        // nothing, which is a fault in the probe that reads as a dead button.
        let lanes: Vec<f32> =
            (1..=4).map(|n| rect.left() + rect.width() * n as f32 / 5.0).collect();
        let mut y = rect.top();
        while y < rect.bottom() {
            for &x in &lanes {
                // **Put the screen back if the press navigated off it.** A
                // sweep presses everything, and everything includes the way
                // out — so one lane landing on Back left the probe pressing
                // its way down the home screen, looking for a control that is
                // on the one it just left. That is a probe finding nothing
                // rather than a screen offering nothing, and the two are
                // indistinguishable from the failure.
                let was = menu.page;
                let point = egui::pos2(x, y);
                tick(menu, down(point));
                let (chose, _) = tick(menu, up(point));
                if answered(menu, &chose) {
                    return true;
                }
                if menu.page != was {
                    menu.page = was;
                }
            }
            y += 6.0;
        }
        false
    }

    /// The home screen's one accent-coloured control, pressed the way a player
    /// presses it.
    #[test]
    fn the_play_button_can_be_pressed() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        assert!(
            probe(&mut menu, at(1.0, false), |m, _| m.page == Page::Play),
            "nothing on the home screen answered a press"
        );
    }

    /// And the world form's, which is the one that had to be reached through
    /// two columns and a frame.
    #[test]
    fn the_make_button_can_be_pressed() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Play;
        menu.attempted = Some("ws://host:8080/ws".into());
        menu.stage = Stage::Choosing { rooms: Vec::new(), note: None };
        menu.draft = Some(Draft { name: "arena".into(), ..Draft::default() });

        assert!(
            probe(&mut menu, at(1.0, false), |_, chose| matches!(chose, Chose::Create { .. })),
            "the world form could not be submitted"
        );
    }

    fn a_room(id: &str) -> RoomInfo {
        RoomInfo {
            id: RoomId::from(id),
            name: id.into(),
            phase: crate::net::MatchPhase::Open,
            victory: None,
            players: 0,
            world: WorldKind::Infinite,
            rules: crate::net::Rules::default(),
        }
    }

    /// **A press on nothing lets the keyboard go.**
    ///
    /// egui gives focus up when a press lands on another widget; a press on
    /// the panel between them lands on nothing, and the field kept both the
    /// focus and its highlight — which reads as a selection there is no way to
    /// escape, because from inside the game there was not one.
    #[test]
    fn pressing_into_nothing_lets_a_field_go() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Play;
        menu.attempted = Some("ws://host:8080/ws".into());
        menu.stage = Stage::Choosing { rooms: Vec::new(), note: None };

        let ctx = egui::Context::default();
        let mut clock = 0.0;
        let mut tick = |menu: &mut Menu, events| {
            clock += 1.0 / 60.0;
            frame(&ctx, clock, menu, at(1.0, false), events)
        };

        // Into the address field, which the play screen always has.
        //
        // **Lanes rather than the centre line**, for the reason `probe` uses
        // them: the centre line was enough while every control ran the full
        // width, and the address field is a field-sized thing in a row of
        // other things now — so a sweep down the middle walks past it.
        let (_, rect) = tick(&mut menu, Vec::new());
        let lanes: Vec<f32> =
            (1..=8).map(|n| rect.left() + rect.width() * n as f32 / 9.0).collect();
        let mut into = None;
        let mut y = rect.top();
        while y < rect.bottom() && into.is_none() {
            for &x in &lanes {
                let point = egui::pos2(x, y);
                tick(&mut menu, down(point));
                tick(&mut menu, up(point));
                if ctx.memory(|m| m.focused()).is_some() {
                    into = Some(point);
                    break;
                }
            }
            y += 6.0;
        }
        let into = into.expect("nothing on the play screen takes the keyboard");

        // A press well away from it — the far corner, which is panel and
        // nothing else.
        let nowhere = rect.min + egui::vec2(4.0, 4.0);
        assert_ne!(nowhere, into);
        tick(&mut menu, down(nowhere));
        tick(&mut menu, up(nowhere));
        assert!(
            ctx.memory(|m| m.focused()).is_none(),
            "the field kept the keyboard after a press on nothing"
        );
    }

    /// **The press that actually starts a game.** Select a room, then join it.
    ///
    /// Two presses rather than one, which is what makes it worth testing: the
    /// row's own click was registered *after* the buttons it reveals, and in
    /// an immediate-mode interface that puts it on top of them — so a press on
    /// Join reached the row instead, and the row's answer to being pressed
    /// while selected is to put itself away. A Join that visibly depresses and
    /// closes the room is exactly what "the buttons work but do not take me to
    /// the game" looks like.
    #[test]
    fn a_room_can_be_selected_and_then_joined() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Play;
        menu.attempted = Some("ws://host:8080/ws".into());
        menu.stage = Stage::Choosing { rooms: vec![a_room("arena")], note: None };

        assert!(
            probe(&mut menu, at(1.0, false), |m, _| m.selected.is_some()),
            "a room could not be selected"
        );
        assert_eq!(menu.selected, Some(RoomId::from("arena")));

        assert!(
            probe(&mut menu, at(1.0, false), |_, chose| matches!(
                chose,
                Chose::Join(id) if id.as_str() == "arena"
            )),
            "the selected room could not be joined"
        );
    }

    /// And watching it, which is the other button the row reveals.
    #[test]
    fn a_selected_room_can_be_watched() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Play;
        menu.attempted = Some("ws://host:8080/ws".into());
        menu.stage = Stage::Choosing { rooms: vec![a_room("arena")], note: None };
        menu.selected = Some(RoomId::from("arena"));

        assert!(
            probe(&mut menu, at(1.0, false), |_, chose| matches!(chose, Chose::Watch(_))),
            "a selected room could not be watched"
        );
    }

    /// **A browser must be able to reach its own server.** The refresh button
    /// lived inside the branch that draws the address as a *field*, and the
    /// web client has a label there instead — so it had no button, and with
    /// nothing else able to ask, the whole client was a dead end. The address
    /// is what differs between the two; asking is not.
    #[test]
    fn the_web_client_can_ask_its_server_and_so_can_a_native_one() {
        // `Menu::new` reads the remembered address on native, so this asserts
        // against the store as much as against the menu.
        let _store = crate::net::keep::lock_store();
        let empty = std::env::temp_dir().join(format!("ck-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        crate::net::keep::keep_in(empty.clone());

        for on_web in [true, false] {
            let mut menu = Menu::new("ws://origin:8080/ws".into(), on_web);
            menu.page = Page::Play;
            // What pressing Play arms: ask straight away rather than waiting
            // for somebody to touch a field they have no reason to touch.
            menu.typed_at = Some(0.0);

            let chose = one_frame(&mut menu, at(10.0, on_web));
            assert!(
                matches!(&chose, Chose::Connect(a) if a == "ws://origin:8080/ws"),
                "on_web={on_web} could not reach its server"
            );
        }
    }

    /// A refusal is asked about again on its own, so a server started a moment
    /// later is found without anybody having to notice a button.
    #[test]
    fn a_refused_address_is_asked_about_again() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), false);
        menu.page = Page::Play;
        menu.stage = Stage::Failed("no answer".into());
        menu.attempted = Some("ws://host:8080/ws".into());

        // The refusal has just arrived: `failed_at` is unset, and the retry
        // starts from here rather than never — which is the bug this replaces.
        let chose = one_frame(&mut menu, at(100.0, false));
        assert!(matches!(chose, Chose::Connect(_)), "a refusal was never retried");
        assert_eq!(menu.failed_at, Some(100.0), "and the cadence started");

        // Not again immediately: a client hammering a port nothing is
        // listening on is a log full of nothing.
        menu.stage = Stage::Failed("no answer".into());
        menu.attempted = Some("ws://host:8080/ws".into());
        assert!(matches!(one_frame(&mut menu, at(101.0, false)), Chose::Nothing));

        // And again once the interval has passed.
        menu.stage = Stage::Failed("no answer".into());
        menu.attempted = Some("ws://host:8080/ws".into());
        let chose = one_frame(&mut menu, at(100.0 + RETRY_EVERY + 0.1, false));
        assert!(matches!(chose, Chose::Connect(_)), "the cadence stopped");
    }

    /// The example fills a blank field **once, on the way in**. Doing it while
    /// the field is drawn made the field impossible to clear: select all,
    /// press delete, and the example was back before the next keystroke.
    #[test]
    fn an_emptied_address_field_stays_empty_while_it_is_being_edited() {
        let mut menu = Menu::new("ws://origin:8080/ws".into(), true);

        // What entering the Play page does.
        let enter = |menu: &mut Menu| {
            if menu.address.trim().is_empty() {
                menu.address = "ws://127.0.0.1:8080/ws".to_string();
            }
        };
        menu.address.clear();
        enter(&mut menu);
        assert_eq!(menu.address, "ws://127.0.0.1:8080/ws", "a blank field is filled on the way in");

        // And then somebody clears it to type their own. Nothing on the draw
        // path may put it back.
        menu.address.clear();
        assert!(menu.address.is_empty(), "the field refilled itself under the cursor");
    }

    /// A field that is never blank, because a hint is a shape and this is a
    /// thing you can press enter on.
    #[test]
    fn the_address_field_offers_an_example_rather_than_a_hint() {
        #[cfg(target_arch = "wasm32")]
        let example = "ws://127.0.0.1:8080/ws";
        #[cfg(not(target_arch = "wasm32"))]
        let example = crate::client::views::game::default_address();
        assert!(example.starts_with("ws://"), "{example}");
        assert!(example.contains(':'), "an example needs a port to edit: {example}");

        // What `server_field` does when somebody clears it.
        let mut address = "   ".to_string();
        if address.trim().is_empty() {
            address = example.to_string();
        }
        assert_eq!(address, example);
    }

    /// Arrow keys walk the list and wrap at both ends, which is what every
    /// list does and what a keyboard user reaches for first.
    #[test]
    fn the_arrow_keys_walk_the_room_list_and_wrap() {
        // The arithmetic the key handler does, in the one place it is worth
        // pinning: the wrap, and what an unselected list does on first press.
        let step = |at: Option<usize>, step: i32, len: usize| -> usize {
            match at {
                None if step > 0 => 0,
                None => len - 1,
                Some(i) => (i as i32 + step).rem_euclid(len as i32) as usize,
            }
        };
        assert_eq!(step(None, 1, 3), 0, "down from nothing takes the first");
        assert_eq!(step(None, -1, 3), 2, "and up takes the last");
        assert_eq!(step(Some(0), 1, 3), 1);
        assert_eq!(step(Some(2), 1, 3), 0, "down off the end wraps");
        assert_eq!(step(Some(0), -1, 3), 2, "and up off the front wraps");
        assert_eq!(step(Some(0), 1, 1), 0, "a list of one goes nowhere");
    }

    /// The room list replaces `Stage::Choosing` wholesale every few seconds.
    /// A draft living inside it would be wiped mid-typing, which is why it
    /// lives on the menu instead.
    #[test]
    fn a_refreshed_room_list_does_not_wipe_a_half_typed_form() {
        let mut menu = Menu::new("ws://host:8080/ws".into(), true);
        menu.draft = Some(Draft { name: "half-ty".into(), ..Draft::default() });
        menu.stage = Stage::Choosing { rooms: Vec::new(), note: None };

        // What `pump_link` does when a fresh listing lands.
        menu.stage = Stage::Choosing { rooms: Vec::new(), note: Some("something".into()) };

        assert_eq!(menu.draft.as_ref().map(|d| d.name.as_str()), Some("half-ty"));
    }

    /// On the web the socket is derived from the page's origin, so a
    /// remembered address from another visit would point the client away from
    /// the machine that served it.
    #[test]
    fn the_web_menu_does_not_remember_an_address() {
        // The store is one per process and these tests run in parallel: taken
        // before anything touches it, or two tests point it at two
        // directories and neither tests what it thinks.
        let _store = crate::net::keep::lock_store();
        let dir = std::env::temp_dir().join(format!("ck-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::net::keep::keep_in(dir.clone());
        crate::net::keep::remember_server("ws://elsewhere:9999/ws");

        let web = Menu::new("ws://origin:8080/ws".into(), true);
        assert_eq!(web.address, "ws://origin:8080/ws", "the page's own origin wins");

        let native = Menu::new("ws://127.0.0.1:8080/ws".into(), false);
        assert_eq!(native.address, "ws://elsewhere:9999/ws", "and what was typed last");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
