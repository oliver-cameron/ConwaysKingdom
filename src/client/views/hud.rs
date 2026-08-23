//! What the player is told about their own state.
//!
//! Read-only: a view of the client, not a place decisions are made. Everything
//! it needs arrives as arguments, so it has no opinion about where the numbers
//! came from and cannot change them.

use crate::client::views::words::hud as words;
use crate::sim::{PlayerId, WorldKind};

/// What the HUD shows. Assembled by the client each frame.
pub struct Status<'a> {
    pub player: PlayerId,
    pub value: i32,
    pub generation: u64,
    pub chunks_held: usize,
    pub chunks_drawn: u32,
    pub zoom: f32,
    pub connected: bool,
    /// Which world on the server this is, once it has said. `None` offline,
    /// where there is only the one and it has no name.
    ///
    /// Shown because a room is a whole separate world: two players who cannot
    /// find each other are far more likely to be in different rooms than at
    /// different ends of one, and nothing else on screen would say so.
    pub room: Option<&'a str>,
    /// The shape of the world, which decides whether the far edge is the
    /// opposite edge. Sent by the server rather than assumed -- nothing a
    /// client can see says whether the ground ends.
    pub world: WorldKind,
    /// Why the last action was refused, if it was.
    pub notice: Option<&'a str>,
    /// Whether the pointer is currently over the interface rather than the
    /// world. Shown because a stuck value here silently eats every click, and
    /// a number on screen is easier to trust than a guess.
    pub pointer_on_ui: bool,
    /// The cell under the cursor. Moves when the mouse moves, so it says at a
    /// glance whether pointer events are arriving at all.
    pub cursor_cell: (i32, i32),
    /// What the last click did, or why it did nothing.
    pub last_action: Option<&'a str>,
    /// The hotbar slot currently selected.
    pub holding: &'a str,
}

/// Returns the rectangle it occupied, so the client knows what it covered.
pub fn show(
    ctx: &egui::Context,
    theme: &crate::client::views::theme::Theme,
    status: &Status<'_>,
) -> Option<egui::Rect> {
    let response = egui::Window::new("kingdom")
        .title_bar(false)
        .resizable(false)
        // Fixed, or dragging it would be indistinguishable from panning the
        // world underneath.
        .movable(false)
        .anchor(
            egui::Align2::LEFT_TOP,
            [theme.metrics.margin, theme.metrics.margin],
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // The same colour the shader gives this player's cells, so the
                // swatch and the board cannot disagree about who is who.
                let (r, g, b) = player_colour(status.player);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                ui.heading(format!("Player {}", status.player.0));
            });

            ui.separator();
            ui.label(format!("Value  {}", status.value));
            ui.label(format!("Generation  {}", status.generation));
            ui.label(format!(
                "Chunks  {} held, {} drawn",
                status.chunks_held, status.chunks_drawn
            ));
            ui.label(format!("Zoom  {:.1} px/cell", status.zoom));

            ui.separator();
            ui.horizontal(|ui| {
                if status.connected {
                    ui.colored_label(theme.palette.good, words::CONNECTED);
                } else {
                    ui.colored_label(theme.palette.warn, words::OFFLINE);
                }
                if let Some(room) = status.room {
                    ui.label(format!("· room {room}"));
                }
            });
            ui.small(match status.world {
                WorldKind::Infinite => words::BOUNDLESS.to_string(),
                WorldKind::Toroidal { rows, cols } => {
                    format!("{rows}x{cols} chunks, wrapping")
                }
            });
            if let Some(notice) = status.notice {
                ui.colored_label(theme.palette.bad, notice);
            }

            ui.separator();
            ui.small(format!(
                "cursor  ({}, {})   {}",
                status.cursor_cell.0,
                status.cursor_cell.1,
                if status.pointer_on_ui { words::OVER_PANEL } else { words::ON_WORLD }
            ));
            ui.small(format!(
                "last  {}",
                status.last_action.unwrap_or(words::NOTHING_YET)
            ));

            ui.separator();
            ui.small(format!("holding  {}", status.holding));
            for hint in words::HINTS {
                ui.small(*hint);
            }
        });
    response.map(|r| r.response.rect)
}

/// The colour the shader gives a player, computed the same way so the HUD
/// swatch matches the cells on the board. OKLab with the chroma bisected down
/// until it fits sRGB, which keeps hue and lightness exactly rather than
/// bending them the way clamping would.
/// What the shader draws a sheet texel as, for this player.
///
/// The sheet carries no hue: a texel is saturation and lightness, and the hue
/// comes from the player's number. Mirrors `shade` and `player_hue` in
/// `grid.wgsl`, which is the one that has to be right — this only has to agree
/// with it.
pub fn shade(lightness: f32, saturation: f32, player: PlayerId) -> (u8, u8, u8) {
    const HUE_STEP: f32 = 0.618_034;
    const TAU: f32 = std::f32::consts::TAU;
    const MAX_CHROMA: f32 = 0.13;

    let hue = (player.0 as f32 * HUE_STEP).fract() * TAU;
    // Player zero is nobody, and nobody's ground is grey.
    let tier = if player.0 == 0 {
        0.0
    } else if player.0 % 2 == 1 {
        1.0
    } else {
        0.55
    };
    // Chroma tapers off at the ends, where there is no room for it.
    let taper = 1.0 - (2.0 * lightness - 1.0).abs().powi(2);
    let chroma = MAX_CHROMA * saturation * tier * taper;
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());

    let l_ = lightness + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = lightness - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = lightness - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let linear = [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ];
    let byte = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    (byte(linear[0]), byte(linear[1]), byte(linear[2]))
}

/// The colour of a player's cells, for a swatch beside their name.
pub fn player_colour(player: PlayerId) -> (u8, u8, u8) {
    shade(0.62, 1.0, player)
}

