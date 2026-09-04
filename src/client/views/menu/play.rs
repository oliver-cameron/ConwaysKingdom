//! A server, what is on it, and a form for what is not.
//!
//! Split from [`super::home`] because the two screens ask different questions
//! and share almost nothing: this one is about a machine somewhere and the
//! worlds on it, and home is about the person in front of the screen.

use super::draft::{Access, Draft, Ends, Kind, Shape};
use super::{describe, players, words, Chose, Menu, Page, Stage, Where, RETRY_EVERY, SETTLE};
use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::net::{RoomId, RoomInfo};

/// phone is two columns of nothing.
/// A server, what is on it, and a form for what is not.
///
/// **Two columns, and the split says something true**: on the left is what
/// already exists — a list the server owns, which changes every few seconds
/// whether or not you touch it — and on the right is what does not exist yet,
/// which is a form, and yours, and stays exactly where you left it. They are
/// not two panels of the same kind, so they are not drawn as two panels of the
/// same kind: the list sits on the panel's own ground and the form is a card.
///
/// One accent per **column**, not per screen. Each column has exactly one
/// thing you would do next in it — join the world you picked, or make the one
/// you described — and they are in different places, so neither is competing
/// to be the one thing.
///
/// Stacked below [`Metrics::two_column_min`], because two columns of form on a
pub(super) fn play(ui: &mut egui::Ui, theme: &Theme, menu: &mut Menu, at: Where) -> Chose {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = Chose::Nothing;

    let reached = matches!(menu.stage, Stage::Choosing { .. });

    // **One line: where you are, what you are reaching, and what it said.**
    // They were three — a heading, a "Server" label over a row, and the answer
    // under that — which is three lines of chrome above the two columns that
    // are the screen. None of them is worth a line of its own: the heading
    // says one word, the label named the field it sits beside, and the answer
    // is three or four.
    let mut reach = None;
    // **Given the width rather than measured for it.** The card centres its
    // children, so a row narrower than the card is placed according to its own
    // size — which egui knows only from the *last* frame. A row that changed
    // width therefore moved everything in it sideways for a frame, and Back
    // slid two hundred pixels the moment the server controls joined the line.
    // Allocating the full width says where the row is without measuring it.
    let full = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(full, 0.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            // Every screen has a way out, by pointer as well as by escape.
            if ui.small_button(w().menu.back).clicked() {
                menu.page = Page::Home;
            }
            ui.heading(w().menu.home.play);
            reach = server_field(ui, theme, menu, at, reached);
        },
    );
    ui.add_space(m.item_spacing);

    // **Both columns, whatever the server has said.** Gating them on a room
    // list having arrived meant that a server which refused once left no way
    // to make a world either — which is not a consequence anybody would choose
    // and is exactly what it looked like from the outside: a screen with
    // nothing on it.
    //
    // The list is empty until there is one; the form is a form either way, and
    // says what it is waiting for at the point of pressing rather than by
    // being absent.
    let (rooms, note) = match &menu.stage {
        Stage::Choosing { rooms, note } => (rooms.clone(), note.clone()),
        _ => (Vec::new(), None),
    };
    if let Some(reach) = reach {
        chose = reach;
    }

    ui.add_space(m.item_spacing * 2.0);
    if let Some(note) = note {
        ui.colored_label(p.bad, note);
        ui.add_space(m.item_spacing);
    }

    // Two columns where there is room for two, one where there is not.
    if ui.available_width() >= m.two_column_min {
        ui.columns(2, |cols| {
            if let Some(what) = rooms_column(&mut cols[0], theme, menu, &rooms, reached) {
                chose = what;
            }
            if let Some(what) = make_column(&mut cols[1], theme, menu, reached) {
                chose = what;
            }
        });
    } else {
        if let Some(what) = rooms_column(ui, theme, menu, &rooms, reached) {
            chose = what;
        }
        ui.add_space(m.item_spacing * 2.0);
        if let Some(what) = make_column(ui, theme, menu, reached) {
            chose = what;
        }
    }

    chose
}

