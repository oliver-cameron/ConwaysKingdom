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
use crate::client::views::words::w;

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;

    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(w().menu.home.settings.key).size(m.text_small));
    let Some(factory) = crate::net::keep::secret() else {
        // A key is made at startup, so reaching this means the store refused
        // it or there was no entropy to make one from — a browser with no
        // `crypto`, or one with storage switched off. Rare, and worth saying
        // rather than showing a blank panel: the consequence is that this
        // client is somebody new everywhere it goes.
        ui.small(w().menu.home.settings.key_none);
        return Chose::Nothing;
    };

    // **What a server calls you, which is safe to look at.** The secret itself
    // is never shown here: it is the most destructive thing in the client and
    // a box that invites typing is the wrong place for it. This names you and
    // cannot be used to be you, because the server picked it and it says
    // nothing about the secret behind it.
    //
    // `None` until a server has been reached, because the server is what
    // issues it — see `net::auth`.
    match crate::net::keep::person() {
        Some(id) => {
            ui.label(egui::RichText::new(id.to_string()).monospace().size(m.text_small));
        }
        None => {
            ui.small(w().menu.home.settings.key_unseen);
        }
    }

    // Natively the key is a file and the honest thing to show is where it is:
    // whoever wants to back it up should copy the file, not select text out of
    // a game. A browser has no path to give, so it offers the file instead.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = crate::net::keep::secret_path() {
        ui.small(words::home::settings::key_lives_at(&path.display().to_string()));
    }

    ui.add_space(m.item_spacing);
    if ui.small_button(words::home::settings::reveal(menu.revealed)).clicked() {
        menu.revealed = !menu.revealed;
        // Loaded on the way open rather than held all the time: a secret that
        // is only in memory while somebody is looking at it is one that cannot
        // be typed over while they are not.
        menu.key = if menu.revealed { factory.written() } else { String::new() };
    }
    if menu.revealed {
        ui.small(w().menu.home.settings.key_note);
        ui.add(
            egui::TextEdit::multiline(&mut menu.key)
                .desired_width(f32::INFINITY)
                .desired_rows(4)
                .font(egui::TextStyle::Monospace),
        );
        // Offered only when what is in the box is a different key from the one
        // this client holds. A button that is always pressable, for a press
        // that usually means "become who I already am", is one that only ever
        // gets pressed by accident.
        let typed = crate::net::Secret::read(&menu.key).ok().map(|k| k.written());
        if let Some(typed) = typed.filter(|t| *t != factory.written()) {
            ui.add_space(m.item_spacing);
            if ui.button(w().menu.home.settings.key_take).clicked() {
                menu.asking = Some(Ask::UseKey(typed));
            }
        }
    }

    ui.add_space(m.item_spacing * 2.5);
    if ui
        .add(egui::Button::new(egui::RichText::new(w().menu.home.settings.forget).color(p.bad)))
        .clicked()
    {
        menu.asking = Some(Ask::Forget);
    }
    ui.small(w().menu.home.settings.forget_note);

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
        Ask::Forget => (w().menu.home.settings.forget_ask, w().menu.home.settings.forget_ask_note),
        Ask::UseKey(_) => (w().menu.home.settings.key_ask, w().menu.home.settings.key_ask_note),
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
                        if ui.button(w().menu.home.settings.cancel).clicked() {
                            answer = Some(false);
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new(w().menu.home.settings.confirm).color(p.bad),
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
