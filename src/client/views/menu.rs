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
    /// A server, its rooms, a code, and the form that makes one.
    Play,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    Never,
    Timer,
    Territory,
}

/// Whether a match is played in sides. Only a match can be: a team is a way of
/// deciding a result, and a world has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Together {
    Solo,
    Teams,
}

/// Whether the ground stops. Two answers, so a row of buttons rather than a
/// list to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Boundless,
    Wrapping,
}

/// A world being described, before it exists.
///
/// Everything here is what was **typed**, including the numbers: a size and a
/// target are held as text so that a field half-way through being corrected is
/// a field with something wrong in it rather than one that snaps back to a
/// number every keystroke. [`Self::parse`] is where typed becomes chosen.
pub struct Draft {
    pub name: String,
    pub shape: Shape,
    /// How many chunks tall and wide, read only when the shape is wrapping.
    ///
    /// Two fields rather than one `ROWSxCOLS` string, because a size is two
    /// numbers and typing it as one was asking the player to learn a format
    /// in order to answer a question they already understood. It also puts the
    /// error where it belongs: a rows field that will not parse is a wrong
    /// number in a labelled box, not a whole size that "is not a size".
    pub rows: String,
    pub cols: String,
    pub ends: Ends,
    /// Generations or squares, read only when it ends.
    pub target: String,
    /// Why the last attempt was refused — by this form, or by the server.
    pub note: Option<String>,
    /// Sent, and waiting for an answer. The form stays on screen while it is
    /// true, because a refusal has to arrive back into something.
    pub asking: bool,
    /// Free-for-all, or sides. Read only when the match ends somehow, because
    /// a world has no result for a side to win.
    pub together: Together,
    /// How many sides, as typed. Read only in a team match.
    pub team_count: String,
    /// Kept out of the listing and reached by a code the server generates.
    ///
    /// The name field is ignored when this is set, and the form says so — a
    /// field that is being quietly discarded is worse than one that is not
    /// there.
    pub private: bool,
}

impl Default for Draft {
    fn default() -> Self {
        let (rows, cols) = crate::sim::DEFAULT_TORUS;
        Self {
            name: String::new(),
            shape: Shape::Boundless,
            rows: rows.to_string(),
            cols: cols.to_string(),
            ends: Ends::Never,
            target: crate::net::DEFAULT_TIMER.to_string(),
            together: Together::Solo,
            team_count: crate::net::MIN_TEAMS.to_string(),
            note: None,
            asking: false,
            private: false,
        }
    }
}

impl Draft {
    /// What was typed, as what was chosen — or the first thing wrong with it.
    ///
    /// Checked here as well as on the server, and that is not duplication for
    /// its own sake: `net::room_name` exists to be callable from both sides so
    /// that a bad name is a message beside the field rather than a round trip
    /// that comes back refused. The server checks anyway, because nothing a
    /// client says about a filename is trusted.
    ///
    /// A field that does not apply is not read. A size typed while boundless
    /// is selected, or a target typed and then switched to "never", is
    /// somebody changing their mind — refusing on it would be refusing a
    /// number nobody is asking to use.
    pub fn parse(&self) -> Result<(RoomName, WorldKind, Option<Victory>, Option<u8>), String> {
        // A private room's name is the code the server generates, so there is
        // nothing here to check and nothing to refuse.
        let name = if self.private { String::new() } else { crate::net::room_name(&self.name)? };
        let shape = match self.shape {
            Shape::Boundless => WorldKind::Infinite,
            Shape::Wrapping => WorldKind::Toroidal {
                rows: chunks(&self.rows, words::make::ROWS)?,
                cols: chunks(&self.cols, words::make::COLS)?,
            },
        };
        let victory = match self.ends {
            Ends::Never => None,
            Ends::Timer => Some(Victory::Timer { generations: self.number()? }),
            Ends::Territory => Some(Victory::Territory { squares: self.number()? as usize }),
        };
        // Sides only on a match, and only when asked for. A world with teams
        // is a world with a field nobody could ever read.
        let teams = match (victory, self.together) {
            (Some(_), Together::Teams) => Some(self.sides()?),
            _ => None,
        };
        Ok((name, shape, victory, teams))
    }

    /// How many sides, or what is wrong with the number.
    fn sides(&self) -> Result<u8, String> {
        let text = self.team_count.trim();
        match text.parse::<u8>() {
            Ok(n) if (crate::net::MIN_TEAMS..=crate::net::MAX_TEAMS).contains(&n) => Ok(n),
            Ok(_) => Err(words::make::sides_range(crate::net::MIN_TEAMS, crate::net::MAX_TEAMS)),
            Err(_) => Err(words::make::not_a_number_for(words::make::SIDES, text)),
        }
    }

    fn number(&self) -> Result<u64, String> {
        let text = self.target.trim();
        match text.parse::<u64>() {
            Ok(0) | Err(_) => Err(words::make::not_a_number(text)),
            Ok(n) => Ok(n),
        }
    }