/// Where to connect, and the reaching itself.
///
/// **There is no button.** Asking a server what it has is not a decision worth
/// a press — it is what the address is *for*, and a field followed by a button
/// that only ever means "yes, that address" is one control too many. So this
/// reaches when the typing settles: on enter, on leaving the field, or after
/// [`SETTLE`] of nothing being typed, whichever comes first.
///
/// Debounced rather than fired per keystroke, because `ws://127.0.0.1:8080/ws`
/// passes through twenty addresses on its way to being one, and every one of
/// them would open a socket.
fn server_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    menu: &mut Menu,
    at: Where,
    reached: bool,
) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut ask = false;
    let mut refresh = false;
    let mut typed = false;

    {
        // **The button comes first and belongs to both clients.** It used to
        // be drawn after the address, inside the branch that makes the address
        // a field — so a browser, whose socket comes from the page it was
        // served by and which therefore has a label rather than a field, got
        // no button at all and had no way to reach anything. The address is
        // what differs between the two; asking is not.
        //
        // One control rather than two: reaching a server for the first time
        // and asking it again are the same act from where a player stands —
        // tell me what is on there, now — so the meaning follows the state and
        // the hover text says which it is.
        // Painted rather than typed, for the reason the back arrow is: the
        // glyph it used to be is in no font this client loads, because this
        // client loads none, and a control that is one symbol has nothing left
        // when the symbol is a box.
        // Sized to the text beside it rather than to a button: it sits in a
        // row of words now, and a square the height of a field made that row
        // as tall as a field for the sake of one glyph.
        let side = m.text_body + 4.0;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
        ui.painter().rect_stroke(
            rect,
            m.rounding,
            egui::Stroke::new(1.0, if response.hovered() { p.text_dim } else { p.line }),
            egui::StrokeKind::Inside,
        );
        crate::client::views::icons::refresh(
            ui.painter(),
            rect,
            if response.hovered() { p.text } else { p.text_dim },
        );
        let go = response.on_hover_text(if reached {
            w().menu.refresh_again
        } else {
            w().menu.refresh_ask
        });

        let entered = if at.on_web {
            // Not a field. The socket is derived from the page's origin, so a
            // typed address here would be a promise the client cannot keep.
            ui.colored_label(p.text_dim, &menu.address);
            false
        } else {
            // **A width of its own, not a share of the row.** Asking for what
            // is left made the row as wide as the address plus everything
            // beside it, and a `horizontal` does not wrap — so the row set the
            // width of the whole screen and the columns under it moved to
            // suit. An address is a known length; this is enough for one.
            let field = ui.add_sized(
                [200.0, m.button_height],
                egui::TextEdit::singleline(&mut menu.address).hint_text(w().menu.server_hint),
            );
            if field.changed() {
                typed = true;
                menu.typed_at = Some(at.now);
                // What is on screen is about an address that is no longer in
                // the field, so it goes rather than contradicting it.
                if !matches!(menu.stage, Stage::Asking) {
                    menu.stage = Stage::Idle;
                }
            }
            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };

        // The button and enter are **deliberate**, so they ask whatever the
        // address is and whatever was asked before. Only the settle is
        // guarded, and only against asking twice about one address with
        // nobody having done anything.
        if go.clicked() || entered {
            // Already connected, and the address has not moved: this is "say
            // that again", which is one small message rather than a new socket.
            refresh = reached && !typed;
            menu.attempted = None;
            ask = true;
        } else if menu.typed_at.is_some_and(|t| at.now - t >= SETTLE) {
            ask = true;
        }
    }

    match &menu.stage {
        Stage::Asking => {
            ui.colored_label(p.text_dim, egui::RichText::new(w().menu.asking).size(m.text_small));
        }
        Stage::Choosing { .. } => {
            ui.colored_label(p.good, egui::RichText::new(w().menu.reached).size(m.text_small));
        }
        Stage::Failed(why) => {
            ui.colored_label(p.bad, egui::RichText::new(why).size(m.text_small));
            // Asked again on its own, on a slow cadence: the usual reason a
            // server does not answer is that it is not running *yet*, and a
            // menu that gives up after one refusal makes that something you
            // have to notice.
            //
            // `failed_at` is stamped when the refusal is on screen and not
            // inside a retry: set only there, it stayed `None` until something
            // had retried, so nothing ever did.
            if menu.failed_at.is_none_or(|t| at.now - t >= RETRY_EVERY) {
                menu.failed_at = Some(at.now);
                menu.attempted = None;
                ask = true;
            }
        }
        // Nothing said. The field has just been typed into and the answer is a
        // fraction of a second away; a line that appeared and vanished between
        // two keystrokes would be noise.
        Stage::Idle => {}
    }

    if refresh {
        return Some(Chose::Refresh);
    }
    if !ask {
        return None;
    }
    menu.typed_at = None;

    // Once per address, for the settle only — a press cleared this above,
    // because a press is somebody asking again on purpose.
    let address = menu.address.trim().to_string();
    if address.is_empty() || menu.attempted.as_deref() == Some(address.as_str()) {
        return None;
    }
    menu.attempted = Some(address.clone());
    Some(Chose::Connect(address))
}

