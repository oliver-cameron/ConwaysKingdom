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
use crate::net::{MatchPhase, Sides, Team, TeamId, Victory};
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
    /// Take this side, or step off one by taking [`TeamId::NONE`].
    TakeSide(TeamId),
    /// Call this side something.
    NameSide(TeamId, String),
}

/// Everything the lobby draws from.
///
/// A struct because the argument list reached eleven, which is the point at
/// which the order of them is the thing most likely to be got wrong — and
/// every one of these is read by name at the other end anyway.
pub struct Look<'a> {
    pub me: PlayerId,
    pub phase: &'a MatchPhase,
    pub victory: Option<Victory>,
    pub players: &'a [(PlayerId, String)],
    /// Whose match it is: the player who may start it.
    pub owner: Option<PlayerId>,
    /// Who blew the whistle, once somebody has.
    pub started_by: Option<PlayerId>,
    pub sides: Sides,
    /// The sides, their names and who is on them. Empty in a free-for-all.
    pub teams: &'a [Team],
}

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    look: &Look<'_>,
    // What is being typed into a side's name box, if anything. Held by the
    // client rather than here, because this panel is rebuilt every frame and
    // a name half-typed would vanish between two of them.
    naming: &mut Option<(TeamId, String)>,
) -> (Option<egui::Rect>, Did) {
    let mut did = Did::Nothing;
    // An open room is not a match, and a running one is a game — neither wants
    // a panel in the middle of it. Only the two ends have anything to say.
    let heading = match look.phase {
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
                    if let Some(victory) = look.victory {
                        ui.colored_label(p.text_dim, describe(victory));
                    }
                    ui.separator();

                    match look.phase {
                        MatchPhase::Over { winner, held, .. } => match winner {
                            Some(id) => {
                                let who = look
                                    .players
                                    .iter()
                                    .find(|(p, _)| p == id)
                                    .map(|(_, name)| name.clone())
                                    .unwrap_or_else(|| format!("player {}", id.0));
                                swatch(ui, *id);
                                ui.heading(if *id == look.me {
                                    words::YOU_WON.to_string()
                                } else {
                                    who
                                });
                                ui.colored_label(p.text_dim, words::held(*held));
                                // Whose match it was. A result with no idea
                                // who called it is a result somebody has to
                                // ask about.
                                if let Some(who) = look.started_by {
                                    let name = look
                                        .players
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
                            ui.label(words::who(look.players.len()));
                            if look.teams.is_empty() {
                                for (id, name) in look.players {
                                    ui.horizontal(|ui| {
                                        swatch(ui, *id);
                                        if *id == look.me {
                                            ui.label(format!("{name}  ({})", words::YOU));
                                        } else {
                                            ui.label(name);
                                        }
                                    });
                                }
                            } else if let Some(what) = side_picker(ui, theme, look, naming) {
                                did = what;
                            }
                            ui.add_space(m.item_spacing);
                            // **Whoever made it blows the whistle.** Anybody
                            // may join a gathering match, and if anybody could
                            // also start it the person who set it up could not
                            // wait for their friends to arrive.
                            let mine = look.owner.is_some_and(|o| o == look.me);
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
                                ui.small(match look.owner {
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

/// Who is on which side, and the two things a player may do about it: join one,
/// and name one.
///
/// **Anybody may take any side and anybody may name any side.** A lobby that
/// stopped you joining your friend because the sides would be uneven is a lobby
/// that makes people argue about the order they clicked in; the evenness is
/// checked when the match is *started*, where it can be fixed and tried again.
/// Naming is the same decision the room name already is: this is a game played
/// together, and a naming fight is a smaller problem than a permission system.
fn side_picker(
    ui: &mut egui::Ui,
    theme: &Theme,
    look: &Look<'_>,
    naming: &mut Option<(TeamId, String)>,
) -> Option<Did> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut did = None;
    let (me, sides, teams, players) = (look.me, look.sides, look.teams, look.players);
    let mine = sides.team_of(me);
    let name_of = |id: PlayerId| {
        players
            .iter()
            .find(|(who, _)| *who == id)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| format!("player {}", id.0))
    };

    for team in teams {
        let ours = team.id == mine;
        egui::Frame::new()
            .fill(if ours { p.surface_lift } else { p.surface })
            .stroke(egui::Stroke::new(1.0, if ours { p.accent } else { p.line }))
            .corner_radius(m.rounding)
            .inner_margin(m.panel_padding * 0.6)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match naming {
                        // Being renamed: the field replaces the label, so the
                        // row does not change height and nothing below it
                        // moves while somebody types.
                        Some((editing, text)) if *editing == team.id => {
                            let field = ui.add_sized(
                                [ui.available_width() * 0.6, m.button_height],
                                egui::TextEdit::singleline(text),
                            );
                            let done =
                                field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if done || ui.small_button(words::KEEP_NAME).clicked() {
                                did = Some(Did::NameSide(team.id, text.clone()));
                                *naming = None;
                            }
                        }
                        _ => {
                            ui.label(egui::RichText::new(&team.name).size(m.text_body));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button(words::RENAME).clicked() {
                                        *naming = Some((team.id, team.name.clone()));
                                    }
                                },
                            );
                        }
                    }
                });

                if team.players.is_empty() {
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(words::NOBODY_ON_IT).size(m.text_small),
                    );
                }
                for &id in &team.players {
                    ui.horizontal(|ui| {
                        swatch(ui, id);
                        let who = name_of(id);
                        ui.colored_label(
                            if id == me { p.text } else { p.text_dim },
                            egui::RichText::new(if id == me {
                                format!("{who}  ({})", words::YOU)
                            } else {
                                who
                            })
                            .size(m.text_small),
                        );
                    });
                }

                // Taking the side you are already on steps off it, so there is
                // a way back to undecided without a second control.
                let label = if ours { words::LEAVE_SIDE } else { words::TAKE_SIDE };
                if ui
                    .add_sized(
                        [ui.available_width(), m.button_height],
                        egui::Button::new(egui::RichText::new(label).size(m.text_small)),
                    )
                    .clicked()
                {
                    did = Some(Did::TakeSide(if ours { TeamId::NONE } else { team.id }));
                }
            });
        ui.add_space(m.item_spacing);
    }

    // Anybody who has not picked. Named rather than left off, because a match
    // will not start while somebody is unplaced and a lobby that does not say
    // who is the wrong place to find that out.
    let stray: Vec<PlayerId> =
        players.iter().map(|(id, _)| *id).filter(|&id| sides.team_of(id).is_none()).collect();
    if !stray.is_empty() {
        ui.colored_label(
            p.warn,
            egui::RichText::new(words::not_picked(
                &stray.iter().map(|&id| name_of(id)).collect::<Vec<_>>().join(", "),
            ))
            .size(m.text_small),
        );
    }

    did
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
