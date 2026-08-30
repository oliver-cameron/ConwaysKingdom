//! What the player is told about their own state.
//!
//! Read-only: a view of the client, not a place decisions are made. Everything
//! it needs arrives as arguments, so it has no opinion about where the numbers
//! came from and cannot change them.

use crate::client::desync::{Geiger, Level};
use crate::client::views::hue::player_colour;
use crate::client::views::words::hud as words;
use crate::sim::{PlayerId, WorldKind};

/// What the HUD shows. Assembled by the client each frame.
pub struct Status<'a> {
    pub player: PlayerId,
    pub value: i32,
    pub generation: u64,
    pub chunks_held: usize,
    pub chunks_drawn: u32,
    pub zoom: f32,
    pub connected: bool,
    /// Which world on the server this is, once it has said. `None` offline,
    /// where there is only the one and it has no name.
    ///
    /// Shown because a room is a whole separate world: two players who cannot
    /// find each other are far more likely to be in different rooms than at
    /// different ends of one, and nothing else on screen would say so.
    pub room: Option<&'a str>,
    /// The shape of the world, which decides whether the far edge is the
    /// opposite edge. Sent by the server rather than assumed -- nothing a
    /// client can see says whether the ground ends.
    pub world: WorldKind,
    /// Why the last action was refused, if it was.
    pub notice: Option<&'a str>,
    /// Whether the pointer is currently over the interface rather than the
    /// world. Shown because a stuck value here silently eats every click, and
    /// a number on screen is easier to trust than a guess.
    pub pointer_on_ui: bool,
    /// The cell under the cursor. Moves when the mouse moves, so it says at a
    /// glance whether pointer events are arriving at all.
    pub cursor_cell: (i32, i32),
    /// What the last click did, or why it did nothing.
    pub last_action: Option<&'a str>,
    /// The hotbar slot currently selected.
    pub holding: &'a str,
    /// Who holds how much ground, most first. Empty until the server has said.
    pub standing: &'a [(PlayerId, u32)],
    /// How badly this client and the server are disagreeing, as a decaying
    /// rate. Shown beside "connected" because that is the claim it qualifies:
    /// a link that is open and a link that is keeping up are two facts, and
    /// only the first of them was on screen.
    pub geiger: Geiger,
    /// Watching without a seat, which the HUD says for the whole visit rather
    /// than once: a spectator whose clicks do nothing needs to know why the
    /// first time and not the fifth.
    pub watching: bool,
    /// This player's rating, and what the last match did to it.
    ///
    /// **Always on screen**, which it was not: it lived on the home screen, so
    /// the number a match is played *for* was the one thing you could not see
    /// while playing one. `None` until a server has said — a client that has
    /// reached nobody has no rating rather than a starting figure.
    pub rating: Option<(i32, Option<i32>)>,
    /// Whether a match is running here, which is what makes giving up and
    /// calling it off mean anything.
    pub in_a_match: bool,
    /// Whether this client may call the match off: it started it.
    pub started_it: bool,
    /// Whether this seat has already given up, so the control says so rather
    /// than offering to do it twice.
    pub forfeited: bool,
}

/// What a press on the HUD meant.
///
/// **The HUD and not a panel**, because there is nowhere else during a running
/// match: the lobby draws for `Gathering` and `Over` only — a panel over a
/// live game is the thing it exists to avoid — and the back arrow beside these
/// *leaves the room*, giving up the seat. Somebody who wants out of a match
/// they are losing should be able to concede it rather than walk out of it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Did {
    #[default]
    Nothing,
    /// Leave the room entirely.
    Back,
    /// Give up, for this seat.
    Forfeit,
    /// Call the whole match off, with the score as it stands.
    EndMatch,
}

/// Whether the panel shows what it shows for a developer rather than a player.
///
/// The cursor's cell, what the last click did, how many chunks are held and
/// drawn, the zoom, and the list of keys: every one of them earned its place
/// while something was being built, and none of them is what somebody playing
/// wants a third of their screen taken by. Off rather than deleted, because
/// each one is the fastest way back to a whole class of bug — a stuck
/// `pointer_on_ui` silently eats every click, and a click on empty ground that
/// takes nothing looks exactly like a click that never arrived.
const DEBUG: bool = false;

