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
    /// A code typed in to reach a room that is not in the listing.
    ///
    /// Beside the room list rather than instead of it: a code is how you reach
    /// somebody's private game, and the list is how you find a public one.
    /// They are two ways in, not two versions of one.
    pub code: String,
    /// What this client has played before, read once when the menu opens.
    ///
    /// Read once rather than every frame, because it comes out of
    /// `localStorage` or a file and neither wants touching sixty times a
    /// second for a number that changes when a game ends.
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
    pub fn parse(&self) -> Result<(RoomName, WorldKind, Option<Victory>), String> {
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
        Ok((name, shape, victory))
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
    /// Shut the form without making anything.
    Cancel,
    /// Make this world on the server already reached, then join it. Parsed
    /// rather than raw: what the player typed became what they chose in
    /// [`Draft::parse`], which is the menu's whole job.
    Create {
        name: RoomName,
        shape: WorldKind,
        victory: Option<Victory>,
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
            code: String::new(),
            record: crate::client::record::Summary::of(&crate::client::record::games()),
            draft: None,
        }
    }
}

/// Draw it, and say what was chosen. Returns the rectangle it covered, so the
/// world behind it does not also take the click.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    menu: &mut Menu,
    on_web: bool,
) -> (Chose, Option<egui::Rect>) {
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    let area = egui::Area::new("menu".into()).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
        ctx,
        |ui| {
            egui::Frame::new()
                .fill(theme.palette.surface)
                .stroke(egui::Stroke::new(1.0, theme.palette.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.6)
                .show(ui, |ui| {
                    // A fixed width, because a panel sized by its contents
                    // jumps every time the room list changes length and a menu
                    // that resizes under the pointer is one whose buttons move
                    // as you reach for them.
                    ui.set_width(m.panel_width);
                    ui.spacing_mut().item_spacing.y = m.item_spacing;
                    match menu.page {
                        Page::Home => chose = home(ui, theme, menu),
                        Page::Play => chose = play(ui, theme, menu, on_web),
                    }
                });
        },
    );

    (chose, Some(area.response.rect))
}

/// Who you are, what you have done, and the way in.
///
/// **One accent-coloured control**, and it is Play. Everything else on this
/// screen is something you read rather than something you press, which is the
/// hierarchy Clash Royale gets right: one thing you are meant to do next, in
/// one colour, and no second thing competing to be it.
fn home(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
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
    record(ui, theme, &menu.record);

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
    }

    // Offline sits here because it is a way to play, and because a player with
    // no server to reach should not have to walk through a screen about
    // servers to get to it.
    ui.add_space(m.item_spacing);
    if ui
        .add_sized(
            [ui.available_width(), m.button_height],
            egui::Button::new(egui::RichText::new(words::ALONE).size(m.text_body)),
        )
        .clicked()
    {
        chose = Chose::Offline;
    }
    ui.small(words::ALONE_NOTE);

    chose
}

/// What this client has played, in four lines it can read at a glance.
fn record(ui: &mut egui::Ui, theme: &Theme, summary: &crate::client::record::Summary) {
    let p = theme.palette;
    let m = theme.metrics;
    if !summary.any() {
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(words::home::NOTHING_YET).size(m.text_small),
        );
        return;
    }
    egui::Frame::new()
        .fill(p.surface_lift)
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(words::home::games(summary.games)).size(m.text_body));
            // Only if there has been one. "0 of 0 matches won" is a line about
            // nothing, and a home screen is not a form to be filled in.
            if summary.matches > 0 {
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(words::home::matches(summary.won, summary.matches))
                        .size(m.text_small),
                );
            }
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(words::home::best(summary.best)).size(m.text_small),
            );
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(words::home::generations(summary.generations))
                    .size(m.text_small),
            );
        });
}