    /// The number that belongs beside the condition now selected. Swapped when
    /// the condition is, because two thousand generations and two thousand
    /// squares are not the same order of thing and carrying one across reads
    /// as the form having kept the wrong number.
    fn retarget(&mut self, ends: Ends) {
        if self.ends == ends {
            return;
        }
        self.ends = ends;
        self.target = match ends {
            Ends::Never => return,
            Ends::Timer => crate::net::DEFAULT_TIMER.to_string(),
            Ends::Territory => crate::net::DEFAULT_TERRITORY.to_string(),
        };
    }
}

/// What the player chose this frame, if anything.
pub enum Chose {
    Nothing,
    /// Ask the server what rooms it has, again. The list refreshes itself
    /// every few seconds; this is for the moment somebody has just made a room
    /// on the other screen and does not want to wait out the interval.
    Refresh,
    /// Play with no server at all. The simulation is deterministic, so this is
    /// a whole game rather than a broken one — just a solitary one.
    Offline,
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
            draft: None,
        }
    }
}

/// Draw it, and say what was chosen. Returns the rectangle it covered, so the
/// world behind it does not also take the click.
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

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    menu: &mut Menu,
    at: Where,
) -> (Chose, Option<egui::Rect>) {
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
    let area = egui::Area::new("menu".into())
        .fixed_pos(screen.min)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            egui::Frame::new().fill(theme.palette.surface).show(ui, |ui| {
                ui.set_min_size(screen.size());
                // Scrolled, because filling the screen does not make the
                // screen taller: a server with a dozen rooms and the form
                // beside them still runs off the bottom of a laptop, and
                // content that cannot be reached is worse than content that
                // looks cramped.
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
                            }
                        });
                        ui.add_space(m.margin * 2.0);
                    });
                });
            });
        });

    (chose, Some(area.response.rect))
}

/// Who you are, what you have done, and the way in.
///
/// **One accent-coloured control**, and it is Play. Everything else on this
/// screen is something you read rather than something you press, which is the
/// hierarchy Clash Royale gets right: one thing you are meant to do next, in
/// one colour, and no second thing competing to be it.
fn home(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    ui.heading(words::TITLE);
    ui.add_space(m.item_spacing * 2.0);

    // The name lives here rather than on the play screen, because it is who
    // you are and not part of choosing a world -- and because it is the same
    // answer whichever world you end up in.
    ui.label(egui::RichText::new(words::home::WHO).size(m.text_small));
    ui.add(
        egui::TextEdit::singleline(&mut menu.name)
            .desired_width(f32::INFINITY)
            .hint_text(words::NAME_HINT),
    );

    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(words::home::RECORD).size(m.text_small));
    super::record::show(ui, theme, &menu.games, &menu.record);

    ui.add_space(m.item_spacing * 2.0);
    if ui
        .add_sized(
            [ui.available_width(), m.action_height],
            egui::Button::new(
                egui::RichText::new(words::home::PLAY).size(m.text_action).color(p.ground),
            )
            .fill(p.accent),
        )
        .clicked()
    {
        menu.page = Page::Play;
        // Never blank, and filled in **here** rather than while the field is
        // drawn. Refilling an empty field every frame is a field that cannot
        // be cleared: select all, press delete, and the example is back before
        // the next keystroke. Once, on the way in, is the whole of what was
        // wanted.
        #[cfg(not(target_arch = "wasm32"))]
        if menu.address.trim().is_empty() {
            menu.address = crate::client::views::battle::default_address().to_string();
        }
        // Ask straight away rather than waiting for somebody to touch a field
        // they have no reason to touch: the address is remembered, or it is an
        // example, and either way the question is the same one.
        menu.typed_at = Some(0.0);
        menu.attempted = None;
    }

    // Offline sits here because it is a way to play, and because a player with
    // no server to reach should not have to walk through a screen about
    // servers to get to it.
    //
    // **Except when you are already enrolled in a match**, where the same
    // press means the opposite: you left a lobby to look at this screen, and
    // starting a solitary game is never what pressing the only other button
    // meant. It becomes the way back in.
    ui.add_space(m.item_spacing);
    let (label, note) = if at.waiting_in_a_match {
        (words::BACK_TO_MATCH, Some(words::BACK_TO_MATCH_NOTE))
    } else {
        // No note. "The rules are the same offline" was answering a question
        // nobody asks standing in front of a button that says Play alone.
        (words::ALONE, None)
    };
    if ui
        .add_sized(
            [ui.available_width(), m.button_height],
            egui::Button::new(egui::RichText::new(label).size(m.text_body)),
        )
        .clicked()
    {
        chose = if at.waiting_in_a_match { Chose::Resume } else { Chose::Offline };
    }
    if let Some(note) = note {
        ui.small(note);
    }

    chose
}

