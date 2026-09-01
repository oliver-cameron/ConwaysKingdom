//! Who you are, what you have done, and the two ways in.
//!
//! The first screen, and deliberately not a screen about servers: a name, a
//! record, Play, and playing alone. What a server has is one press away and is
//! [`super::play`]'s business.

use super::{words, Chose, Menu, Page, Where};
use crate::client::views::theme::Theme;

/// one colour, and no second thing competing to be it.
pub(super) fn home(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    ui.heading(words::TITLE);
    ui.add_space(m.item_spacing * 2.0);

    // The name lives here rather than on the play screen, because it is who
    // you are and not part of choosing a world -- and because it is the same
    // answer whichever world you end up in.
    ui.label(egui::RichText::new(words::home::WHO).size(m.text_small));
    ui.add(
        egui::TextEdit::singleline(&mut menu.name)
            .desired_width(f32::INFINITY)
            .hint_text(words::NAME_HINT),
    );

    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(words::home::RECORD).size(m.text_small));
    // **Above the record rather than inside it**, because they answer
    // different questions and only one of them is comparable. What is in
    // `views::record` is what this client has done — its own history, kept in
    // its own store — and a rating is what a *server* thinks of you against
    // everybody else there. Folding the second into the first would suggest
    // the client had worked it out, which it must never look like it can.
    if let Some(r) = menu.rating {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(crate::client::views::words::rating(r.number))
                    .size(m.text_action)
                    .color(p.text),
            );
            // Said once, after the match that caused it. A number that moves
            // with no account of why is one people stop reading.
            if let Some(change) = r.change.filter(|c| *c != 0) {
                ui.label(
                    egui::RichText::new(words::home::rating_change(change))
                        .size(m.text_small)
                        .color(if change > 0 { p.good } else { p.bad }),
                );
            }
        });
        // **Under the number rather than beside it.** An unearned rating is
        // still the number you have, so it is shown at full size and marked;
        // dimming or hiding it would answer "what am I rated" with a riddle.
        if r.provisional {
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(crate::client::views::words::provisional(r.games))
                    .size(m.text_small),
            );
        }
        ui.add_space(m.item_spacing);
    }
    // **Under the rating, which is the thing it explains.** A number with no
    // way to ask what is behind it is a number people stop reading.
    //
    // Full width like everything else on this screen and unaccented, because
    // the one accent here is Play. A small button would be a word to aim at on
    // a phone.
    if ui
        .add_sized(
            [ui.available_width(), m.button_height],
            egui::Button::new(
                egui::RichText::new(words::home::PROFILE).size(m.text_small).color(p.text),
            )
            .fill(p.surface),
        )
        .clicked()
    {
        chose = Chose::Profile;
    }
    ui.add_space(m.item_spacing);
    crate::client::views::record::show(ui, theme, &menu.games, &menu.record);

    ui.add_space(m.item_spacing * 2.0);
    if ui
        .add_sized(
            [ui.available_width(), m.action_height],
            egui::Button::new(
                egui::RichText::new(words::home::PLAY).size(m.text_action).color(p.ground),
            )
            .fill(p.accent),
        )
        .clicked()
    {
        menu.page = Page::Play;
        // Never blank, and filled in **here** rather than while the field is
        // drawn. Refilling an empty field every frame is a field that cannot
        // be cleared: select all, press delete, and the example is back before
        // the next keystroke. Once, on the way in, is the whole of what was
        // wanted.
        #[cfg(not(target_arch = "wasm32"))]
        if menu.address.trim().is_empty() {
            menu.address = crate::client::views::game::default_address().to_string();
        }
        // Ask straight away rather than waiting for somebody to touch a field
        // they have no reason to touch: the address is remembered, or it is an
        // example, and either way the question is the same one.
        menu.typed_at = Some(0.0);
        menu.attempted = None;
    }

    // Offline sits here because it is a way to play, and because a player with
    // no server to reach should not have to walk through a screen about
    // servers to get to it.
    //
    // **Except when you are already enrolled in a match**, where the same
    // press means the opposite: you left a lobby to look at this screen, and
    // starting a solitary game is never what pressing the only other button
    // meant. It becomes the way back in.
    ui.add_space(m.item_spacing);
    let (label, note) = if at.waiting_in_a_match {
        (words::BACK_TO_MATCH, Some(words::BACK_TO_MATCH_NOTE))
    } else {
        // No note. "The rules are the same offline" was answering a question
        // nobody asks standing in front of a button that says Play alone.
        (words::ALONE, None)
    };
    if ui
        .add_sized(
            [ui.available_width(), m.button_height],
            egui::Button::new(egui::RichText::new(label).size(m.text_body)),
        )
        .clicked()
    {
        // **To the form, not into a world.** This used to build whatever the
        // command line had said and drop you in it, so a solitary game could
        // not be a small torus and could not end.
        if at.waiting_in_a_match {
            chose = Chose::Resume;
        } else {
            menu.page = Page::Alone;
        }
    }
    if let Some(note) = note {
        ui.small(note);
    }

    // **At the foot, and behind a press.** Everything above this is a way to
    // play; a player key is not one, and it used to sit here as an editable
    // field — which put the most destructive control in the client directly
    // under the second thing anybody reads.
    ui.add_space(m.item_spacing * 3.0);
    let label = if menu.advanced { words::home::SETTINGS_HIDE } else { words::home::SETTINGS };
    if ui.small_button(label).clicked() {
        menu.advanced = !menu.advanced;
    }
    if menu.advanced {
        chose = super::settings::show(ui, theme, menu);
    }

    chose
}
