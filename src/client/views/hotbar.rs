//! What a click puts down.
//!
//! A row of slots along the bottom, one selected. Number keys pick a slot, and
//! so does clicking one.
//!
//! Slots are data rather than a match arm somewhere, so adding one is adding a
//! row to [`SLOTS`] — the bar sizes itself and the keys follow.

use crate::client::views::theme::Theme;
use crate::net::Placement;

/// What a drag lays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// Every cell the pointer crosses. Drawing, rather than specifying: you
    /// watch the line appear under your hand and stop when it looks right.
    Pencil,
    /// Every cell between the two corners. A pane is a shape you place, and
    /// dragging one out says how big before it exists.
    Rectangle,
    /// A fixed pattern, dropped where you point. The drag positions it and
    /// the release commits it, so you can see where it lands before it does.
    ///
    /// Cells are offsets from the pointer, so a pattern is written the way it
    /// looks. Nothing new reaches the wire: a stamp is a `Paint` of the cells
    /// it covers, judged against territory and value like any other.
    Stamp(&'static [(i32, i32)]),
}

/// A 2x2 block: the still life everyone starts with. Holds its shape forever,
/// which makes it the thing to build against rather than with.
const BLOCK: [(i32, i32); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];

/// A blinker: three in a row, flipping between horizontal and vertical. The
/// cheapest thing that moves, and the cheapest way to see a generation pass.
const BLINKER: [(i32, i32); 3] = [(0, 0), (0, 1), (0, 2)];

/// A glider, travelling down and to the right one cell every four
/// generations.
///
/// The only ranged answer to somebody's ice: a pane is broken by life
/// reaching it, and a glider is how life reaches somewhere you cannot.
const GLIDER: [(i32, i32); 5] = [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)];

/// One thing a player can place.
pub struct Slot {
    pub name: &'static str,
    /// What the server is asked for. A name rather than cell bits, so the
    /// server can judge the request.
    pub placement: Placement,
    /// What dragging with this slot held lays down.
    pub stroke: Stroke,
}

pub const SLOTS: [Slot; 5] = [
    Slot { name: "Life", placement: Placement::Life, stroke: Stroke::Pencil },
    // Ice is a flag rather than a kind, so a pane lies over a living cell as
    // readily as over empty ground.
    Slot { name: "Ice", placement: Placement::Ice, stroke: Stroke::Rectangle },
    // Patterns, which are Life laid in a shape worth knowing. They are the
    // vocabulary the rules actually reward: something that holds, something
    // that moves in place, and something that travels.
    Slot { name: "Block", placement: Placement::Life, stroke: Stroke::Stamp(&BLOCK) },
    Slot { name: "Blinker", placement: Placement::Life, stroke: Stroke::Stamp(&BLINKER) },
    Slot { name: "Glider", placement: Placement::Life, stroke: Stroke::Stamp(&GLIDER) },
];

/// Where a pattern's cells land when it is dropped at `at`.
pub fn stamp_at(pattern: &[(i32, i32)], at: (i32, i32)) -> Vec<(i32, i32)> {
    pattern.iter().map(|&(r, c)| (at.0 + r, at.1 + c)).collect()
}

/// Which slot a digit selects, if any. `1` is the first.
pub fn slot_for_digit(digit: u32) -> Option<usize> {
    let index = (digit as usize).checked_sub(1)?;
    (index < SLOTS.len()).then_some(index)
}

pub struct Shown {
    /// What the bar covered, so clicks on it do not reach the world.
    pub rect: Option<egui::Rect>,
    /// A slot the player just clicked.
    pub picked: Option<usize>,
}

pub fn show(ctx: &egui::Context, theme: &Theme, selected: usize) -> Shown {
    let p = theme.palette;
    let m = theme.metrics;
    let mut picked = None;

    let response = egui::Area::new("hotbar".into())
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -m.margin])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 0.6)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, slot) in SLOTS.iter().enumerate() {
                            if draw_slot(ui, theme, slot, index, index == selected) {
                                picked = Some(index);
                            }
                        }
                    });
                });
        });

    Shown { rect: Some(response.response.rect), picked }
}

/// One slot. Returns whether it was clicked.
fn draw_slot(
    ui: &mut egui::Ui,
    theme: &Theme,
    slot: &Slot,
    index: usize,
    selected: bool,
) -> bool {
    let p = theme.palette;
    let m = theme.metrics;
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

    // The number that selects it, small and in the corner.
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("{}", index + 1),
        egui::FontId::proportional(10.0),
        if selected { p.accent } else { p.text_dim },
    );
    painter.text(
        rect.center() + egui::vec2(0.0, 4.0),
        egui::Align2::CENTER_CENTER,
        slot.name,
        egui::FontId::proportional(11.0),
        if selected { p.text } else { p.text_dim },
    );

    response.clicked()
}
