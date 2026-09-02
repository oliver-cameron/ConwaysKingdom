//! How to play, for somebody who has just arrived.
//!
//! **Not the key list.** `?` is a lookup table for somebody who already knows
//! what they are looking for; this is for somebody who does not yet know that
//! placing is confined to ground they already hold. Every entry is a rule
//! people lose to before they learn it, and none of them is visible on the
//! board. The order is the order they bite in.
//!
//! **Each one shows the cell it is about.** A page explaining what a mine does
//! while showing no mine is a page of assertions — and the art is right there,
//! tinted in the reader's own colour by the same sheet the hotbar draws from.
//!
//! The argument behind each rule is in [game.md]; what is here is the shortest
//! form that still explains rather than asserts.
//!
//! [game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::{words, Chose, Menu, Page, Where};
use crate::client::views::icons::Icons;
use crate::client::views::theme::Theme;
use crate::net::Placement;
use crate::sim::{Cell, PlayerId};

/// The cell each rule is about, in the order [`words::howto::RULES`] gives
/// them. Two lists that have to agree, which `a_face_for_every_rule` is what
/// makes checkable — the alternative is a placement in the string table, and
/// `words` holds strings.
const FACES: [Option<Placement>; 5] = [
    None,
    Some(Placement::Mine),
    Some(Placement::Turret),
    Some(Placement::Ice),
    Some(Placement::Payload),
];

/// The cell for a rule, or the player's own live cell where the rule is about
/// ground rather than about a machine.
fn face(
    painter: &egui::Painter,
    rect: egui::Rect,
    what: Option<Placement>,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) {
    let placement = what.unwrap_or(Placement::Life);
    match sheet {
        Some(sheet) => {
            let tile = placement.apply_to(Cell::DEAD, player).sprite();
            painter.image(sheet, rect, Icons::uv(tile), egui::Color32::WHITE);
        }
        // Before the sheet is registered, a square in the player's colour —
        // the page has to read on its first frame like any other.
        None => {
            let (r, g, b) = crate::client::views::hue::player_colour(player);
            painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
        }
    }
}

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);
    // Somebody reading this has usually not been given a number yet, and the
    // page is about *their* cells, so it is drawn in the first player's colour
    // rather than in grey.
    let me = PlayerId(1);

    ui.horizontal(|ui| {
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
        ui.heading(words::howto::TITLE);
    });
    ui.add_space(m.item_spacing * 0.5);
    ui.label(egui::RichText::new(words::howto::NOTE).size(m.text_body).color(p.text_dim));
    ui.add_space(m.item_spacing);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, (heading, body)) in words::howto::RULES.iter().enumerate() {
            // **A card each**, because these are five separate things somebody
            // has to hold on to rather than five paragraphs of one argument.
            // Run together they read as a wall and get skipped, which is the
            // one outcome a page like this cannot afford.
            egui::Frame::new()
                .fill(p.surface)
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        let side = m.slot;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                        face(ui.painter(), rect, FACES[i], me, at.sheet);
                        ui.add_space(m.item_spacing);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(*heading).size(m.text_body).color(p.text));
                            ui.add_space(2.0);
                            // Body at the same size as everything else on the
                            // screen. It was `text_small` and dim, which is
                            // how a footnote is set — and these are the page.
                            ui.label(
                                egui::RichText::new(*body).size(m.text_small).color(p.text_dim),
                            );
                        });
                    });
                });
            ui.add_space(m.item_spacing);
        }

        // **Last, and set apart**, because it is a tip rather than a rule —
        // and it is the one that opens the game up.
        ui.add_space(m.item_spacing);
        egui::Frame::new()
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.accent))
            .corner_radius(m.rounding)
            .inner_margin(m.panel_padding)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(words::howto::TIP_TITLE).size(m.text_body).color(p.accent),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(words::howto::TIP).size(m.text_small).color(p.text_dim),
                );
            });
        ui.add_space(m.item_spacing * 2.0);
    });
    Chose::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two lists in step: a cell for every rule and no cell without one.
    #[test]
    fn a_face_for_every_rule() {
        assert_eq!(FACES.len(), words::howto::RULES.len());
    }
}
