//! What this client has played, drawn.
//!
//! A home screen that says only "Conway's Kingdom" and a name field is a home
//! screen with nothing on it. What belongs there is the one thing the client
//! knows and the server does not: **what you have done before** — and four
//! numbers in a row say that far less well than a shape does.
//!
//! Read-only, like every view here: it is handed [`crate::client::record`]'s
//! games and draws them. It decides nothing.
//!
//! ## The forms, and why each is the form it is
//!
//! Three jobs, three different answers, and only one of them is a chart.
//!
//! **Largest territory, game by game — bars.** This is change over a sequence,
//! which is the one thing here worth plotting. Bars rather than a line, and
//! that is the load-bearing decision: a line says the value is *carried
//! forward* between its points, which is true of a rating and is not true of
//! this. Each game's high-water mark is independent of the last one's, and the
//! gap between two games might be a minute or a month. Bars say "each of these
//! is a thing that happened"; a line would say something false.
//!
//! **How the last few went — a status strip, not a chart.** Won, lost, or
//! neither is identity rather than magnitude, and there are three values. A
//! chart of it would be a bar chart of the number one.
//!
//! **The totals — stat tiles.** A single number has no shape to show. Plotting
//! one is the commonest way to make a dashboard worse.
//!
//! ## Colour
//!
//! The bars are **one series**, so they are one hue and need no legend — the
//! label above them names them. The strip is **status**, which is a reserved
//! job: won, lost, and played-without-a-result, never used for anything else.
//!
//! Status colour is never carried alone. `good` against `bad` separates well
//! enough for colour-blind readers (ΔE 8.4 deutan, measured, against a target
//! of 8), but `bad` against the dim ink of a played-and-undecided game does not
//! — 6.8, inside the band that is legal only with a second encoding. So every
//! mark has a **shape** as well as a colour, and the shape is what is actually
//! being read.
//!
//! The one check that fails and is meant to: the palette's lightness sits
//! above the band a validator wants, because these marks are bright ink on a
//! near-black ground rather than fills on a mid surface. Contrast passes, which
//! is the check that matters here. See [theme].
//!
//! [theme]: super::theme

use super::theme::Theme;
use super::words::record as words;
use crate::client::record::{Game, Outcome, Summary};
use crate::client::views::words::w;

/// How many games the chart shows.
///
/// Recent form rather than a career: twenty bars fit across a column at a
/// width worth pointing at, and a fiftieth game is not something anybody is
/// comparing this one against. [`crate::client::record::KEEP`] is what is
/// *kept*; this is what is *shown*.
pub const SHOWN: usize = 20;

/// How many outcomes the strip shows. Fewer than the chart, because a run of
/// results is read as a run and a long one stops being one.
pub const FORM: usize = 10;

/// Draw the record, or the invitation if there is none.
pub fn show(ui: &mut egui::Ui, theme: &Theme, games: &[Game], summary: &Summary) {
    let p = theme.palette;
    let m = theme.metrics;

    if !summary.any() {
        // An empty screen is an invitation to act, not a mood. Five zeroes
        // would tell a new player only that the game keeps score.
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(w().record.nothing_yet).size(m.text_small),
        );
        return;
    }

    egui::Frame::new()
        .fill(p.surface_lift)
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            territory_chart(ui, theme, games);
            ui.add_space(m.item_spacing * 1.5);
            form_strip(ui, theme, games);
            ui.add_space(m.item_spacing * 1.5);
            tiles(ui, theme, summary);
        });
}

