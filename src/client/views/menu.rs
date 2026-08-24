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
use crate::net::RoomInfo;
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

    let area = egui::Area::new("menu".into())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
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
                    ui.set_width(420.0);
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
                                    [ui.available_width(), 40.0],
                                    egui::Button::new(
                                        egui::RichText::new(words::LOOK).size(15.0),
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
                            }
                        }
                    }

                    ui.add_space(m.item_spacing * 2.0);
                    ui.separator();
                    ui.add_space(m.item_spacing);
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new(egui::RichText::new(words::ALONE).size(14.0)),
                        )
                        .clicked()
                    {
                        chose = Chose::Offline;
                    }
                    ui.small(words::ALONE_NOTE);
                });
        });

    (chose, Some(area.response.rect))
}

/// One room: what it is called, whether anybody is in it, and whether it ends.
fn room_button(ui: &mut egui::Ui, theme: &Theme, room: &RoomInfo) -> bool {
    let p = theme.palette;
    let response = ui.add_sized(
        [ui.available_width(), 54.0],
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
        egui::FontId::proportional(14.0),
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
        egui::FontId::proportional(12.0),
        if matches!(room.phase, crate::net::MatchPhase::Gathering) { p.good } else { p.text_dim },
    );
    painter.text(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        players(room.players),
        egui::FontId::proportional(13.0),
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
        assert_eq!(
            describe(WorldKind::Toroidal { rows: 6, cols: 8 }),
            "6×8 chunks, wrapping"
        );
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
