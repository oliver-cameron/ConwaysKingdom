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
use crate::net::{RoomInfo, RoomName, Victory};
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

pub struct Menu {
    pub name: String,
    /// Where to connect. Fixed on the web — the page came from a server, so
    /// that is the server — and typed natively, where there is no page to
    /// have come from.
    pub address: String,
    pub stage: Stage,
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
    /// `ROWSxCOLS`, read only when the shape is wrapping.
    pub size: String,
    pub ends: Ends,
    /// Generations or squares, read only when it ends.
    pub target: String,
    /// Why the last attempt was refused — by this form, or by the server.
    pub note: Option<String>,
    /// Sent, and waiting for an answer. The form stays on screen while it is
    /// true, because a refusal has to arrive back into something.
    pub asking: bool,
}

impl Default for Draft {
    fn default() -> Self {
        let (rows, cols) = crate::sim::DEFAULT_TORUS;
        Self {
            name: String::new(),
            shape: Shape::Boundless,
            size: format!("{rows}x{cols}"),
            ends: Ends::Never,
            target: crate::net::DEFAULT_TIMER.to_string(),
            note: None,
            asking: false,
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
        let name = crate::net::room_name(&self.name)?;
        let shape = match self.shape {
            Shape::Boundless => WorldKind::Infinite,
            Shape::Wrapping => crate::sim::parse_torus(self.size.trim())
                .map_err(|_| words::make::not_a_size(self.size.trim()))?,
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
    /// Join this room on the server already reached.
    Join(String),
    /// Shut the form without making anything.
    Cancel,
    /// Make this world on the server already reached, then join it. Parsed
    /// rather than raw: what the player typed became what they chose in
    /// [`Draft::parse`], which is the menu's whole job.
    Create {
        name: RoomName,
        shape: WorldKind,
        victory: Option<Victory>,
    },
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
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    let area = egui::Area::new("menu".into()).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
        ctx,
        |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.6)
                .show(ui, |ui| {
                    // A fixed width, because a panel sized by its contents
                    // jumps every time the room list changes length and a menu
                    // that resizes under the pointer is one whose buttons move
                    // as you reach for them.
                    //
                    // Wide, now that the menu has the screen rather than a
                    // corner of it: the world used to be drawn behind and the
                    // panel had to stay out of its way. It does not any more,
                    // so the things you actually click are the size of things
                    // you click rather than the size of a HUD row.
                    ui.set_width(m.panel_width);
                    ui.spacing_mut().item_spacing.y = m.item_spacing;

                    ui.heading(words::TITLE);
                    ui.add_space(m.item_spacing);

                    ui.label(words::NAME);
                    ui.add(
                        egui::TextEdit::singleline(&mut menu.name)
                            .desired_width(f32::INFINITY)
                            .hint_text(words::NAME_HINT),
                    );

                    ui.add_space(m.item_spacing);
                    if on_web {
                        // Not a field. The socket is derived from the page's
                        // origin, so a typed address here would be a promise
                        // the client cannot keep.
                        ui.label(words::SERVER);
                        ui.colored_label(p.text_dim, &menu.address);
                    } else {
                        ui.label(words::SERVER);
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
                                        egui::RichText::new(words::LOOK).size(m.text_action),
                                    ),
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
                                // Cannot happen with the server as it stands,
                                // which always keeps a default room -- said
                                // out loud rather than drawn as an empty gap,
                                // because a blank panel reads as the menu
                                // being broken.
                                ui.colored_label(p.warn, words::NO_ROOMS);
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label(words::ROOMS);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button(words::REFRESH).clicked() {
                                                chose = Chose::Refresh;
                                            }
                                        },
                                    );
                                });
                                for room in rooms {
                                    if room_button(ui, theme, room) {
                                        chose = Chose::Join(room.name.clone());
                                    }
                                }
                                // What is behind the list, before anybody
                                // reads the names -- generals.io puts the
                                // count on the way in for the same reason.
                                ui.colored_label(
                                    p.text_dim,
                                    egui::RichText::new(words::rooms_here(
                                        rooms.len(),
                                        rooms.iter().map(|r| r.players).sum(),
                                    ))
                                    .size(m.text_small),
                                );
                            }

                            ui.add_space(m.item_spacing);
                            match &mut menu.draft {
                                // Opening it is one press and no screen
                                // change: the form appears where the button
                                // was. Depth is what a menu spends first.
                                None => {
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), m.button_height],
                                            egui::Button::new(
                                                egui::RichText::new(words::make::OPEN)
                                                    .size(m.text_body),
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

                    ui.add_space(m.item_spacing * 2.0);
                    ui.separator();
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
                });
        },
    );

    (chose, Some(area.response.rect))
}

/// The form, in place under the room list. `Some` when it was submitted.
///
/// **One labelled row per decision, and a row appears only when the decision
/// it belongs to is live** — the shape of the thing is borrowed from Infinite
/// Chess, which never shows a board size for a variant that has none. Three
/// rows for a world, five for a match, rather than five rows with two of them
/// greyed out and nothing to say why.
///
/// Toggle rows rather than drop-downs: every choice here is two or three wide,
/// so a row of buttons shows the whole of it where a list shows one of it —
/// and a drop-down wants a popup layer, which is one more thing to keep off
/// the world behind the menu.
///
/// The action sits at the **foot**, under the fields rather than over them,
/// because on a phone that is where a thumb is.
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

            ui.label(egui::RichText::new(words::make::NAME).size(m.text_small));
            ui.add(
                egui::TextEdit::singleline(&mut draft.name)
                    .desired_width(f32::INFINITY)
                    .hint_text(words::make::NAME_HINT),
            );

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
                ui.add(
                    egui::TextEdit::singleline(&mut draft.size)
                        .desired_width(f32::INFINITY)
                        .hint_text(words::make::SIZE_HINT),
                );
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(words::make::SIZE_NOTE).size(m.text_small),
                );
            }

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
                        chose = Some(Chose::Create { name, shape, victory });
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
fn room_button(ui: &mut egui::Ui, theme: &Theme, room: &RoomInfo) -> bool {
    let p = theme.palette;
    let response = ui.add_sized(
        [ui.available_width(), theme.metrics.row_height],
        egui::Button::new("").fill(p.surface_lift),
    );

    let rect = response.rect;
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_stroke(
            rect,
            theme.metrics.rounding,
            egui::Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    painter.text(
        rect.left_center() + egui::vec2(10.0, -6.0),
        egui::Align2::LEFT_CENTER,
        &room.name,
        egui::FontId::proportional(theme.metrics.text_body),
        p.text,
    );
    // A room and a match are the same thing to everything else, so this list
    // is the one place the difference has to show — clicking into a match that
    // has already started only to be refused is a worse way to find out.
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
        egui::FontId::proportional(theme.metrics.text_small),
        if matches!(room.phase, crate::net::MatchPhase::Gathering) { p.good } else { p.text_dim },
    );
    painter.text(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        players(room.players),
        egui::FontId::proportional(theme.metrics.text_small),
        if room.players > 0 { p.good } else { p.text_dim },
    );

    response.clicked()
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
            size: "6x8".into(),
            ..Draft::default()
        };
        assert_eq!(torus.parse().unwrap().1, WorldKind::Toroidal { rows: 6, cols: 8 });

        let cup = Draft {
            name: "cup".into(),
            ends: Ends::Territory,
            target: "500".into(),
            // Left behind by a change of mind, and never read, because the
            // shape is boundless.
            size: "nonsense".into(),
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
            size: "big".into(),
            ..Draft::default()
        };
        assert!(sizeless.parse().is_err());
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
