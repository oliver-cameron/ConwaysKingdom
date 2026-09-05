//! The groups you are in, and the worlds only they can see.
//!
//! A party is a **list of people with a private set of worlds** — see
//! [`crate::server::parties`] for what that is and why it is a list rather
//! than a code. This page is one list of parties, and inside the one picked
//! out, its people and its worlds. A world here joins the way a row in the
//! room list does, by its id, so there is one way into a world rather than two.
//!
//! **Asking somebody in is not on this page.** An invitation names a person,
//! and the place a person is named is their profile — a row on the people
//! page, a name in a lobby — so that is where the button is, one per party you
//! are in that they are not. The note at the top says so.
//!
//! Read-only about the server, like every view here: it holds what it was told
//! and returns what was chosen.

use crate::client::views::menu::{Chose, Menu};
use crate::client::views::theme::Theme;
use crate::client::views::words::menu as words;
use crate::client::views::words::w;
use crate::net::PartyInfo;

pub fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);
    let mut chose = Chose::Nothing;

    ui.horizontal(|ui| {
        if ui.small_button(w().menu.back).clicked() {
            menu.page = menu.page.back();
        }
        ui.heading(w().menu.parties.title);
    });
    ui.colored_label(p.text_dim, w().menu.parties.note);
    ui.add_space(m.item_spacing);

    // **One accent to the column, on whatever the next step is.** With the
    // listing back and nothing in it, that step is making a party; with a
    // party to pick it is the Join inside the selection, so Make takes the
    // surface fill and the column keeps the one accent the room list keeps.
    // Before the listing arrives there is nothing yet to point at.
    let nothing_yet = menu.parties.as_ref().is_some_and(Vec::is_empty);
    let (fill, ink) = if nothing_yet { (p.accent, p.ground) } else { (p.surface, p.text) };
    ui.horizontal(|ui| {
        let go = ui.add_sized(
            [m.action_height * 1.4, m.button_height],
            egui::Button::new(
                egui::RichText::new(w().menu.parties.make).size(m.text_small).color(ink),
            )
            .fill(fill),
        );
        let field = ui.add_sized(
            [ui.available_width(), m.button_height],
            egui::TextEdit::singleline(&mut menu.party_name).hint_text(w().menu.parties.name_hint),
        );
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (go.clicked() || entered) && !menu.party_name.trim().is_empty() {
            chose = Chose::MakeParty(menu.party_name.trim().to_string());
            menu.party_name.clear();
        }
    });
    ui.add_space(m.item_spacing * 2.0);

    match &menu.parties {
        None => {
            ui.colored_label(p.text_dim, w().menu.parties.asking);
        }
        Some(parties) if parties.is_empty() => {
            ui.colored_label(p.text_dim, w().menu.parties.none_yet);
        }
        Some(parties) => {
            let parties = parties.clone();
            for party in &parties {
                let selected = menu.selected_party.as_ref() == Some(&party.id);
                match row(ui, theme, party, selected) {
                    Picked::Nothing => {}
                    Picked::Select => {
                        menu.selected_party = if selected { None } else { Some(party.id.clone()) };
                    }
                    Picked::Join(room) => chose = Chose::Join(room),
                    Picked::NewWorld => {
                        menu.describe_for_party(party.id.clone(), party.name.clone());
                    }
                    Picked::Leave => {
                        menu.selected_party = None;
                        chose = Chose::LeaveParty(party.id.clone());
                    }
                }
            }
        }
    }
    chose
}

/// What a party row was pressed for.
enum Picked {
    Nothing,
    Select,
    /// One of its worlds, by id.
    Join(crate::net::RoomId),
    /// Into the make-a-world form with this party as the answer to who.
    NewWorld,
    Leave,
}

/// One party: its name and how much is in it, and — when it is the one
/// selected — its people, its worlds and what can be done with it. The room
/// list's shape, for the reason the room list has it: a row of controls on
/// every entry is a list twice as busy to read.
fn row(ui: &mut egui::Ui, theme: &Theme, party: &PartyInfo, selected: bool) -> Picked {
    let (m, p) = (theme.metrics, theme.palette);
    let mut picked = Picked::Nothing;

    egui::Frame::new()
        .fill(if selected { p.surface_lift } else { p.surface })
        .stroke(egui::Stroke::new(1.0, if selected { p.accent } else { p.line }))
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding * 0.6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(&party.name).size(m.text_body));
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(words::parties::summary(
                    party.members.len(),
                    party.rooms.len(),
                ))
                .size(m.text_small),
            );
            // Taken before the controls exist, so it covers none of them —
            // the room list learned this the hard way.
            let head = ui.min_rect();
            if ui.interact(head, ui.id().with(&party.id.0), egui::Sense::CLICK).clicked() {
                picked = Picked::Select;
            }
            if !selected {
                return;
            }

            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(w().menu.parties.members).size(m.text_small));
            for member in &party.members {
                ui.horizontal(|ui| {
                    // The colour is theirs, off the fingerprint, as it is on
                    // the people page.
                    let (r, g, b) = crate::client::views::hue::player_colour(crate::sim::PlayerId(
                        crate::client::views::menu::person_hue(&member.who),
                    ));
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, m.text_body), egui::Sense::hover());
                    ui.painter().rect_filled(swatch, 1.0, egui::Color32::from_rgb(r, g, b));
                    // The name as it was given, and nothing where there is
                    // none: the fingerprint beside it is what tells two
                    // people apart, here as on the people page. Standing the
                    // reader's own name in for a missing one made every
                    // nameless member read as the reader.
                    ui.label(egui::RichText::new(&member.name).size(m.text_body).color(p.text));
                    ui.label(
                        egui::RichText::new(member.who.short())
                            .size(m.text_small)
                            .color(p.text_dim)
                            .monospace(),
                    );
                    if member.online {
                        ui.colored_label(
                            p.good,
                            egui::RichText::new(w().menu.parties.online).size(m.text_small),
                        );
                    }
                });
            }

            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(w().menu.parties.worlds).size(m.text_small));
            if party.rooms.is_empty() {
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(w().menu.parties.no_worlds).size(m.text_small),
                );
            }
            for room in &party.rooms {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&room.name).size(m.text_body));
                    ui.colored_label(
                        if room.players > 0 { p.good } else { p.text_dim },
                        egui::RichText::new(crate::client::views::menu::players(room.players))
                            .size(m.text_small),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [m.action_height * 1.4, m.button_height],
                                egui::Button::new(
                                    egui::RichText::new(w().menu.watch.join)
                                        .size(m.text_small)
                                        .color(p.ground),
                                )
                                .fill(p.accent),
                            )
                            .clicked()
                        {
                            picked = Picked::Join(room.id.clone());
                        }
                    });
                });
            }

            ui.add_space(m.item_spacing);
            ui.horizontal_top(|ui| {
                ui.set_min_height(m.button_height);
                let each = (ui.available_width() - m.item_spacing) / 2.0;
                if ui
                    .add_sized(
                        [each, m.button_height],
                        egui::Button::new(
                            egui::RichText::new(w().menu.parties.new_world).size(m.text_small),
                        ),
                    )
                    .clicked()
                {
                    picked = Picked::NewWorld;
                }
                if ui
                    .add_sized(
                        [each, m.button_height],
                        egui::Button::new(
                            egui::RichText::new(w().menu.parties.leave).size(m.text_small),
                        ),
                    )
                    .clicked()
                {
                    picked = Picked::Leave;
                }
            });
        });

    picked
}
