//! How the interface looks.
//!
//! Everything visual is in [`Palette`] and [`Metrics`], so a change of colours
//! or spacing is a change to two structs rather than a hunt through the views.
//! No view names a colour directly.
//!
//! The default takes after Pezzza's simulations: near-black ground so the world
//! is the brightest thing on screen, flat panels with no gradients or shadows,
//! one accent used sparingly, and thin separators rather than boxes. The
//! interface should read as an instrument beside the simulation, not a frame
//! around it.

/// Colours, as sRGB bytes.
///
/// Deliberately few. A palette that names every widget state ends up describing
/// egui rather than the game; these are roles, and the widget states are
/// derived from them.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Behind everything. Matches the world's clear colour so the panel does
    /// not sit in a lighter hole.
    pub ground: egui::Color32,
    /// Panel fill.
    pub surface: egui::Color32,
    /// Panel fill when hovered or active.
    pub surface_lift: egui::Color32,
    /// Hairlines and panel edges.
    pub line: egui::Color32,
    pub text: egui::Color32,
    pub text_dim: egui::Color32,
    /// Used once per panel at most: selection, and the thing being pointed at.
    pub accent: egui::Color32,
    pub good: egui::Color32,
    pub warn: egui::Color32,
    pub bad: egui::Color32,
}

/// Spacing and shape.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub rounding: f32,
    pub panel_padding: f32,
    pub item_spacing: f32,
    /// Edge of the screen to the nearest panel.
    pub margin: f32,
    /// A hotbar slot is square, this wide.
    pub slot: f32,
    /// How wide a panel that is the whole screen's business is — the menu,
    /// and every form on it. Fixed rather than sized by its contents: a panel
    /// that grows and shrinks as a list changes length moves the buttons out
    /// from under the hand reaching for them.
    pub panel_width: f32,
    /// The one control per screen you are meant to press next. Taller than
    /// the rest, and the only one that gets the accent.
    pub action_height: f32,
    /// Everything else that can be pressed.
    pub button_height: f32,
    /// One room in a list: two lines of text and room to point at.
    pub row_height: f32,
    /// Type, in points. Three sizes and no more — a fourth is always somebody
    /// nudging one of these rather than a decision.
    pub text_action: f32,
    pub text_body: f32,
    pub text_small: f32,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub palette: Palette,
    pub metrics: Metrics,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: Palette {
                ground: egui::Color32::from_rgb(9, 10, 13),
                surface: egui::Color32::from_rgb(20, 22, 28),
                surface_lift: egui::Color32::from_rgb(31, 34, 42),
                line: egui::Color32::from_rgb(48, 52, 63),
                text: egui::Color32::from_rgb(226, 229, 236),
                text_dim: egui::Color32::from_rgb(129, 136, 152),
                accent: egui::Color32::from_rgb(122, 196, 255),
                good: egui::Color32::from_rgb(126, 209, 148),
                warn: egui::Color32::from_rgb(226, 176, 96),
                bad: egui::Color32::from_rgb(232, 122, 112),
            },
            metrics: Metrics {
                rounding: 4.0,
                panel_padding: 10.0,
                item_spacing: 6.0,
                margin: 14.0,
                slot: 44.0,
                panel_width: 420.0,
                action_height: 40.0,
                button_height: 36.0,
                row_height: 54.0,
                text_action: 15.0,
                text_body: 14.0,
                text_small: 12.0,
            },
        }
    }
}

impl Theme {
    /// Push the theme into egui. Called once at startup and again whenever the
    /// theme changes, rather than every frame — egui keeps its style.
    pub fn apply(&self, ctx: &egui::Context) {
        let p = self.palette;
        let m = self.metrics;

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = p.surface;
        visuals.window_fill = p.surface;
        visuals.extreme_bg_color = p.ground;
        visuals.faint_bg_color = p.surface_lift;
        visuals.window_stroke = egui::Stroke::new(1.0, p.line);
        visuals.override_text_color = Some(p.text);
        visuals.hyperlink_color = p.accent;
        visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
        visuals.selection.stroke = egui::Stroke::new(1.0, p.accent);

        // Flat: no drop shadows anywhere. A shadow implies the panel floats
        // above the world, and it should read as part of the instrument.
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.popup_shadow = egui::epaint::Shadow::NONE;

        let rounding = egui::CornerRadius::same(m.rounding as u8);
        visuals.window_corner_radius = rounding;
        visuals.menu_corner_radius = rounding;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = rounding;
            widget.bg_stroke = egui::Stroke::new(1.0, p.line);
        }
        visuals.widgets.noninteractive.bg_fill = p.surface;
        visuals.widgets.inactive.bg_fill = p.surface;
        visuals.widgets.hovered.bg_fill = p.surface_lift;
        visuals.widgets.active.bg_fill = p.surface_lift;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.accent.gamma_multiply(0.6));
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, p.accent);

        // Both themes, so the interface does not change with the system's
        // light/dark setting -- the world is always dark, and a light panel
        // beside it would glare.
        ctx.all_styles_mut(|style| {
            style.visuals = visuals.clone();
            style.spacing.item_spacing = egui::vec2(m.item_spacing, m.item_spacing);
            style.spacing.window_margin = egui::Margin::same(m.panel_padding as i8);
            style.spacing.button_padding = egui::vec2(8.0, 5.0);
        });
    }

    /// The world's clear colour, so the two agree without either guessing.
    pub fn clear_color(&self) -> wgpu::Color {
        let [r, g, b, _] = self.palette.ground.to_normalized_gamma_f32();
        // The surface is sRGB and converts on write, so hand it linear.
        let to_linear = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        wgpu::Color {
            r: to_linear(r) as f64,
            g: to_linear(g) as f64,
            b: to_linear(b) as f64,
            a: 1.0,
        }
    }
}
