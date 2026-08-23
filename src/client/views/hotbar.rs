//! What a click puts down.
//!
//! Two segments along the bottom, one thing selected across both:
//!
//! ```text
//!     [ Life  Mine │ Ice ]   [ Grab  stamps … ⋯ ]
//! ```
//!
//! Segmented because the halves are not alike. The tools are the game's own
//! vocabulary and never change; the stamps are whatever you happened to
//! capture, and there may be none or thirty. Run together, the Ice key would
//! move every time you saved a pattern.
//!
//! Ice sits with the other tools but behind a rule, because it is the one that
//! walls people off and should not be a neighbour of the one you draw with.
//!
//! **Keys: the digits are the stamps, shift and a digit is a tool.** The
//! stamps get the bare digits because they are what you hold ten of and swap
//! between without looking. Binding is by *physical* key so it is the same key
//! on any layout; the label is learned from what that key actually types,
//! because shift and `1` is `!` on one keyboard and something else on
//! Programmer Dvorak, and nothing but the keyboard can say which.

use crate::client::views::icons::{self, Icons};
use crate::client::views::stamp::{Library, Stamp};
use crate::client::views::theme::Theme;
use crate::client::views::words::hotbar as words;
use crate::net::Placement;
use crate::sim::{Cell, Kind, PlayerId};

/// What a drag lays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stroke {
    /// Every cell the pointer crosses. Drawing, rather than specifying: you
    /// watch the line appear under your hand and stop when it looks right.
    Pencil,
    /// Every cell between the two corners. A pane is a shape you place, and
    /// dragging one out says how big before it exists.
    Rectangle,
}

/// One of the fixed tools.
pub struct Tool {
    pub name: &'static str,
    /// The cell this puts down, so a button can show it rather than spell it.
    pub shows: Cell,
    /// What the server is asked for. A name rather than cell bits, so the
    /// server can judge the request.
    pub placement: Placement,
    /// What dragging with this held lays down.
    pub stroke: Stroke,
}

/// What each tool leaves on a square, which is what its button shows. Built
/// here rather than named, so a button cannot show one thing and lay another.
const LIVE: Cell = Cell::DEAD.with_alive(true);
const MINED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::MINE);
const TURRETED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::TURRET);
const ICED: Cell = Cell::DEAD.with_ice(true);

/// The left segment: what you draw with.
pub const DRAWN: [Tool; 3] = [
    Tool { name: words::LIFE, shows: LIVE, placement: Placement::Life, stroke: Stroke::Pencil },
    // A pencil, not a rectangle: a mine is placed a few at a time and into a
    // pattern, because what it is worth depends on what it is next to.
    Tool { name: words::MINE, shows: MINED, placement: Placement::Mine, stroke: Stroke::Pencil },
    // A pencil for the same reason, and a shorter stroke: a turret is placed
    // in fours, because one live cell on its own dies of loneliness and the
    // 2x2 block is the cheapest thing that does not. Four cells is a gesture,
    // which is what a pencil is for.
    Tool {
        name: words::TURRET,
        shows: TURRETED,
        placement: Placement::Turret,
        stroke: Stroke::Pencil,
    },
];

/// The right segment, on its own.
pub const WALLED: Tool =
    // Ice is a flag rather than a kind, so a pane lies over a living cell as
    // readily as over empty ground.
    Tool { name: words::ICE, shows: ICED, placement: Placement::Ice, stroke: Stroke::Rectangle };

/// What the hand is holding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Held {
    /// One of [`DRAWN`].
    Draw(usize),
    /// Nothing yet: the next drag takes a stamp rather than laying anything.
    ///
    /// Its own square because otherwise there is nowhere to start. Capturing
    /// used to be "drag with a stamp held", which is fine once you have one and
    /// impossible before — the first stamp had no way to exist.
    Capture,
    /// A stamp, by its place in the library.
    Stamp(usize),
    Ice,
}

impl Default for Held {
    fn default() -> Self {
        Self::Draw(0)
    }
}

impl Held {
    /// Whether a drag with this held takes a stamp rather than laying one.
    pub fn captures(self) -> bool {
        matches!(self, Self::Capture | Self::Stamp(_))
    }

