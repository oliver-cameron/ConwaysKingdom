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
        /// **Your own diary**, when the profile is yours. `None` for anybody
        /// else's, and that is a rule rather than an omission: a server can
        /// only vouch for what happened on it, and what this client has played
        /// elsewhere is not something anybody else should be shown as a fact.
        ///
        /// Shown beside the server's count deliberately. The two disagreeing
        /// is fine and readable — one is "what I have played" and the other is
        /// "what I have played *here*".
        yours: Option<&'a crate::client::record::Summary>,
    },
}

pub fn show(ctx: &egui::Context, theme: &Theme, look: &Look, open: &mut bool) -> Shown<()> {
    let p = theme.palette;
    crate::client::views::panel(
        ctx,
        theme,
        crate::client::views::Panel::middle("profile", words::TITLE),
        open,
        |ui| match look {
            Look::Asking => {
                ui.colored_label(p.text_dim, words::ASKING);
            }
            Look::Unknown => {
                ui.colored_label(p.text_dim, words::UNKNOWN);
            }
            Look::Found { it, hue, yours } => body(ui, theme, it, *hue, *yours),
        },
    )
}

fn body(
    ui: &mut egui::Ui,
    theme: &Theme,
    it: &crate::net::Profile,
    hue: Option<egui::Color32>,
    yours: Option<&crate::client::record::Summary>,
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
        if yours.is_some() {
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
    curve(ui, theme, &it.history);
    ui.add_space(m.item_spacing);

    ui.label(words::matches(it.games));
    ui.label(words::best(it.best));

    // **And your own diary under it, when it is yours.** Two counts that sound
    // like the same thing and are not: above is what this server has seen you
    // do, below is every game this client has played anywhere. They disagree,
    // and the two headings are what makes that readable rather than a bug.
    let Some(mine) = yours else { return };
    ui.add_space(m.item_spacing * 2.0);
    ui.separator();
    ui.colored_label(p.text_dim, egui::RichText::new(words::EVERYWHERE).size(m.text_small));
    ui.add_space(m.item_spacing);
    if mine.any() {
        ui.label(words::played(mine.games, mine.won));
        ui.label(words::best(mine.best as usize));
    } else {
        ui.colored_label(p.text_dim, crate::client::views::words::record::NOTHING_YET);
    }
}

/// **Where the rating has been**, as a line rather than a figure.
///
/// A rating means nothing on its own — only differences do — so the most
/// useful thing beside the number is the number twenty matches ago. One point
/// per settled match, oldest at the left.
///
/// Drawn rather than charted: there are no axes and no grid, because the shape
/// is the whole message and a scale would be four more numbers to read on a
/// panel that already has five. The two ends are labelled instead, which is
/// the one comparison anybody actually makes.
fn curve(ui: &mut egui::Ui, theme: &Theme, history: &[i32]) {
    let (p, m) = (theme.palette, theme.metrics);
    // Two points is the fewest that can be a line. One match is a dot, and a
    // dot says less than the figure above it already does.
    if history.len() < 2 {
        return;
    }
    ui.add_space(m.item_spacing);
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, m.button_height), egui::Sense::hover());

    let (lo, hi) = history.iter().fold((i32::MAX, i32::MIN), |(a, b), &n| (a.min(n), b.max(n)));
    // A flat run is a real thing to have had, and scaling by its range would
    // be dividing by nought — so it draws down the middle rather than not at
    // all.
    let span = (hi - lo).max(1) as f32;
    let step = rect.width() / (history.len() - 1) as f32;
    let at = |i: usize, n: i32| {
        let t = if hi == lo { 0.5 } else { (n - lo) as f32 / span };
        egui::pos2(rect.left() + i as f32 * step, rect.bottom() - t * rect.height())
    };
    let points: Vec<egui::Pos2> = history.iter().enumerate().map(|(i, &n)| at(i, n)).collect();

    // The ground it sits on, so a line low in the box reads as low rather than
    // as floating.
    ui.painter().rect_filled(rect, m.rounding * 0.5, p.surface_lift);
    ui.painter().add(egui::Shape::line(points.clone(), egui::Stroke::new(1.5, p.accent)));
    // And where it ended, because that is the number above it and this is what
    // joins the two.
    if let Some(&last) = points.last() {
        ui.painter().circle_filled(last, 2.5, p.accent);
    }

    ui.horizontal(|ui| {
        ui.colored_label(p.text_dim, egui::RichText::new(lo.to_string()).size(m.text_small));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(p.text_dim, egui::RichText::new(hi.to_string()).size(m.text_small));
        });
    });
}
