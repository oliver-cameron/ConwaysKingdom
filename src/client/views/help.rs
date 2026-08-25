//! Every key, on one screen, behind `?`.
//!
//! Taken from [chess-tui], which puts its whole vocabulary a single keystroke
//! away and expects nobody to read a manual. This game had none at all: the
//! hotbar shows what the digits put down, and nothing on screen said that space
//! pans, that a middle drag pans, that shift hurries it, or that a drag lays a
//! run. All of that lived in [docs/game.md], which is the one document a
//! player in a browser will never open.
//!
//! **Grouped by what a key acts on**, not by which key it is. A list sorted by
//! keycap is a lookup table for somebody who already knows the answer; a list
//! sorted by "moving about", "building", "getting out" is one you can read
//! when you do not.
//!
//! The rows are the only place in the client set in **monospace**, which is
//! doing work rather than decoration: a column of keycaps that do not line up
//! is a column you have to read rather than scan.
//!
//! [chess-tui]: https://github.com/thomas-mauran/chess-tui
//! [docs/game.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/game.md

use super::theme::Theme;
use super::words::help as words;

/// One group of keys, and what they are for.
struct Group {
    heading: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

/// What the client can be told to do, by what it is being told to do it to.
///
/// Here rather than beside the handlers that read them, for the reason
/// [`super::words`] exists at all: what the game *says* is a decision, and a
/// list of keys that drifts out of step with the keys is worse than no list.
/// Anything added to `on_key` belongs here in the same commit.
const GROUPS: &[Group] = &[
    Group {
        heading: words::LOOKING,
        keys: &[
            (words::keys::PAN_KEYS, words::PAN),
            (words::keys::PAN_FAST, words::PAN_FASTER),
            (words::keys::PAN_DRAG, words::PAN_BY_HAND),
            (words::keys::ZOOM, words::ZOOM),
        ],
    },
    Group {
        heading: words::BUILDING,
        keys: &[
            (words::keys::TOOLS, words::TOOLS),
            (words::keys::STAMPS, words::STAMPS),
            (words::keys::DRAG, words::DRAG),
        ],
    },
    Group {
        heading: words::GETTING_ABOUT,
        keys: &[
            (words::keys::WALK, words::WALK),
            (words::keys::CHOOSE, words::CHOOSE),
            (words::keys::MOVE_ON, words::MOVE_ON),
            (words::keys::BACK, words::BACK),
            (words::keys::HELP, words::HELP),
        ],
    },
];

/// Draw it. Returns the rectangle covered, so a click on it does not also
/// reach the world, and whether it was dismissed.
pub fn show(ctx: &egui::Context, theme: &Theme) -> (Option<egui::Rect>, bool) {
    let p = theme.palette;
    let m = theme.metrics;
    let mut close = false;

    // The widest keycap across every group, so the two columns line up down
    // the whole panel rather than per group — measured rather than guessed,
    // because a hard-coded width is a width that is wrong on the next key
    // somebody adds.
    let widest = GROUPS
        .iter()
        .flat_map(|g| g.keys.iter())
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(0);

    let area = egui::Area::new("help".into()).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
        ctx,
        |ui| {
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
                                close = true;
                            }
                        });
                    });

                    for group in GROUPS {
                        ui.add_space(m.item_spacing * 1.5);
                        ui.colored_label(
                            p.text_dim,
                            egui::RichText::new(group.heading).size(m.text_small),
                        );
                        for (key, what) in group.keys {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{key:widest$}"))
                                        .monospace()
                                        .size(m.text_small)
                                        .color(p.accent),
                                );
                                ui.colored_label(
                                    p.text,
                                    egui::RichText::new(*what).size(m.text_small),
                                );
                            });
                        }
                    }

                    ui.add_space(m.item_spacing * 1.5);
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(words::DISMISS).size(m.text_small),
                    );
                });
        },
    );

    (Some(area.response.rect), close)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is the documentation, so an empty group or a blank key is a
    /// row that says nothing — and this file is the one place a key gets
    /// written down, so a mistake here is a key nobody finds.
    #[test]
    fn every_row_says_something() {
        assert!(!GROUPS.is_empty());
        for group in GROUPS {
            assert!(!group.heading.is_empty());
            assert!(!group.keys.is_empty(), "{} has no keys", group.heading);
            for (key, what) in group.keys {
                assert!(!key.is_empty(), "a blank keycap under {}", group.heading);
                assert!(!what.is_empty(), "{key} does not say what it does");
            }
        }
    }

    /// Grouped by what a key acts on, so one key belongs in one place. The
    /// same keycap listed twice is two answers to one question.
    #[test]
    fn no_key_is_listed_twice() {
        let mut seen = Vec::new();
        for (key, _) in GROUPS.iter().flat_map(|g| g.keys.iter()) {
            assert!(!seen.contains(key), "{key} is listed twice");
            seen.push(key);
        }
    }

    /// The two columns line up down the whole panel, which is what the
    /// monospace is for. Padding to the widest is the only way that holds when
    /// somebody adds a longer key.
    #[test]
    fn the_keycap_column_is_as_wide_as_its_widest_key() {
        let widest = GROUPS
            .iter()
            .flat_map(|g| g.keys.iter())
            .map(|(key, _)| key.chars().count())
            .max()
            .unwrap();
        for (key, _) in GROUPS.iter().flat_map(|g| g.keys.iter()) {
            assert!(
                format!("{key:widest$}").chars().count() >= widest,
                "{key} would not fill the column"
            );
        }
    }
}
