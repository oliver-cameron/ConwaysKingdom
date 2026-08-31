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

use crate::client::views::theme::Theme;
use crate::client::views::words::help as words;

/// One group of keys, and what they are for.
struct Group {
    heading: &'static str,
    keys: Vec<(String, &'static str)>,
}

/// What the keys on this keyboard actually print, as far as anybody has found
/// out by pressing them.
///
/// **Every row bound by position asks**, which is three of them and used to be
/// one. A positional binding is the same key everywhere and prints something
/// different on every layout: the pan cluster is `,aoe` on Dvorak, and the
/// digit row types `&é"'(-è_ç` unshifted on a French keyboard — so a help
/// screen saying `1-9` was naming nine keys that layout does not have, on the
/// one screen that exists to be read by somebody who does not know the keys
/// yet.
///
/// The rows bound by *character* do not ask. `R`, `F`, `?` and `~` are the
/// same label everywhere because that is what they are bound to, and
/// [`input::mnemonic`] is what makes that true on a keyboard which cannot type
/// them.
///
/// `None` for a key nobody has pressed, which is answered with the half of the
/// row that is the same everywhere rather than with a guess.
///
/// [`input::mnemonic`]: crate::client::views::game::input::mnemonic
#[derive(Default)]
pub struct Keys {
    /// What the four pan keys print, in order, once all four are known.
    pub pan: Option<String>,
    /// What the bare digit row prints — the stamps.
    pub stamps: Option<String>,
    /// What shift and the first four digits print — the tools.
    pub tools: Option<String>,
}

/// What the client can be told to do, by what it is being told to do it to.
///
/// Here rather than beside the handlers that read them, for the reason
/// [`super::words`] exists at all: what the game *says* is a decision, and a
/// list of keys that drifts out of step with the keys is worse than no list.
/// Anything added to `on_key` belongs here in the same commit.
fn groups(keys: &Keys) -> Vec<Group> {
    let pan = match &keys.pan {
        Some(cluster) => words::keys::pan(cluster),
        None => words::keys::PAN_ARROWS.to_string(),
    };
    let stamps = keys.stamps.clone().unwrap_or_else(|| words::keys::STAMPS.to_string());
    let tools = match &keys.tools {
        Some(row) => words::keys::with_shift(row),
        None => words::keys::TOOLS.to_string(),
    };

    vec![
        Group {
            heading: words::LOOKING,
            keys: vec![
                (pan, words::PAN),
                (words::keys::PAN_FAST.into(), words::PAN_FASTER),
                (words::keys::PAN_DRAG.into(), words::PAN_BY_HAND),
                (words::keys::ZOOM.into(), words::ZOOM),
            ],
        },
        Group {
            heading: words::BUILDING,
            keys: vec![
                (tools, words::TOOLS),
                (stamps, words::STAMPS),
                (words::keys::TURN.into(), words::TURN),
                (words::keys::MIRROR.into(), words::MIRROR),
                (words::keys::DRAG.into(), words::DRAG),
            ],
        },
        Group {
            heading: words::THE_CLOCK,
            keys: vec![
                (words::keys::PLAY.into(), words::PLAY),
                (words::keys::STEP_ONE.into(), words::STEP_ONE),
            ],
        },
        Group {
            heading: words::GETTING_ABOUT,
            keys: vec![
                (words::keys::WALK.into(), words::WALK),
                (words::keys::CHOOSE.into(), words::CHOOSE),
                (words::keys::MOVE_ON.into(), words::MOVE_ON),
                (words::keys::BACK.into(), words::BACK),
                (words::keys::HELP.into(), words::HELP),
            ],
        },
    ]
}

/// Draw it. Returns the rectangle covered, so a click on it does not also
/// reach the world, and whether it was dismissed.
/// What a press on the key list meant.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Did {
    #[default]
    Nothing,
    Close,
}

