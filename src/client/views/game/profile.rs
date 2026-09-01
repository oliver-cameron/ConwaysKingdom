//! What a server says about somebody, over the world rather than instead of it.
//!
//! **A panel and not a page**, which is the one design decision in the file.
//! A profile is looked at *from* somewhere — a name in a lobby, a bar in the
//! standings — and every one of those places is inside a world. A page would
//! answer "who is that" by taking you out of the game you were asking from,
//! and you would have to find your way back to the thing you were looking at.
//! So it sits over the board the way [`super::help`] and [`super::rules`] do.
//!
//! **Everything on it is the server's.** That is the line
//! [player profiles] draws: client state is self-asserted, so a rating you
//! keep is a rating you can type, and the same goes for how many matches
//! somebody has played. The one thing here a player chose is their name, and
//! it is shown as a name rather than as a fact — the fingerprint beside it is
//! the part that cannot be picked, and is what tells two people called alice
//! apart without either having to accept being alice2.
//!
//! [player profiles]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#player-profiles

use crate::client::views::theme::Theme;
use crate::client::views::words::profile as words;
use crate::client::views::Shown;

/// What the panel was told.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Did {
    pub close: bool,
}

/// What to draw, which is either an answer or the wait for one.
///
/// **Three states, not two.** A profile that has not arrived and one the
/// server has never heard of look the same as an empty panel and are not the
/// same thing, and the difference is exactly what somebody who clicked a name
/// wants to know: "asking" is a reason to wait and "not here" is not.
pub enum Look<'a> {
    /// Asked for, and the server has not answered yet.
    Asking,
    /// A person this server has never met, which is a real answer.
    Unknown,
    /// What it said.
    Found {
        it: &'a crate::net::Profile,
        /// Their colour in this room, if they are in it. A profile is looked
        /// at from a lobby or a standings bar, and the swatch is what joins
        /// the panel to the row it was opened from.
        hue: Option<egui::Color32>,
        /// Whether this is the client's own.
        mine: bool,
    },
}

pub fn show(ctx: &egui::Context, theme: &Theme, look: &Look) -> Shown<Did> {
    let (p, m) = (theme.palette, theme.metrics);
    let mut did = Did::default();

    let area = egui::Area::new("profile".into())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.6)
                .show(ui, |ui| {
                    ui.set_width(theme.panel_width(ctx.content_rect().width()));
                    ui.horizontal(|ui| {
                        ui.heading(words::TITLE);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(words::CLOSE).clicked() {
                                did.close = true;
                            }
                        });
                    });
                    ui.add_space(m.item_spacing);
                    match look {
                        Look::Asking => ui.colored_label(p.text_dim, words::ASKING),
                        Look::Unknown => ui.colored_label(p.text_dim, words::UNKNOWN),
                        Look::Found { it, hue, mine } => {
                            body(ui, theme, it, *hue, *mine);
                            ui.label("")
                        }
                    };
                });
        });
    Shown::new(area.response.rect, did)
}

fn body(
    ui: &mut egui::Ui,
    theme: &Theme,
    it: &crate::net::Profile,
    hue: Option<egui::Color32>,
    mine: bool,
) {
    let (p, m) = (theme.palette, theme.metrics);

    // The name, the swatch and the fingerprint on one line, because they are
    // one answer to "who is that". The swatch first: it is what the eye came
    // from, since the row that opened this was a colour before it was a name.
    ui.horizontal(|ui| {
        if let Some(hue) = hue {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(m.text_action, m.text_action),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(rect, m.rounding * 0.5, hue);
        }
        ui.label(egui::RichText::new(&it.name).size(m.text_action).color(p.text));
        // **Dim, and never absent.** It is not decoration and it is not the
        // headline: the name is what somebody reads and this is what settles
        // which of two of them it is.
        ui.colored_label(p.text_dim, egui::RichText::new(it.who.short()).size(m.text_small));
        if mine {
            ui.colored_label(p.text_dim, egui::RichText::new(words::YOU).size(m.text_small));
        }
    });
    ui.add_space(m.item_spacing);

    // **What this server can vouch for, and it says so.** A profile from one
    // server is not a profile: a server can only speak for what happened on
    // it, and a screen that did not say that would read as a record of a
    // person rather than of a visit.
    ui.colored_label(p.text_dim, egui::RichText::new(words::HERE).size(m.text_small));
    ui.add_space(m.item_spacing);

    ui.label(
        egui::RichText::new(crate::client::views::words::rating(it.rating))
            .size(m.text_action)
            .color(p.text),
    );
    if it.provisional {
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(crate::client::views::words::provisional(it.games))
                .size(m.text_small),
        );
    }
    ui.add_space(m.item_spacing);

    ui.label(words::matches(it.games));
    ui.label(words::best(it.best));
}