/// A server, what is on it, and a form for what is not.
///
/// **Two columns, and the split says something true**: on the left is what
/// already exists — a list the server owns, which changes every few seconds
/// whether or not you touch it — and on the right is what does not exist yet,
/// which is a form, and yours, and stays exactly where you left it. They are
/// not two panels of the same kind, so they are not drawn as two panels of the
/// same kind: the list sits on the panel's own ground and the form is a card.
///
/// One accent per **column**, not per screen. Each column has exactly one
/// thing you would do next in it — join the world you picked, or make the one
/// you described — and they are in different places, so neither is competing
/// to be the one thing.
///
/// Stacked below [`Metrics::two_column_min`], because two columns of form on a
/// phone is two columns of nothing.
fn play(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    ui.horizontal(|ui| {
        // Every screen has a way out of it, by pointer as well as by escape.
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
        ui.heading(words::home::PLAY);
    });
    ui.add_space(m.item_spacing);

    // **Both columns, whatever the server has said.** Gating them on a room
    // list having arrived meant that a server which refused once left no way
    // to make a world either — which is not a consequence anybody would choose
    // and is exactly what it looked like from the outside: a screen with
    // nothing on it.
    //
    // The list is empty until there is one; the form is a form either way, and
    // says what it is waiting for at the point of pressing rather than by
    // being absent.
    let (rooms, note) = match &menu.stage {
        Stage::Choosing { rooms, note } => (rooms.clone(), note.clone()),
        _ => (Vec::new(), None),
    };
    let reached = matches!(menu.stage, Stage::Choosing { .. });

    if let Some(reach) = server_field(ui, theme, menu, at, reached) {
        chose = reach;
    }
    ui.add_space(m.item_spacing);

    ui.add_space(m.item_spacing * 2.0);
    if let Some(note) = note {
        ui.colored_label(p.bad, note);
        ui.add_space(m.item_spacing);
    }

    // Two columns where there is room for two, one where there is not.
    if ui.available_width() >= m.two_column_min {
        ui.columns(2, |cols| {
            if let Some(what) = rooms_column(&mut cols[0], theme, menu, &rooms, reached) {
                chose = what;
            }
            if let Some(what) = make_column(&mut cols[1], theme, menu, reached) {
                chose = what;
            }
        });
    } else {
        if let Some(what) = rooms_column(ui, theme, menu, &rooms, reached) {
            chose = what;
        }
        ui.add_space(m.item_spacing * 2.0);
        if let Some(what) = make_column(ui, theme, menu, reached) {
            chose = what;
        }
    }

    chose
}

/// Where to connect, and the reaching itself.
///
/// **There is no button.** Asking a server what it has is not a decision worth
/// a press — it is what the address is *for*, and a field followed by a button
/// that only ever means "yes, that address" is one control too many. So this
/// reaches when the typing settles: on enter, on leaving the field, or after
/// [`SETTLE`] of nothing being typed, whichever comes first.
///
/// Debounced rather than fired per keystroke, because `ws://127.0.0.1:8080/ws`
/// passes through twenty addresses on its way to being one, and every one of
/// them would open a socket.
fn server_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    menu: &mut Menu,
    at: Where,
    reached: bool,
) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut ask = false;
    let mut refresh = false;
    let mut typed = false;

    ui.label(egui::RichText::new(words::SERVER).size(m.text_small));

    ui.horizontal(|ui| {
        // **The button comes first and belongs to both clients.** It used to
        // be drawn after the address, inside the branch that makes the address
        // a field — so a browser, whose socket comes from the page it was
        // served by and which therefore has a label rather than a field, got
        // no button at all and had no way to reach anything. The address is
        // what differs between the two; asking is not.
        //
        // One control rather than two: reaching a server for the first time
        // and asking it again are the same act from where a player stands —
        // tell me what is on there, now — so the meaning follows the state and
        // the hover text says which it is.
        let go = ui
            .add_sized(
                [m.button_height, m.button_height],
                egui::Button::new(egui::RichText::new(words::REFRESH).size(m.text_action)),
            )
            .on_hover_text(if reached { words::REFRESH_AGAIN } else { words::REFRESH_ASK });

        let entered = if at.on_web {
            // Not a field. The socket is derived from the page's origin, so a
            // typed address here would be a promise the client cannot keep.
            ui.colored_label(p.text_dim, &menu.address);
            false
        } else {
            let field = ui.add_sized(
                [ui.available_width(), m.button_height],
                egui::TextEdit::singleline(&mut menu.address).hint_text(words::SERVER_HINT),
            );
            if field.changed() {
                typed = true;
                menu.typed_at = Some(at.now);
                // What is on screen is about an address that is no longer in
                // the field, so it goes rather than contradicting it.
                if !matches!(menu.stage, Stage::Asking) {
                    menu.stage = Stage::Idle;
                }
            }
            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };

        // The button and enter are **deliberate**, so they ask whatever the
        // address is and whatever was asked before. Only the settle is
        // guarded, and only against asking twice about one address with
        // nobody having done anything.
        if go.clicked() || entered {
            // Already connected, and the address has not moved: this is "say
            // that again", which is one small message rather than a new socket.
            refresh = reached && !typed;
            menu.attempted = None;
            ask = true;
        } else if menu.typed_at.is_some_and(|t| at.now - t >= SETTLE) {
            ask = true;
        }
    });

    match &menu.stage {
        Stage::Asking => {
            ui.colored_label(p.text_dim, egui::RichText::new(words::ASKING).size(m.text_small));
        }
        Stage::Choosing { .. } => {
            ui.colored_label(p.good, egui::RichText::new(words::REACHED).size(m.text_small));
        }
        Stage::Failed(why) => {
            ui.colored_label(p.bad, egui::RichText::new(why).size(m.text_small));
            // Asked again on its own, on a slow cadence: the usual reason a
            // server does not answer is that it is not running *yet*, and a
            // menu that gives up after one refusal makes that something you
            // have to notice.
            //
            // `failed_at` is stamped when the refusal is on screen and not
            // inside a retry: set only there, it stayed `None` until something
            // had retried, so nothing ever did.
            if menu.failed_at.is_none_or(|t| at.now - t >= RETRY_EVERY) {
                menu.failed_at = Some(at.now);
                menu.attempted = None;
                ask = true;
            }
        }
        // Nothing said. The field has just been typed into and the answer is a
        // fraction of a second away; a line that appeared and vanished between
        // two keystrokes would be noise.
        Stage::Idle => {}
    }

    if refresh {
        return Some(Chose::Refresh);
    }
    if !ask {
        return None;
    }
    menu.typed_at = None;

    // Once per address, for the settle only — a press cleared this above,
    // because a press is somebody asking again on purpose.
    let address = menu.address.trim().to_string();
    if address.is_empty() || menu.attempted.as_deref() == Some(address.as_str()) {
        return None;
    }
    menu.attempted = Some(address.clone());
    Some(Chose::Connect(address))
}

