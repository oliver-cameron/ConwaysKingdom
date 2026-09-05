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

use crate::client::views::hue::team_colour;
use crate::client::views::theme::Theme;
use crate::client::views::words::lobby as words;
use crate::client::views::words::menu::watch as whistle;
use crate::client::views::words::w;
use crate::net::{Level, MatchPhase, Team, Victory};
use crate::sim::PlayerId;

/// **What colour a seat draws in: its team's, or its own.**
///
/// A team is a player and a player is a hue, so everybody at one team's
/// controls shares one swatch — which is the whole of what a team looks like
/// and is worth getting right *here*, because the lobby is where somebody
/// checks they are on the side they meant to be.
///
/// It used to look the seat's own number up, so a two-team lobby showed its
/// players in the third and fourth colours of the wheel while the board they
/// were about to play on drew them in the first and second. The board was
/// right: cells carry the team's number.
fn drawn_as(seat: PlayerId, teams: &[Team]) -> PlayerId {
    teams.iter().find(|t| t.players.contains(&seat)).map_or(seat, |t| t.id)
}

/// Draw it, if this room is a match with something to say. Returns the
/// rectangle it covered, so the client knows what the pointer is over.
/// What a press in the lobby meant.
#[derive(Default)]
pub enum Did {
    #[default]
    Nothing,
    /// Back to the menu.
    Leave,
    /// Blow the whistle. Only offered to whoever made the match.
    Start,
    /// A name was pressed: show what this server says about them.
    Look(crate::net::PersonId),
    /// Take the controls of this team's player, or step off by naming your
    /// own number.
    JoinTeam(PlayerId),
    /// Call this team something.
    NameTeam(PlayerId, String),
    /// Seat a player the server plays, on this team or wherever the lobby
    /// would put anybody. Any seated player may; the seat count already
    /// shown counts it, because a bot takes one of the fifteen.
    AddBot { team: Option<PlayerId>, level: Level },
    /// Take one out again.
    RemoveBot(PlayerId),
}

/// Everything the lobby draws from.
///
/// A struct because the argument list reached eleven, which is the point at
/// which the order of them is the thing most likely to be got wrong — and
/// every one of these is read by name at the other end anyway.
pub struct Look<'a> {
    /// Your own **seat** -- the number on your row -- and never the side you
    /// place as. Everything in here is compared against seats: whose match it
    /// is, which row is yours, which side you are on.
    pub me: PlayerId,
    /// Why the last press in this lobby was refused, if it was — the whistle,
    /// a side, a bot. Shown in the lobby that produced it, to whoever pressed:
    /// a refusal in the HUD's corner is a refusal nobody reads, and a button
    /// that appears to do nothing reads as a broken lobby.
    pub refused: Option<&'a str>,
    pub phase: &'a MatchPhase,
    pub victory: Option<Victory>,
    pub players: &'a [crate::net::Seat],
    /// Whose match it is: the player who may start it.
    pub owner: Option<PlayerId>,
    /// Who blew the whistle, once somebody has.
    pub started_by: Option<PlayerId>,
    /// The teams, their names and who is at each one's controls. Empty in a
    /// free-for-all.
    pub teams: &'a [Team],
    /// The code that reaches this room, if it is private — the thing you hand
    /// to whoever is playing, read off while you wait for them.
    pub code: Option<&'a str>,
    /// Every player's hue, so a swatch and the board agree about who is who.
    /// A team is a player, so everybody at one team's controls draws from one
    /// swatch — there is no family of hue to sort out.
    pub hues: &'a [f32; PlayerId::COUNT],
}

