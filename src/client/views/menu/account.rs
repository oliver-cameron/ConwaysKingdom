//! You: your name, what you are rated, what you have played, who else is here,
//! and the key all of it hangs off.
//!
//! **One page, because they are one subject.** These were four things on the
//! home screen competing with the way in — a name field, a rating, a record, a
//! settings drawer — and the home screen's job is to get somebody into a game.
//! What a player is, is a place they visit occasionally and read carefully,
//! which is the opposite kind of screen.

use super::{words, Chose, Menu, Page, Where};
use crate::client::views::theme::Theme;

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let (m, p) = (theme.metrics, theme.palette);
    let mut chose = Chose::Nothing;
    // Whoever this server has said you are. `None` offline, and before a first
    // join, which is what the two arms below are for.
    let mine = menu.whoami.clone();

    ui.horizontal(|ui| {
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
    });
    ui.add_space(m.item_spacing);

    // **You, at the top: your face and your name**, rather than a heading
    // naming the category of page you are on. You know you opened your own
    // account; what is worth the space is which account it is.
    ui.horizontal(|ui| {
        let side = m.action_height * 1.4;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        match mine.as_ref() {
            // Derived from the key, so it is yours and nobody chose it — see
            // [`crate::client::views::face`].
            Some(who) => crate::client::views::face::show(ui.painter(), rect, who),
            // No server has named you yet, so there is no key to draw one
            // from — but an empty box is worse than a stand-in. Derived from
            // the name instead and drawn dim, so it is visibly provisional
            // rather than silently becoming somebody else's on your first
            // join.
            None => crate::client::views::face::show_placeholder(
                ui.painter(),
                rect,
                &menu.name,
                p.text_dim,
            ),
        }
        ui.add_space(m.item_spacing);
        ui.vertical(|ui| {
            let shown = menu.name.trim();
            ui.heading(if shown.is_empty() { words::account::TITLE } else { shown });
            match mine.as_ref() {
                Some(who) => {
                    ui.label(
                        egui::RichText::new(who.short())
                            .size(m.text_small)
                            .color(p.text_dim)
                            .monospace(),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new(words::account::UNNAMED)
                            .size(m.text_small)
                            .color(p.text_dim),
                    );
                }
            }
        });
    });
    ui.add_space(m.item_spacing * 2.0);

    // Here rather than on the way in: it is who you are, and it is the same
    // answer whichever world you end up in.
    ui.label(egui::RichText::new(words::home::WHO).size(m.text_small));
    ui.add(
        egui::TextEdit::singleline(&mut menu.name)
            .desired_width(f32::INFINITY)
            .hint_text(words::NAME_HINT),
    );
    ui.add_space(m.item_spacing * 2.0);

    // **Above the record rather than inside it**, because they answer
    // different questions and only one of them is comparable. The record is
    // what this client has done, kept in its own store; a rating is what a
    // *server* thinks of you against everybody else there. Folding the second
    // into the first would suggest the client had worked it out, which it must
    // never look like it can.
    if let Some(r) = menu.rating {
        ui.label(egui::RichText::new(words::account::RATED).size(m.text_small));
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
        // **Under the number rather than instead of it.** An unearned rating
        // is still the number you have, so it is shown at full size and
        // marked; hiding it answers "what am I rated" with a riddle.
        if r.provisional {
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(crate::client::views::words::provisional(r.games))
                    .size(m.text_small),
            );
        }
        ui.add_space(m.item_spacing);
    }

    let flat = |ui: &mut egui::Ui, label: &str| {
        crate::client::views::wide(
            ui,
            egui::RichText::new(label).size(m.text_small).color(p.text),
            m.button_height,
            p.surface,
        )
        .clicked()
    };

    if flat(ui, words::home::PROFILE) {
        chose = Chose::Profile;
    }
    // Only where a server has been reached — there is nobody to ask otherwise,
    // and a button that cannot work is worse than no button.
    if at.reached && flat(ui, words::home::PEOPLE) {
        menu.page = Page::People;
        // Asked on the way in, so the board is up before anybody types. An
        // empty query is the leaderboard.
        chose = Chose::FindPeople(String::new());
    }
    ui.add_space(m.item_spacing);

    ui.label(egui::RichText::new(words::home::RECORD).size(m.text_small));
    crate::client::views::record::show(ui, theme, &menu.games, &menu.record);

    ui.add_space(m.item_spacing * 2.0);
    let label = if menu.advanced { words::home::SETTINGS_HIDE } else { words::home::SETTINGS };
    if ui.small_button(label).clicked() {
        menu.advanced = !menu.advanced;
    }
    if menu.advanced {
        let what = super::settings::show(ui, theme, menu);
        if !matches!(what, Chose::Nothing) {
            chose = what;
        }
    }
    chose
}
