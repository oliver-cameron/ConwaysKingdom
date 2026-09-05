//! How to play, for somebody who has just arrived.
//!
//! **Not the key list.** `?` is a lookup table for somebody who already knows
//! what they are looking for; this is for somebody who does not yet know that
//! placing is confined to ground they already hold. Every entry is a rule
//! people lose to before they learn it, and none of them is visible on the
//! board. The order is the order they bite in.
//!
//! **Each one shows the cell it is about.** A page explaining what a factory does
//! while showing no factory is a page of assertions — and the art is right there,
//! tinted in the reader's own colour by the same sheet the hotbar draws from.
//!
//! The argument behind each rule is in [game.md]; what is here is the shortest
//! form that still explains rather than asserts.
//!
//! [game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::{Chose, Menu, Where};
use crate::client::views::icons::Icons;
use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::net::Placement;
use crate::sim::{Cell, PlayerId};

/// The cell each rule is about, in the order [`w().menu.howto.rules`] gives
/// them. Two lists that have to agree, which `a_face_for_every_rule` is what
/// makes checkable — the alternative is a placement in the string table, and
/// `words` holds strings.
const FACES: [Option<Placement>; 5] = [
    None,
    Some(Placement::Factory),
    Some(Placement::Turret),
    Some(Placement::Ice),
    Some(Placement::Dynamite),
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

/// A block of prose that actually wraps.
///
/// **Width has to be handed to it.** Inside a horizontal layout egui has no
/// bound to wrap against, so a paragraph set beside a picture ran on in one
/// line and pushed the card wider than the screen. Allocating the remaining
/// width before writing into it is what makes these read as paragraphs.
fn column(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let rest = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(rest, 0.0),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_max_width(rest);
            contents(ui);
        },
    );
}

/// One card: a panel with padding, at the full width of the page.
fn card(ui: &mut egui::Ui, theme: &Theme, accent: bool, contents: impl FnOnce(&mut egui::Ui)) {
    let (m, p) = (theme.metrics, theme.palette);
    let mut frame =
        egui::Frame::new().fill(p.surface).corner_radius(m.rounding).inner_margin(m.panel_padding);
    if accent {
        frame = frame.stroke(egui::Stroke::new(1.0, p.accent));
    }
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        contents(ui);
    });
    ui.add_space(m.item_spacing);
}

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);
    // Somebody reading this has usually not been given a number yet, and the
    // page is about *their* cells, so it is drawn in the first player's colour
    // rather than in grey.
    let me = PlayerId(1);

    ui.horizontal(|ui| {
        if ui.small_button(w().menu.back).clicked() {
            menu.page = menu.page.back();
        }
        ui.heading(w().menu.howto.title);
    });
    ui.add_space(4.0);
    column(ui, |ui| {
        ui.label(egui::RichText::new(w().menu.howto.note).size(m.text_small).color(p.text_dim));
    });
    ui.add_space(m.item_spacing);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, (heading, body)) in w().menu.howto.rules.iter().enumerate() {
            // **A card each**, because these are five separate things somebody
            // has to hold on to rather than five paragraphs of one argument.
            // Run together they read as a wall and get skipped, which is the
            // one outcome a page like this cannot afford.
            card(ui, theme, false, |ui| {
                ui.horizontal_top(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(m.slot, m.slot), egui::Sense::hover());
                    face(ui.painter(), rect, FACES[i], me, at.sheet);
                    ui.add_space(m.panel_padding);
                    column(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*heading).size(m.text_body).color(p.text).strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(*body).size(m.text_small).color(p.text_dim));
                    });
                });
            });
        }

        // **The patches, and they are optional by being here.** A scroll view
        // is the right home for something somebody may want and may not: five
        // rules and out is a complete visit, and so is stopping to find out
        // what a factory actually does. See [`super::tutorial`].
        ui.add_space(m.item_spacing);
        for (i, (heading, body)) in w().menu.tutorial.lessons.iter().enumerate() {
            let Some(patch) = menu.patches.get_mut(i) else { break };
            patch.tick(at.now);
            card(ui, theme, false, |ui| {
                column(ui, |ui| {
                    ui.label(
                        egui::RichText::new(*heading).size(m.text_body).color(p.text).strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(*body).size(m.text_small).color(p.text_dim));
                    ui.add_space(m.item_spacing);
                });
                super::tutorial::show(ui, theme, patch, at.sheet);
            });
            // A patch that is running wants the next frame, and nothing else on
            // this page does.
            if patch.running {
                ui.ctx().request_repaint();
            }
        }

        // **Set apart**, because it is a tip rather than a rule — and it is the
        // one that opens the game up.
        ui.add_space(m.item_spacing);
        card(ui, theme, true, |ui| {
            ui.label(
                egui::RichText::new(w().menu.howto.tip_title)
                    .size(m.text_body)
                    .color(p.accent)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(w().menu.howto.tip).size(m.text_small).color(p.text_dim));
        });

        // **And a word about Conway, at the end.** The rule underneath all of
        // this is his and he did not want to be remembered for it, so this says
        // what he would rather you looked up — briefly, linking out rather than
        // explaining, and not sentimental. Somebody who has read to the bottom
        // of a page about the Game of Life is exactly the person who should be
        // told there is far more.
        ui.add_space(m.item_spacing * 2.0);
        ui.separator();
        ui.add_space(m.item_spacing);
        column(ui, |ui| {
            ui.label(egui::RichText::new(w().menu.conway.title).size(m.text_body).color(p.text));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(w().menu.conway.body).size(m.text_small).color(p.text_dim),
            );
            ui.add_space(m.item_spacing);
            for (name, what, url) in w().menu.conway.work {
                ui.hyperlink_to(
                    egui::RichText::new(*name).size(m.text_small).color(p.accent),
                    *url,
                );
                ui.label(egui::RichText::new(*what).size(m.text_small).color(p.text_dim));
                ui.add_space(m.item_spacing * 0.75);
            }
        });
        ui.add_space(m.item_spacing * 3.0);
    });
    Chose::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two lists in step: a cell for every rule and no cell without one.
    #[test]
    fn a_face_for_every_rule() {
        assert_eq!(FACES.len(), w().menu.howto.rules.len());
    }
}