/// What is already here: a list the server owns.
fn rooms_column(
    ui: &mut egui::Ui,
    theme: &Theme,
    menu: &mut Menu,
    rooms: &[RoomInfo],
    // Whether the server has answered, which is a different thing from having
    // answered with nothing: one is a pause and the other is an invitation.
    reached: bool,
) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = None;

    // No refresh here. It is beside the address, which is the same act from
    // where the player stands and was two buttons for one thing.
    ui.label(egui::RichText::new(w().menu.rooms).size(m.text_small));

    if !reached {
        // No answer yet, which is a different thing from a server with nothing
        // on it and reads differently: one is waiting, the other is an
        // invitation.
        ui.colored_label(p.text_dim, egui::RichText::new(w().menu.not_asked).size(m.text_body));
    } else if rooms.is_empty() {
        // An invitation rather than a complaint: there is a form in the next
        // column and this is the moment to point at it.
        ui.colored_label(p.text_dim, egui::RichText::new(w().menu.no_rooms).size(m.text_body));
    } else {
        // Arrow keys walk the list, and enter takes the selection. A list you
        // can only reach with a pointer is a list a keyboard cannot use.
        //
        // Read before the rows are drawn so a press moves the selection in the
        // same frame it happens, rather than a frame behind the eye.
        if !ui.memory(|mem| mem.focused().is_some()) {
            let step = ui.input(|i| {
                i.key_pressed(egui::Key::ArrowDown) as i32
                    - i.key_pressed(egui::Key::ArrowUp) as i32
            });
            if step != 0 {
                let at =
                    menu.selected.as_ref().and_then(|id| rooms.iter().position(|r| r.id == *id));
                let next = match at {
                    // Nothing picked yet: down takes the first, up the last,
                    // which is what every list does.
                    None if step > 0 => 0,
                    None => rooms.len() - 1,
                    Some(i) => (i as i32 + step).rem_euclid(rooms.len() as i32) as usize,
                };
                menu.selected = Some(rooms[next].id.clone());
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(id) = menu.selected.clone() {
                    chose = Some(Chose::Join(id));
                }
            }
        }

        for room in rooms {
            let selected = menu.selected.as_ref() == Some(&room.id);
            match room_row(ui, theme, room, selected) {
                Picked::Nothing => {}
                // Selecting the one already selected puts it away, so a press
                // has somewhere to go back to.
                Picked::Select => {
                    menu.selected = if selected { None } else { Some(room.id.clone()) }
                }
                // The **id**, not the name on the row: two rooms may read
                // alike and only one of them was pressed.
                Picked::Join => chose = Some(Chose::Join(room.id.clone())),
                Picked::Watch => chose = Some(Chose::Watch(room.id.clone())),
            }
        }
        ui.colored_label(
            p.text_dim,
            egui::RichText::new(words::rooms_here(
                rooms.len(),
                rooms.iter().map(|r| r.players).sum(),
            ))
            .size(m.text_small),
        );
    }

    // A code, under the list rather than instead of it: the list is how you
    // find a public world and a code is how you reach somebody's private one.
    // Two ways into what already exists, which is what this column is.
    ui.add_space(m.item_spacing * 2.0);
    ui.label(egui::RichText::new(w().menu.code.label).size(m.text_small));
    ui.horizontal(|ui| {
        let go = ui.add_sized(
            [m.action_height * 1.4, m.button_height],
            egui::Button::new(egui::RichText::new(w().menu.code.go).size(m.text_small)),
        );
        let field = ui.add_sized(
            [ui.available_width(), m.button_height],
            egui::TextEdit::singleline(&mut menu.code).hint_text(w().menu.code.hint),
        );
        // Return submits, because a six-character field is one you type and
        // press enter on without looking for a button.
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (go.clicked() || entered) && !menu.code.trim().is_empty() {
            // A code reaches a room the same way an id does — the server
            // resolves an id, then a name, then a code — so the client needs
            // no second message for it.
            chose = Some(Chose::Join(RoomId(menu.code.trim().to_string())));
        }
    });

    chose
}