/// What is already here: a list the server owns.
fn rooms_column(
    ui: &mut egui::Ui,
    theme: &Theme,
    menu: &mut Menu,
    rooms: &[RoomInfo],
    // Whether the server has answered, which is a different thing from having
    // answered with nothing: one is a pause and the other is an invitation.
    reached: bool,
) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = None;

    // No refresh here. It is beside the address, which is the same act from
    // where the player stands and was two buttons for one thing.
    ui.label(egui::RichText::new(words::ROOMS).size(m.text_small));

    if !reached {
        // No answer yet, which is a different thing from a server with nothing
        // on it and reads differently: one is waiting, the other is an
        // invitation.
        ui.colored_label(p.text_dim, egui::RichText::new(words::NOT_ASKED).size(m.text_body));
    } else if rooms.is_empty() {
        // An invitation rather than a complaint: there is a form in the next
        // column and this is the moment to point at it.
        ui.colored_label(p.text_dim, egui::RichText::new(words::NO_ROOMS).size(m.text_body));
    } else {
        // Arrow keys walk the list, and enter takes the selection. A list you
        // can only reach with a pointer is a list a keyboard cannot use.
        //
        // Read before the rows are drawn so a press moves the selection in the
        // same frame it happens, rather than a frame behind the eye.
        if !ui.memory(|mem| mem.focused().is_some()) {
            let step = ui.input(|i| {
                i.key_pressed(egui::Key::ArrowDown) as i32
                    - i.key_pressed(egui::Key::ArrowUp) as i32
            });
            if step != 0 {
                let at =
                    menu.selected.as_ref().and_then(|id| rooms.iter().position(|r| r.id == *id));
                let next = match at {
                    // Nothing picked yet: down takes the first, up the last,
                    // which is what every list does.
                    None if step > 0 => 0,
                    None => rooms.len() - 1,
                    Some(i) => (i as i32 + step).rem_euclid(rooms.len() as i32) as usize,
                };
                menu.selected = Some(rooms[next].id.clone());
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(id) = menu.selected.clone() {
                    chose = Some(Chose::Join(id));
                }
            }
        }

        for room in rooms {
            let selected = menu.selected.as_ref() == Some(&room.id);
            match room_row(ui, theme, room, selected) {
                Picked::Nothing => {}
                // Selecting the one already selected puts it away, so a press
                // has somewhere to go back to.
                Picked::Select => {
                    menu.selected = if selected { None } else { Some(room.id.clone()) }
                }
                // The **id**, not the name on the row: two rooms may read
                // alike and only one of them was pressed.
                Picked::Join => chose = Some(Chose::Join(room.id.clone())),
                Picked::Watch => chose = Some(Chose::Watch(room.id.clone())),
            }
        }
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(words::rooms_here(
                rooms.len(),
                rooms.iter().map(|r| r.players).sum(),
            ))
            .size(m.text_small),
        );
    }

    // A code, under the list rather than instead of it: the list is how you
    // find a public world and a code is how you reach somebody's private one.
    // Two ways into what already exists, which is what this column is.
    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(words::code::LABEL).size(m.text_small));
    ui.horizontal(|ui| {
        let go = ui.add_sized(
            [m.action_height * 1.4, m.button_height],
            egui::Button::new(egui::RichText::new(words::code::GO).size(m.text_small)),
        );
        let field = ui.add_sized(
            [ui.available_width(), m.button_height],
            egui::TextEdit::singleline(&mut menu.code).hint_text(words::code::HINT),
        );
        // Return submits, because a six-character field is one you type and
        // press enter on without looking for a button.
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (go.clicked() || entered) && !menu.code.trim().is_empty() {
            // A code reaches a room the same way an id does — the server
            // resolves an id, then a name, then a code — so the client needs
            // no second message for it.
            chose = Some(Chose::Join(RoomId(menu.code.trim().to_string())));
        }
    });

    chose
}

