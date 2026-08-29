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

/// What a drag lays: the **shape** axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stroke {
    /// Every cell the pointer crosses. Drawing, rather than specifying: you
    /// watch the line appear under your hand and stop when it looks right.
    Pencil,
    /// Every cell between the two corners. A pane is a shape you place, and
    /// dragging one out says how big before it exists.
    Rectangle,
}

/// One of the four things a cell can be: the **kind** axis.
///
/// It was a tool, which is to say a shape and a material at once — a mine came
/// with a pencil and ice came with a rectangle, and there was no way to draw a
/// line of ice or sweep a pane of mines. The two are chosen separately now and
/// this is half of the choice.
pub struct Tool {
    pub name: &'static str,
    /// The cell this puts down, so a button can show it rather than spell it.
    pub shows: Cell,
    /// What the server is asked for. A name rather than cell bits, so the
    /// server can judge the request.
    pub placement: Placement,
    /// The shape this kind is usually wanted in — **a default and not a
    /// constraint**, which is the difference between this and the tool it grew
    /// out of.
    ///
    /// A mine is placed a few at a time and into a pattern, because what it is
    /// worth depends on what it is next to; a turret goes down in fours,
    /// because one live cell on its own dies of loneliness and a 2x2 block is
    /// the cheapest thing that does not. Both are gestures, which is what a
    /// pencil is for. Ice is a wall, and a wall is a thing you say the size of
    /// before it exists.
    ///
    /// The shape axis can still be set to anything; this is only where the one
    /// key that resets it goes.
    pub usually: Shape,
}

/// What each tool leaves on a square, which is what its button shows. Built
/// here rather than named, so a button cannot show one thing and lay another.
const LIVE: Cell = Cell::DEAD.with_alive(true);
const MINED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::MINE);
const TURRETED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::TURRET);
const ICED: Cell = Cell::DEAD.with_ice(true);

/// The left segment: what you draw with.
/// The four kinds, in the order they sit on the bar.
///
/// **One list, and ice is in it.** Ice used to live apart because it was the
/// tool that walls people off and because it came with a different stroke;
/// with the stroke chosen separately there is nothing left to separate it by,
/// and a fourth kind now appears here by existing.
pub const KINDS: [Tool; 4] = [
    Tool { name: words::LIFE, shows: LIVE, placement: Placement::Life, usually: Shape::Draw },
    Tool { name: words::MINE, shows: MINED, placement: Placement::Mine, usually: Shape::Draw },
    Tool {
        name: words::TURRET,
        shows: TURRETED,
        placement: Placement::Turret,
        usually: Shape::Draw,
    },
    Tool { name: words::ICE, shows: ICED, placement: Placement::Ice, usually: Shape::Rect },
];

/// What a gesture makes: the shape axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shape {
    /// A pencil. Every cell the pointer crosses.
    #[default]
    Draw,
    /// A pane. Every cell between the corners.
    Rect,
    /// Nothing yet: the next drag takes a stamp rather than laying anything.
    ///
    /// Its own square because otherwise there is nowhere to start. Capturing
    /// used to be "drag with a stamp held", which is fine once you have one
    /// and impossible before — the first stamp had no way to exist.
    Capture,
    /// A saved pattern, by its place in the library.
    Stamp(usize),
}

/// **Two axes: a shape and what it is made of.**
///
/// It was one — a list of tools and stamps where picking any of them replaced
/// everything about the last. So a mine was always a pencil, ice was always a
/// pane, and a stamp was always whatever it had been captured as; there was no
/// way to draw a line of ice, sweep a pane of mines, or lay a glider in
/// anything but the material it was built from.
///
/// Separating them makes each choice mean one thing. The shape says how the
/// cells are chosen and the kind says what goes in them, and every combination
/// is reachable rather than the dozen somebody happened to list.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Held {
    pub shape: Shape,
    /// How the held pattern is turned. Nothing but a stamp has an orientation,
    /// and a stamp keeps this across a change of material — turning a glider
    /// and then deciding it should be ice is two decisions, not one undone.
    pub turn: crate::client::views::stamp::Turn,
    /// Which of [`KINDS`]. An index rather than the `Placement`, so a square
    /// on the bar and the thing it lays cannot come apart.
    pub kind: usize,
}

