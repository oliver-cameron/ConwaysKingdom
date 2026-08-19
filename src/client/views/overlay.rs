//! What the pointer is about to do, drawn over the world.
//!
//! Read-only, and given everything in screen points: the camera arithmetic
//! belongs to whoever owns the camera, and a view doing its own would be a
//! second place for it to be wrong. Same arrangement as [`super::hud`].
//!
//! Painted into `Order::Background`, which is above the world — the world is
//! drawn before egui in the same pass — and below every panel, so a rectangle
//! swept under the hotbar does not cover it.
//!
//! Nothing here is interactive. It is a layer painter rather than an `Area`
//! precisely so it cannot claim the pointer: an `Area` under the cursor would
//! register as a widget being dragged, and the world would stop receiving the
//! very gesture this exists to show.

use super::theme::Theme;

/// Everything the overlay draws, assembled by the client each frame.
#[derive(Default)]
pub struct Marks {
    /// The cell under the pointer. Absent when the pointer is over a panel,
    /// when the view is being moved, or when cells are too small to point at
    /// one — a box around a two-pixel cell claims a precision the pointer
    /// does not have.
    pub hover: Option<egui::Rect>,
    /// The rectangle a drag has swept so far.
    pub selection: Option<Selection>,
}

pub struct Selection {
    pub rect: egui::Rect,
    /// The player's own colour. Ice has no colour of its own — the shader
    /// tints all four cell states with the owner's hue and tells them apart by
    /// sprite — so the preview does the same and hatches instead.
    pub tint: egui::Color32,
    /// Whether what is being laid is a pane, which is drawn hatched. The flat
    /// stand-in for a texture: the same colour, a different surface.
    pub hatched: bool,
    /// Size and price, as `Ice 6x4 · 24 cells · −120`.
    pub label: String,
    /// Whether the drag would be allowed. A refused drag is drawn as refused
    /// *while the button is still down*, so the answer arrives before the
    /// commitment rather than after it — and a fill is all or nothing, so a
    /// refusal means no cells at all rather than as many as could be paid for.
    pub allowed: bool,
}

pub fn show(ctx: &egui::Context, theme: &Theme, marks: &Marks) {
    if marks.hover.is_none() && marks.selection.is_none() {
        return;
    }
    let p = theme.palette;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("world-marks"),
    ));

    if let Some(rect) = marks.hover {
        painter.rect_filled(rect, 0.0, p.accent.gamma_multiply(0.10));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, p.accent.gamma_multiply(0.75)),
            egui::StrokeKind::Inside,
        );
    }

    let Some(selection) = &marks.selection else { return };
    let edge = if selection.allowed { selection.tint } else { p.bad };
    painter.rect_filled(selection.rect, 0.0, edge.gamma_multiply(0.16));
    if selection.hatched {
        hatch(&painter, selection.rect, edge.gamma_multiply(0.45));
    }
    painter.rect_stroke(
        selection.rect,
        0.0,
        egui::Stroke::new(1.5, edge),
        egui::StrokeKind::Inside,
    );
    chip(&painter, theme, selection);
}

/// Diagonals across a rectangle, clipped to it.
///
/// Drawn as a fixed number of lines rather than one every `SPACING` points, so
/// a rectangle swept across the whole screen costs the same as a small one:
/// at one pixel per cell a drag can cover a few thousand cells, and a line
/// per six points of that is a few hundred draw calls for a decoration.
fn hatch(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    const SPACING: f32 = 7.0;
    const MOST: usize = 96;

    let reach = rect.width() + rect.height();
    let step = (reach / MOST as f32).max(SPACING);
    let stroke = egui::Stroke::new(1.0, colour);
    let painter = painter.with_clip_rect(rect);

    let mut offset = 0.0;
    while offset < reach {
        painter.line_segment(
            [
                egui::pos2(rect.left() + offset, rect.top()),
                egui::pos2(rect.left(), rect.top() + offset),
            ],
            stroke,
        );
        offset += step;
    }
}

/// The size and price, in a small panel above the rectangle's top-left corner.
///
/// Above rather than inside: a rectangle can be one cell tall, and a label
/// inside one would be unreadable. Clamped to the screen, because a drag that
/// starts at the top of the window has nothing above it.
fn chip(painter: &egui::Painter, theme: &Theme, selection: &Selection) {
    let p = theme.palette;
    let m = theme.metrics;
    let colour = if selection.allowed { p.text } else { p.bad };
    let galley = painter.layout_no_wrap(
        selection.label.clone(),
        egui::FontId::proportional(11.0),
        colour,
    );

    let padding = egui::vec2(6.0, 3.0);
    let size = galley.size() + padding * 2.0;
    let screen = painter.clip_rect();
    let wanted = egui::pos2(
        selection.rect.left(),
        selection.rect.top() - size.y - 4.0,
    );
    let at = egui::pos2(
        wanted.x.clamp(screen.left() + 4.0, (screen.right() - size.x - 4.0).max(screen.left())),
        wanted.y.clamp(screen.top() + 4.0, (screen.bottom() - size.y - 4.0).max(screen.top())),
    );
    let rect = egui::Rect::from_min_size(at, size);

    painter.rect_filled(rect, m.rounding, p.surface);
    painter.rect_stroke(
        rect,
        m.rounding,
        egui::Stroke::new(1.0, if selection.allowed { p.line } else { p.bad }),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, colour);
}
