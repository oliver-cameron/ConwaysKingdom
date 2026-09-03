//! What the game's rules are doing here, and the switches for them.
//!
//! **Two, and they are two.** `free_hand` was one flag doing both, which is
//! the sort of thing that is fine until somebody wants one of them: placing
//! anywhere and placing for nothing are separate rules with separate reasons —
//! `net::may_place` is about where your influence reaches and `net::price` is
//! about what you can afford — and an experiment might reasonably want the map
//! open and the economy on, or the reverse.
//!
//! **Only in a laboratory**, which is a kind of room rather than a mode this
//! client is in. Everywhere else these are the rules of the game: a client
//! turning them off on its own would predict placements the server refuses,
//! which reads as the game being broken rather than as a setting having no
//! effect. The room holds them, so everybody in one sees the same board —
//! see [`crate::net::Rules`].

use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::client::views::Shown;

/// Which switch was thrown, if either was.
///
/// **An enum, because one press is one press.** It was a struct of two
/// `Option<bool>`s and a close flag, which says a frame can throw both at once
/// — it cannot — and made the caller write `told.anywhere.unwrap_or(current)`
/// to put back what nobody touched. The close flag went to
/// [`crate::client::views::panel`], which owns it for every panel.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Did {
    #[default]
    Nothing,
    Anywhere(bool),
    Free(bool),
}

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    rules: crate::net::Rules,
    open: &mut bool,
) -> Shown<Did> {
    let (p, m) = (theme.palette, theme.metrics);
    let (mut anywhere, mut free) = (rules.place_anywhere, rules.place_free);

    // Above the bar and to its right, which is where the square that opens it
    // is: a panel that appeared somewhere else would be a panel you have to
    // look for after pressing something.
    let where_ = crate::client::views::Panel {
        id: "rules",
        title: w().hotbar.rules,
        at: egui::Align2::RIGHT_BOTTOM,
        offset: [-m.margin, -(m.slot + m.margin * 4.0)],
    };
    crate::client::views::panel(ctx, theme, where_, open, |ui| {
        let mut did = Did::Nothing;
        if ui.checkbox(&mut anywhere, w().hotbar.anywhere).changed() {
            did = Did::Anywhere(anywhere);
        }
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(w().hotbar.anywhere_note).size(m.text_small),
        );

        ui.add_space(m.item_spacing);
        if ui.checkbox(&mut free, w().hotbar.free).changed() {
            did = Did::Free(free);
        }
        ui.colored_label(p.text_dim, egui::RichText::new(w().hotbar.free_note).size(m.text_small));
        did
    })
}