pub fn show(ctx: &egui::Context, theme: &Theme, keys: &Keys) -> crate::client::views::Shown<Did> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut did = Did::Nothing;

    // The widest keycap across every group, so the two columns line up down
    // the whole panel rather than per group — measured rather than guessed,
    // because a hard-coded width is a width that is wrong on the next key
    // somebody adds.
    let groups = groups(keys);
    let widest = groups
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
                                did = Did::Close;
                            }
                        });
                    });

                    for group in &groups {
                        ui.add_space(m.item_spacing * 1.5);
                        ui.colored_label(
                            p.text_dim,
                            egui::RichText::new(group.heading).size(m.text_small),
                        );
                        for (key, what) in &group.keys {
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

    crate::client::views::Shown::new(area.response.rect, did)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list as somebody who has pressed nothing sees it, which is the
    /// worst case for every assertion below.
    fn cold() -> Vec<Group> {
        groups(&Keys::default())
    }

    /// The list is the documentation, so an empty group or a blank key is a
    /// row that says nothing — and this file is the one place a key gets
    /// written down, so a mistake here is a key nobody finds.
    #[test]
    fn every_row_says_something() {
        let groups = cold();
        assert!(!groups.is_empty());
        for group in &groups {
            assert!(!group.heading.is_empty());
            assert!(!group.keys.is_empty(), "{} has no keys", group.heading);
            for (key, what) in &group.keys {
                assert!(!key.is_empty(), "a blank keycap under {}", group.heading);
                assert!(!what.is_empty(), "{key} does not say what it does");
            }
        }
    }

    /// **The pan row says what the keys actually print.** It said "WASD" to
    /// everybody, which is a name for a shape on the board and only spells
    /// itself on one layout: on Dvorak those four keys type `,aoe`, so the one
    /// list that exists to be read by somebody who does not know the keys was
    /// telling them the wrong ones.
    #[test]
    fn the_pan_row_follows_the_keyboard() {
        let row = |pan: Option<&str>| {
            groups(&Keys { pan: pan.map(str::to_string), ..Keys::default() })
                .into_iter()
                .flat_map(|g| g.keys)
                .map(|(key, _)| key)
                .find(|key| key.contains("arrows"))
                .expect("the pan row went missing")
        };
        assert!(row(Some("wasd")).starts_with("wasd"));
        assert!(row(Some(",aoe")).starts_with(",aoe"), "a Dvorak board was told WASD");
        // And before anybody has pressed one of them there is nothing to
        // report, which is answered with the half that is the same everywhere
        // rather than with a guess.
        assert_eq!(row(None), "arrows");
    }

    /// Grouped by what a key acts on, so one key belongs in one place. The
    /// same keycap listed twice is two answers to one question.
    #[test]
    fn no_key_is_listed_twice() {
        let mut seen = Vec::new();
        let groups = cold();
        for (key, _) in groups.iter().flat_map(|g| g.keys.iter()) {
            assert!(!seen.contains(&key), "{key} is listed twice");
            seen.push(key);
        }
    }

    /// The two columns line up down the whole panel, which is what the
    /// monospace is for. Padding to the widest is the only way that holds when
    /// somebody adds a longer key.
    #[test]
    fn the_keycap_column_is_as_wide_as_its_widest_key() {
        let groups = cold();
        let widest = groups
            .iter()
            .flat_map(|g| g.keys.iter())
            .map(|(key, _)| key.chars().count())
            .max()
            .unwrap();
        for (key, _) in groups.iter().flat_map(|g| g.keys.iter()) {
            assert!(
                format!("{key:widest$}").chars().count() >= widest,
                "{key} would not fill the column"
            );
        }
    }

    /// **The digit rows follow the keyboard too**, which they did not. They
    /// are bound by position — the row is a shape — and a French keyboard
    /// types `&é"'(-è_ç` where a US one types `1-9`, so the one screen that
    /// exists to be read by somebody who does not know the keys was naming
    /// nine of them that layout does not have.
    #[test]
    fn the_digit_rows_follow_the_keyboard() {
        let azerty = Keys {
            pan: Some("zqsd".into()),
            stamps: Some("&é\"'(-è_çà".into()),
            tools: Some("1234".into()),
        };
        let caps: Vec<String> =
            groups(&azerty).into_iter().flat_map(|g| g.keys).map(|(k, _)| k).collect();
        assert!(caps.iter().any(|k| k.contains("&é")), "the stamp row still said 1-9: {caps:?}");
        assert!(caps.iter().any(|k| k.contains("shift") && k.contains("1234")));
        assert!(!caps.iter().any(|k| k == "1-9"), "a US label survived: {caps:?}");

        // And before anybody has pressed one, the row a US keyboard has is
        // still the honest guess rather than a blank.
        let caps: Vec<String> = cold().into_iter().flat_map(|g| g.keys).map(|(k, _)| k).collect();
        assert!(caps.iter().any(|k| k == words::keys::STAMPS));
    }
}