/// What does not exist yet: a form, and yours.
///
/// Always here rather than behind a press. It had to be opened when it lived
/// under the list, because a form there pushed the list off the screen; in a
/// column of its own there is nothing to push, and a button whose only job is
/// to reveal what would fit anyway is a press that buys nothing.
pub(super) fn make_column(
    ui: &mut egui::Ui,
    theme: &Theme,
    menu: &mut Menu,
    reached: bool,
) -> Option<Chose> {
    let draft = menu.draft.get_or_insert_with(Draft::default);
    let made = make_form(ui, theme, draft, reached);

    // **With no server, the same form plays it here** rather than refusing.
    // Every question on it — how big, does it end, how — is answerable without
    // anybody else; what a server adds is a name, a listing and other people,
    // which is exactly what the form hides when there is nobody to ask.
    // `make_form` decides which of the two it is producing, so there is
    // nothing to rewrite here.
    made
}

/// What a room row was clicked for. Two things can be done with a room, so a
/// bool would have to be a bool about which.
enum Picked {
    Nothing,
    /// Point at this room, so its actions appear inside it.
    Select,
    Join,
    Watch,
}

fn make_form(ui: &mut egui::Ui, theme: &Theme, draft: &mut Draft, reached: bool) -> Option<Chose> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut chose = None;

    egui::Frame::new()
        .fill(p.surface_lift)
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(w().menu.make.title).size(m.text_body));
            ui.add_space(m.item_spacing);

            // Not asked for on a private room, whose name is the code the
            // server generates. A field being quietly discarded is worse than
            // one that is not there — the same rule that hides the size on a
            // boundless world.
            // A world nobody else can reach needs no name, no listing and no
            // sides: there is one player. Hidden rather than disabled, because
            // a field that cannot be wrong is a field that should not be
            // asked about.
            if reached && draft.access == Access::Listed {
                ui.label(egui::RichText::new(w().menu.make.name).size(m.text_small));
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .desired_width(f32::INFINITY)
                        .hint_text(w().menu.make.name_hint),
                );
                ui.add_space(m.item_spacing);
            }

            // **What kind of room, asked once and answered before anything
            // it decides.** A match is the only one with a way to win, and an
            // experiment is the only one whose rules are yours — so the rows
            // below it appear or do not according to this.
            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(w().menu.make.kind).size(m.text_small));
            // **All three, with or without a server.** A solitary match is a
            // real thing and always was — the client is the authority offline,
            // so there is one world, one clock and one player, and `Victory`
            // is something it can settle for itself. Leaving it off this form
            // hid a feature the client already had.
            toggles(
                ui,
                theme,
                &mut draft.kind,
                &[
                    (Kind::World, w().menu.make.world),
                    (Kind::Match, w().menu.make.r#match),
                    (Kind::Experiment, w().menu.make.experiment),
                ],
            );
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(match draft.kind {
                    Kind::World => w().menu.make.world_note,
                    Kind::Match => w().menu.make.match_note,
                    Kind::Experiment => w().menu.make.experiment_note,
                })
                .size(m.text_small),
            );

            {
                ui.add_space(m.item_spacing);
                ui.label(egui::RichText::new(w().menu.make.shape).size(m.text_small));
                // **The size lives inside the Wrapping button.** As its own row it
                // pushed everything under it down the moment the shape changed, so
                // choosing a shape moved the button you were about to press next —
                // and on a small screen it pushed the action off the bottom. The
                // row grows in place instead: the option that has a size is the
                // one that holds it.
                shape_row(ui, theme, draft);
            }

            // Teams, on any of the three: a team is people playing as one
            // player, which is worth having without a result to win and worth
            // having in a laboratory, where it is who shares a bench. Needs a
            // server, because it needs somebody to share with.
            if reached {
                ui.add_space(m.item_spacing);
                ui.label(egui::RichText::new(w().menu.make.together).size(m.text_small));
                toggles(
                    ui,
                    theme,
                    &mut draft.teams,
                    &[(false, w().menu.make.solo), (true, w().menu.make.teams)],
                );
                if draft.teams {
                    ui.add_space(m.item_spacing);
                    ui.horizontal_top(|ui| {
                        ui.set_min_height(m.button_height);
                        ui.colored_label(
                            p.text_dim,
                            egui::RichText::new(w().menu.make.sides).size(m.text_small),
                        );
                        ui.add_sized(
                            [m.action_height * 1.6, m.button_height],
                            egui::TextEdit::singleline(&mut draft.team_count),
                        );
                    });
                    ui.colored_label(
                        p.text_dim,
                        egui::RichText::new(w().menu.make.sides_note).size(m.text_small),
                    );
                }
            }

            // **Three answers, and solo is one of them.** Playing alone was a
            // page of its own, asking these same questions, reached from
            // somewhere else — which made it a thing you had to already know
            // about. It is not a different errand: it is this form with the
            // last question answered "nobody".
            //
            // Shown whether or not a server has answered. With none, the only
            // answer that can work is `Solo`, and the toggles say so by being
            // the place the choice lives rather than by disappearing.
            //
            // **And it is the only place it lives.** Playing alone had a page
            // and then a full-width button under this form, and it is neither:
            // it is this form with the last question answered "nobody". What
            // that costs is findability, which the *label* pays rather than a
            // second control -- the answer is called "Play alone" and not
            // "Just me" for exactly that reason.
            ui.add_space(m.item_spacing);
            ui.label(egui::RichText::new(w().menu.make.private).size(m.text_small));
            let mut access = draft.access;
            let listed = [
                (Access::Listed, w().menu.make.listed),
                (Access::ByCode, w().menu.make.unlisted),
                (Access::Solo, w().menu.make.solo_access),
            ];
            toggles(ui, theme, &mut access, if reached { &listed } else { &listed[2..] });
            draft.access = if reached { access } else { Access::Solo };
            ui.colored_label(
                p.text_dim,
                egui::RichText::new(match draft.access {
                    Access::Listed => w().menu.make.listed_note,
                    Access::ByCode => w().menu.make.unlisted_note,
                    Access::Solo => w().menu.make.solo_note,
                })
                .size(m.text_small),
            );

            if draft.kind == Kind::Match {
                ui.add_space(m.item_spacing);
                ui.label(egui::RichText::new(w().menu.make.ends).size(m.text_small));
                let mut ends = draft.ends;
                toggles(
                    ui,
                    theme,
                    &mut ends,
                    &[
                        (Ends::Timer, w().menu.make.timer),
                        (Ends::Territory, w().menu.make.territory),
                    ],
                );
                draft.retarget(ends);
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(match draft.ends {
                        Ends::Timer => w().menu.make.timer_note,
                        Ends::Territory => w().menu.make.territory_note,
                    })
                    .size(m.text_small),
                );
            }

            if draft.kind == Kind::Match {
                ui.add_space(m.item_spacing);
                ui.label(
                    egui::RichText::new(match draft.ends {
                        Ends::Territory => w().menu.make.squares,
                        _ => w().menu.make.generations,
                    })
                    .size(m.text_small),
                );
                ui.add(egui::TextEdit::singleline(&mut draft.target).desired_width(f32::INFINITY));
                ui.colored_label(
                    p.warn,
                    egui::RichText::new(w().menu.make.match_waits).size(m.text_small),
                );
            }

            if let Some(note) = &draft.note {
                ui.add_space(m.item_spacing);
                ui.colored_label(p.bad, egui::RichText::new(note).size(m.text_small));
            }

            ui.add_space(m.item_spacing * 2.0);
            if draft.asking {
                ui.colored_label(
                    p.text_dim,
                    egui::RichText::new(w().menu.make.making).size(m.text_small),
                );
            } else if crate::client::views::wide(
                ui,
                // **The action follows the last answer on the form**, which is
                // who can find it. Make it on the server, or play it here.
                egui::RichText::new(if reached && draft.access != Access::Solo {
                    w().menu.make.make
                } else {
                    w().menu.make.alone
                })
                .size(m.text_action)
                // The accent belongs on it either way now: the form always has
                // an action that works, because solo is one of its answers
                // rather than something that happens when nothing else can.
                .color(p.ground),
                m.action_height,
                p.accent,
            )
            .clicked()
            {
                // Refused here or refused there, into the same line under the
                // same form: a name that is too long and a name already taken
                // are the same kind of answer to the player.
                if reached && draft.access != Access::Solo {
                    match draft.parse() {
                        Ok(made) => {
                            draft.note = None;
                            draft.asking = true;
                            chose = Some(made);
                        }
                        Err(why) => draft.note = Some(why),
                    }
                } else {
                    // **With no server, the same form plays it here.** This
                    // sent a `Create` whatever the button said, so pressing
                    // "Play alone" asked a server that was not there and came
                    // back "the connection went away" — `Chose::Alone` existed
                    // the whole time and nothing produced it.
                    //
                    // `world` rather than `parse`: a name, a listing and sides
                    // are what a *server* adds, and the form already hides all
                    // three when there is nobody to ask.
                    match draft.world() {
                        Ok((shape, victory)) => {
                            draft.note = None;
                            chose = Some(Chose::Alone {
                                shape,
                                victory,
                                laboratory: draft.kind == Kind::Experiment,
                            });
                        }
                        Err(why) => draft.note = Some(why),
                    }
                }
            }
            ui.add_space(m.item_spacing);
            if crate::client::views::wide(
                ui,
                egui::RichText::new(w().menu.make.clear).size(m.text_small),
                m.button_height,
                p.surface,
            )
            .clicked()
            {
                chose = Some(Chose::Clear);
            }
        });

    chose
}