impl Held {
    /// Whether a drag with this held takes a stamp rather than laying one.
    pub fn captures(self) -> bool {
        matches!(self.shape, Shape::Capture | Shape::Stamp(_))
    }

    /// The kind this is holding.
    pub fn tool(self) -> Option<&'static Tool> {
        KINDS.get(self.kind)
    }

    pub fn placement(self) -> Option<Placement> {
        self.tool().map(|t| t.placement)
    }

    /// What a drag with this held draws.
    ///
    /// A stamp is placed by a click, so a drag with one held sweeps out the
    /// rectangle that **captures** another — holding a stamp already means you
    /// are thinking about stamps, which is why that needs no bar of its own.
    pub fn stroke(self) -> Stroke {
        match self.shape {
            Shape::Draw => Stroke::Pencil,
            Shape::Rect | Shape::Capture | Shape::Stamp(_) => Stroke::Rectangle,
        }
    }

    /// Back to the shape this kind is usually wanted in.
    ///
    /// **One key, and it goes somewhere rather than somewhere else.** A toggle
    /// between draw and pane is a key whose meaning depends on what you last
    /// pressed, so using it means remembering where you are; this always lands
    /// in the same place for a given material — a pencil for life, mines and
    /// turrets, a pane for ice — and is therefore also the way out of a stamp
    /// or a capture without looking at the bar to see what it will do.
    ///
    /// The other shape is a click away on the bar, which is the right home for
    /// the choice you make occasionally.
    pub fn defaulted(self) -> Self {
        let shape = self.tool().map(|k| k.usually).unwrap_or_default();
        // The turn goes too. "Put me back to normal" that left a pattern
        // rotated would be a reset somebody has to reset after.
        Self { shape, turn: Default::default(), ..self }
    }
}

