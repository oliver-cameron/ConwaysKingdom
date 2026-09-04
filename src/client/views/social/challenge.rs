//! Somebody wants a game.
//!
//! **An invitation, not a summons.** It is a panel over whatever you were
//! doing rather than a screen that replaces it, for the same reason a profile
//! is: being asked a question should not take away the thing you were in the
//! middle of. Nothing about it is modal — the world goes on stepping behind it
//! and the menu goes on being usable.
//!
//! **Two answers and both are sent.** A decline reaches the person who asked,
//! because the point of asking somebody is finding out, and silence is the one
//! answer that cannot be told apart from not having seen it. There is no third
//! button for later: a challenge already waits in the server's hands until its
//! target is heard from — see [`crate::server::rooms`] — so putting it off is
//! what closing the client does anyway.
//!
//! What accepting *is* is a `Join` on the room the server already made. The
//! match is an ordinary room and nothing in it knows how it began.

use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::client::views::Shown;
use crate::net::Profile;

/// What the player did with it.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub enum Did {
    #[default]
    Nothing,
    /// Join the room they made. The answer goes up as well, because "they are
    /// coming" is what the person who asked is waiting to hear.
    ///
    /// Carries nothing: whatever is holding the challenge holds the room, and
    /// a panel that repeated it would be a second copy to keep in step.
    Accept,
    Decline,
}

pub fn show(ctx: &egui::Context, theme: &Theme, from: &Profile, open: &mut bool) -> Shown<Did> {
    let (p, m) = (theme.palette, theme.metrics);
    crate::client::views::panel(
        ctx,
        theme,
        crate::client::views::Panel::middle("challenge", w().challenge.title),
        open,
        |ui| {
            let mut did = Did::Nothing;

            // Who, with a face and a rating rather than an id: this is a
            // question about a *person*, and the whole of what a profile is for
            // is telling two people with one name apart.
            super::profile::who_is_it(ui, theme, &from.name, Some(&from.who), None, false);
            ui.add_space(m.item_spacing);
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(crate::client::views::words::challenge::terms(
                    crate::net::CHALLENGE_SQUARES,
                ))
                .size(m.text_small),
            );

            ui.add_space(m.item_spacing * 2.0);
            if crate::client::views::wide(
                ui,
                egui::RichText::new(w().challenge.accept).size(m.text_body).color(p.ground),
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
                egui::RichText::new(w().challenge.decline).size(m.text_body).color(p.text),
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