    /// The tool this is, if it is one. A stamp is not: it lays whatever it
    /// captured, which may be several placements at once.
    pub fn tool(self) -> Option<&'static Tool> {
        match self {
            Self::Draw(i) => DRAWN.get(i),
            Self::Ice => Some(&WALLED),
            Self::Capture | Self::Stamp(_) => None,
        }
    }

    pub fn placement(self) -> Option<Placement> {
        self.tool().map(|t| t.placement)
    }

    /// What a drag with this held draws. A stamp is placed by a click, so a
    /// drag with one held sweeps out the rectangle that **captures** another —
    /// which is the one gesture the two hotbars in the old plan existed to keep
    /// apart, and here it needs no second bar because holding a stamp already
    /// means you are thinking about stamps.
    pub fn stroke(self) -> Stroke {
        match self.tool() {
            Some(tool) => tool.stroke,
            None => Stroke::Rectangle,
        }
    }
}

/// Something on the bar that a key can pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Held(Held),
    /// The stamps that did not fit.
    More,
}

/// **The digits are the stamps.** `1` to `9` then `0`, which is ten and is why
/// [`ON_THE_BAR`](crate::client::views::stamp::ON_THE_BAR) is ten.
///
/// The stamps get them because they are the keys you reach for without
/// looking, and stamps are the thing you hold ten of and swap between. The
/// three tools never change and never grow, so they can afford a modifier.
pub fn stamp_for_digit(digit: u32) -> Option<usize> {
    match digit {
        0 => Some(9),
        1..=9 => Some(digit as usize - 1),
        _ => None,
    }
}

/// The keys that are not stamps, in the order they sit on the bar. Shift and a
/// digit picks one of these.
pub fn shifted(_library: &Library) -> Vec<Key> {
    vec![
        Key::Held(Held::Draw(0)),
        Key::Held(Held::Draw(1)),
        Key::Held(Held::Ice),
        Key::Held(Held::Capture),
        Key::More,
    ]
}

/// Which of those shift and this digit picks.
pub fn shifted_for_digit(digit: u32, library: &Library) -> Option<Key> {
    let index = (digit as usize).checked_sub(1)?;
    shifted(library).get(index).copied()
}

/// What a stamp's square shows in its corner.
fn stamp_hint(index: usize) -> Option<String> {
    match index {
        0..=8 => Some(format!("{}", index + 1)),
        9 => Some("0".into()),
        _ => None,
    }
}

/// What a tool's square shows: whatever shift and that digit types on the
/// keyboard in front of the player, once they have pressed it, and a plain
/// `S`-and-digit until then.
fn tool_hint(index: usize, typed: &Typed) -> Option<String> {
    let digit = u32::try_from(index).ok()? + 1;
    if digit > 9 {
        return None;
    }
    Some(match typed(digit) {
        Some(what) => what,
        None => format!("S{digit}"),
    })
}

/// What shift and a digit types here. Asked of the keyboard rather than
/// assumed — see the module note.
pub type Typed<'a> = dyn Fn(u32) -> Option<String> + 'a;

pub struct Shown {
    /// What the bar covered, so clicks on it do not reach the world.
    pub rect: Option<egui::Rect>,
    /// What the player just clicked.
    pub picked: Option<Key>,
}

/// Everything a square might need to draw itself.
pub struct Look<'a> {
    pub theme: &'a Theme,
    /// The sprite sheet in this player's colour, if it could be built.
    pub sheet: Option<egui::TextureId>,
    pub player: PlayerId,
    /// What shift and a digit types here.
    pub typed: &'a Typed<'a>,
}