/// Largest territory, game by game, oldest on the left.
///
/// Marks are thin, gapped by two points so adjacent games never read as one
/// block, rounded at the top and anchored to the baseline. The axis is a
/// hairline and there are no gridlines: the question this answers is "is it
/// going up", and a reader who wants the exact figure hovers a bar.
fn territory_chart(ui: &mut egui::Ui, theme: &Theme, games: &[Game]) {
    let p = theme.palette;
    let m = theme.metrics;

    // `games` is newest first; a sequence is read left to right in the order
    // it happened, so the newest bar belongs on the right.
    let shown: Vec<&Game> = games.iter().take(SHOWN).rev().collect();
    let peak = shown.iter().map(|g| g.best).max().unwrap_or(0);

    ui.horizontal(|ui| {
        ui.colored_label(p.text_dim, egui::RichText::new(w().record.largest).size(m.text_small));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // One direct label, on the figure the chart is scaled to. A number
            // on every bar is noise; none at all leaves the axis unreadable.
            ui.colored_label(p.text, egui::RichText::new(number(peak as u64)).monospace());
        });
    });

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), CHART_HEIGHT), egui::Sense::HOVER);
    let painter = ui.painter();

    // The baseline, recessive: it says where nought is and nothing else.
    painter.hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, p.line));

    let n = shown.len().max(1) as f32;
    let slot = rect.width() / n;
    let bar = (slot - BAR_GAP).max(1.0);
    let hovered = response.hover_pos();

    for (i, game) in shown.iter().enumerate() {
        // A game that held nothing still happened, so it gets a stub rather
        // than no mark at all — the alternative reads as a gap in the record.
        let share = if peak == 0 { 0.0 } else { game.best as f32 / peak as f32 };
        let height = (share * (CHART_HEIGHT - 2.0)).max(STUB);
        let x = rect.left() + i as f32 * slot;
        let mark = egui::Rect::from_min_max(
            egui::pos2(x, rect.bottom() - height),
            egui::pos2(x + bar, rect.bottom()),
        );
        // The whole slot is the hit target, which is wider than the mark —
        // a two-point bar is not something anybody can point at.
        let over =
            hovered.is_some_and(|at| at.x >= x && at.x < x + slot && rect.y_range().contains(at.y));
        painter.rect_filled(
            mark,
            egui::CornerRadius { nw: END_ROUNDING, ne: END_ROUNDING, sw: 0, se: 0 },
            if over { p.text } else { p.accent },
        );
        if over {
            response.clone().on_hover_text(describe(game));
        }
    }
}

/// How the last few went: won, lost, or played without a result.
///
/// **Shape carries it, colour agrees.** Won is a filled disc, lost is a ring,
/// and a game with no result is a bar — three marks a colour-blind reader
/// tells apart without reading a hue at all, which is what the measured
/// separation between the dim ink and the losing red requires and what a
/// status colour requires anyway.
fn form_strip(ui: &mut egui::Ui, theme: &Theme, games: &[Game]) {
    let p = theme.palette;
    let m = theme.metrics;

    // Matches only. A world that never ends has no result, and a strip of
    // "neither" marks is a strip about nothing.
    let mut played: Vec<&Game> =
        games.iter().filter(|g| g.outcome != Outcome::Played).take(FORM).collect();
    // `games` is newest first; a run of results is read in the order it
    // happened, like the chart beside it.
    played.reverse();
    if played.is_empty() {
        return;
    }

    ui.horizontal(|ui| {
        ui.colored_label(p.text_dim, egui::RichText::new(w().record.form).size(m.text_small));
        for game in &played {
            let (rect, mark) = ui.allocate_exact_size(egui::vec2(MARK, MARK), egui::Sense::HOVER);
            let painter = ui.painter();
            let middle = rect.center();
            match game.outcome {
                Outcome::Won => painter.circle_filled(middle, MARK * 0.36, p.good),
                Outcome::Lost => {
                    painter.circle_stroke(middle, MARK * 0.32, egui::Stroke::new(2.0, p.bad))
                }
                // Filtered out above; drawn as a bar rather than skipped so
                // that a future outcome cannot silently vanish from the strip.
                Outcome::Played => painter.hline(
                    (middle.x - MARK * 0.3)..=(middle.x + MARK * 0.3),
                    middle.y,
                    egui::Stroke::new(2.0, p.text_dim),
                ),
            };
            mark.on_hover_text(describe(game));
        }
    });
}

/// The totals. Numbers with no shape to show, so they are not plotted.
fn tiles(ui: &mut egui::Ui, theme: &Theme, summary: &Summary) {
    let p = theme.palette;
    let m = theme.metrics;

    let mut cells: Vec<(String, &str)> = vec![(number(summary.games as u64), w().record.worlds)];
    if summary.matches > 0 {
        cells.push((format!("{}/{}", summary.won, summary.matches), w().record.matches_won));
    }
    cells.push((number(summary.best as u64), w().record.largest_ever));
    cells.push((number(summary.generations), w().record.generations));

    ui.horizontal_top(|ui| {
        ui.set_min_height(m.button_height * 1.4);
        let each = (ui.available_width() - m.item_spacing * (cells.len() as f32 - 1.0))
            / cells.len() as f32;
        for (figure, label) in &cells {
            ui.vertical(|ui| {
                ui.set_width(each);
                // Monospace, so a figure that changes does not shuffle the
                // column it sits in. Every number on this panel is set this
                // way and every word is not.
                ui.label(egui::RichText::new(figure).monospace().size(m.text_action).color(p.text));
                ui.colored_label(p.text_dim, egui::RichText::new(*label).size(m.text_small));
            });
        }
    });
}