/// What does not exist yet: a form, and yours.
///
/// Always here rather than behind a press. It had to be opened when it lived
/// under the list, because a form there pushed the list off the screen; in a
/// column of its own there is nothing to push, and a button whose only job is
/// to reveal what would fit anyway is a press that buys nothing.
fn make_column(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, reached: bool) -> Option<Chose> {
    let draft = menu.draft.get_or_insert_with(Draft::default);
    let made = make_form(ui, theme, draft, reached);
    // Refused here rather than by the form being missing: somebody who has
    // filled it in should be told what it is waiting for, not left wondering
    // where it went.
    if made.is_some() && !reached {
        draft.asking = false;
        draft.note = Some(words::make::NO_SERVER.into());
        return None;
    }
    made
}

/// What a room row was clicked for. Two things can be done with a room, so a
/// bool would have to be a bool about which.
enum Picked {
    Nothing,
    /// Point at this room, so its actions appear inside it.
    Select,
    Join,
    Watch,
}

fn make_form(ui: &mut egui::Ui, theme: &Theme, draft: &mut Draft, reached: bool) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = None;

    egui::Frame::new()
        .fill(p.surface_lift)
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(words::make::TITLE).size(m.text_body));
            ui.add_space(m.item_spacing);

            // Not asked for on a private room, whose name is the code the
            // server generates. A field being quietly discarded is worse than
            // one that is not there — the same rule that hides the size on a
            // boundless world.
            if !draft.private {
                ui.label(egui::RichText::new(words::make::NAME).size(m.text_small));
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .desired_width(f32::INFINITY)
                        .hint_text(words::make::NAME_HINT),
                );
                ui.add_space(m.item_spacing);
            }

            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(words::make::SHAPE).size(m.text_small));
            // **The size lives inside the Wrapping button.** As its own row it
            // pushed everything under it down the moment the shape changed, so
            // choosing a shape moved the button you were about to press next —
            // and on a small screen it pushed the action off the bottom. The
            // row grows in place instead: the option that has a size is the
            // one that holds it.
            shape_row(ui, theme, draft);

            // Sides, and only on a match: a team is a way of deciding a
            // result and a world has none, so this row appears with the same
            // rule every other conditional row here follows.
            if draft.ends != Ends::Never {
                ui.add_space(m.item_spacing);
                ui.label(egui::RichText::new(words::make::TOGETHER).size(m.text_small));
                let mut together = draft.together;
                toggles(
                    ui,
                    theme,
                    &mut together,
                    &[(Together::Solo, words::make::SOLO), (Together::Teams, words::make::TEAMS)],
                );
                draft.together = together;
                if draft.together == Together::Teams {
                    ui.add_space(m.item_spacing);
                    ui.horizontal_top(|ui| {
                        ui.set_min_height(m.button_height);
                        ui.colored_label(
                            p.text_dim,
                            egui::RichText::new(words::make::SIDES).size(m.text_small),
                        );
                        ui.add_sized(
                            [m.action_height * 1.6, m.button_height],
                            egui::TextEdit::singleline(&mut draft.team_count)
                                .horizontal_align(egui::Align::Center),
                        );
                    });
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(words::make::SIDES_NOTE).size(m.text_small),
                    );
                }
            }

            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(words::make::PRIVATE).size(m.text_small));
            let mut private = draft.private;
            toggles(
                ui,
                theme,
                &mut private,
                &[(false, words::make::LISTED), (true, words::make::UNLISTED)],
            );
            draft.private = private;
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(if draft.private {
                    words::make::UNLISTED_NOTE
                } else {
                    words::make::LISTED_NOTE
                })
                .size(m.text_small),
            );

            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(words::make::ENDS).size(m.text_small));
            let mut ends = draft.ends;
            toggles(
                ui,
                theme,
                &mut ends,
                &[
                    (Ends::Never, words::make::NEVER),
                    (Ends::Timer, words::make::TIMER),
                    (Ends::Territory, words::make::TERRITORY),
                ],
            );
            draft.retarget(ends);

            // What the choice above actually means, in a line. The three
            // words on the buttons are short enough to be guessed wrong.
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(match draft.ends {
                    Ends::Never => words::make::NEVER_NOTE,
                    Ends::Timer => words::make::TIMER_NOTE,
                    Ends::Territory => words::make::TERRITORY_NOTE,
                })
                .size(m.text_small),
            );

            if draft.ends != Ends::Never {
                ui.add_space(m.item_spacing);
                ui.label(
                    egui::RichText::new(match draft.ends {
                        Ends::Territory => words::make::SQUARES,
                        _ => words::make::GENERATIONS,
                    })
                    .size(m.text_small),
                );
                ui.add(egui::TextEdit::singleline(&mut draft.target).desired_width(f32::INFINITY));
                ui.colored_label(
                    p.warn,
                    egui::RichText::new(words::make::MATCH_WAITS).size(m.text_small),
                );
            }

            if let Some(note) = &draft.note {
                ui.add_space(m.item_spacing);
                ui.colored_label(p.bad, egui::RichText::new(note).size(m.text_small));
            }

            ui.add_space(m.item_spacing * 2.0);
            if draft.asking {
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(words::make::MAKING).size(m.text_small),
                );
            } else if ui
                .add_sized(
                    [ui.available_width(), m.action_height],
                    egui::Button::new(
                        egui::RichText::new(words::make::MAKE)
                            .size(m.text_action)
                            // The accent is for the thing you are meant to
                            // press next, and until a server has answered that
                            // is not this one.
                            .color(if reached { p.ground } else { p.text }),
                    )
                    .fill(if reached { p.accent } else { p.surface }),
                )
                .clicked()
            {
                // Refused here or refused there, into the same line under the
                // same form: a name that is too long and a name already taken
                // are the same kind of answer to the player.
                match draft.parse() {
                    Ok((name, shape, victory, teams)) => {
                        draft.note = None;
                        draft.asking = true;
                        chose = Some(Chose::Create {
                            name,
                            shape,
                            victory,
                            teams,
                            private: draft.private,
                        });
                    }
                    Err(why) => draft.note = Some(why),
                }
            }
            ui.add_space(m.item_spacing);
            if ui
                .add_sized(
                    [ui.available_width(), m.button_height],
                    egui::Button::new(egui::RichText::new(words::make::CLEAR).size(m.text_small)),
                )
                .clicked()
            {
                chose = Some(Chose::Clear);
            }
        });

    chose
}