/// Shape, with the size inside the option it belongs to.
///
/// **A toggle that can hold the number fields**, which is what the row wanted
/// all along and what two attempts at it missed. It was a bare button beside a
/// framed panel, then two framed panels — and a frame brings its own padding,
/// so the row came out half again as tall as every other toggle on the form
/// and then twice as tall once a text field was in it.
///
/// So the cell is a rectangle of exactly [`Metrics::button_height`], painted
/// like a toggle and laid out inside itself. Nothing can push it taller,
/// because nothing is measured: the box is the size and the contents go in it.
///
/// [`Metrics::button_height`]: crate::client::views::theme::Metrics::button_height
fn shape_row(ui: &mut egui::Ui, theme: &Theme, draft: &mut Draft) {
    let m = theme.metrics;
    let wrapping = draft.shape == Shape::Wrapping;

    ui.horizontal(|ui| {
        let each = (ui.available_width() - m.item_spacing) / 2.0;
        if cell(ui, theme, each, !wrapping, w().menu.make.boundless, |_, _| {}) {
            draft.shape = Shape::Boundless;
        }
        // **Shown either way**, greyed when the world does not wrap. Appearing
        // and disappearing made the button change width as you chose, so the
        // two options moved under the pointer — and a control that is not
        // there cannot be read before it matters.
        //
        // `12x12`, which is how a size is written and how `--torus` takes one.
        // The unit is on hover: worth knowing once and worth no space after.
        let (rows, cols) = (&mut draft.rows, &mut draft.cols);
        let pressed = cell(ui, theme, each, wrapping, w().menu.make.wrapping, |ui, ink| {
            ui.add_enabled_ui(wrapping, |ui| {
                let box_ = |ui: &mut egui::Ui, field: &mut String| {
                    ui.add(
                        egui::TextEdit::singleline(field)
                            .desired_width(m.button_height * 0.9)
                            .margin(egui::Margin::symmetric(2, 0)),
                    )
                    .on_hover_text(w().menu.make.size_note);
                };
                box_(ui, rows);
                ui.colored_label(ink, egui::RichText::new(w().menu.make.by).size(m.text_small));
                box_(ui, cols);
            });
        });
        if pressed {
            draft.shape = Shape::Wrapping;
        }
    });
}