impl Look<'_> {
    /// Who is sitting in this seat, if anybody here is.
    pub fn seat(&self, id: PlayerId) -> Option<&crate::net::Seat> {
        self.players.iter().find(|s| s.id == id)
    }

    /// What to call a number, which is a name if the roster has one and the
    /// number itself if it does not.
    ///
    /// One function because three places were doing this lookup by hand, each
    /// with its own spelling of the fallback — and a match's *winner* is the
    /// one place it matters, since a player who has left is exactly who a
    /// result is most likely to name.
    pub fn name_of(&self, id: PlayerId) -> String {
        self.seat(id).map(|s| s.name.clone()).unwrap_or_else(|| format!("player {}", id.0))
    }
}

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    look: &Look<'_>,
    // What is being typed into a side's name box, if anything. Held by the
    // client rather than here, because this panel is rebuilt every frame and
    // a name half-typed would vanish between two of them.
    naming: &mut Option<(PlayerId, String)>,
    // How hard the next bot plays: one picker for the whole lobby, held by the
    // client for the same reason.
    bot_level: &mut Level,
) -> crate::client::views::Shown<Did> {
    let mut did = Did::Nothing;
    // An open room is not a match, and a running one is a game — neither wants
    // a panel in the middle of it. Only the two ends have anything to say.
    let heading = match look.phase {
        MatchPhase::Gathering => w().lobby.waiting,
        MatchPhase::Over { .. } => w().lobby.finished,
        _ => return crate::client::views::Shown::nowhere(),
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
                        ui.colored_label(
                            p.text_dim,
                            crate::client::views::words::describe(victory),
                        );
                    }
                    // The code, where somebody waiting for their friends can
                    // read it off and send it. It appears once in the menu
                    // when the room is made and is gone the moment they leave
                    // that screen — which is a minute before they want it.
                    if let Some(code) = look.code {
                        ui.add_space(m.item_spacing);
                        ui.colored_label(
                            p.text_dim,
                            egui::RichText::new(w().lobby.code).size(m.text_small),
                        );
                        // Monospace and larger than the prose around it: this
                        // is a thing to be copied character by character, and
                        // an l and a 1 have to be told apart. The alphabet
                        // leaves those out, but the setting says so anyway.
                        ui.label(
                            egui::RichText::new(code)
                                .monospace()
                                .size(m.text_action)
                                .color(p.accent),
                        );
                    }
                    ui.separator();

                    match look.phase {
                        MatchPhase::Over { winner, held, .. } => match winner {
                            Some(id) => {
                                let who = look.name_of(*id);
                                swatch(ui, drawn_as(*id, look.teams), look.hues);
                                ui.heading(if *id == look.me {
                                    w().lobby.you_won.to_string()
                                } else {
                                    who
                                });
                                ui.colored_label(p.text_dim, words::held(*held));
                                // Whose match it was. A result with no idea
                                // who called it is a result somebody has to
                                // ask about.
                                if let Some(who) = look.started_by {
                                    let name = look.name_of(who);
                                    ui.colored_label(
                                        p.text_dim,
                                        egui::RichText::new(whistle::started_by(&name))
                                            .size(m.text_small),
                                    );
                                }
                            }
                            None => {
                                ui.label(w().lobby.nobody);
                            }
                        },
                        _ => {
                            ui.label(words::who(look.players.len()));
                            if look.teams.is_empty() {
                                for seat in look.players {
                                    ui.horizontal(|ui| {
                                        swatch(ui, drawn_as(seat.id, look.teams), look.hues);
                                        // **A name is a way in**, which is the
                                        // whole reason a seat carries a person:
                                        // "who is that" is asked here more than
                                        // anywhere else, and the answer is what
                                        // this server has seen them do.
                                        if let Some(what) = who_row(ui, seat, look.me) {
                                            did = what;
                                        }
                                    });
                                }
                                // One for the room, since there is no side
                                // to put it on.
                                if let Some(what) = add_bot(ui, theme, None, bot_level) {
                                    did = what;
                                }
                            } else if let Some(what) =
                                team_picker(ui, theme, look, naming, bot_level)
                            {
                                did = what;
                            }
                            ui.add_space(m.item_spacing);
                            // **Whoever made it blows the whistle.** Anybody
                            // may join a gathering match, and if anybody could
                            // also start it the person who set it up could not
                            // wait for their friends to arrive.
                            let mine = look.owner.is_some_and(|o| o == look.me);
                            if mine {
                                if crate::client::views::wide(
                                    ui,
                                    egui::RichText::new(w().menu.watch.start)
                                        .size(m.text_action)
                                        .color(p.ground),
                                    m.action_height,
                                    p.accent,
                                )
                                .clicked()
                                {
                                    did = Did::Start;
                                }
                            } else {
                                // Said rather than left blank: a lobby that
                                // does nothing and explains nothing is
                                // indistinguishable from one that is broken.
                                ui.small(match look.owner {
                                    Some(_) => w().menu.watch.not_yours,
                                    None => w().menu.watch.at_console,
                                });
                            }
                            // **For whoever pressed, owner or not.** A side or
                            // a bot is refused into the lobby the way the
                            // whistle is, and anybody seated may press those;
                            // drawn under the owner's button only, a full
                            // room was a press that did nothing for everybody
                            // else.
                            match look.refused {
                                Some(why) => {
                                    ui.colored_label(
                                        p.warn,
                                        egui::RichText::new(why).size(m.text_small),
                                    );
                                }
                                None if mine => {
                                    ui.small(w().menu.watch.start_note);
                                }
                                None => {}
                            }
                        }
                    }
                    ui.add_space(m.item_spacing);
                    ui.separator();
                    // A lobby with no way out is a room you are locked in
                    // until somebody else decides otherwise.
                    if crate::client::views::wide(
                        ui,
                        egui::RichText::new(w().hud.back_hint),
                        30.0,
                        p.surface,
                    )
                    .clicked()
                    {
                        did = Did::Leave;
                    }
                });
        });

    crate::client::views::Shown::new(area.response.rect, did)
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
fn team_picker(
    ui: &mut egui::Ui,
    theme: &Theme,
    look: &Look<'_>,
    naming: &mut Option<(PlayerId, String)>,
    bot_level: &mut Level,
) -> Option<Did> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut did = None;
    let (me, teams) = (look.me, look.teams);
    // Which team's controls this seat is at, if any. Read out of the roster
    // rather than out of a map of allegiances, because the roster is what a
    // team *is* now: the player, and who is driving it.
    let mine = teams.iter().find(|t| t.players.contains(&me)).map(|t| t.id);

    for team in teams {
        let ours = Some(team.id) == mine;
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
                            if done || ui.small_button(w().lobby.keep_name).clicked() {
                                did = Some(Did::NameTeam(team.id, text.clone()));
                                *naming = None;
                            }
                        }
                        _ => {
                            ui.label(egui::RichText::new(&team.name).size(m.text_body));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button(w().lobby.rename).clicked() {
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
                        egui::RichText::new(w().lobby.nobody_on_it).size(m.text_small),
                    );
                }
                for &id in &team.players {
                    ui.horizontal(|ui| {
                        swatch(ui, drawn_as(id, look.teams), look.hues);
                        // A name on a side is a way into a profile too, which
                        // is where it is most wanted: a team match is where
                        // "who is that" gets asked about somebody you are
                        // about to play against.
                        match look.seat(id) {
                            Some(seat) => {
                                if let Some(what) = who_row(ui, seat, me) {
                                    did = Some(what);
                                }
                            }
                            None => {
                                ui.colored_label(
                                    p.text_dim,
                                    egui::RichText::new(look.name_of(id)).size(m.text_small),
                                );
                            }
                        }
                    });
                }

                // Taking the side you are already on steps off it, so there is
                // a way back to undecided without a second control.
                let label = if ours { w().lobby.leave_side } else { w().lobby.take_side };
                if crate::client::views::wide(
                    ui,
                    egui::RichText::new(label).size(m.text_small),
                    m.button_height,
                    p.surface,
                )
                .clicked()
                {
                    did = Some(Did::JoinTeam(if ours { me } else { team.id }));
                }
                // **A bot per side is how sides are balanced**, which is the
                // reason the control is on the side rather than on the room.
                if let Some(what) = add_bot(ui, theme, Some(team.id), bot_level) {
                    did = Some(what);
                }
            });
        ui.add_space(m.item_spacing);
    }

    // Anybody who has not picked. Named rather than left off, because a match
    // will not start while somebody is unplaced and a lobby that does not say
    // who is the wrong place to find that out.
    let on_a_team: Vec<PlayerId> = teams.iter().flat_map(|t| t.players.iter().copied()).collect();
    let stray: Vec<PlayerId> =
        look.players.iter().map(|s| s.id).filter(|id| !on_a_team.contains(id)).collect();
    if !stray.is_empty() {
        ui.colored_label(
            p.warn,
            egui::RichText::new(words::not_picked(
                &stray.iter().map(|&id| look.name_of(id)).collect::<Vec<_>>().join(", "),
            ))
            .size(m.text_small),
        );
    }

    did
}