/// One side of a wrapping world, in chunks, or what is wrong with it.
///
/// Named in the error, because with two fields "that is not a number" would
/// not say which one. Bounded above as well as below: a torus is allocated
/// whole, so a thousand by a thousand is not a slow world, it is a client that
/// asks its own machine for sixteen gigabytes and stops.
fn chunks(text: &str, which: &str) -> Result<i32, String> {
    let text = text.trim();
    match text.parse::<i32>() {
        Ok(n) if (1..=MAX_CHUNKS).contains(&n) => Ok(n),
        Ok(_) => Err(words::make::out_of_range(which, MAX_CHUNKS)),
        Err(_) => Err(words::make::not_a_number_for(which, text)),
    }
}

/// The largest a wrapping world may be asked for, per side, in chunks.
///
/// A torus is allocated whole rather than growing into what is used, so this
/// is a real memory figure and not a preference: at sixty-four, a side is a
/// thousand cells and the world is about a megabyte of cells, which is
/// nothing. It is here to stop a typo asking for a world that will not fit,
/// not to say what makes a good arena.
pub const MAX_CHUNKS: i32 = 64;

/// Shape, with the size inside the option it belongs to.
///
/// Two cells side by side. Boundless is a plain toggle; Wrapping is a toggle
/// with two number fields under it, which appear only when it is the one
/// chosen — so the **row** grows and the form does not, and nothing below
/// moves when the shape changes.
fn shape_row(ui: &mut egui::Ui, theme: &Theme, draft: &mut Draft) {
    let p = theme.palette;
    let m = theme.metrics;
    let wrapping = draft.shape == Shape::Wrapping;
    // Tall enough for the fields when they are there, and the same height
    // either way so that choosing does not move the row's own bottom edge.
    let tall = m.button_height * 2.0 + m.item_spacing * 2.0 + m.text_small;

    ui.horizontal_top(|ui| {
        ui.set_min_height(tall);
        let each = (ui.available_width() - m.item_spacing) / 2.0;

        ui.vertical(|ui| {
            ui.set_width(each);
            if ui
                .add_sized(
                    [each, m.button_height],
                    toggle(theme, words::make::BOUNDLESS, !wrapping),
                )
                .clicked()
            {
                draft.shape = Shape::Boundless;
            }
        });

        ui.vertical(|ui| {
            ui.set_width(each);
            if ui
                .add_sized([each, m.button_height], toggle(theme, words::make::WRAPPING, wrapping))
                .clicked()
            {
                draft.shape = Shape::Wrapping;
            }
            if wrapping {
                ui.add_space(m.item_spacing);
                ui.horizontal_top(|ui| {
                    let half = (ui.available_width() - m.item_spacing) / 2.0;
                    for (label, field) in
                        [(words::make::ROWS, &mut draft.rows), (words::make::COLS, &mut draft.cols)]
                    {
                        ui.vertical(|ui| {
                            ui.set_width(half);
                            ui.colored_label(
                                p.text_dim,
                                egui::RichText::new(label).size(m.text_small),
                            );
                            ui.add(
                                egui::TextEdit::singleline(field)
                                    .desired_width(f32::INFINITY)
                                    .horizontal_align(egui::Align::Center),
                            );
                        });
                    }
                });
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(words::make::SIZE_NOTE).size(m.text_small),
                );
            }
        });
    });
}

