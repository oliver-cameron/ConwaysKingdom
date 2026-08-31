//! A laboratory rather than a match.
//!
//! **The simulation is the hard part and it is already done.** `sim` is a
//! deterministic cellular automaton with chunked storage and a step that is a
//! pure function of state and tick, and [its liveness is exactly B3/S23] — so
//! a pattern written down by somebody else runs here the way it runs anywhere.
//! What stands between that and Golly is not the simulation; it is that the
//! *game* is in the way — the server is the clock, you may only build where
//! your influence reaches, and placing costs money.
//!
//! So this page is mostly subtraction. It describes a world the way
//! [`super::alone`] does, and then says which of the game's rules to take off.
//!
//! [its liveness is exactly B3/S23]: crate::sim::World::step

use super::draft::{Ends, Shape};
use super::words;
use super::{Chose, Menu, Page};
use crate::client::views::theme::Theme;

pub fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let (p, m) = (theme.palette, theme.metrics);
    let mut chose = Chose::Nothing;

    ui.horizontal(|ui| {
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
        ui.heading(words::lab::TITLE);
    });
    ui.colored_label(p.text_dim, words::lab::NOTE);
    ui.add_space(m.item_spacing);

    let draft = menu.draft.get_or_insert_with(Default::default);
    // **A laboratory is boundless and never ends.** Both are game answers to
    // game questions: a torus is a shape a *match* wants so its ground is
    // finite and contested, and a victory condition is a way to win. Neither
    // means anything to somebody watching a pattern, and offering them would
    // be offering two ways to make an experiment stop being one.
    draft.shape = Shape::Boundless;
    draft.ends = Ends::Never;

    ui.label(egui::RichText::new(words::lab::RULES).size(m.text_small));
    ui.checkbox(&mut menu.lab_free_hand, words::lab::FREE_HAND);
    ui.colored_label(
        p.text_dim,
        egui::RichText::new(words::lab::FREE_HAND_NOTE).size(m.text_small),
    );

    ui.add_space(m.item_spacing);
    ui.colored_label(p.text_dim, egui::RichText::new(words::lab::CLOCK).size(m.text_small));

    ui.add_space(m.item_spacing * 2.0);
    if ui
        .add_sized(
            [ui.available_width(), m.action_height],
            egui::Button::new(
                egui::RichText::new(words::lab::OPEN).size(m.text_action).color(p.ground),
            )
            .fill(p.accent),
        )
        .clicked()
    {
        chose = Chose::Experiment { free_hand: menu.lab_free_hand };
    }
    chose
}