/// One person in a roster: their name, and their fingerprint if they have one.
///
/// A button rather than a label, because it opens their profile — and a
/// **frameless** one, so a roster still reads as a list of names rather than
/// as a column of controls. What says it can be pressed is the pointer
/// changing over it, which is the promise every other name-shaped thing on
/// this screen already makes.
fn who_row(ui: &mut egui::Ui, seat: &crate::net::Seat, me: PlayerId) -> Option<Did> {
    let label =
        if seat.id == me { format!("{}  ({})", seat.label(), w().lobby.you) } else { seat.label() };
    // **Their face beside their name**, which is what makes a roster somewhere
    // you recognise people rather than a list of strings. Derived from the key
    // and so already theirs everywhere else it is drawn — see
    // [`crate::client::views::social::face`]. A seat with no key has nothing behind it
    // to draw, and gets the space rather than a stand-in, so the names below it
    // still line up.
    let side = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    if let Some(who) = &seat.who {
        crate::client::views::social::face::show(ui.painter(), rect, who);
    }
    let pressed = ui.add(egui::Button::new(label).frame(false)).clicked();
    // A seat the server plays says so, and anybody seated may take it away:
    // it was added from this lobby by somebody, and a bot nobody can remove
    // is a seat nobody can have back.
    if seat.bot {
        ui.weak(w().lobby.bot);
        if ui.small_button(w().lobby.remove_bot).clicked() {
            return Some(Did::RemoveBot(seat.id));
        }
    }
    // Nobody to look up: a client with no key is somebody this server will not
    // remember, so there is nothing behind the name to show.
    seat.who.clone().filter(|_| pressed).map(Did::Look)
}

