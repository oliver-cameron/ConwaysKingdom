//! What the player is told about their own state.
//!
//! Read-only: a view of the client, not a place decisions are made. Everything
//! it needs arrives as arguments, so it has no opinion about where the numbers
//! came from and cannot change them.

use crate::sim::PlayerId;

/// What the HUD shows. Assembled by the client each frame.
pub struct Status<'a> {
    pub player: PlayerId,
    pub value: i32,
    pub generation: u64,
    pub chunks_held: usize,
    pub chunks_drawn: u32,
    pub zoom: f32,
    pub connected: bool,
    /// Why the last action was refused, if it was.
    pub notice: Option<&'a str>,
}

pub fn show(ctx: &egui::Context, status: &Status<'_>) {
    egui::Window::new("kingdom")
        .title_bar(false)
        .resizable(false)
        // Fixed, or dragging it would be indistinguishable from panning the
        // world underneath.
        .movable(false)
        .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
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
            if status.connected {
                ui.colored_label(egui::Color32::from_rgb(120, 210, 140), "connected");
            } else {
                ui.colored_label(egui::Color32::from_rgb(220, 170, 90), "offline");
            }
            if let Some(notice) = status.notice {
                ui.colored_label(egui::Color32::from_rgb(230, 120, 110), notice);
            }

            ui.separator();
            ui.small("left click: take a cell   right click: place one");
            ui.small("drag or arrows to pan, wheel or pinch to zoom");
        });
}

/// The colour the shader gives a player, computed the same way so the HUD
/// swatch matches the cells on the board. OKLab with the chroma bisected down
/// until it fits sRGB, which keeps hue and lightness exactly rather than
/// bending them the way clamping would.
pub fn player_colour(player: PlayerId) -> (u8, u8, u8) {
    const HUE_STEP: f32 = 0.618_034;
    const TAU: f32 = std::f32::consts::TAU;

    let hue = (player.0 as f32 * HUE_STEP).fract() * TAU;
    let saturation = if player.0 % 2 == 1 { 1.0 } else { 0.55 };
    let lightness = 0.62f32;

    let oklab_to_linear = |l: f32, a: f32, b: f32| {
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
        let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        [
            4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
            -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
            -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
        ]
    };
    let inside = |c: [f32; 3]| c.iter().all(|v| (-0.0005..=1.0005).contains(v));

    let chroma = 0.30 * saturation * (1.0 - (2.0 * lightness - 1.0).abs());
    let (dx, dy) = (hue.cos(), hue.sin());
    let mut scale = 1.0;
    if !inside(oklab_to_linear(lightness, chroma * dx, chroma * dy)) {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..8 {
            let mid = (lo + hi) * 0.5;
            let c = chroma * mid;
            if inside(oklab_to_linear(lightness, c * dx, c * dy)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        scale = lo;
    }
    let c = chroma * scale;
    let linear = oklab_to_linear(lightness, c * dx, c * dy);

    // egui takes sRGB bytes, so encode; the shader hands linear to a surface
    // that does this in hardware.
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    (encode(linear[0]), encode(linear[1]), encode(linear[2]))
}