/// **One option, anywhere on this form**: a toggle-shaped box of a fixed
/// height, with its label and whatever else belongs to it inside.
///
/// The box is allocated first and everything is drawn *into* it, so the height
/// is the height whatever goes in — which a `Frame` cannot promise, because a
/// frame grows to fit, and which is what made the shape row twice the height
/// of every other row twice over.
///
/// **Left-aligned, and everything here is.** The shape options hold two number
/// fields beside their label and so cannot be an `egui::Button`, which centres;
/// the rest were buttons and did. A form where the alignment depends on which
/// question a row is asking is a form that looks broken, so the cell is the
/// only option-shaped thing on the screen and it aligns one way.
fn cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    on: bool,
    label: &str,
    inside: impl FnOnce(&mut egui::Ui, egui::Color32),
) -> bool {
    let (p, m) = (theme.palette, theme.metrics);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, m.button_height), egui::Sense::click());
    ui.painter().rect(
        rect,
        m.rounding,
        if on { p.accent } else { p.surface },
        egui::Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    let ink = if on { p.ground } else { p.text };
    // Down the middle vertically and against the left edge horizontally, which
    // is the one alignment a row holding a label *and* two number fields can
    // keep — and so is the one every row keeps.
    let inner = rect.shrink2(egui::vec2(m.panel_padding * 0.5, 0.0));
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
    child.spacing_mut().item_spacing.x = m.item_spacing * 0.5;
    child.colored_label(ink, egui::RichText::new(label).size(m.text_small));
    inside(&mut child, ink);
    response.clicked()
}