/// A server, its rooms, a code, and the form that makes one.
fn play(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, on_web: bool) -> Chose {
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

    if on_web {
        // Not a field. The socket is derived from the page's origin, so a
        // typed address here would be a promise the client cannot keep.
        ui.label(egui::RichText::new(words::SERVER).size(m.text_small));
        ui.colored_label(p.text_dim, &menu.address);
    } else {
        ui.label(egui::RichText::new(words::SERVER).size(m.text_small));
        ui.add(
            egui::TextEdit::singleline(&mut menu.address)
                .desired_width(f32::INFINITY)
                .hint_text(words::SERVER_HINT),
        );
    }

    ui.add_space(m.item_spacing * 2.0);
    match &menu.stage {
        Stage::Idle | Stage::Failed(_) => {
            if let Stage::Failed(why) = &menu.stage {
                ui.colored_label(p.bad, why);
                ui.add_space(m.item_spacing);
            }
            if ui
                .add_sized(
                    [ui.available_width(), m.action_height],
                    egui::Button::new(
                        egui::RichText::new(words::LOOK).size(m.text_action).color(p.ground),
                    )
                    .fill(p.accent),
                )
                .clicked()
            {
                chose = Chose::Connect(menu.address.clone());
            }
        }
        Stage::Asking => {
            ui.colored_label(p.text_dim, words::ASKING);
        }
        Stage::Choosing { rooms, note } => {
            if let Some(note) = note {
                ui.colored_label(p.bad, note);
                ui.add_space(m.item_spacing);
            }
            if rooms.is_empty() {
                ui.colored_label(p.warn, words::NO_ROOMS);
            } else {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(words::ROOMS).size(m.text_small));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(words::REFRESH).clicked() {
                            chose = Chose::Refresh;
                        }
                    });
                });
                for room in rooms {
                    match room_button(ui, theme, room) {
                        Picked::Nothing => {}
                        // The **id**, not the name on the button: two rooms
                        // may read alike and only one of them is the one that
                        // was clicked.
                        Picked::Join => chose = Chose::Join(room.id.clone()),
                        Picked::Watch => chose = Chose::Watch(room.id.clone()),
                    }
                }
                // What is behind the list, before anybody reads the names --
                // generals.io puts the count on the way in for the same
                // reason.
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(words::rooms_here(
                        rooms.len(),
                        rooms.iter().map(|r| r.players).sum(),
                    ))
                    .size(m.text_small),
                );
            }

            // A code, beside the list rather than instead of it: the list is
            // how you find a public world and a code is how you reach
            // somebody's private one. Two ways in, not two versions of one.
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
                // Return submits, because a six-character field is one you
                // type and press enter on without looking for a button.
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (go.clicked() || entered) && !menu.code.trim().is_empty() {
                    // A code reaches a room the same way an id does -- the
                    // server resolves an id, then a name, then a code -- so
                    // the client does not need a second message for it.
                    chose = Chose::Join(RoomId(menu.code.trim().to_string()));
                }
            });

            ui.add_space(m.item_spacing);
            match &mut menu.draft {
                // Opening it is one press and no screen change: the form
                // appears where the button was. Depth is what a menu spends
                // first.
                None => {
                    if ui
                        .add_sized(
                            [ui.available_width(), m.button_height],
                            egui::Button::new(
                                egui::RichText::new(words::make::OPEN).size(m.text_body),
                            ),
                        )
                        .clicked()
                    {
                        menu.draft = Some(Draft::default());
                    }
                }
                Some(draft) => {
                    if let Some(made) = make_form(ui, theme, draft) {
                        chose = made;
                    }
                }
            }
        }
    }

    chose
}

/// What a room row was clicked for. Two things can be done with a room, so a
/// bool would have to be a bool about which.
enum Picked {
    Nothing,
    Join,
    Watch,
}