pub fn show(ctx: &egui::Context, look: &Look<'_>, held: Held, library: &Library) -> Shown {
    let theme = look.theme;
    let typed = look.typed;
    let m = theme.metrics;
    let mut picked = None;
    // Shift keys run over the bar's non-stamp squares in the order they sit.
    let mut shift = 0usize;

    let response = egui::Area::new("hotbar".into())
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -m.margin])
        .show(ctx, |ui| {
            // Top-aligned, and with the row's height stated up front.
            //
            // `ui.horizontal` centres each item against the row, and the row's
            // height is whatever the tallest item turns out to be — which is
            // not known when the first one is placed. The segments came out the
            // same height and thirteen pixels apart.
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = m.item_spacing * 1.5;
                ui.set_min_height(m.slot + m.panel_padding * 1.2 + 2.0);

                segment(ui, theme, |ui| {
                    for (i, tool) in DRAWN.iter().enumerate() {
                        let key = Held::Draw(i);
                        if square(
                            ui,
                            look,
                            Face::Sprite(tool.shows),
                            tool.name,
                            tool_hint(shift, typed),
                            held == key,
                        ) {
                            picked = Some(Key::Held(key));
                        }
                        shift += 1;
                    }
                    // Ice is a tool, and it is the one that walls people off,
                    // so it lives here but behind a rule.
                    rule(ui, theme);
                    if square(
                        ui,
                        look,
                        Face::Sprite(WALLED.shows),
                        WALLED.name,
                        tool_hint(shift, typed),
                        held == Held::Ice,
                    ) {
                        picked = Some(Key::Held(Held::Ice));
                    }
                    shift += 1;
                });

                // The stamps segment stands even when it is empty, so the bar
                // does not change shape the first time anything is captured.
                segment(ui, theme, |ui| {
                    // The capture square first, and always: it is where a
                    // library comes from, so it cannot be behind having one.
                    if square(
                        ui,
                        look,
                        Face::Camera,
                        words::CAPTURE,
                        tool_hint(shift, typed),
                        held == Held::Capture,
                    ) {
                        picked = Some(Key::Held(Held::Capture));
                    }
                    shift += 1;
                    for i in 0..library.on_the_bar() {
                        let key = Held::Stamp(i);
                        let Some(stamp) = library.get(i) else { continue };
                        if square(
                            ui,
                            look,
                            Face::Pattern(stamp),
                            &stamp.name,
                            stamp_hint(i),
                            held == key,
                        ) {
                            picked = Some(Key::Held(key));
                        }
                    }
                    // The library is always one key away, whether or not
                    // anything overflowed: it is where a stamp is named, looked
                    // at, and thrown away.
                    rule(ui, theme);
                    let overflow = library.len() - library.on_the_bar();
                    let label =
                        if overflow > 0 { format!("+{overflow}") } else { "…".to_string() };
                    if square(
                        ui,
                        look,
                        Face::Text(&label),
                        words::LIBRARY,
                        tool_hint(shift, typed),
                        false,
                    ) {
                        picked = Some(Key::More);
                    }
                    shift += 1;
                });
            });
        });

    Shown { rect: Some(response.response.rect), picked }
}

/// A hairline between two squares in one segment: same group, different job.
fn rule(ui: &mut egui::Ui, theme: &Theme) {
    let m = theme.metrics;
    // Allocated a full square tall so it takes part in the row like everything
    // else, and painted short so it reads as a divider.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, m.slot), egui::Sense::hover());
    let short = rect.shrink2(egui::vec2(0.0, m.slot * 0.15));
    ui.painter().rect_filled(short, 0.0, theme.palette.line);
}

/// One boxed group of squares.
fn segment(ui: &mut egui::Ui, theme: &Theme, contents: impl FnOnce(&mut egui::Ui)) {
    let p = theme.palette;
    let m = theme.metrics;
    egui::Frame::new()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding * 0.6)
        .show(ui, |ui| {
            // Every segment is one square tall, whatever is in it, so two of
            // them side by side line up without either having to know what the
            // other holds.
            ui.set_min_height(m.slot);
            ui.horizontal_top(|ui| {
                ui.set_min_height(m.slot);
                contents(ui);
            });
        });
}

/// One square. Returns whether it was clicked. `digit` is the key that picks
/// it, and is only shown while there is a key left to show it with.
/// What fills the middle of a square.
enum Face<'a> {
    /// A cell, drawn from the sheet as the world would draw it.
    Sprite(Cell),
    /// A pattern, drawn as the cells it is.
    Pattern(&'a Stamp),
    /// A camera, for the square that takes one.
    Camera,
    /// Words, for the square that has no picture.
    Text(&'a str),
}

/// One square: a picture of what it does, the key that picks it, and its name
/// on hover.
///
/// A picture rather than the word, because what you are choosing is what will
/// be on the board and the board is where you are looking. The word is still
/// there, as a tooltip, for the first time somebody wonders.
fn square(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    face: Face<'_>,
    name: &str,
    key: Option<String>,
    selected: bool,
) -> bool {
    let p = look.theme.palette;
    let m = look.theme.metrics;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(m.slot, m.slot), egui::Sense::click());

    let fill = if selected {
        p.accent.gamma_multiply(0.22)
    } else if response.hovered() {
        p.surface_lift
    } else {
        p.surface
    };
    let edge = if selected { p.accent } else { p.line };
    let painter = ui.painter();
    painter.rect_filled(rect, m.rounding, fill);
    painter.rect_stroke(
        rect,
        m.rounding,
        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, edge),
        egui::StrokeKind::Inside,
    );

    // The picture sits inside the key's corner, so the two never overlap.
    let inner = rect.shrink(m.slot * 0.18);
    let ink = if selected { p.text } else { p.text_dim };
    match face {
        Face::Sprite(cell) => match look.sheet {
            Some(sheet) => {
                painter.image(sheet, inner, Icons::uv(cell.tile()), egui::Color32::WHITE);
            }
            None => draw_text(painter, inner, name, ink),
        },
        Face::Pattern(stamp) => stamp.draw(painter, inner, look.player, look.sheet),
        Face::Camera => icons::camera(painter, inner, ink),
        Face::Text(text) => draw_text(painter, inner, text, ink),
    }

    if let Some(key) = key {
        painter.text(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            key,
            egui::FontId::proportional(10.0),
            if selected { p.accent } else { p.text_dim },
        );
    }

    response.on_hover_text(name).clicked()
}