// fn toggle_inset(theme: &Theme, label: &str, on: bool, )

/// One decision as a row of buttons, the chosen one wearing the accent.
///
/// The whole choice on screen at once, which is the argument against a
/// drop-down for anything this narrow: two or three words fit, and a player
/// reading a form should not have to open something to find out what the
/// alternatives were.
fn toggles<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &mut T,
    options: &[(T, &str)],
) {
    let m = theme.metrics;
    // Top-aligned with a stated height, because `ui.horizontal` centres every
    // item against a row height it does not know until the last one is
    // measured -- see docs/gotchas.md.
    ui.horizontal_top(|ui| {
        ui.set_min_height(m.button_height);
        let each = (ui.available_width() - m.item_spacing * (options.len() as f32 - 1.0))
            / options.len() as f32;
        for (option, label) in options {
            // **The same cell every option on this form is.** These were
            // `egui::Button`s, which centre their label, beside a shape row
            // that could not be one because it has fields in it — so half the
            // form was centred and half was not, and no row on it agreed with
            // the row above. One helper, one alignment.
            if cell(ui, theme, each, *value == *option, label, |_, _| {}) {
                *value = *option;
            }
        }
    });
}

/// One room in the list: what it is called, whether anybody is in it, whether
/// it ends — and, **if it is the one selected**, what can be done with it.
///
/// The actions live inside the selection rather than beside every row. A row
/// of buttons on every entry makes the list twice as tall and twice as busy to
/// read, and most of those buttons belong to rooms nobody is looking at. One
/// selection, and Join and Watch appear in it.
///
/// Watching is offered on **every** room and not only on matches, because
/// no late joining is a rule about players: a match already running is exactly
/// the room whose only way in is to watch.
fn room_row(ui: &mut egui::Ui, theme: &Theme, room: &RoomInfo, selected: bool) -> Picked {
    let p = theme.palette;
    let m = theme.metrics;
    let mut picked = Picked::Nothing;

    egui::Frame::new()
        .fill(if selected { p.surface_lift } else { p.surface })
        .stroke(egui::Stroke::new(1.0, if selected { p.accent } else { p.line }))
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding * 0.6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(m.row_height * 0.6);
                ui.label(egui::RichText::new(&room.name).size(m.text_body));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(
                        if room.players > 0 { p.good } else { p.text_dim },
                        egui::RichText::new(players(room.players)).size(m.text_small),
                    );
                });
            });

            // A room and a match are the same thing to everything else, so
            // this list is the one place the difference has to show — clicking
            // into a match that has already started only to be refused is a
            // worse way to find out.
            let mut under = describe(room.world);
            let kind = crate::net::RoomKind::of(room.victory, &room.rules);
            if let Some(name) = crate::client::views::words::room_kind(kind) {
                under = format!("{under} · {name}");
            }
            if let Some(victory) = room.victory {
                under = format!(
                    "{under} · {} · {}",
                    crate::client::views::words::phase(&room.phase),
                    crate::client::views::words::describe(victory)
                );
            }
            // The one thing about a laboratory worth knowing before going in:
            // a stopped world looks exactly like a broken one from outside.
            if room.rules.laboratory && room.rules.paused {
                under = format!("{under} · {}", w().stopped);
            }
            ui.colored_label(
                if matches!(room.phase, crate::net::MatchPhase::Gathering) {
                    p.good
                } else {
                    p.text_dim
                },
                egui::RichText::new(under).size(m.text_small),
            );

            // **Selection takes the name and the line under it, and stops
            // there.** It used to take the whole row and was registered
            // *after* the buttons below, which in an immediate-mode interface
            // puts it on top of them: a press on Join never reached Join, it
            // reached the row, and the row's answer to being pressed while
            // selected is to deselect. That is a Join button that visibly
            // depresses and puts the room away instead of entering it.
            //
            // Taken before the buttons exist, so there is nothing for it to
            // cover.
            let head = ui.min_rect();
            if ui.interact(head, ui.id().with(&room.id), egui::Sense::CLICK).clicked() {
                picked = Picked::Select;
            }

            if selected {
                ui.add_space(m.item_spacing);
                ui.horizontal_top(|ui| {
                    ui.set_min_height(m.button_height);
                    let each = (ui.available_width() - m.item_spacing) / 2.0;
                    if ui
                        .add_sized(
                            [each, m.button_height],
                            egui::Button::new(
                                egui::RichText::new(w().menu.watch.join)
                                    .size(m.text_small)
                                    .color(p.ground),
                            )
                            .fill(p.accent),
                        )
                        .clicked()
                    {
                        picked = Picked::Join;
                    }
                    if ui
                        .add_sized(
                            [each, m.button_height],
                            egui::Button::new(
                                egui::RichText::new(w().menu.watch.watch).size(m.text_small),
                            ),
                        )
                        .clicked()
                    {
                        picked = Picked::Watch;
                    }
                });
            }
        });

    picked
}
