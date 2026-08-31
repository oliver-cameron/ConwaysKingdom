//! Describing a world to play in on your own.
//!
//! **The same form the server screen uses**, because the questions are the
//! same ones: how big, does it end, how. What a server adds is a name, a
//! listing and other people, and [`play::make_form`] hides all three when
//! there is nobody to ask — so this page is that form with a heading over it
//! and a way back.
//!
//! A page rather than a button, which is what it was. "Play alone" went
//! straight into a world built from whatever the command line had said, so a
//! solitary game could not be a small torus and could not have a way to win —
//! and the form that asks those questions was reachable only by going to the
//! server screen and finding it under a room list nobody had asked for.

use super::play;
use super::words;
use super::{Chose, Menu, Page};
use crate::client::views::theme::Theme;

pub fn show(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu) -> Chose {
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    // Every screen has a way out, by pointer as well as by escape — the same
    // control the server screen uses, in the same place.
    ui.horizontal(|ui| {
        if ui.small_button(words::BACK).clicked() {
            menu.page = Page::Home;
        }
        ui.heading(words::alone::TITLE);
    });
    ui.colored_label(theme.palette.text_dim, words::alone::NOTE);
    ui.add_space(m.item_spacing);

    // `reached: false` is what makes it the solitary form: no name, no
    // listing, no sides, and its action plays the world here rather than
    // asking a server for one.
    if let Some(what) = play::make_column(ui, theme, menu, false) {
        chose = what;
    }
    chose
}