/// Something on the bar that a key can pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// Pick a shape, leaving what it is made of alone.
    Shape(Shape),
    /// Pick a kind, leaving the shape alone. **That is the whole point of two
    /// axes**: choosing a material does not put your pencil down.
    Kind(usize),
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
    // **Derived from [`KINDS`], not listed beside it.** It was a hand-written
    // list once and went out by one the moment a tool was added, so the bar
    // labelled its squares from its own layout and the keyboard disagreed.
    (0..KINDS.len()).map(Key::Kind).chain([Key::Shape(Shape::Capture), Key::More]).collect()
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
    /// What the kind axis is holding, which is what a stamp's square is drawn
    /// in: a pattern is a shape, so the thumbnail shows what it would come out
    /// as rather than how it was captured.
    pub what: Placement,
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

                // **What it is made of.** One segment, four kinds, and ice
                // among them: it used to sit behind a rule because it came
                // with a different stroke, and the stroke is the other axis
                // now.
                segment(ui, theme, |ui| {
                    for (i, tool) in KINDS.iter().enumerate() {
                        if square(
                            ui,
                            look,
                            Face::Sprite(tool.shows),
                            tool.name,
                            tool_hint(shift, typed),
                            held.kind == i,
                        ) {
                            picked = Some(Key::Kind(i));
                        }
                        shift += 1;
                    }
                });

                // The stamps segment stands even when it is empty, so the bar
                // does not change shape the first time anything is captured.
                segment(ui, theme, |ui| {
                    // **How the cells are chosen.** Draw and pane first,
                    // because they are what most gestures are, and the toggle
                    // between them is the one key on the bar that is a verb.
                    for (shape, name) in [(Shape::Draw, words::DRAW), (Shape::Rect, words::PANE)] {
                        if square(
                            ui,
                            look,
                            Face::Text(name),
                            name,
                            (shape == Shape::Draw).then(|| words::FLIP_KEY.to_string()),
                            held.shape == shape,
                        ) {
                            picked = Some(Key::Shape(shape));
                        }
                    }
                    rule(ui, theme);
                    // The capture square: it is where a library comes from, so
                    // it cannot be behind having one.
                    if square(
                        ui,
                        look,
                        Face::Camera,
                        words::CAPTURE,
                        tool_hint(shift, typed),
                        held.shape == Shape::Capture,
                    ) {
                        picked = Some(Key::Shape(Shape::Capture));
                    }
                    shift += 1;
                    for i in 0..library.on_the_bar() {
                        let Some(stamp) = library.get(i) else { continue };
                        // The held square shows the pattern **as it would be
                        // laid**, turn and all. A thumbnail that stayed upright
                        // while the preview under the pointer rotated would be
                        // two answers to what is about to happen.
                        let turned;
                        let stamp = if held.shape == Shape::Stamp(i) && !held.turn.is_default() {
                            turned = stamp.turned(held.turn);
                            &turned
                        } else {
                            stamp
                        };
                        if square(
                            ui,
                            look,
                            Face::Pattern(stamp),
                            &stamp.name,
                            stamp_hint(i),
                            held.shape == Shape::Stamp(i),
                        ) {
                            picked = Some(Key::Shape(Shape::Stamp(i)));
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
    let (rect, response) = ui.allocate_exact_size(egui::vec2(m.slot, m.slot), egui::Sense::click());

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
        Face::Pattern(stamp) => stamp.draw(painter, inner, look.what, look.player, look.sheet),
        Face::Camera => icons::camera(painter, inner, ink),
        Face::Text(text) => draw_text(painter, inner, text, ink),
    }

    if let Some(key) = key {
        // Brighter as well as bigger: a key hint at `text_dim` over a sprite
        // was the least legible thing on the screen, and it is the one piece
        // of writing here somebody is looking for rather than reading.
        shadowed(
            painter,
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            &key,
            13.0,
            if selected { p.accent } else { p.text },
        );
    }

    response.on_hover_text(name).clicked()
}

fn draw_text(painter: &egui::Painter, rect: egui::Rect, text: &str, colour: egui::Color32) {
    shadowed(painter, rect.center(), egui::Align2::CENTER_CENTER, text, 14.0, colour);
}

/// Text with something dark under it, so it reads whatever it is over.
///
/// **Every square on this bar has a picture behind its writing** — a sprite,
/// a pattern, the world showing through a gap — and thin light glyphs on top
/// of a busy one are a smear rather than a word. A shadow is the cheap answer
/// and the right one here: no panel behind the text, which would cover the
/// picture the square exists to show, and no outline, which at this size turns
/// a glyph into a blob.
///
/// Offset by one point rather than blurred, because a blur is several draws
/// and this is drawn per square per frame.
fn shadowed(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: impl std::fmt::Display,
    size: f32,
    colour: egui::Color32,
) {
    let font = egui::FontId::proportional(size);
    let text = text.to_string();
    painter.text(
        at + egui::vec2(1.0, 1.0),
        align,
        &text,
        font.clone(),
        egui::Color32::BLACK.gamma_multiply(0.75),
    );
    painter.text(at, align, text, font, colour);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::views::stamp::{Stamp, ON_THE_BAR};

    fn library(n: usize) -> Library {
        let mut library = Library::default();
        for i in 0..n {
            library.keep(Stamp { name: format!("s{i}"), cells: vec![(0, 0)], size: (1, 1) });
        }
        library
    }

    /// The bar reads left to right whatever is on it, and Ice is always the
    /// last thing — so the key that walls somebody off does not move when you
    /// capture a pattern.
    /// The kinds keep their keys however many patterns are captured, which is
    /// the whole reason the bar is split.
    #[test]
    fn the_kind_keys_never_move() {
        for n in [0, 1, ON_THE_BAR, ON_THE_BAR + 5] {
            let keys = shifted(&library(n));
            let expected: Vec<Key> = (0..KINDS.len()).map(Key::Kind).collect();
            assert_eq!(&keys[..expected.len()], &expected[..], "{n} stamps");
        }
    }

    /// and they keep the order they sit in.
    #[test]
    fn shift_picks_the_kinds_in_the_order_they_sit() {
        let few = library(1);
        // Every kind, in the order it sits on the bar. Ice is among them now:
        // it used to sit apart because it came with a different stroke, and
        // the stroke is the other axis.
        for (i, tool) in KINDS.iter().enumerate() {
            assert_eq!(
                shifted_for_digit(i as u32 + 1, &few),
                Some(Key::Kind(i)),
                "shift and {} should pick {}",
                i + 1,
                tool.name
            );
        }
        let after = KINDS.len() as u32;
        assert_eq!(shifted_for_digit(after + 1, &few), Some(Key::Shape(Shape::Capture)));
        assert_eq!(shifted_for_digit(after + 2, &few), Some(Key::More));
        assert_eq!(shifted_for_digit(after + 3, &few), None);

        // And not one of them moves when a pattern is captured.
        let many = library(ON_THE_BAR + 1);
        assert_eq!(shifted(&many), shifted(&few));
    }

    /// **The two axes are independent, which is the whole of the change.**
    ///
    /// Every combination is reachable now rather than the dozen somebody
    /// happened to list: a pane of mines and a pencil of ice were both
    /// unsayable, because a tool carried its stroke with it.
    #[test]
    fn a_shape_and_a_kind_are_chosen_separately() {
        let ice = KINDS.iter().position(|k| k.placement == Placement::Ice).unwrap();
        let drawn_ice = Held { shape: Shape::Draw, kind: ice, turn: Default::default() };
        assert_eq!(drawn_ice.stroke(), Stroke::Pencil, "a line of ice was unsayable");
        assert_eq!(drawn_ice.placement(), Some(Placement::Ice));

        let panes_of_mine = Held {
            shape: Shape::Rect,
            kind: KINDS.iter().position(|k| k.placement == Placement::Mine).unwrap(),
            turn: Default::default(),
        };
        assert_eq!(panes_of_mine.stroke(), Stroke::Rectangle, "a pane of mines was unsayable");
        assert_eq!(panes_of_mine.placement(), Some(Placement::Mine));

        // A stamp is a shape, so it keeps whatever it is being made of --
        // which is what "remove the stamps know what they are made of" means
        // at the point of placing one.
        let stamped = Held { shape: Shape::Stamp(0), kind: ice, turn: Default::default() };
        assert_eq!(stamped.placement(), Some(Placement::Ice), "a stamp lost the held kind");
        assert_eq!(stamped.stroke(), Stroke::Rectangle, "a drag with a stamp still captures");
    }

    /// Capturing is a rectangle, and it has a square of its own — there has to
    /// be a way to take the first stamp, and "drag with a stamp held" is not
    /// one when you have none.
    #[test]
    fn capturing_is_reachable_with_nothing_captured() {
        assert!(Held { shape: Shape::Capture, kind: 0, turn: Default::default() }.captures());
        assert!(
            Held { shape: Shape::Stamp(3), kind: 0, turn: Default::default() }.captures(),
            "a stamp takes the next one"
        );
        assert!(!Held { shape: Shape::Draw, kind: 0, turn: Default::default() }.captures());
        assert!(!Held { shape: Shape::Rect, kind: 0, turn: Default::default() }.captures());
        assert!(shifted(&library(0)).contains(&Key::Shape(Shape::Capture)));
    }

    /// **The one key lands in one place**, which is what makes it usable
    /// without looking: a toggle's meaning depends on what was pressed last,
    /// and this always puts the shape back to whatever the held material is
    /// usually wanted in.
    #[test]
    fn the_reset_key_goes_to_what_the_kind_is_usually_wanted_in() {
        let kind = |what: Placement| KINDS.iter().position(|k| k.placement == what).unwrap();

        for (what, expected) in [
            (Placement::Life, Shape::Draw),
            (Placement::Mine, Shape::Draw),
            (Placement::Turret, Shape::Draw),
            (Placement::Ice, Shape::Rect),
        ] {
            let held = Held { shape: Shape::Capture, kind: kind(what), turn: Default::default() };
            assert_eq!(held.defaulted().shape, expected, "{what:?}");
            // Twice is the same place, which a toggle could not promise.
            assert_eq!(
                held.defaulted().defaulted().shape,
                expected,
                "{what:?} moved on a second press"
            );
            assert_eq!(held.defaulted().kind, held.kind, "resetting a shape changed the material");
        }

        // And it is the way out of a stamp, whatever is held.
        let stamped =
            Held { shape: Shape::Stamp(4), kind: kind(Placement::Ice), turn: Default::default() };
        assert_eq!(stamped.defaulted().shape, Shape::Rect);
    }
}
