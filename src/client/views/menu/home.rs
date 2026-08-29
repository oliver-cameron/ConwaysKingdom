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
            menu.address = crate::client::views::battle::default_address().to_string();
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
        chose = if at.waiting_in_a_match { Chose::Resume } else { Chose::Offline };
    }
    if let Some(note) = note {
        ui.small(note);
    }

    // **At the foot, because it is maintenance and not a way to play.**
    // Everything above it is what somebody came here to do; this is what they
    // came here to do once, on the day they moved to another browser.
    ui.add_space(m.item_spacing * 3.0);
    ui.label(egui::RichText::new(words::home::KEY).size(m.text_small));
    if menu.key.is_empty() && crate::net::keep::person(&menu.address).is_none() {
        // Nothing to show and nothing to paste over: a key is something a
        // server hands out, and this client has not been handed one.
        ui.small(words::home::KEY_NONE);
        return chose;
    }
    ui.add(egui::TextEdit::singleline(&mut menu.key).desired_width(f32::INFINITY));
    ui.small(words::home::KEY_NOTE);
    // Offered only when the field says something else and that something else
    // reads as a key. A button that is always pressable, for a press that
    // usually means "adopt what I already am", is a button that only ever
    // gets pressed by accident.
    let typed = crate::net::Person::parse(&menu.key).ok();
    let mine = crate::net::keep::person(&menu.address);
    if let Some(typed) = typed.filter(|t| Some(t) != mine.as_ref()) {
        ui.add_space(m.item_spacing);
        if ui.button(words::home::KEY_TAKE).clicked() {
            chose = Chose::UseKey(typed.key());
        }
    }

    chose
}
