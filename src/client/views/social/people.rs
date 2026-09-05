//! Who else plays here.
//!
//! One list and one field. Empty, it is the leaderboard — the server answers
//! its best rated to a query that asks for nothing — and typing turns the same
//! list into a search. They are one question asked two ways, which is why
//! there is one screen and one message rather than a board and a finder.

use crate::client::views::menu::{Chose, Menu};
use crate::client::views::theme::Theme;
use crate::client::views::words::menu as words;
use crate::client::views::words::w;

/// A row is a person, and **the fingerprint is part of it rather than a
/// detail**. A name is self-chosen, so two people may both be alice and the
/// short form of the key is what tells them apart — the same thing
/// `net::Seat::label` prints in a lobby, so a row here and a row there name a
/// person the same way.
fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    rank: Option<usize>,
    who: &crate::net::Profile,
) -> Option<Chose> {
    let (m, p) = (theme.metrics, theme.palette);
    let mut chose = None;
    let response = ui
        .scope(|ui| {
            ui.horizontal(|ui| {
                // Only on the board. A search is not an ordering, so numbering
                // it would say the third match is worse than the second.
                if let Some(n) = rank {
                    ui.add_sized(
                        [28.0, m.row_height * 0.5],
                        egui::Label::new(
                            egui::RichText::new(format!("{n}"))
                                .size(m.text_small)
                                .color(p.text_dim)
                                .monospace(),
                        ),
                    );
                }
                // **From the fingerprint, not from the row.** A person's
                // colour has to be theirs wherever it is drawn, and a colour
                // taken from a position in a list would make the top of the
                // leaderboard one colour and a search for the same person
                // another. `PersonId` is what a person is, so it is what the
                // colour comes off — the same reasoning as the identicon in
                // planned.md#a-face, which this is the cheap version of.
                let (r, g, b) = crate::client::views::hue::player_colour(crate::sim::PlayerId(
                    crate::client::views::menu::person_hue(&who.who),
                ));
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(6.0, m.text_body), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 1.0, egui::Color32::from_rgb(r, g, b));
                ui.label(egui::RichText::new(&who.name).size(m.text_body).color(p.text));
                ui.label(
                    egui::RichText::new(who.who.short())
                        .size(m.text_small)
                        .color(p.text_dim)
                        .monospace(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // **Monospaced, because it is a figure in a column.** A
                    // rating set in a proportional face shuffles sideways as
                    // it changes, and a list of them never lines up.
                    ui.label(
                        egui::RichText::new(words::people::rating(who.rating, who.provisional))
                            .size(m.text_body)
                            .color(if who.provisional { p.text_dim } else { p.text })
                            .monospace(),
                    );
                });
            });
        })
        .response;
    let hit = ui.interact(response.rect, ui.id().with(&who.who.0), egui::Sense::click());
    if hit.hovered() {
        ui.painter().rect_filled(response.rect, m.rounding, p.surface_lift);
    }
    if hit.clicked() {
        chose = Some(Chose::LookAt(who.who.clone()));
    }
    chose
}

pub fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);
    let mut chose = Chose::Nothing;

    ui.horizontal(|ui| {
        if ui.small_button(w().menu.back).clicked() {
            menu.page = menu.page.back();
        }
        ui.heading(w().menu.people.title);
    });
    ui.colored_label(p.text_dim, w().menu.people.note);
    ui.add_space(m.item_spacing);

    // **Asked on every change and not on a button.** A search you have to
    // submit is a search people submit once and then retype; the cap on the
    // answer is what makes asking freely affordable.
    let field = ui.add_sized(
        [ui.available_width(), m.button_height],
        egui::TextEdit::singleline(&mut menu.finding)
            .hint_text(w().menu.people.hint)
            .margin(egui::Margin::symmetric(m.panel_padding as i8, 8)),
    );
    if field.changed() {
        chose = Chose::FindPeople(menu.finding.clone());
    }
    ui.add_space(m.item_spacing);

    let looking = !menu.finding.trim().is_empty();
    match &menu.people {
        // Nothing has come back yet. Said rather than left blank, because an
        // empty panel and a slow server look the same.
        None => {
            ui.colored_label(p.text_dim, w().menu.people.asking);
        }
        Some((_, found)) if found.is_empty() => {
            ui.colored_label(
                p.text_dim,
                if looking { w().menu.people.nobody } else { w().menu.people.nobody_yet },
            );
        }
        Some((_, found)) => {
            let found = found.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, who) in found.iter().enumerate() {
                    if let Some(what) = row(ui, theme, (!looking).then_some(i + 1), who) {
                        chose = what;
                    }
                }
            });
            // Only when the list is as long as the answer can be, so it is a
            // fact about this answer rather than a standing disclaimer.
            if found.len() >= crate::net::PEOPLE_MOST {
                ui.add_space(m.item_spacing);
                ui.colored_label(p.text_dim, words::people::capped(crate::net::PEOPLE_MOST));
            }
        }
    }
    chose
}