/// Draw it. Returns the rectangle it occupied, so the client knows what it
/// covered, and whether the way out was clicked.
pub fn show(
    ctx: &egui::Context,
    theme: &crate::client::views::theme::Theme,
    status: &Status<'_>,
) -> crate::client::views::Shown<Did> {
    let mut did = Did::Nothing;
    let response = egui::Window::new("kingdom")
        .title_bar(false)
        .resizable(false)
        // Fixed, or dragging it would be indistinguishable from panning the
        // world underneath.
        .movable(false)
        .anchor(egui::Align2::LEFT_TOP, [theme.metrics.margin, theme.metrics.margin])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // The way out, in the row that is already about who you are
                // and where. A floating button would have to sit somewhere,
                // and everywhere it could sit is over the world.
                // Painted rather than typed: the arrow it used to be is not in
                // any font this client loads, because this client loads none,
                // so the one control whose job is to be recognised at a glance
                // was a square. See `icons::back`.
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::click());
                let palette = theme.palette;
                let ink = if response.hovered() { palette.text } else { palette.text_dim };
                ui.painter().rect_stroke(
                    rect,
                    theme.metrics.rounding,
                    egui::Stroke::new(
                        1.0,
                        if response.hovered() {
                            palette.line
                        } else {
                            palette.line.gamma_multiply(0.6)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                crate::client::views::icons::back(ui.painter(), rect.shrink(5.0), ink);
                if response.on_hover_text(words::BACK_HINT).clicked() {
                    did = Did::Back;
                }
                // The same colour the shader gives this player's cells, so the
                // swatch and the board cannot disagree about who is who.
                let (r, g, b) = player_colour(status.player);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                ui.heading(format!("Player {}", status.player.0));
            });

            // The purse, the ground held, the tick and the rating are on the
            // **bar** — see `hotbar::standing`. They were here, in the
            // opposite corner from the squares and the pointer, which put the
            // numbers that change while you play furthest from where you play.
            // What is left is about the *connection*, which you do not watch.
            ui.separator();
            if DEBUG {
                ui.label(format!(
                    "Chunks  {} held, {} drawn",
                    status.chunks_held, status.chunks_drawn
                ));
                ui.label(format!("Zoom  {:.1} px/cell", status.zoom));
            }

            standings(ui, theme, status);

            ui.separator();
            ui.horizontal(|ui| {
                if status.connected {
                    // Connected, and then how well. Silent until there has
                    // been something to be silent about: a link that has
                    // never slipped says nothing, because a reassurance
                    // nobody asked for is one more thing to read every frame.
                    let p = theme.palette;
                    let (word, colour) = match status.geiger.level() {
                        Level::Quiet if !status.geiger.ever() => (words::CONNECTED, p.good),
                        Level::Quiet => (crate::client::views::words::desync::SETTLED, p.good),
                        Level::Background => {
                            (crate::client::views::words::desync::BACKGROUND, p.good)
                        }
                        Level::Noticeable => {
                            (crate::client::views::words::desync::NOTICEABLE, p.warn)
                        }
                        Level::Alarming => (crate::client::views::words::desync::ALARMING, p.bad),
                    };
                    ui.colored_label(colour, word);
                } else {
                    ui.colored_label(theme.palette.warn, words::OFFLINE);
                }
                if let Some(room) = status.room {
                    ui.label(format!("· room {room}"));
                }
                if status.watching {
                    ui.colored_label(
                        theme.palette.accent,
                        crate::client::views::words::menu::watch::WATCHING,
                    );
                }
            });
            // What the last match did to the rating. The rating itself is on
            // the bar; this is the half that is only worth a line for as long
            // as it is news.
            if let Some((_, Some(change))) =
                status.rating.filter(|(_, c)| c.is_some_and(|c| c != 0))
            {
                let colour = if change > 0 { theme.palette.good } else { theme.palette.bad };
                ui.colored_label(colour, words::rating_change(change));
            }
            ui.small(match status.world {
                WorldKind::Infinite => words::BOUNDLESS.to_string(),
                WorldKind::Toroidal { rows, cols } => {
                    format!("{rows}x{cols} chunks, wrapping")
                }
            });
            if let Some(notice) = status.notice {
                ui.colored_label(theme.palette.bad, notice);
            }

            // **Getting out of a match, as against getting out of the room.**
            // The arrow at the top leaves entirely, giving up the seat; these
            // two say how the match itself ends. Only while one is running,
            // because neither means anything otherwise.
            if status.in_a_match {
                ui.separator();
                ui.horizontal(|ui| {
                    if status.forfeited {
                        ui.colored_label(theme.palette.text_dim, words::GAVE_UP);
                    } else if ui
                        .small_button(words::FORFEIT)
                        .on_hover_text(words::FORFEIT_HINT)
                        .clicked()
                    {
                        did = Did::Forfeit;
                    }
                    if status.started_it
                        && ui
                            .small_button(words::END_MATCH)
                            .on_hover_text(words::END_MATCH_HINT)
                            .clicked()
                    {
                        did = Did::EndMatch;
                    }
                });
            }

            if DEBUG {
                ui.separator();
                ui.small(crate::client::views::words::desync::reading(
                    status.geiger.rate(),
                    status.geiger.total(),
                ));
                ui.small(format!(
                    "cursor  ({}, {})   {}",
                    status.cursor_cell.0,
                    status.cursor_cell.1,
                    if status.pointer_on_ui { words::OVER_PANEL } else { words::ON_WORLD }
                ));
                ui.small(format!("last  {}", status.last_action.unwrap_or(words::NOTHING_YET)));

                ui.separator();
                ui.small(format!("holding  {}", status.holding));
                for hint in words::HINTS {
                    ui.small(*hint);
                }
            }
        });
    crate::client::views::Shown::new(response.map(|r| r.response.rect), did)
}