/// The control that seats a bot: how hard it plays, and a button.
///
/// The level is one picker for the whole lobby rather than one per side,
/// because it is a preference and not a fact about a side, and a row of three
/// words under every team is three words too many.
fn add_bot(
    ui: &mut egui::Ui,
    theme: &Theme,
    team: Option<PlayerId>,
    level: &mut Level,
) -> Option<Did> {
    let m = theme.metrics;
    let mut did = None;
    ui.horizontal(|ui| {
        for (i, each) in Level::ALL.into_iter().enumerate() {
            ui.selectable_value(
                level,
                each,
                egui::RichText::new(w().lobby.levels[i]).size(m.text_small),
            );
        }
        if ui.small_button(w().lobby.add_bot).clicked() {
            did = Some(Did::AddBot { team, level: *level });
        }
    });
    did
}

/// The same colour the shader gives this player's cells, so the lobby and the
/// board cannot disagree about who is who.
fn swatch(ui: &mut egui::Ui, player: PlayerId, hues: &[f32; PlayerId::COUNT]) {
    let (r, g, b) = team_colour(player, hues);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(id: u8, players: &[u8]) -> Team {
        Team {
            id: PlayerId(id),
            name: format!("Team {id}"),
            players: players.iter().map(|&p| PlayerId(p)).collect(),
        }
    }

    /// **A seat draws in its team's colour**, because a team is a player and a
    /// player is a hue — so everybody at one team's controls is one colour on
    /// the board, and the lobby has to agree with the board about that.
    ///
    /// It looked the seat's own number up, so a two-team lobby showed its
    /// players in the third and fourth colours of the wheel while the match
    /// they were about to play drew them in the first and second.
    #[test]
    fn a_seat_is_drawn_in_its_teams_colour() {
        let teams = [team(1, &[3, 5]), team(2, &[4])];
        assert_eq!(drawn_as(PlayerId(3), &teams), PlayerId(1));
        assert_eq!(drawn_as(PlayerId(5), &teams), PlayerId(1), "allies are one colour");
        assert_eq!(drawn_as(PlayerId(4), &teams), PlayerId(2));
        // Nobody's team is your own number, which is what a free-for-all is
        // and what somebody who has not picked yet should look like.
        assert_eq!(drawn_as(PlayerId(6), &teams), PlayerId(6));
        assert_eq!(drawn_as(PlayerId(3), &[]), PlayerId(3), "a match with no teams");
    }
}