fn draw_text(painter: &egui::Painter, rect: egui::Rect, text: &str, colour: egui::Color32) {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(11.0),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::views::stamp::{Stamp, ON_THE_BAR};

    fn library(n: usize) -> Library {
        let mut library = Library::default();
        for i in 0..n {
            library.keep(Stamp {
                name: format!("s{i}"),
                cells: vec![((0, 0), Placement::Life)],
                size: (1, 1),
            });
        }
        library
    }

    /// The bar reads left to right whatever is on it, and Ice is always the
    /// last thing — so the key that walls somebody off does not move when you
    /// capture a pattern.
    /// The tools keep their keys however many patterns are captured, which is
    /// the whole reason the bar is split.
    #[test]
    fn the_tool_keys_never_move() {
        for n in [0, 1, ON_THE_BAR, ON_THE_BAR + 5] {
            let keys = shifted(&library(n));
            assert_eq!(&keys[..3], &[
                Key::Held(Held::Draw(0)),
                Key::Held(Held::Draw(1)),
                Key::Held(Held::Ice),
            ], "{n} stamps");
        }
    }

    /// Past the limit the extra stamps go behind one key rather than making the
    /// bar longer than a hand can read.
    #[test]
    fn the_bar_stops_growing_and_the_rest_go_behind_a_menu() {
        assert_eq!(library(ON_THE_BAR).on_the_bar(), ON_THE_BAR);
        assert_eq!(library(ON_THE_BAR + 50).on_the_bar(), ON_THE_BAR);
        // The library is always one key away: it is where a stamp is named,
        // looked at and thrown away, not only where the overflow lives.
        assert!(shifted(&library(0)).contains(&Key::More));
    }

    /// The digits are the stamps: 1 to 9 and then 0, which is ten of them and
    /// is why the bar holds ten.
    #[test]
    fn the_digits_are_the_stamps() {
        assert_eq!(stamp_for_digit(1), Some(0));
        assert_eq!(stamp_for_digit(9), Some(8));
        assert_eq!(stamp_for_digit(0), Some(9), "zero is the tenth, not the first");
        let reached: std::collections::HashSet<usize> =
            (0..=9).filter_map(stamp_for_digit).collect();
        assert_eq!(reached.len(), ON_THE_BAR, "every square on the bar has a key");
    }

    /// The tools never change and never grow, so they can afford a modifier —
    /// and they keep the order they sit in.
    #[test]
    fn shift_picks_the_tools_in_the_order_they_sit() {
        let few = library(1);
        assert_eq!(shifted_for_digit(1, &few), Some(Key::Held(Held::Draw(0))));
        assert_eq!(shifted_for_digit(2, &few), Some(Key::Held(Held::Draw(1))));
        assert_eq!(shifted_for_digit(3, &few), Some(Key::Held(Held::Ice)));
        assert_eq!(shifted_for_digit(4, &few), Some(Key::Held(Held::Capture)));
        assert_eq!(shifted_for_digit(5, &few), Some(Key::More));
        assert_eq!(shifted_for_digit(6, &few), None);

        // And not one of them moves when a pattern is captured.
        let many = library(ON_THE_BAR + 1);
        assert_eq!(shifted(&many), shifted(&few));
    }

    /// Capturing is a rectangle, and it has a square of its own — there has to
    /// be a way to take the first stamp, and "drag with a stamp held" is not
    /// one when you have none.
    #[test]
    fn a_stamp_drags_a_rectangle_and_life_draws_a_line() {
        assert_eq!(Held::Draw(0).stroke(), Stroke::Pencil);
        assert_eq!(Held::Ice.stroke(), Stroke::Rectangle);
        assert_eq!(Held::Stamp(0).stroke(), Stroke::Rectangle);
        assert_eq!(Held::Capture.stroke(), Stroke::Rectangle);
        assert_eq!(Held::Stamp(0).placement(), None, "a stamp lays what it caught");

        assert!(Held::Capture.captures());
        assert!(Held::Stamp(3).captures(), "and a stamp still takes the next one");
        assert!(!Held::Draw(0).captures());
        assert!(!Held::Ice.captures());

        // Reachable with an empty library, which is the whole point of it.
        assert!(shifted(&library(0)).contains(&Key::Held(Held::Capture)));
    }
}
