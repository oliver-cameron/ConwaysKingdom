//! What the player is told about their own state.
//!
//! Read-only: a view of the client, not a place decisions are made. Everything
//! it needs arrives as arguments, so it has no opinion about where the numbers
//! came from and cannot change them.

use crate::client::desync::{Geiger, Level};
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
    /// Who holds how much ground, most first. Empty until the server has said.
    pub standing: &'a [(PlayerId, u32)],
    /// How badly this client and the server are disagreeing, as a decaying
    /// rate. Shown beside "connected" because that is the claim it qualifies:
    /// a link that is open and a link that is keeping up are two facts, and
    /// only the first of them was on screen.
    pub geiger: Geiger,
    /// Watching without a seat, which the HUD says for the whole visit rather
    /// than once: a spectator whose clicks do nothing needs to know why the
    /// first time and not the fifth.
    pub watching: bool,
}

/// Whether the panel shows what it shows for a developer rather than a player.
///
/// The cursor's cell, what the last click did, how many chunks are held and
/// drawn, the zoom, and the list of keys: every one of them earned its place
/// while something was being built, and none of them is what somebody playing
/// wants a third of their screen taken by. Off rather than deleted, because
/// each one is the fastest way back to a whole class of bug — a stuck
/// `pointer_on_ui` silently eats every click, and a click on empty ground that
/// takes nothing looks exactly like a click that never arrived.
const DEBUG: bool = false;

