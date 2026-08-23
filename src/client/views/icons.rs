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

use crate::render::atlas::{SHEET_N, TILE_N};
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
        let across = (SHEET_N / TILE_N) as f32;
        let (x, y) = ((tile % 16) as f32 / across, (tile / 16) as f32 / across);
        let edge = 1.0 / across;
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(edge, edge))
    }
}

/// The whole sheet, converted from saturation-lightness-coverage into this
/// player's colours.
fn tint(player: PlayerId) -> Option<egui::ColorImage> {
    let sheet = crate::render::atlas::decoded()?;
    let mut pixels = Vec::with_capacity((SHEET_N * SHEET_N) as usize);
    for texel in sheet.chunks_exact(4) {
        let (r, g, b) = super::hud::shade(
            texel[1] as f32 / 255.0,
            texel[0] as f32 / 255.0,
            player,
        );
        pixels.push(egui::Color32::from_rgba_unmultiplied(r, g, b, texel[3]));
    }
    Some(egui::ColorImage {
        size: [SHEET_N as usize, SHEET_N as usize],
        pixels,
        source_size: egui::vec2(SHEET_N as f32, SHEET_N as f32),
    })
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
