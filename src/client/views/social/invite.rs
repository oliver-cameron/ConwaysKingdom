//! Somebody holds a door open.
//!
//! The challenge panel's shape, for a different question. A challenge is a game
//! made for two; this is a door — a private world, or a party — that somebody
//! has opened for you by name. It is a panel over whatever you were doing for
//! the reason [`super::challenge`] is: being offered something should not take
//! away the thing you were in the middle of.
//!
//! **Accepting is the join, and declining is closing the panel.** Nothing goes
//! back on a decline, and that is the difference from a challenge: a challenge
//! is a question, and silence to a question cannot be told from not having
//! seen it. An invitation is an offer, and it stands on the server whether or
//! not it was looked at — the door stays open, and the person who opened it is
//! not owed an answer for having done so.

use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::client::views::Shown;
use crate::net::Profile;

/// What the player did with it.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub enum Did {
    #[default]
    Nothing,
    /// Go through. Carries nothing, for the reason a challenge's does not:
    /// whatever holds the invitation holds the door it opens.
    Accept,
    Decline,
}

/// `terms` is what the door opens onto, in a sentence — a room by name, or a
/// party by name — because the panel is the same for both and the sentence is
/// the whole of what differs.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    from: &Profile,
    terms: &str,
    open: &mut bool,
) -> Shown<Did> {
    let (p, m) = (theme.palette, theme.metrics);
    crate::client::views::panel(
        ctx,
        theme,
        crate::client::views::Panel::middle("invite", w().invite.title),
        open,
        |ui| {
            let mut did = Did::Nothing;

            super::profile::who_is_it(ui, theme, &from.name, Some(&from.who), None, false);
            ui.add_space(m.item_spacing);
            ui.colored_label(p.text_dim, egui::RichText::new(terms).size(m.text_small));

            ui.add_space(m.item_spacing * 2.0);
            if crate::client::views::wide(
                ui,
                egui::RichText::new(w().invite.accept).size(m.text_body).color(p.ground),
                m.action_height,
                p.accent,
            )
            .clicked()
            {
                did = Did::Accept;
            }
            ui.add_space(m.item_spacing);
            if crate::client::views::wide(
                ui,
                egui::RichText::new(w().invite.decline).size(m.text_body).color(p.text),
                m.button_height,
                p.surface,
            )
            .clicked()
            {
                did = Did::Decline;
            }
            did
        },
    )
}
