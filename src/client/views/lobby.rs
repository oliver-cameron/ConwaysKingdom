//! The screen before a match starts.
//!
//! A match gathers, and while it gathers **nothing happens**: the world does
//! not step and no action is taken. Without a screen saying so, that is
//! indistinguishable from a game that is broken — a player looks at a still
//! world, clicks, and nothing appears. So the lobby is not decoration. It is
//! the difference between "waiting" and "not working".
//!
//! Over the world rather than instead of it, the way the menu is: the board is
//! what they came for, and covering it entirely to say "soon" tells them less
//! than showing it does.

use crate::client::views::hud::player_colour;
use crate::client::views::theme::Theme;
use crate::client::views::words::lobby as words;
use crate::client::views::words::menu::watch as whistle;
use crate::net::{MatchPhase, Victory};
use crate::sim::PlayerId;

/// Draw it, if this room is a match with something to say. Returns the
/// rectangle it covered, so the client knows what the pointer is over.
/// What a press in the lobby meant.
pub enum Did {
    Nothing,
    /// Back to the menu.
    Leave,
    /// Blow the whistle. Only offered to whoever made the match.
    Start,
}

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    me: PlayerId,
    phase: &MatchPhase,
    victory: Option<Victory>,
    players: &[(PlayerId, String)],
    owner: Option<PlayerId>,
    started_by: Option<PlayerId>,
) -> (Option<egui::Rect>, Did) {
    let mut did = Did::Nothing;
    // An open room is not a match, and a running one is a game — neither wants
    // a panel in the middle of it. Only the two ends have anything to say.
    let heading = match phase {
        MatchPhase::Gathering => words::WAITING,
        MatchPhase::Over { .. } => words::FINISHED,
        _ => return (None, Did::Nothing),
    };
    let p = theme.palette;
    let m = theme.metrics;

    let area = egui::Area::new("lobby".into())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.6)
                .show(ui, |ui| {
                    ui.set_width(theme.panel_width(ctx.content_rect().width()) * 0.7);
                    ui.heading(heading);
                    if let Some(victory) = victory {
                        ui.colored_label(p.text_dim, describe(victory));
                    }
                    ui.separator();

                    match phase {
                        MatchPhase::Over { winner, held, .. } => match winner {
                            Some(id) => {
                                let who = players
                                    .iter()
                                    .find(|(p, _)| p == id)
                                    .map(|(_, name)| name.clone())
                                    .unwrap_or_else(|| format!("player {}", id.0));
                                swatch(ui, *id);
                                ui.heading(if *id == me {
                                    words::YOU_WON.to_string()
                                } else {
                                    who
                                });
                                ui.colored_label(p.text_dim, words::held(*held));
                                // Whose match it was. A result with no idea
                                // who called it is a result somebody has to
                                // ask about.
                                if let Some(who) = started_by {
                                    let name = players
                                        .iter()
                                        .find(|(id, _)| *id == who)
                                        .map(|(_, n)| n.clone())
                                        .unwrap_or_else(|| format!("player {}", who.0));
                                    ui.colored_label(
                                        p.text_dim,
                                        egui::RichText::new(whistle::started_by(&name))
                                            .size(m.text_small),
                                    );
                                }
                            }
                            None => {
                                ui.label(words::NOBODY);
                            }
                        },
                        _ => {
                            ui.label(words::who(players.len()));
                            for (id, name) in players {
                                ui.horizontal(|ui| {
                                    swatch(ui, *id);
                                    if *id == me {
                                        ui.label(format!("{name}  ({})", words::YOU));
                                    } else {
                                        ui.label(name);
                                    }
                                });
                            }
                            ui.add_space(m.item_spacing);
                            // **Whoever made it blows the whistle.** Anybody
                            // may join a gathering match, and if anybody could
                            // also start it the person who set it up could not
                            // wait for their friends to arrive.
                            let mine = owner.is_some_and(|o| o == me);
                            if mine {
                                if ui
                                    .add_sized(
                                        [ui.available_width(), m.action_height],
                                        egui::Button::new(
                                            egui::RichText::new(whistle::START)
                                                .size(m.text_action)
                                                .color(p.ground),
                                        )
                                        .fill(p.accent),
                                    )
                                    .clicked()
                                {
                                    did = Did::Start;
                                }
                                ui.small(whistle::START_NOTE);
                            } else {
                                // Said rather than left blank: a lobby that
                                // does nothing and explains nothing is
                                // indistinguishable from one that is broken.
                                ui.small(match owner {
                                    Some(_) => whistle::NOT_YOURS,
                                    None => whistle::AT_CONSOLE,
                                });
                            }
                        }
                    }
                    ui.add_space(m.item_spacing);
                    ui.separator();
                    // A lobby with no way out is a room you are locked in
                    // until somebody else decides otherwise.
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new(crate::client::views::words::hud::BACK_HINT),
                        )
                        .clicked()
                    {
                        did = Did::Leave;
                    }
                });
        });

    (Some(area.response.rect), did)
}

/// The same colour the shader gives this player's cells, so the lobby and the
/// board cannot disagree about who is who.
fn swatch(ui: &mut egui::Ui, player: PlayerId) {
    let (r, g, b) = player_colour(player);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
}

pub fn describe(victory: Victory) -> String {
    match victory {
        Victory::Timer { generations } => words::timer(generations),
        Victory::Territory { squares } => words::territory(squares),
    }
}
