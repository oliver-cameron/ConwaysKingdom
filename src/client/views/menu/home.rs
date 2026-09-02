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

    // **Three, in the middle, and nothing else.** This screen held a name
    // field, a rating, a record, two lookups and a settings drawer, all of
    // which are things you read rather than things you press — and the one
    // control anybody opens the game to use was underneath them. What a player
    // *is* now lives on [`super::account`], which is a page you visit
    // occasionally and read carefully; this one asks a single question and the
    // answer is one of three presses.
    ui.vertical_centered(|ui| {
        ui.add_space(m.item_spacing * 4.0);
        ui.heading(words::TITLE);
        ui.add_space(m.item_spacing * 5.0);
    });

    // Above the three, and only when there is one: a match you have already
    // joined is not a fourth way in, it is the way back to where you were.
    if at.waiting_in_a_match {
        if crate::client::views::wide(
            ui,
            egui::RichText::new(words::BACK_TO_MATCH).size(m.text_action).color(p.ground),
            m.action_height,
            p.accent,
        )
        .clicked()
        {
            chose = Chose::Resume;
        }
        ui.small(words::BACK_TO_MATCH_NOTE);
        ui.add_space(m.item_spacing * 2.0);
    }

    // **Play is the accent unless the match above took it.** One accent a
    // screen, on the thing you are meant to press next.
    let lead = !at.waiting_in_a_match;
    if crate::client::views::wide(
        ui,
        egui::RichText::new(words::home::PLAY).size(m.text_action).color(if lead {
            p.ground
        } else {
            p.text
        }),
        m.action_height,
        if lead { p.accent } else { p.surface },
    )
    .clicked()
    {
        // **Solo is not a fourth button**, because it is not a different
        // errand: the same form describes a world either way and answers
        // "make it here" or "make it on that server" depending on whether one
        // replied. So Play goes to the one screen and the branch happens
        // there — see [`super::play`], whose action reads "Play alone" until a
        // server has answered.
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

    ui.add_space(m.item_spacing);
    // **Your own name on your own button.** The account page is about you, and
    // a label saying who you are is worth more than one naming a category.
    // Falls back to the category before anybody has typed a name.
    let mine = menu.name.trim();
    let account = if mine.is_empty() { words::home::ACCOUNT.to_string() } else { mine.to_string() };
    if crate::client::views::wide(
        ui,
        egui::RichText::new(account).size(m.text_body).color(p.text),
        m.action_height,
        p.surface,
    )
    .clicked()
    {
        menu.page = Page::Account;
    }
    ui.add_space(m.item_spacing);
    if crate::client::views::wide(
        ui,
        egui::RichText::new(words::home::HOWTO).size(m.text_body).color(p.text),
        m.action_height,
        p.surface,
    )
    .clicked()
    {
        menu.page = Page::HowToPlay;
    }

    chose
}