/// Draw it. Returns the rectangle it occupied, so the client knows what it
/// covered, and whether the way out was clicked.
pub fn show(
    ctx: &egui::Context,
    theme: &crate::client::views::theme::Theme,
    status: &Status<'_>,
) -> (Option<egui::Rect>, bool) {
    let mut back = false;
    let response = egui::Window::new("kingdom")
        .title_bar(false)
        .resizable(false)
        // Fixed, or dragging it would be indistinguishable from panning the
        // world underneath.
        .movable(false)
        .anchor(egui::Align2::LEFT_TOP, [theme.metrics.margin, theme.metrics.margin])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // The way out, in the row that is already about who you are
                // and where. A floating button would have to sit somewhere,
                // and everywhere it could sit is over the world.
                // Painted rather than typed: the arrow it used to be is not in
                // any font this client loads, because this client loads none,
                // so the one control whose job is to be recognised at a glance
                // was a square. See `icons::back`.
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::click());
                let palette = theme.palette;
                let ink = if response.hovered() { palette.text } else { palette.text_dim };
                ui.painter().rect_stroke(
                    rect,
                    theme.metrics.rounding,
                    egui::Stroke::new(
                        1.0,
                        if response.hovered() {
                            palette.line
                        } else {
                            palette.line.gamma_multiply(0.6)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                crate::client::views::icons::back(ui.painter(), rect.shrink(5.0), ink);
                if response.on_hover_text(words::BACK_HINT).clicked() {
                    back = true;
                }
                // The same colour the shader gives this player's cells, so the
                // swatch and the board cannot disagree about who is who.
                let (r, g, b) = player_colour(status.player);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                ui.heading(format!("Player {}", status.player.0));
            });

            ui.separator();
            ui.label(format!("Value  {}", status.value));
            ui.label(format!("Generation  {}", status.generation));
            if DEBUG {
                ui.label(format!(
                    "Chunks  {} held, {} drawn",
                    status.chunks_held, status.chunks_drawn
                ));
                ui.label(format!("Zoom  {:.1} px/cell", status.zoom));
            }

            standings(ui, theme, status);

            ui.separator();
            ui.horizontal(|ui| {
                if status.connected {
                    // Connected, and then how well. Silent until there has
                    // been something to be silent about: a link that has
                    // never slipped says nothing, because a reassurance
                    // nobody asked for is one more thing to read every frame.
                    let p = theme.palette;
                    let (word, colour) = match status.geiger.level() {
                        Level::Quiet if !status.geiger.ever() => (words::CONNECTED, p.good),
                        Level::Quiet => (crate::client::views::words::desync::SETTLED, p.good),
                        Level::Background => {
                            (crate::client::views::words::desync::BACKGROUND, p.good)
                        }
                        Level::Noticeable => {
                            (crate::client::views::words::desync::NOTICEABLE, p.warn)
                        }
                        Level::Alarming => (crate::client::views::words::desync::ALARMING, p.bad),
                    };
                    ui.colored_label(colour, word);
                } else {
                    ui.colored_label(theme.palette.warn, words::OFFLINE);
                }
                if let Some(room) = status.room {
                    ui.label(format!("· room {room}"));
                }
                if status.watching {
                    ui.colored_label(
                        theme.palette.accent,
                        crate::client::views::words::menu::watch::WATCHING,
                    );
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

            if DEBUG {
                ui.separator();
                ui.small(crate::client::views::words::desync::reading(
                    status.geiger.rate(),
                    status.geiger.total(),
                ));
                ui.small(format!(
                    "cursor  ({}, {})   {}",
                    status.cursor_cell.0,
                    status.cursor_cell.1,
                    if status.pointer_on_ui { words::OVER_PANEL } else { words::ON_WORLD }
                ));
                ui.small(format!("last  {}", status.last_action.unwrap_or(words::NOTHING_YET)));

                ui.separator();
                ui.small(format!("holding  {}", status.holding));
                for hint in words::HINTS {
                    ui.small(*hint);
                }
            }
        });
    (response.map(|r| r.response.rect), back)
}

/// Who is winning, as bars.
///
/// A bar rather than a number because the question is *who is ahead and by how
/// much*, and that is a comparison — six figures in a column have to be read
/// and subtracted, where six bars are one glance. The numbers are there beside
/// them for when the answer is close.
///
/// Scaled to the leader rather than to the world: what is being asked is how
/// the players compare with each other, and against the size of a boundless
/// world every bar would be a sliver.
///
/// Each bar is drawn in **its player's own colour**, the same one the shader
/// gives their cells, so a bar and the ground it counts cannot disagree about
/// whose it is.
fn standings(ui: &mut egui::Ui, theme: &crate::client::views::theme::Theme, status: &Status<'_>) {
    if status.standing.is_empty() {
        return;
    }
    ui.separator();
    ui.small(words::HOLDING);

    let most = status.standing.iter().map(|&(_, n)| n).max().unwrap_or(1).max(1) as f32;
    let width = ui.available_width().max(80.0);
    for &(player, held) in status.standing.iter().take(SHOWN) {
        let (r, g, b) = player_colour(player);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());

        // The full width in the faintest ink, so a short bar reads as a
        // fraction of something rather than as a stub floating in space.
        ui.painter().rect_filled(rect, 2.0, theme.palette.line);
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * (held as f32 / most), rect.height()),
        );
        ui.painter().rect_filled(filled, 2.0, egui::Color32::from_rgb(r, g, b));

        // Yours named, everybody else's numbered: on a board where every
        // player is a colour and a number, the one thing worth spelling out is
        // which of them is you.
        let label = if player == status.player {
            format!("you  {held}")
        } else {
            format!("{}  {held}", player.0)
        };
        ui.painter().text(
            rect.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(10.0),
            theme.palette.text,
        );
    }
    if status.standing.len() > SHOWN {
        ui.small(format!("+{} more", status.standing.len() - SHOWN));
    }
}

/// How many bars fit before the panel is a leaderboard rather than a HUD.
///
/// Thirty-one players can have been through a world, and a column of thirty-one
/// bars is a screen of its own. Whoever is winning is at the top, and you are
/// interested in the rest of the field only once you are in it.
const SHOWN: usize = 6;

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
    shade_at(
        lightness,
        saturation,
        player,
        (player.0 as f32 * crate::client::views::hue::STEP).fract(),
    )
}

/// The same, at a hue somebody else worked out — which is how a team's colour
/// reaches a swatch. See [`crate::client::views::hue`], which is the one place
/// a hue is decided and is handed to the shader as a whole table.
pub fn shade_at(lightness: f32, saturation: f32, player: PlayerId, turn: f32) -> (u8, u8, u8) {
    const TAU: f32 = std::f32::consts::TAU;
    const MAX_CHROMA: f32 = 0.13;

    let hue = turn * TAU;
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

/// The same, for a player whose team decides their hue.
pub fn team_colour(player: PlayerId, hues: &[f32; PlayerId::COUNT]) -> (u8, u8, u8) {
    shade_at(0.62, 1.0, player, hues[player.0 as usize])
}