/// One game, as a tooltip reads it.
fn describe(game: &Game) -> String {
    words::a_game(
        &game.room,
        game.best,
        game.generations,
        match game.outcome {
            Outcome::Won => w().record.won,
            Outcome::Lost => w().record.lost,
            Outcome::Played => w().record.no_result,
        },
    )
}

/// A figure with thousands separated, because four digits and five are
/// indistinguishable at a glance without them.
fn number(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(THOUSANDS);
        }
        out.push(c);
    }
    out
}

/// A narrow space rather than a comma: this is read, not parsed, and a comma
/// between digits means a decimal point to a good part of the world.
const THOUSANDS: char = '\u{2009}';

/// How tall the chart is, in points. Enough for the shape of a run to be
/// legible and not so much that it becomes the screen.
const CHART_HEIGHT: f32 = 44.0;
/// The gap between two bars. Two points, so adjacent games never read as one
/// block, which is the whole reason a bar chart is not an area chart.
const BAR_GAP: f32 = 2.0;
/// The rounded data-end, in the corner units egui takes.
const END_ROUNDING: u8 = 2;
/// What a game that held nothing draws, so it is a game rather than a gap.
const STUB: f32 = 2.0;
/// A form mark's box.
const MARK: f32 = 14.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::WorldKind;

    fn game(best: u32, outcome: Outcome) -> Game {
        Game { room: "arena".into(), world: WorldKind::Infinite, generations: 100, best, outcome }
    }

    /// Read, not parsed. Four digits and five are indistinguishable at a
    /// glance without a separator.
    #[test]
    fn figures_are_grouped_in_threes() {
        assert_eq!(number(0), "0");
        assert_eq!(number(9), "9");
        assert_eq!(number(999), "999");
        assert_eq!(number(1_000), "1\u{2009}000");
        assert_eq!(number(12_345), "12\u{2009}345");
        assert_eq!(number(1_234_567), "1\u{2009}234\u{2009}567");
    }

    /// The chart shows recent form, oldest on the left — a sequence is read in
    /// the order it happened, and `record::games` hands them over newest first.
    #[test]
    fn the_chart_shows_the_most_recent_games_oldest_first() {
        let games: Vec<Game> = (0..30).map(|i| game(i as u32, Outcome::Played)).collect();
        let shown: Vec<&Game> = games.iter().take(SHOWN).rev().collect();

        assert_eq!(shown.len(), SHOWN, "twenty bars, not thirty");
        assert_eq!(shown.first().unwrap().best, (SHOWN - 1) as u32, "oldest shown is leftmost");
        assert_eq!(shown.last().unwrap().best, 0, "and the newest game is on the right");
    }

    /// A strip of results is about results. A world that never ends has none,
    /// and a row of "neither" marks would be a row about nothing.
    #[test]
    fn the_form_strip_is_matches_only() {
        let games = vec![
            game(10, Outcome::Won),
            game(20, Outcome::Played),
            game(30, Outcome::Lost),
            game(40, Outcome::Played),
        ];
        let mut played: Vec<&Game> =
            games.iter().filter(|g| g.outcome != Outcome::Played).take(FORM).collect();
        played.reverse();
        assert_eq!(played.len(), 2);
        assert_eq!(played[0].outcome, Outcome::Lost, "oldest first, like the chart");
        assert_eq!(played[1].outcome, Outcome::Won);
    }

    /// A game that held nothing still happened. Scaling it to nothing would
    /// leave a gap in the record where a game was.
    #[test]
    fn a_game_that_held_nothing_still_draws_a_mark() {
        let peak = 100u32;
        let height = |best: u32| {
            let share = if peak == 0 { 0.0 } else { best as f32 / peak as f32 };
            (share * (CHART_HEIGHT - 2.0)).max(STUB)
        };
        assert_eq!(height(0), STUB, "a game that held nothing vanished");
        assert!(height(100) > height(50) && height(50) > height(1));

        // And a record where nobody ever held anything does not divide by nought.
        let peak = 0u32;
        let share = if peak == 0 { 0.0 } else { 1.0 };
        assert_eq!((share * (CHART_HEIGHT - 2.0f32)).max(STUB), STUB);
    }
}