fn make_form(ui: &mut egui::Ui, theme: &Theme, draft: &mut Draft) -> Option<Chose> {
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
            toggles(
                ui,
                theme,
                &mut draft.shape,
                &[
                    (Shape::Boundless, words::make::BOUNDLESS),
                    (Shape::Wrapping, words::make::WRAPPING),
                ],
            );

            // Only where it means something. A size on a boundless world is
            // a field with no answer.
            if draft.shape == Shape::Wrapping {
                ui.add_space(m.item_spacing);
                ui.label(egui::RichText::new(words::make::SIZE).size(m.text_small));
                // Two labelled numbers rather than one `ROWSxCOLS` string. A
                // size *is* two numbers, and asking for it as one made the
                // player learn a format to answer a question they already
                // understood — and put the error on the whole size rather than
                // on the field with the wrong number in it.
                ui.horizontal_top(|ui| {
                    ui.set_min_height(m.button_height);
                    let each = (ui.available_width() - m.item_spacing) / 2.0;
                    for (label, field) in
                        [(words::make::ROWS, &mut draft.rows), (words::make::COLS, &mut draft.cols)]
                    {
                        ui.vertical(|ui| {
                            ui.set_width(each);
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
                        egui::RichText::new(words::make::MAKE).size(m.text_action).color(p.ground),
                    )
                    .fill(p.accent),
                )
                .clicked()
            {
                // Refused here or refused there, into the same line under the
                // same form: a name that is too long and a name already taken
                // are the same kind of answer to the player.
                match draft.parse() {
                    Ok((name, shape, victory)) => {
                        draft.note = None;
                        draft.asking = true;
                        chose =
                            Some(Chose::Create { name, shape, victory, private: draft.private });
                    }
                    Err(why) => draft.note = Some(why),
                }
            }
            ui.add_space(m.item_spacing);
            if ui
                .add_sized(
                    [ui.available_width(), m.button_height],
                    egui::Button::new(egui::RichText::new(words::make::CANCEL).size(m.text_small)),
                )
                .clicked()
            {
                chose = Some(Chose::Cancel);
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
fn room_button(ui: &mut egui::Ui, theme: &Theme, room: &RoomInfo) -> Picked {
    let p = theme.palette;
    let m = theme.metrics;
    let mut picked = Picked::Nothing;

    ui.horizontal_top(|ui| {
        ui.set_min_height(m.row_height);
        let watch_width = m.action_height * 1.6;
        let response = ui.add_sized(
            [ui.available_width() - watch_width - m.item_spacing, m.row_height],
            egui::Button::new("").fill(p.surface_lift),
        );

        let rect = response.rect;
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_stroke(
                rect,
                m.rounding,
                egui::Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        painter.text(
            rect.left_center() + egui::vec2(10.0, -6.0),
            egui::Align2::LEFT_CENTER,
            &room.name,
            egui::FontId::proportional(m.text_body),
            p.text,
        );
        // A room and a match are the same thing to everything else, so this
        // list is the one place the difference has to show — clicking into a
        // match that has already started only to be refused is a worse way to
        // find out.
        let mut under = describe(room.world);
        if let Some(victory) = room.victory {
            under = format!(
                "{under} · {} · {}",
                crate::client::views::words::phase(&room.phase),
                crate::client::views::lobby::describe(victory)
            );
        }
        painter.text(
            rect.left_center() + egui::vec2(14.0, 13.0),
            egui::Align2::LEFT_CENTER,
            under,
            egui::FontId::proportional(m.text_small),
            if matches!(room.phase, crate::net::MatchPhase::Gathering) {
                p.good
            } else {
                p.text_dim
            },
        );
        painter.text(
            rect.right_center() - egui::vec2(14.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            players(room.players),
            egui::FontId::proportional(m.text_small),
            if room.players > 0 { p.good } else { p.text_dim },
        );

        if response.clicked() {
            picked = Picked::Join;
        }
        if ui
            .add_sized(
                [watch_width, m.row_height],
                egui::Button::new(
                    egui::RichText::new(crate::client::views::words::menu::watch::WATCH)
                        .size(m.text_small),
                ),
            )
            .clicked()
        {
            picked = Picked::Watch;
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
            ("arena".into(), WorldKind::Infinite, None),
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