/// One option in a toggle row: the chosen one wears the accent.
fn toggle(theme: &Theme, label: &str, on: bool) -> egui::Button<'static> {
    let p = theme.palette;
    egui::Button::new(
        egui::RichText::new(label.to_string()).size(theme.metrics.text_small).color(if on {
            p.ground
        } else {
            p.text
        }),
    )
    .fill(if on { p.accent } else { p.surface })
}

/// One decision as a row of buttons, the chosen one wearing the accent.
///
/// The whole choice on screen at once, which is the argument against a
/// drop-down for anything this narrow: two or three words fit, and a player
/// reading a form should not have to open something to find out what the
/// alternatives were.
fn toggles<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &mut T,
    options: &[(T, &str)],
) {
    let p = theme.palette;
    let m = theme.metrics;
    // Top-aligned with a stated height, because `ui.horizontal` centres every
    // item against a row height it does not know until the last one is
    // measured -- see docs/gotchas.md.
    ui.horizontal_top(|ui| {
        ui.set_min_height(m.button_height);
        let each = (ui.available_width() - m.item_spacing * (options.len() as f32 - 1.0))
            / options.len() as f32;
        for (option, label) in options {
            let on = *value == *option;
            let button =
                egui::Button::new(egui::RichText::new(*label).size(m.text_small).color(if on {
                    p.ground
                } else {
                    p.text
                }))
                .fill(if on { p.accent } else { p.surface });
            if ui.add_sized([each, m.button_height], button).clicked() {
                *value = *option;
            }
        }
    });
}

/// One room: what it is called, whether anybody is in it, and whether it ends.
/// One room: what it is called, whether anybody is in it, and whether it ends
/// — with the two things that can be done to it.
///
/// Watching is a small button rather than a second full-width row, because it
/// is the rarer of the two and a list where every entry is two equal choices
/// is a list twice as long to read. It is offered on every room and not only
/// on matches: **no late joining is a rule about players**, so a match already
/// running is exactly the room whose only way in is to watch.
/// One room in the list: what it is called, whether anybody is in it, whether
/// it ends — and, **if it is the one selected**, what can be done with it.
///
/// The actions live inside the selection rather than beside every row. A row
/// of buttons on every entry makes the list twice as tall and twice as busy to
/// read, and most of those buttons belong to rooms nobody is looking at. One
/// selection, and Join and Watch appear in it.
///
/// Watching is offered on **every** room and not only on matches, because
/// no late joining is a rule about players: a match already running is exactly
/// the room whose only way in is to watch.
fn room_row(ui: &mut egui::Ui, theme: &Theme, room: &RoomInfo, selected: bool) -> Picked {
    let p = theme.palette;
    let m = theme.metrics;
    let mut picked = Picked::Nothing;

    egui::Frame::new()
        .fill(if selected { p.surface_lift } else { p.surface })
        .stroke(egui::Stroke::new(1.0, if selected { p.accent } else { p.line }))
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding * 0.6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(m.row_height * 0.6);
                ui.label(egui::RichText::new(&room.name).size(m.text_body));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(
                        if room.players > 0 { p.good } else { p.text_dim },
                        egui::RichText::new(players(room.players)).size(m.text_small),
                    );
                });
            });

            // A room and a match are the same thing to everything else, so
            // this list is the one place the difference has to show — clicking
            // into a match that has already started only to be refused is a
            // worse way to find out.
            let mut under = describe(room.world);
            if let Some(victory) = room.victory {
                under = format!(
                    "{under} · {} · {}",
                    crate::client::views::words::phase(&room.phase),
                    crate::client::views::lobby::describe(victory)
                );
            }
            ui.colored_label(
                if matches!(room.phase, crate::net::MatchPhase::Gathering) {
                    p.good
                } else {
                    p.text_dim
                },
                egui::RichText::new(under).size(m.text_small),
            );

            // **Selection takes the name and the line under it, and stops
            // there.** It used to take the whole row and was registered
            // *after* the buttons below, which in an immediate-mode interface
            // puts it on top of them: a press on Join never reached Join, it
            // reached the row, and the row's answer to being pressed while
            // selected is to deselect. That is a Join button that visibly
            // depresses and puts the room away instead of entering it.
            //
            // Taken before the buttons exist, so there is nothing for it to
            // cover.
            let head = ui.min_rect();
            if ui.interact(head, ui.id().with(&room.id), egui::Sense::CLICK).clicked() {
                picked = Picked::Select;
            }

            if selected {
                ui.add_space(m.item_spacing);
                ui.horizontal_top(|ui| {
                    ui.set_min_height(m.button_height);
                    let each = (ui.available_width() - m.item_spacing) / 2.0;
                    if ui
                        .add_sized(
                            [each, m.button_height],
                            egui::Button::new(
                                egui::RichText::new(words::watch::JOIN)
                                    .size(m.text_small)
                                    .color(p.ground),
                            )
                            .fill(p.accent),
                        )
                        .clicked()
                    {
                        picked = Picked::Join;
                    }
                    if ui
                        .add_sized(
                            [each, m.button_height],
                            egui::Button::new(
                                egui::RichText::new(words::watch::WATCH).size(m.text_small),
                            ),
                        )
                        .clicked()
                    {
                        picked = Picked::Watch;
                    }
                });
            }
        });

    picked
}

