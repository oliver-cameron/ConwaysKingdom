//! The things somebody does once, if ever.
//!
//! **Behind a press, and that is the point of the file.** A player key was on
//! the home screen in an editable field, which put the most destructive thing
//! in the client — a box where typing over what is there makes you a different
//! person — directly under the second thing anybody reads. Nothing here is a
//! way to play, and the home screen is only ways to play.
//!
//! Neither press acts on its own. Both set [`super::Ask`] and the confirmation
//! is drawn over the whole screen by [`super::show`], because both are
//! irreversible in the strongest sense available: whoever holds a key is that
//! person, so there is no copy anywhere to restore from and no account to ask.

use super::{words, Ask, Chose, Menu};
use crate::client::views::theme::Theme;

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;

    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(words::home::settings::KEY).size(m.text_small));
    if menu.key.is_empty() {
        // Nothing to show and nothing to paste over. A key is made on the
        // first join rather than at startup, so a client that has never
        // reached a server has none and saying so is the whole answer.
        ui.small(words::home::settings::KEY_NONE);
    } else {
        // Editable because it is also how another one is brought in: showing a
        // key and accepting one are the same question, and two boxes side by
        // side would be two answers to it. Typing here changes nothing until
        // the button below is pressed and the asking is answered.
        ui.add(egui::TextEdit::singleline(&mut menu.key).desired_width(f32::INFINITY));
        ui.small(words::home::settings::KEY_NOTE);
        // Offered only when the field says something else and that something
        // else reads as a key. A button that is always pressable, for a press
        // that usually means "become who I already am", is one that only ever
        // gets pressed by accident.
        let typed = crate::net::Key::read(&menu.key).ok().map(|k| k.written());
        let mine = crate::net::keep::key().map(|k| k.written());
        if let Some(typed) = typed.filter(|t| Some(t) != mine.as_ref()) {
            ui.add_space(m.item_spacing);
            if ui.button(words::home::settings::KEY_TAKE).clicked() {
                menu.asking = Some(Ask::UseKey(typed));
            }
        }
    }

    ui.add_space(m.item_spacing * 2.5);
    if ui
        .add(egui::Button::new(egui::RichText::new(words::home::settings::FORGET).color(p.bad)))
        .clicked()
    {
        menu.asking = Some(Ask::Forget);
    }
    ui.small(words::home::settings::FORGET_NOTE);

    Chose::Nothing
}

/// The question, over everything, with the consequence written out.
///
/// Over rather than beside: what is being confirmed is not a preference, and a
/// row of two buttons under a paragraph is something people press through. It
/// is drawn from [`super::show`] rather than from here so that it sits above
/// every screen, since the answer changes what the client *is* and not what it
/// is looking at.
pub(super) fn confirm(ctx: &egui::Context, theme: &Theme, ask: &Ask) -> Option<bool> {
    let p = theme.palette;
    let m = theme.metrics;
    let (title, note) = match ask {
        Ask::Forget => (words::home::settings::FORGET_ASK, words::home::settings::FORGET_ASK_NOTE),
        Ask::UseKey(_) => (words::home::settings::KEY_ASK, words::home::settings::KEY_ASK_NOTE),
    };

    let mut answer = None;
    egui::Area::new("confirm".into())
        .order(egui::Order::Foreground)
        .fade_in(false)
        .movable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.6)
                .show(ui, |ui| {
                    ui.set_width(theme.panel_width(ctx.content_rect().width()) * 0.7);
                    ui.heading(title);
                    ui.add_space(m.item_spacing);
                    ui.label(egui::RichText::new(note).size(m.text_small));
                    ui.add_space(m.item_spacing * 2.0);
                    ui.horizontal(|ui| {
                        // The safe answer first and the destructive one after
                        // it, so the button under the pointer on the way in is
                        // the one that changes nothing.
                        if ui.button(words::home::settings::CANCEL).clicked() {
                            answer = Some(false);
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(words::home::settings::CONFIRM).color(p.bad),
                            ))
                            .clicked()
                        {
                            answer = Some(true);
                        }
                    });
                });
        });
    // Escape is the other way out of anything in this client, and a question
    // nobody answers is answered "no".
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(false);
    }
    answer
}
