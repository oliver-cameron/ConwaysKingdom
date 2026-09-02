//! The sprite sheet, as something egui can draw.
//!
//! A button that says "Life" tells you the word. A button that shows the cell
//! tells you what will be on the board — which is the thing you are choosing.
//! So the hotbar draws the same art the world does, sampled from the same
//! sheet, tinted with the same hue.
//!
//! The tint is why this cannot be a plain image. The sheet carries no colour:
//! a texel is saturation, lightness and coverage, and the hue arrives at draw
//! time from the player's number. The shader does that on the GPU for the
//! world; here it is done once on the CPU, per player, and handed to egui as
//! an ordinary texture.
//!
//! Rebuilt when the player number changes, which happens once — on `Welcome` —
//! so "once" is the honest cost.

use crate::render::atlas::{SHEET_H, SHEET_W, TILE_N};
use crate::sim::PlayerId;

/// The sheet, tinted for one player, as an egui texture.
#[derive(Default)]
pub struct Icons {
    tinted: Option<(PlayerId, egui::TextureHandle)>,
}

impl Icons {
    /// The sheet in this player's colour, building it if it is not already.
    pub fn sheet(&mut self, ctx: &egui::Context, player: PlayerId) -> Option<egui::TextureId> {
        if !matches!(&self.tinted, Some((who, _)) if *who == player) {
            let image = tint(player)?;
            // Nearest, and no mip chain: the art is sixteen pixels square and
            // is meant to stay pixel art at whatever size a button is.
            let handle = ctx.load_texture("sprites", image, egui::TextureOptions::NEAREST);
            self.tinted = Some((player, handle));
        }
        self.tinted.as_ref().map(|(_, handle)| handle.id())
    }

    /// Where a cell's tile sits in the sheet, as egui wants it: fractions of
    /// the whole image. Low nibble across, high nibble down, which is the tile
    /// byte's own arithmetic.
    pub fn uv(tile: u8) -> egui::Rect {
        let across = (SHEET_W / TILE_N) as f32;
        let (x, y) = ((tile % 16) as f32 / across, (tile / 16) as f32 / across);
        let edge = 1.0 / across;
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(edge, edge))
    }
}

/// The whole sheet, converted from saturation-lightness-coverage into this
/// player's colours.
fn tint(player: PlayerId) -> Option<egui::ColorImage> {
    let sheet = crate::render::atlas::decoded()?;
    let mut pixels = Vec::with_capacity((SHEET_W * SHEET_H) as usize);
    for texel in sheet.chunks_exact(4) {
        let (r, g, b) = crate::client::views::hue::shade(
            texel[1] as f32 / 255.0,
            texel[0] as f32 / 255.0,
            player,
        );
        pixels.push(egui::Color32::from_rgba_unmultiplied(r, g, b, texel[3]));
    }
    Some(egui::ColorImage {
        size: [SHEET_W as usize, SHEET_H as usize],
        pixels,
        source_size: egui::vec2(SHEET_W as f32, SHEET_H as f32),
    })
}

/// A left-pointing arrow, drawn rather than typed.
///
/// **It was `\u{2190}` and it came out as a box.** No font is loaded anywhere
/// in this client — `Views::new` never touches `FontDefinitions` — so the
/// glyphs available are whatever egui bundles, and a character outside that
/// coverage renders as tofu. Which is a fair description of the back button:
/// the one control on the screen whose whole job is to be recognised at a
/// glance, drawn as a square.
///
/// A font would fix it and is a bigger decision; this is the fix that is
/// correct whatever font is chosen later. It is also the pattern already here
/// — see [`camera`] — and it scales with the button rather than with a point
/// size, which matters on a control that is twenty-two pixels wide.
pub fn back(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    let width = rect.width().min(rect.height());
    let stroke = egui::Stroke::new((width * 0.11).max(1.2), colour);
    let mid = rect.center();
    // The shaft, across the middle two thirds, so the head has somewhere to
    // sit without touching the edge.
    let (left, right) = (mid.x - width * 0.30, mid.x + width * 0.30);
    painter.line_segment([egui::pos2(left, mid.y), egui::pos2(right, mid.y)], stroke);
    // And the head: two strokes rather than a filled triangle, so it reads the
    // same weight as the shaft at every size.
    let reach = width * 0.22;
    for up in [-1.0, 1.0] {
        painter.line_segment(
            [egui::pos2(left, mid.y), egui::pos2(left + reach, mid.y + reach * up)],
            stroke,
        );
    }
}

/// A circular arrow, for asking again.
///
/// Drawn for the reason [`back`] is: it was `\u{21bb}` and no font this client
/// loads has it, because this client loads none.
///
/// An arc rather than a ring, so the gap and the head say which way it goes —
/// a closed circle with a triangle on it reads as a target.
pub fn refresh(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    let radius = rect.width().min(rect.height()) * 0.36;
    let stroke = egui::Stroke::new((radius * 0.30).max(1.2), colour);
    let centre = rect.center();

    // Three quarters of a turn, from just past the top round to the left,
    // leaving the corner the head goes in.
    const STEPS: usize = 18;
    let angle = |t: f32| std::f32::consts::TAU * (-0.20 + t * 0.78);
    let arc: Vec<egui::Pos2> = (0..=STEPS)
        .map(|i| {
            let a = angle(i as f32 / STEPS as f32);
            centre + egui::vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(arc, stroke));

    // The head, on the end the arc starts at, pointing the way it travels.
    let a = angle(0.0);
    let tip = centre + egui::vec2(a.cos(), a.sin()) * radius;
    let along = egui::vec2(-a.sin(), a.cos()) * radius * 0.55;
    let across = egui::vec2(a.cos(), a.sin()) * radius * 0.45;
    painter.add(egui::Shape::convex_polygon(
        vec![tip + across, tip - across, tip - along],
        colour,
        egui::Stroke::NONE,
    ));
}

/// A camera, drawn rather than sampled, because capturing is not a cell and
/// the sheet has no picture of it.
///
/// Painted from primitives instead of added to the sheet: a tile there is a
/// *kind*, and the rule would then have to say what a camera cell does.
pub fn camera(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    let body = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, rect.height() * 0.04),
        egui::vec2(rect.width() * 0.72, rect.height() * 0.52),
    );
    let stroke = egui::Stroke::new((rect.width() * 0.07).max(1.0), colour);
    painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);

    // The bump on top, where the viewfinder goes.
    let hump = egui::Rect::from_min_size(
        egui::pos2(body.left() + body.width() * 0.18, body.top() - body.height() * 0.22),
        egui::vec2(body.width() * 0.30, body.height() * 0.24),
    );
    painter.rect_filled(hump, 1.0, colour);

    painter.circle_stroke(body.center(), body.height() * 0.28, stroke);
}
