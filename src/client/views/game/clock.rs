//! How much of a match is left, along the top of the screen.
//!
//! A running match otherwise says nothing about itself: the board looks like
//! any other world, and a player cannot tell whether there are ten generations
//! left or ten thousand. Which is the whole of the difference between a match
//! and a sandbox — everything you decide in one depends on how much of it is
//! left to decide in.
//!
//! Along the top because it is the one thing that is true of the *match*
//! rather than of you. The HUD's corner is about a player; the middle of the
//! top is about the room everybody is in.

use crate::client::views::theme::Theme;
use crate::client::views::words::clock as words;
use crate::net::{MatchPhase, Victory};

/// How long a count of generations lasts, in seconds, at this room's rate.
///
/// **Asked of the room rather than assumed.** This was one over a constant
/// that meant four a second, with a doc comment admitting it was wrong for a
/// server started at a different span — and it was wrong in the worst place,
/// since the number it feeds is a *countdown* somebody is timing a match
/// against. The bar was right either way, a bar being a fraction.
fn seconds(generations: u64, bpm: u16) -> u64 {
    let per_second = bpm.max(1) as f64 / 60.0;
    (generations as f64 / per_second).round() as u64
}

/// Draw it, if there is a match running. Returns what it covered.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    generation: u64,
    phase: &MatchPhase,
    victory: Option<Victory>,
    standing: &[crate::net::Holding],
    paused: bool,
    // This room's rate, for turning a count of generations into a time -- see
    // `net::Rules::bpm`.
    bpm: u16,
) -> crate::client::views::Shown<()> {
    // **A stopped world says so, and nothing else has to.** A board that is
    // not moving is indistinguishable from one that has settled, and settling
    // is a thing patterns actually do — so without this the first minute of an
    // experiment is spent wondering which it was. Here rather than in the HUD
    // for the reason the rest of this strip is: it is a fact about the room
    // and not about the player.
    if paused {
        return pill(ctx, theme, words::paused_at(generation));
    }
    // Only while it is running. A gathering match has its lobby and a decided
    // one has its result, and both of those say more than a clock could.
    let MatchPhase::Running { from } = phase else { return crate::client::views::Shown::nowhere() };
    let Some(victory) = victory else { return crate::client::views::Shown::nowhere() };

    let (left, done, of) = match victory {
        Victory::Timer { generations } => {
            let gone = generation.saturating_sub(*from);
            let left = generations.saturating_sub(gone);
            (words::generations_left(left, seconds(left, bpm)), gone, generations)
        }
        Victory::Territory { squares } => {
            // Against the **leader**, not against you: the question a target
            // asks is how close anybody is to ending it.
            let most = standing.first().map(|h| h.score as u64).unwrap_or(0);
            (words::squares_left(squares as u64, most), most, squares as u64)
        }
    };

    let p = theme.palette;
    let m = theme.metrics;
    let area = egui::Area::new("clock".into())
        .anchor(egui::Align2::CENTER_TOP, [0.0, m.margin])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding)
                .show(ui, |ui| {
                    ui.set_width(240.0);
                    ui.label(left);

                    // How much of it has gone, which is the part a number does
                    // badly: "1240 left" means nothing without "of what".
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 6.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 3.0, p.line);
                    let across = (done as f32 / of.max(1) as f32).clamp(0.0, 1.0);
                    let filled = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * across, rect.height()),
                    );
                    // Turning as it runs out, so the last of it is visible
                    // without reading anything.
                    let ink = if across > 0.9 { p.warn } else { p.accent };
                    ui.painter().rect_filled(filled, 3.0, ink);
                });
        });
    crate::client::views::Shown::new(area.response.rect, ())
}

/// One line in the strip, with nothing under it.
fn pill(ctx: &egui::Context, theme: &Theme, text: String) -> crate::client::views::Shown<()> {
    let (p, m) = (theme.palette, theme.metrics);
    let area = egui::Area::new("clock".into())
        .anchor(egui::Align2::CENTER_TOP, [0.0, m.margin])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.warn))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding)
                .show(ui, |ui| {
                    ui.colored_label(p.warn, text);
                });
        });
    crate::client::views::Shown::new(area.response.rect, ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::views::words::clock as w;

    /// The arithmetic, which is the part that can be wrong without anybody
    /// noticing until a match ends at the wrong moment.
    #[test]
    fn what_is_left_counts_from_when_it_started() {
        // A match that began at generation 500 and runs for 2000 is a fifth
        // gone at 900, whatever the world's own generation happens to be.
        let (from, generations) = (500u64, 2000u64);
        let gone = 900u64.saturating_sub(from);
        assert_eq!(gone, 400);
        assert_eq!(generations.saturating_sub(gone), 1600);

        // And it floors rather than wrapping: a client a few generations ahead
        // of the server would otherwise read four billion left.
        let over = 3000u64.saturating_sub(from);
        assert_eq!(generations.saturating_sub(over), 0);
    }

    /// Seconds, from the rate the world is stepped at.
    #[test]
    fn generations_read_as_a_clock() {
        assert_eq!(seconds(4, crate::net::DEFAULT_BPM), 1, "four a second");
        assert_eq!(seconds(2000, crate::net::DEFAULT_BPM), 500);
        // And a room set to half speed takes twice as long to get there.
        assert_eq!(seconds(2000, crate::net::DEFAULT_BPM / 2), 1000, "a slower room read fast");
        assert_eq!(
            w::generations_left(2000, seconds(2000, crate::net::DEFAULT_BPM)),
            "2000 left  ·  8:20"
        );
        assert_eq!(w::generations_left(0, 0), "time", "which is what a whistle is");
    }
}
