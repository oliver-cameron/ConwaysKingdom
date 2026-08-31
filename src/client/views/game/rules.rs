//! What the game's rules are doing here, and the switches for them.
//!
//! **Two, and they are two.** `free_hand` was one flag doing both, which is
//! the sort of thing that is fine until somebody wants one of them: placing
//! anywhere and placing for nothing are separate rules with separate reasons —
//! `net::may_place` is about where your influence reaches and `net::price` is
//! about what you can afford — and an experiment might reasonably want the map
//! open and the economy on, or the reverse.
//!
//! Only offline. Connected, these are the server's rules and a client turning
//! them off would predict placements the server refuses, which reads as the
//! game being broken rather than as a setting having no effect.

use crate::client::views::theme::Theme;
use crate::client::views::words::hotbar as words;
use crate::client::views::Shown;

/// What the panel was told.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Did {
    pub anywhere: Option<bool>,
    pub free: Option<bool>,
    pub close: bool,
}

pub fn show(ctx: &egui::Context, theme: &Theme, anywhere: bool, free: bool) -> Shown<Did> {
    let (p, m) = (theme.palette, theme.metrics);
    let mut did = Did::default();
    let (mut anywhere, mut free) = (anywhere, free);

    // Above the bar and to its right, which is where the square that opens it
    // is: a panel that appeared somewhere else would be a panel you have to
    // look for after pressing something.
    let area = egui::Area::new("rules".into())
        .anchor(egui::Align2::RIGHT_BOTTOM, [-m.margin, -(m.slot + m.margin * 4.0)])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding)
                .show(ui, |ui| {
                    ui.set_max_width(280.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(words::RULES).size(m.text_body));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(words::CLOSE).clicked() {
                                did.close = true;
                            }
                        });
                    });
                    ui.add_space(m.item_spacing);

                    if ui.checkbox(&mut anywhere, words::ANYWHERE).changed() {
                        did.anywhere = Some(anywhere);
                    }
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(words::ANYWHERE_NOTE).size(m.text_small),
                    );

                    ui.add_space(m.item_spacing);
                    if ui.checkbox(&mut free, words::FREE).changed() {
                        did.free = Some(free);
                    }
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(words::FREE_NOTE).size(m.text_small),
                    );
                });
        });
    Shown::new(area.response.rect, did)
}
