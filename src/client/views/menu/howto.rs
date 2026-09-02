//! How to play, for somebody who has just arrived.
//!
//! **Not the key list.** `?` is a lookup table for somebody who already knows
//! what they are looking for; this is for somebody who does not know yet that
//! placing is confined to ground they already hold. Every entry here is a rule
//! people lose to before they learn it, and none of them is visible on the
//! board. The order is the order they bite in.
//!
//! The argument for each is in [game.md]; what is here is the shortest form
//! that still explains rather than asserts.
//!
//! [game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::{words, Chose, Menu, Page};
use crate::client::views::theme::Theme;

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);

    ui.horizontal(|ui| {
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
        ui.heading(words::howto::TITLE);
    });
    ui.colored_label(p.text_dim, words::howto::NOTE);
    ui.add_space(m.item_spacing);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (heading, body) in words::howto::RULES {
            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(*heading).size(m.text_body).color(p.text).strong());
            ui.label(egui::RichText::new(*body).size(m.text_small).color(p.text_dim));
        }
        // **Last, and set apart**, because it is the one entry that is a tip
        // rather than a rule — and it is the one that opens the game up.
        ui.add_space(m.item_spacing * 2.0);
        ui.separator();
        ui.add_space(m.item_spacing);
        ui.label(
            egui::RichText::new(words::howto::TIP_TITLE).size(m.text_body).color(p.accent).strong(),
        );
        ui.label(egui::RichText::new(words::howto::TIP).size(m.text_small).color(p.text_dim));
    });
    Chose::Nothing
}