/// Who is winning, as bars.
///
/// A bar rather than a number because the question is *who is ahead and by how
/// much*, and that is a comparison — six figures in a column have to be read
/// and subtracted, where six bars are one glance. The numbers are there beside
/// them for when the answer is close.
///
/// Scaled to the leader rather than to the world: what is being asked is how
/// the players compare with each other, and against the size of a boundless
/// world every bar would be a sliver.
///
/// Each bar is drawn in **its player's own colour**, the same one the shader
/// gives their cells, so a bar and the ground it counts cannot disagree about
/// whose it is.
fn standings(ui: &mut egui::Ui, theme: &crate::client::views::theme::Theme, status: &Status<'_>) {
    if status.standing.is_empty() {
        return;
    }
    ui.separator();
    ui.small(words::HOLDING);

    let most = status.standing.iter().map(|&(_, n)| n).max().unwrap_or(1).max(1) as f32;
    let width = ui.available_width().max(80.0);
    for &(player, held) in status.standing.iter().take(SHOWN) {
        let (r, g, b) = player_colour(player);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());

        // The full width in the faintest ink, so a short bar reads as a
        // fraction of something rather than as a stub floating in space.
        ui.painter().rect_filled(rect, 2.0, theme.palette.line);
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * (held as f32 / most), rect.height()),
        );
        ui.painter().rect_filled(filled, 2.0, egui::Color32::from_rgb(r, g, b));

        // Yours named, everybody else's numbered: on a board where every
        // player is a colour and a number, the one thing worth spelling out is
        // which of them is you.
        let label = if player == status.player {
            format!("you  {held}")
        } else {
            format!("{}  {held}", player.0)
        };
        ui.painter().text(
            rect.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(10.0),
            theme.palette.text,
        );
    }
    if status.standing.len() > SHOWN {
        ui.small(format!("+{} more", status.standing.len() - SHOWN));
    }
}

/// How many bars fit before the panel is a leaderboard rather than a HUD.
///
/// Thirty-one players can have been through a world, and a column of thirty-one
/// bars is a screen of its own. Whoever is winning is at the top, and you are
/// interested in the rest of the field only once you are in it.
const SHOWN: usize = 6;