/// How a world's shape reads to somebody choosing between two of them.
pub fn describe(world: WorldKind) -> String {
    match world {
        WorldKind::Infinite => "boundless".to_string(),
        WorldKind::Toroidal { rows, cols } => format!("{rows}×{cols} chunks, wrapping"),
    }
}

/// Players connected now. "1 player" rather than "1 players", because the menu
/// is the first thing anybody reads.
fn players(n: u32) -> String {
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
    #[test]
    fn a_draft_becomes_a_room_or_says_what_is_wrong() {
        let world = Draft { name: "  Arena ".into(), ..Draft::default() };
        assert_eq!(
            world.parse().unwrap(),
            ("arena".into(), WorldKind::Infinite, None, None),
            "trimmed and lowercased, the way the server will name it"
        );

        let torus = Draft {
            name: "ring".into(),
            shape: Shape::Wrapping,
            rows: "6".into(),
            cols: "8".into(),
            ..Draft::default()
        };
        assert_eq!(torus.parse().unwrap().1, WorldKind::Toroidal { rows: 6, cols: 8 });

        let cup = Draft {
            name: "cup".into(),
            ends: Ends::Territory,
            target: "500".into(),
            // Left behind by a change of mind, and never read, because the
            // shape is boundless.
            rows: "nonsense".into(),
            ..Draft::default()
        };
        assert_eq!(cup.parse().unwrap().2, Some(Victory::Territory { squares: 500 }));
        assert_eq!(cup.parse().unwrap().3, None, "a match is solo unless asked otherwise");

        // Sides, and only on a match: a world has no result for a side to win.
        let sided = Draft {
            name: "cup".into(),
            ends: Ends::Timer,
            together: Together::Teams,
            team_count: "3".into(),
            ..Draft::default()
        };
        assert_eq!(sided.parse().unwrap().3, Some(3));
        let world_with_sides =
            Draft { name: "hall".into(), together: Together::Teams, ..Draft::default() };
        assert_eq!(
            world_with_sides.parse().unwrap().3,
            None,
            "a world that never ends has no sides, whatever the toggle says"
        );
        let too_many = Draft {
            name: "cup".into(),
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
        let endless =
            Draft { name: "cup".into(), ends: Ends::Timer, target: "0".into(), ..Draft::default() };
        assert!(endless.parse().is_err(), "a match of zero is over already");
    }

    /// Two thousand generations and two thousand squares are not the same
    /// order of thing, so the number follows the condition rather than being
    /// carried across it.
    #[test]
    fn the_target_follows_the_win_condition() {
        let mut draft = Draft::default();
        assert_eq!(draft.ends, Ends::Never);

        draft.retarget(Ends::Timer);
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
        let (chose, rect) = show(ctx, &theme, menu, at);
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
            let (_, rect) = show(&ctx, &theme, &mut menu, at(0.0, false));
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
            let (_, rect) = show(&ctx, &theme, &mut menu, at(0.0, false));
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
                let point = egui::pos2(x, y);
                tick(menu, down(point));
                let (chose, _) = tick(menu, up(point));
                if answered(menu, &chose) {
                    return true;
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
        let (_, rect) = tick(&mut menu, Vec::new());
        let mut into = None;
        let mut y = rect.top();
        while y < rect.bottom() && into.is_none() {
            let point = egui::pos2(rect.center().x, y);
            tick(&mut menu, down(point));
            tick(&mut menu, up(point));
            if ctx.memory(|m| m.focused()).is_some() {
                into = Some(point);
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
        let example = crate::client::views::battle::default_address();
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
