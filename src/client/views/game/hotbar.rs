//! What a click puts down.
//!
//! ```text
//!   [ figures ] [ Life Factory Turret Ice ] [ shape │ grab │ stamps │ ⋯ ] [ ▶ +1 ⚙ ] [ ? ]
//! ```
//!
//! [`slots`] is that line as a list, and the keyboard, the key list and the
//! layout all read it.
//!
//! Digits are the stamps, shift-and-a-digit is everything else. Bound by
//! position so it is the same key on every layout, labelled with whatever that
//! key types — see [`hint`].

use crate::client::views::game::stamp::{Library, Stamp};
use crate::client::views::glyph;
use crate::client::views::icons::{self, Icons};
use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::net::Placement;
use crate::sim::{Cell, Kind, PlayerId};

/// What a drag lays: the **shape** axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stroke {
    /// Every cell the pointer crosses. Drawing, rather than specifying: you
    /// watch the line appear under your hand and stop when it looks right.
    Pencil,
    /// Every cell between the two corners. A pane is a shape you place, and
    /// dragging one out says how big before it exists.
    Rectangle,
}

/// One of the four things a cell can be: the **kind** axis.
///
/// It was a tool, which is to say a shape and a material at once — a factory came
/// with a pencil and ice came with a rectangle, and there was no way to draw a
/// line of ice or sweep a pane of factories. The two are chosen separately now and
/// this is half of the choice.
#[derive(Clone, Copy)]
pub struct Tool {
    pub name: &'static str,
    /// The cell this puts down, so a button can show it rather than spell it.
    pub shows: Cell,
    /// What the server is asked for. A name rather than cell bits, so the
    /// server can judge the request.
    pub placement: Placement,
    /// The shape this kind is usually wanted in: a pencil for life, a
    /// rectangle for ice, which is a wall and so a thing you say the size of.
    /// Taken when the square is picked — see [`crate::client::views::game::GameApp::pick`]
    /// — and still overridable afterwards, because the two axes are two.
    pub usually: Shape,
    /// Whether a rule goes before this square, because what it lays is a
    /// different sort of thing from what the one before it lays.
    pub apart: bool,
}

/// What each tool leaves on a square, which is what its button shows. Built
/// here rather than named, so a button cannot show one thing and lay another.
const LIVE: Cell = Cell::DEAD.with_alive(true);
const FACTORIED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::FACTORY);
const TURRETED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::TURRET);
const ICED: Cell = Cell::DEAD.with_ice(true);
/// Age nought, which is a fuse that has not started. The bar shows what a
/// square puts down, and what it puts down is unlit.
const DYNAMITED: Cell = Cell::DEAD.with_alive(true).with_kind(Kind::DYNAMITE);

/// The left segment: what you draw with.
/// The kinds, in the order they sit on the bar.
///
/// **One list, and ice is in it.** Ice lived apart while it came with a stroke
/// of its own; with the stroke chosen separately there is nothing left to
/// separate it by, and a new kind appears here by existing.
/// **A function, because a name is a word and words are chosen at run time.**
///
/// This was a `const`, which it could be while every string was a `const` too.
/// A language is picked rather than compiled in now — see
/// [`crate::client::views::words`] — so the table is built when it is asked
/// for. It is five structs of pointers and it is asked for once a frame.
pub fn kinds() -> [Tool; 5] {
    [
        Tool {
            name: w().hotbar.life,
            shows: LIVE,
            placement: Placement::Life,
            usually: Shape::Draw,
            apart: false,
        },
        Tool {
            name: w().hotbar.factory,
            shows: FACTORIED,
            placement: Placement::Factory,
            usually: Shape::Draw,
            apart: false,
        },
        Tool {
            name: w().hotbar.turret,
            shows: TURRETED,
            placement: Placement::Turret,
            usually: Shape::Draw,
            apart: false,
        },
        // A pencil, not a pane. A dynamite is placed one at a time and kept alive
        // by what is built round it, so a drag that laid twenty is a gesture
        // nobody wants and could afford even less.
        Tool {
            name: w().hotbar.dynamite,
            shows: DYNAMITED,
            placement: Placement::Dynamite,
            usually: Shape::Draw,
            apart: false,
        },
        // **Last, and set apart.** Every square before it puts a cell on a square;
        // ice puts a pane *over* one, and is the only thing here that is not a
        // living thing you own. A rule before it says so without a word.
        Tool {
            name: w().hotbar.ice,
            shows: ICED,
            placement: Placement::Ice,
            usually: Shape::Rect,
            apart: true,
        },
    ]
}

/// What a gesture makes: the shape axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shape {
    /// A pencil. Every cell the pointer crosses.
    #[default]
    Draw,
    /// A pane. Every cell between the corners.
    Rect,
    /// Nothing yet: the next drag takes a stamp rather than laying anything.
    ///
    /// Its own square because otherwise there is nowhere to start. Capturing
    /// used to be "drag with a stamp held", which is fine once you have one
    /// and impossible before — the first stamp had no way to exist.
    Capture,
    /// A saved pattern, by its place in the library.
    Stamp(usize),
}

impl Shape {
    /// The other of the two shapes a gesture is usually in.
    ///
    /// From a stamp or a capture this is `Draw`, so the one square is a way
    /// back to drawing as well as a way between the two — the same reasoning
    /// as the key beside it.
    pub fn other(self) -> Self {
        match self {
            Self::Draw => Self::Rect,
            _ => Self::Draw,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Draw => w().hotbar.draw,
            Self::Rect => w().hotbar.pane,
            Self::Capture => w().hotbar.capture,
            Self::Stamp(_) => w().hotbar.pattern,
        }
    }
}

/// **Two axes: a shape and what it is made of.** The shape says how cells are
/// chosen, the kind says what goes in them, and every combination is reachable
/// — a line of ice and a pane of factories were both unsayable when it was one
/// list of tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Held {
    pub shape: Shape,
    /// How the held pattern is turned. Nothing but a stamp has an orientation,
    /// and a stamp keeps this across a change of material — turning a glider
    /// and then deciding it should be ice is two decisions, not one undone.
    pub turn: crate::client::views::game::stamp::Turn,
    /// Which of [`kinds`]. An index rather than the `Placement`, so a square
    /// on the bar and the thing it lays cannot come apart.
    pub kind: usize,
}

impl Held {
    /// Whether a drag with this held takes a stamp rather than laying one.
    pub fn captures(self) -> bool {
        matches!(self.shape, Shape::Capture | Shape::Stamp(_))
    }

    /// The kind this is holding.
    ///
    /// By value rather than by reference, because the table is built when it is
    /// asked for now — a `Tool` is five pointers and a bool, so copying one is
    /// cheaper than the `static` it used to be borrowed from was to reach.
    pub fn tool(self) -> Option<Tool> {
        kinds().get(self.kind).copied()
    }

    pub fn placement(self) -> Option<Placement> {
        self.tool().map(|t| t.placement)
    }

    /// What a drag draws. A stamp is placed by a click, so dragging with one
    /// held sweeps the rectangle that captures another.
    pub fn stroke(self) -> Stroke {
        match self.shape {
            Shape::Draw => Stroke::Pencil,
            Shape::Rect | Shape::Capture | Shape::Stamp(_) => Stroke::Rectangle,
        }
    }

    /// Back to the shape this kind is usually wanted in — which is also the
    /// way out of a stamp or a capture without looking at the bar.
    pub fn defaulted(self) -> Self {
        let shape = self.tool().map(|k| k.usually).unwrap_or_default();
        // The turn goes too. "Put me back to normal" that left a pattern
        // rotated would be a reset somebody has to reset after.
        Self { shape, turn: Default::default(), ..self }
    }
}

/// Something on the bar that a key can pick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// Open the list of every key. A square as well as a key, because `?` was
    /// discoverable only by pressing `?`.
    Help,
    /// Take a pattern off the board, rather than laying one.
    ///
    /// **Its own variant rather than a `Shape`**, and so is [`Self::Stamp`].
    /// This was `Shape(Shape)`, which admitted `Draw` and `Rect` — two shapes
    /// that have no square of their own, because the flip square is where both
    /// live. Nothing ever built one, and the arm that would have drawn it gave
    /// a blank square with an empty tooltip. Two variants for the two things
    /// that *are* squares leaves nothing to draw blank.
    Capture,
    /// A saved pattern, by its place in the library.
    Stamp(usize),
    /// Pick a kind, leaving the shape alone: choosing a material does not put
    /// your pencil down.
    Kind(usize),
    /// The stamps that did not fit.
    More,
    /// Run the world, or stop it.
    Run,
    /// One generation, and stay stopped.
    Step,
    /// Open the laboratory's settings: what the game's rules are doing.
    Rules,
    /// Empty this laboratory. Only offered where the clock is, because both
    /// are things only a laboratory lets anybody do.
    Wipe,
    /// The shape square: pencil and pane are one choice with two answers, so
    /// this is the other one. A `Key` rather than a `Shape` because which one
    /// it means depends on what is held, and [`slots`] is a fixed list.
    Flip,
}

/// Which key picks a square.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// Shift and a digit: the tools.
    Shift(u32),
    /// A bare digit: the stamps. `1` to `9` then `0`, which is ten and is why
    /// [`ON_THE_BAR`](crate::client::views::game::stamp::ON_THE_BAR) is ten.
    Digit(u32),
    /// A key of its own, spelled the way a keycap is.
    Named(&'static str),
    /// **The space bar**, which has no spelling.
    ///
    /// Its own variant because neither bundled face has a glyph for one — see
    /// [`crate::client::views::icons::space_bar`], which draws it. It was
    /// `Named("run, or stop running (alone, or in a laboratory)")`: the
    /// *description* of the key, printed at 13px into the corner of a 44px
    /// square, where every other square has one or two characters.
    Space,
    None,
}

/// Which boxed group a square sits in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// What a cell is made of.
    Kinds,
    /// What shape it is drawn in, and the patterns.
    Shapes,
    /// The world's clock. Offline only: connected, the server keeps time.
    Clock,
    Help,
}

/// One square on the bar.
pub struct Slot {
    pub key: Key,
    pub group: Group,
    pub press: Press,
    /// A hairline before it: same group, different job.
    pub rule: bool,
}

/// The whole bar, in the order it sits. The keyboard, the key list and the
/// layout all read this, so they cannot go out by one. No egui, so it tests
/// without a window.
pub fn slots(library: &Library, clock: bool) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut shift = 0;
    let mut tool = |out: &mut Vec<Slot>, key, group, rule| {
        shift += 1;
        out.push(Slot { key, group, press: Press::Shift(shift), rule });
    };

    for (i, kind) in kinds().iter().enumerate() {
        tool(&mut out, Key::Kind(i), Group::Kinds, kind.apart);
    }
    tool(&mut out, Key::Flip, Group::Shapes, false);
    tool(&mut out, Key::Capture, Group::Shapes, true);

    // A slot on the bar is a place; the stamp standing in it is looked up.
    // They were one number while the bar was the first ten of the library and
    // are not once a stamp can be pinned — and what is *held* stays a library
    // index, so re-pinning does not change what is in your hand.
    for (slot, i) in library.bar().into_iter().enumerate() {
        out.push(Slot {
            key: Key::Stamp(i),
            group: Group::Shapes,
            press: Press::Digit(if slot == 9 { 0 } else { slot as u32 + 1 }),
            rule: slot == 0,
        });
    }
    tool(&mut out, Key::More, Group::Shapes, true);

    if clock {
        for (key, press, rule) in [
            (Key::Run, Press::Space, false),
            (Key::Step, Press::Named(w().hotbar.step_key), false),
            (Key::Rules, Press::None, false),
            // Behind a rule, because it is the one square here that cannot be
            // undone: the other three change what the world is doing and this
            // one throws it away.
            (Key::Wipe, Press::None, true),
        ] {
            out.push(Slot { key, group: Group::Clock, press, rule });
        }
    }
    out.push(Slot {
        key: Key::Help,
        group: Group::Help,
        press: Press::Named(w().hotbar.help),
        rule: false,
    });
    out
}

/// Which stamp slot a bare digit picks.
pub fn stamp_for_digit(digit: u32) -> Option<usize> {
    match digit {
        0 => Some(9),
        1..=9 => Some(digit as usize - 1),
        _ => None,
    }
}

/// What shift and a digit picks, read off [`slots`] so it cannot disagree
/// with what is drawn.
pub fn shifted_for_digit(digit: u32, library: &Library) -> Option<Key> {
    slots(library, true).into_iter().find(|s| s.press == Press::Shift(digit)).map(|s| s.key)
}

/// The squares shift and a digit reaches, which is what the key list counts.
pub fn shifted(library: &Library) -> Vec<Key> {
    slots(library, true)
        .into_iter()
        .filter(|s| matches!(s.press, Press::Shift(_)))
        .map(|s| s.key)
        .collect()
}

/// What a square's corner says, on *this* keyboard.
///
/// Asked rather than assumed: the digit row prints `&é"'(-è_çà` on AZERTY, and
/// a bar labelled `1`-`0` there names ten keys the keyboard does not have. The
/// US guess is seeded, so the fallbacks only fire on a key nothing has been
/// able to name.
fn hint(press: Press, look: &Look<'_>) -> Option<String> {
    match press {
        Press::Shift(d) => Some((look.typed)(d).unwrap_or_else(|| format!("S{d}"))),
        Press::Digit(d) => Some((look.plain)(d).unwrap_or_else(|| d.to_string())),
        Press::Named(key) => Some(key.to_string()),
        // Drawn by `square`, so there is nothing to spell.
        Press::Space | Press::None => None,
    }
}

/// What shift and a digit types here. Asked of the keyboard rather than
/// assumed — see the module note.
pub type Typed<'a> = dyn Fn(u32) -> Option<String> + 'a;

/// Everything a square might need to draw itself.
pub struct Look<'a> {
    pub theme: &'a Theme,
    /// What the kind axis is holding, which is what a stamp's square is drawn
    /// in: a pattern is a shape, so the thumbnail shows what it would come out
    /// as rather than how it was captured.
    pub what: Placement,
    /// The sprite sheet in this player's colour, if it could be built.
    pub sheet: Option<egui::TextureId>,
    pub player: PlayerId,
    /// What shift and a digit types here.
    pub typed: &'a Typed<'a>,
    /// And what the digit types on its own, which is what a stamp square
    /// shows. Two closures rather than one taking a shift flag, because every
    /// call site knows which row it is drawing.
    pub plain: &'a Typed<'a>,
    /// Whether this client keeps its own time, which is to say whether it is
    /// offline. Connected, the server is the clock and the section is not
    /// drawn — a stopped board would be a lie about a world that is moving.
    pub own_clock: bool,
    pub paused: bool,
    /// Whether the rules panel is open, so its square reads as pressed.
    pub showing_rules: bool,
    /// Whether this room charges for placing — see `net::Rules::place_free`. A
    /// price on a square that will not charge you is a number to plan around
    /// that is not true.
    pub free: bool,
}

/// What a square shows, what it is called, and whether it is the current one.
fn face_of<'a>(
    key: Key,
    held: Held,
    library: &'a Library,
    look: &Look<'_>,
    turned: &'a mut Option<Stamp>,
    label: &'a mut String,
) -> (Face<'a>, &'a str, bool) {
    match key {
        Key::Kind(i) => (Face::Sprite(kinds()[i].shows), kinds()[i].name, held.kind == i),
        // One square, not two: draw and pane are one choice with two answers,
        // so it shows which is current rather than offering both. Clicking it
        // is also the way back to drawing from a stamp.
        //
        // **Lit only when it is what is held**, which it was not: it read as
        // pressed unconditionally, so with a stamp in hand the bar showed a
        // highlighted pencil beside a highlighted stamp and claimed you were
        // holding both. What the square shows follows the same rule — the
        // shape you are actually in, not the one this kind is usually wanted
        // in, so a pane held as a pencil says pencil.
        Key::Flip => {
            let flip = flip_face(held);
            (Face::Icon(flip.icon), flip.name, flip.lit)
        }
        Key::Capture => (Face::Camera, w().hotbar.capture, held.shape == Shape::Capture),
        Key::Stamp(i) => {
            let stamp = library.get(i).expect("a slot names a stamp the library holds");
            // Shown as it would be laid, turn and all: a thumbnail that stayed
            // upright while the preview rotated would be two answers to what
            // is about to happen.
            if held.shape == Shape::Stamp(i) && !held.turn.is_default() {
                *turned = Some(stamp.turned(held.turn));
            }
            let shown = turned.as_ref().unwrap_or(stamp);
            (Face::Pattern(shown), &shown.name, held.shape == Shape::Stamp(i))
        }
        // **A stamp, because that is what is behind the square.** It was an
        // ellipsis, in a row where everything else is a picture of what it
        // does; three dots say "there is more" without saying more of what.
        // How many did not fit goes in the hover, which is where a number
        // nobody has to act on belongs.
        Key::More => {
            let over = library.len() - library.on_the_bar();
            *label = if over > 0 {
                format!("{} (+{over})", w().hotbar.library)
            } else {
                w().hotbar.library.to_string()
            };
            (Face::Icon(glyph::STAMP), label, false)
        }
        Key::Run if look.paused => (Face::Icon(glyph::PLAY), w().hotbar.run_hint, true),
        Key::Run => (Face::Icon(glyph::PAUSE), w().hotbar.stop_hint, false),
        Key::Step => (Face::Icon(glyph::STEP), w().hotbar.step_hint, false),
        Key::Rules => (Face::Icon(glyph::GEAR), w().hotbar.rules_hint, look.showing_rules),
        Key::Wipe => (Face::Icon(glyph::TRASH), w().hotbar.wipe_hint, false),
        Key::Help => (Face::Icon(glyph::HELP), w().hotbar.help_hint, false),
    }
}

/// **What a square costs to lay one of**, or `None` for one that costs nothing.
///
/// Only the materials. A shape is free, the clock is free, and a stamp's price
/// is its cells times whatever it is being made of — a number that changes with
/// the *other* axis, so putting it on a stamp's square would be a figure that
/// moved when you pressed something else.
///
/// **Nothing at all where placing is free**, which a laboratory's rules panel
/// can turn on: a price tag on a square that will not charge you is worse than
/// no price tag, because it is a number somebody could plan around.
fn price_of(key: Key, look: &Look<'_>) -> Option<i32> {
    if look.free {
        return None;
    }
    match key {
        Key::Kind(i) => Some(kinds().get(i)?.placement.cost()),
        _ => None,
    }
}

/// What the shape square shows, says, and whether it is lit.
///
/// **One struct because it is one decision.** The picture and the word were
/// chosen apart and came to disagree: with a stamp in hand the square drew a
/// *pencil* — correctly, because pressing it goes back to drawing — and
/// labelled itself "pattern", which is what you are holding rather than what
/// the square does. Two fields set in one place cannot drift like that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Flip {
    icon: &'static str,
    name: &'static str,
    /// Whether this is the shape in hand, rather than the one pressing it
    /// would put you in.
    lit: bool,
}

/// A free function because it is the whole of the decision and needs none of
/// the frame around it — which is what lets a test say that holding a stamp
/// does not also light the pencil, and that the square never names one thing
/// while drawing another.
fn flip_face(held: Held) -> Flip {
    // Never `Capture` or `Stamp`: those two have squares of their own, drawn
    // as a camera and as the pattern itself, and neither is a glyph. What
    // reaches here is the shape a *kind* is usually wanted in, which is one of
    // these two -- `a_tool_is_usually_drawn_or_panned` holds that.
    let of = |shape: Shape| match shape {
        Shape::Rect => Flip { icon: glyph::RECT, name: shape.name(), lit: false },
        _ => Flip { icon: glyph::PENCIL, name: Shape::Draw.name(), lit: false },
    };
    match held.shape {
        Shape::Draw | Shape::Rect => Flip { lit: true, ..of(held.shape) },
        // Neither, so the square is the way *out* rather than a claim to be
        // what is in your hand. It shows where flipping would put you, and
        // now says the same thing.
        Shape::Capture | Shape::Stamp(_) => of(kinds()[held.kind].usually),
    }
}

/// How big a square can be, and how many rows the bar needs.
///
/// **Wraps before it shrinks**, which is the opposite of what it did. Shrinking
/// first was argued as "a shorter row of smaller squares reads better than two
/// rows of large ones, and a row costs height" — and the height is real, but
/// the arithmetic went straight past it: `available / squares` clamped at
/// [`SMALLEST`] meant a narrow window got **twenty-two point squares in one
/// row** rather than forty-four point ones in two. Twenty-two points is not a
/// touch target and the sprite in it is not a picture of anything.
///
/// So: the fewest rows that fit at full size, up to [`MOST_ROWS`]. Only past
/// that does anything shrink, and then it shrinks to fill those rows rather
/// than to fit one.
fn fit(squares: usize, available: f32, theme: &Theme) -> (f32, usize) {
    /// Below this a sprite is not a picture of anything, and a finger is not
    /// hitting one square rather than its neighbour.
    const SMALLEST: f32 = 22.0;
    /// **Three, because a bar four rows deep is a wall.** Past this the height
    /// costs more than the size does, and shrinking is the better trade — the
    /// thing that was wrong was reaching for it first.
    const MOST_ROWS: usize = 3;

    let m = theme.metrics;
    let step = |size: f32| size + m.item_spacing * 1.5;
    let fits = |size: f32, rows: usize| {
        let across = squares.div_ceil(rows.max(1)) as f32;
        across * step(size) <= available
    };

    for rows in 1..=MOST_ROWS {
        if fits(m.slot, rows) {
            return (m.slot, rows);
        }
    }
    // Still too wide at full size in three rows, so fill three and shrink to
    // whatever those hold.
    let across = squares.div_ceil(MOST_ROWS) as f32;
    let size = (available / across - m.item_spacing * 1.5).clamp(SMALLEST, m.slot);
    (size, MOST_ROWS)
}

pub fn show(
    ctx: &egui::Context,
    look: &Look<'_>,
    held: Held,
    library: &Library,
    status: &crate::client::views::game::hud::Status,
) -> crate::client::views::Shown<Option<Key>> {
    let theme = look.theme;
    let m = theme.metrics;
    let mut picked = None;

    let slots = slots(library, look.own_clock);
    // The figures at the left are the widest thing on the bar and the least
    // urgent — they are also what the HUD used to carry — so on a narrow
    // screen they go before anything you press does.
    let screen = ctx.content_rect().width();
    let figures = screen > 640.0;
    let room = screen - m.margin * 2.0 - if figures { 280.0 } else { 0.0 };
    let (size, rows) = fit(slots.len(), room, theme);
    let per_row = slots.len().div_ceil(rows.max(1));

    let area = egui::Area::new("hotbar".into())
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -m.margin])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = m.item_spacing * 0.5;
            // Left, like everything else: when the bar wraps to two rows the
            // second starts under the first rather than being centred against
            // it, so a square keeps its x when the window narrows.
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                for (row, line) in slots.chunks(per_row).enumerate() {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = m.item_spacing * 1.5;
                        if row == 0 && figures {
                            standing(ui, theme, status);
                        }
                        // One boxed segment per group, and a group never
                        // straddles two rows because the chunking is by group
                        // within a row.
                        for group in line.chunk_by(|a, b| a.group == b.group) {
                            segment(ui, theme, size, |ui| {
                                for slot in group {
                                    if slot.rule {
                                        rule(ui, theme, size);
                                    }
                                    let (mut turned, mut label) = (None, String::new());
                                    let (face, name, on) = face_of(
                                        slot.key,
                                        held,
                                        library,
                                        look,
                                        &mut turned,
                                        &mut label,
                                    );
                                    let key = hint(slot.press, look);
                                    let drawn = slot.press == Press::Space;
                                    let price = price_of(slot.key, look);
                                    if square(ui, look, size, face, name, key, drawn, price, on) {
                                        picked = Some(slot.key);
                                    }
                                }
                            });
                        }
                    });
                }
            });
        });

    crate::client::views::Shown::new(area.response.rect, picked)
}

/// A hairline between two squares in one segment: same group, different job.
fn rule(ui: &mut egui::Ui, theme: &Theme, size: f32) {
    // A full square tall so it takes part in the row like everything else, and
    // painted short so it reads as a divider.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, size), egui::Sense::hover());
    let short = rect.shrink2(egui::vec2(0.0, size * 0.15));
    ui.painter().rect_filled(short, 0.0, theme.palette.line);
}

/// Who you are and how you are doing, at the left end of the bar — where you
/// are looking already. Dropped on a narrow screen: widest and least urgent.
fn standing(ui: &mut egui::Ui, theme: &Theme, status: &crate::client::views::game::hud::Status) {
    let p = theme.palette;
    let m = theme.metrics;
    // Monospaced and zero-padded: these change every generation, and a
    // proportional digit is a different width from its neighbour, so the
    // figure grew and shrank and the label under it slid about.
    let stat = |ui: &mut egui::Ui, what: &str, value: u64, digits: usize, ink: egui::Color32| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let text = format!("{value:0>digits$}");
            ui.colored_label(ink, egui::RichText::new(text).monospace().size(m.text_body).strong());
            ui.colored_label(p.text_dim, egui::RichText::new(what).monospace().size(m.text_small));
        });
    };
    segment(ui, theme, m.slot, |ui| {
        let (r, g, b) = crate::client::views::hue::player_colour(status.player);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, m.slot), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(r, g, b));
        ui.spacing_mut().item_spacing.x = m.item_spacing * 1.5;

        // Six figures, which is what `Player::MAX_VALUE` allows and therefore
        // what the column has to be wide enough for.
        stat(ui, w().hotbar.purse, status.value.max(0) as u64, 6, p.accent);
        // **What you hold, not what you are scored on.** The two differ by
        // the patch everybody is granted, and the score leaves it out — so
        // this read nought for as long as somebody built only inside their own
        // ground, which a block does for ever.
        let ground: u32 =
            status.standing.iter().find(|h| h.who == status.player).map(|h| h.ground).unwrap_or(0);
        stat(ui, w().hotbar.ground, ground as u64, 6, p.text);
        stat(ui, w().hotbar.tick, status.generation, 6, p.text);
        // Silent when there is none: a client that has reached no server has
        // no rating rather than a starting figure, and a dash where a number
        // goes is one more thing to read.
        //
        // No provisional mark here, deliberately. That mark exists so a rating
        // read as a *claim* is not taken for one it is not — a leaderboard,
        // somebody else's profile — and this bar is your own readout of your
        // own number. It is on the home screen and on a profile.
        if let Some(r) = status.rating {
            let ink = match r.change {
                Some(c) if c > 0 => p.good,
                Some(c) if c < 0 => p.bad,
                _ => p.text,
            };
            stat(ui, w().hotbar.rating, r.number.max(0) as u64, 4, ink);
        }
    });
}

fn segment(ui: &mut egui::Ui, theme: &Theme, size: f32, contents: impl FnOnce(&mut egui::Ui)) {
    let p = theme.palette;
    let m = theme.metrics;
    egui::Frame::new()
        .fill(p.surface)
        .stroke(egui::Stroke::new(1.0, p.line))
        .corner_radius(m.rounding)
        .inner_margin(m.panel_padding * 0.6)
        .show(ui, |ui| {
            // Every segment is one square tall, whatever is in it, so two of
            // them side by side line up without either knowing what the other
            // holds.
            ui.set_min_height(size);
            ui.horizontal_top(|ui| {
                ui.set_min_height(size);
                contents(ui);
            });
        });
}

/// What fills the middle of a square.
enum Face<'a> {
    /// A cell, drawn from the sheet as the world would draw it.
    Sprite(Cell),
    /// A pattern, drawn as the cells it is.
    Pattern(&'a Stamp),
    /// A camera, for the square that takes one.
    Camera,
    /// Words, for the square that has no picture.
    /// An icon, from the face that has icons — see [`glyph`].
    Icon(&'a str),
}

/// One square: a picture of what it does, the key that picks it, and its name
/// on hover. A picture rather than a word, because what you are choosing is
/// what will be on the board.
///
/// Returns whether it was clicked. `key` is shown only while there is a key
/// left to show it with.
fn square(
    ui: &mut egui::Ui,
    look: &Look<'_>,
    size: f32,
    face: Face<'_>,
    name: &str,
    key: Option<String>,
    // Whether the key in the corner is a space bar, which has no spelling and
    // is drawn -- see `views::icons::space_bar`.
    space: bool,
    // What it costs to lay one, if it costs anything -- see `price_of`.
    price: Option<i32>,
    selected: bool,
) -> bool {
    let p = look.theme.palette;
    let m = look.theme.metrics;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());

    let fill = if selected {
        p.accent.gamma_multiply(0.22)
    } else if response.hovered() {
        p.surface_lift
    } else {
        p.surface
    };
    let edge = if selected { p.accent } else { p.line };
    let painter = ui.painter();
    painter.rect_filled(rect, m.rounding, fill);
    painter.rect_stroke(
        rect,
        m.rounding,
        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, edge),
        egui::StrokeKind::Inside,
    );

    // The picture sits inside the key's corner, so the two never overlap.
    let inner = rect.shrink(size * 0.18);
    let ink = if selected { p.text } else { p.text_dim };
    match face {
        Face::Sprite(cell) => match look.sheet {
            Some(sheet) => {
                painter.image(sheet, inner, Icons::uv(cell.sprite()), egui::Color32::WHITE);
            }
            None => draw_text(painter, inner, name, ink),
        },
        Face::Pattern(stamp) => stamp.draw(painter, inner, look.player, look.sheet),
        Face::Camera => icons::camera(painter, inner, ink),
        // Larger than a word, because a glyph drawn at a word's size in a
        // square meant for a sprite reads as a speck.
        Face::Icon(icon) => shadowed_in(
            painter,
            inner.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            inner.height() * 0.62,
            ink,
            egui::FontFamily::Name(glyph::FAMILY.into()),
        ),
    }

    // Brighter as well as bigger: a key hint at `text_dim` over a sprite was
    // the least legible thing on the screen, and it is the one piece of
    // writing here somebody is looking for rather than reading.
    let ink = if selected { p.accent } else { p.text };
    if space {
        // Sized to the writing it stands in for, and sat where its baseline
        // would be, so a bar and a `.` in the next square line up.
        let at = rect.left_top() + egui::vec2(4.0, 3.0);
        icons::space_bar(painter, egui::Rect::from_min_size(at, egui::vec2(15.0, 10.0)), ink);
    } else if let Some(key) = key {
        shadowed(
            painter,
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            &key,
            13.0,
            ink,
        );
    }

    // **The price, in the far corner from the key.** Two numbers on one square
    // want telling apart at a glance, and opposite corners does that with no
    // label on either: the one you press is top left and the one it costs is
    // bottom right. Dim, because it is a fact to check rather than a thing to
    // hunt for -- the key is what somebody is looking for.
    if let Some(price) = price {
        shadowed(
            painter,
            rect.right_bottom() + egui::vec2(-4.0, -2.0),
            egui::Align2::RIGHT_BOTTOM,
            &price.to_string(),
            11.0,
            p.text_dim,
        );
    }

    crate::client::views::hover_centred(response, name).clicked()
}

fn draw_text(painter: &egui::Painter, rect: egui::Rect, text: &str, colour: egui::Color32) {
    shadowed(painter, rect.center(), egui::Align2::CENTER_CENTER, text, 14.0, colour);
}

/// Text with something dark under it, so it reads over a sprite. A shadow
/// rather than a panel, which would cover the picture the square exists to
/// show, and offset rather than blurred, which is several draws.
fn shadowed(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: impl std::fmt::Display,
    size: f32,
    colour: egui::Color32,
) {
    shadowed_in(painter, at, align, text, size, colour, egui::FontFamily::Proportional)
}

/// The same, in a named family — which is how an icon is drawn, since an icon
/// is only ever asked of the font that has icons.
#[allow(clippy::too_many_arguments)]
fn shadowed_in(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: impl std::fmt::Display,
    size: f32,
    colour: egui::Color32,
    family: egui::FontFamily,
) {
    let font = egui::FontId::new(size, family);
    let text = text.to_string();
    painter.text(
        at + egui::vec2(1.0, 1.0),
        align,
        &text,
        font.clone(),
        egui::Color32::BLACK.gamma_multiply(0.75),
    );
    painter.text(at, align, text, font, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every kind gets a square.** The dynamite existed, burned its fuse and
    /// detonated for a whole commit with no way to put one down, because
    /// `kinds` is the bar and it was not in it. A kind that cannot be placed
    /// is a kind that is not in the game.
    #[test]
    fn every_kind_has_a_square_and_a_key() {
        let bar = slots(&Library::default(), true);
        for (i, tool) in kinds().iter().enumerate() {
            let slot = bar
                .iter()
                .find(|s| s.key == Key::Kind(i))
                .unwrap_or_else(|| panic!("{} has no square on the bar", tool.name));
            assert!(
                matches!(slot.press, Press::Shift(_)),
                "{} has a square and no key to reach it",
                tool.name
            );
        }
        assert!(
            kinds().iter().any(|t| t.placement == crate::net::Placement::Dynamite),
            "the dynamite is not among the kinds the bar offers"
        );
    }

    /// **The clock is there when it is yours**, and gone when it is not. A
    /// stopped board in a world the server is stepping would be a lie.
    #[test]
    fn the_clock_squares_are_there_when_the_clock_is_ours() {
        let ours = slots(&Library::default(), true);
        for key in [Key::Run, Key::Step, Key::Rules] {
            assert!(ours.iter().any(|s| s.key == key), "{key:?} is missing when time is ours");
        }
        let theirs = slots(&Library::default(), false);
        for key in [Key::Run, Key::Step, Key::Rules] {
            assert!(!theirs.iter().any(|s| s.key == key), "{key:?} is offered on a server's clock");
        }
    }

    /// **One square is lit at a time on each axis.** The shape square read as
    /// pressed unconditionally, so holding a stamp lit both it and the pencil
    /// and the bar claimed you were holding two things.
    #[test]
    fn holding_a_stamp_does_not_also_light_the_pencil() {
        let lit = |held: Held| flip_face(held).lit;
        assert!(lit(Held::default()), "a pencil in hand should light the shape square");
        assert!(
            !lit(Held { shape: Shape::Stamp(0), ..Default::default() }),
            "a stamp in hand lit the pencil as well"
        );
        assert!(
            !lit(Held { shape: Shape::Capture, ..Default::default() }),
            "a capture in hand lit the pencil as well"
        );
    }

    /// **A square you can hit beats a bar that is one line.**
    ///
    /// It shrank before it wrapped, and the arithmetic went straight past the
    /// middle: `available / squares` clamped at the floor gave twenty-two
    /// point squares in *one* row where two rows of full-size ones fit fine.
    /// Twenty-two points is not a touch target.
    #[test]
    fn the_bar_wraps_before_it_shrinks() {
        let theme = Theme::default();
        let full = theme.metrics.slot;
        let squares = slots(&library(3), true).len();
        let step = full + theme.metrics.item_spacing * 1.5;

        // Everything on one line, when there is room for it.
        assert_eq!(fit(squares, squares as f32 * step, &theme), (full, 1));

        // Exactly the room a given number of rows needs, computed rather than
        // guessed at as a fraction: the last row is short, so two rows need
        // `ceil(n/2)` across and not half of `n`.
        let room_for = |rows: usize| squares.div_ceil(rows) as f32 * step;
        for rows in [2, 3] {
            let (size, got) = fit(squares, room_for(rows), &theme);
            assert_eq!(size, full, "it shrank at {rows} rows when it could have wrapped");
            assert_eq!(got, rows, "wrong number of rows for exactly {rows} rows of room");
        }

        // One point short of what two rows need is three rows, still full
        // size — the next thing tried is another row, never a smaller square.
        let (size, rows) = fit(squares, room_for(2) - 1.0, &theme);
        assert_eq!((size, rows), (full, 3));
    }

    /// **And it does shrink eventually**, because a bar four rows deep is a
    /// wall. Past three the height costs more than the size does.
    #[test]
    fn a_window_too_narrow_for_three_rows_gets_smaller_squares() {
        let theme = Theme::default();
        let squares = slots(&library(3), true).len();
        let (size, rows) = fit(squares, 200.0, &theme);
        assert!(size < theme.metrics.slot, "it never shrank");
        assert!(size >= 22.0, "it shrank past what a sprite can be drawn in");
        assert_eq!(rows, 3, "it grew a fourth row instead of shrinking");
    }

    /// **Every material carries its price and nothing else does.**
    ///
    /// A shape is free, the clock is free, and a stamp's price is its cells
    /// times whatever it is being made of — a figure that moves when you press
    /// the *other* axis, which is the one thing a number on a button must not
    /// do.
    #[test]
    fn a_price_is_on_the_materials_and_on_nothing_else() {
        let theme = Theme::default();
        let library = library(3);
        let look = look(&theme, &library, false);

        for slot in slots(&library, true) {
            let price = price_of(slot.key, &look);
            match slot.key {
                Key::Kind(i) => {
                    assert_eq!(price, Some(kinds()[i].placement.cost()), "{:?}", slot.key);
                    assert!(price.unwrap() > 0, "a material that costs nothing: {:?}", slot.key);
                }
                other => assert_eq!(price, None, "{other:?} carries a price"),
            }
        }
    }

    /// **Nothing where placing is free**, which a laboratory can turn on: a
    /// price on a square that will not charge you is a number somebody could
    /// plan around that is not true.
    #[test]
    fn a_free_hand_shows_no_prices() {
        let theme = Theme::default();
        let library = library(0);
        let look = look(&theme, &library, true);
        for slot in slots(&library, true) {
            assert_eq!(price_of(slot.key, &look), None, "{:?} priced in a free room", slot.key);
        }
    }

    /// **The square never names one thing while drawing another.** It drew a
    /// pencil with a stamp in hand — correctly, since pressing it goes back to
    /// drawing — and called itself "pattern", which is what was in your hand
    /// rather than what the square does.
    #[test]
    fn the_shape_square_says_what_it_draws() {
        let says = |held: Held| {
            let flip = flip_face(held);
            (flip.icon, flip.name)
        };
        let pencil = (glyph::PENCIL, Shape::Draw.name());
        let pane = (glyph::RECT, Shape::Rect.name());

        assert_eq!(says(Held::default()), pencil);
        assert_eq!(says(Held { shape: Shape::Rect, ..Default::default() }), pane);

        // Holding a pattern or a capture: the square is the way back to
        // drawing, and both halves of it say so.
        for shape in [Shape::Stamp(0), Shape::Capture] {
            let flip = flip_face(Held { shape, ..Default::default() });
            assert_ne!(flip.name, Shape::Stamp(0).name(), "{shape:?} named what was held");
            assert_eq!((flip.icon, flip.name), pencil, "{shape:?}");
        }
    }

    /// The shape square draws a glyph, and only two of the four shapes have
    /// one — a capture is a drawn camera and a pattern is its own thumbnail.
    /// What reaches [`flip_face`] is a kind's usual shape, so that has to stay
    /// one of the two.
    #[test]
    fn a_tool_is_usually_drawn_or_panned() {
        for tool in kinds() {
            assert!(
                matches!(tool.usually, Shape::Draw | Shape::Rect),
                "{} is usually {:?}, which the shape square cannot draw",
                tool.name,
                tool.usually
            );
        }
    }

    /// **One list, so the keyboard and the layout cannot go out by one.**
    /// They were two, and this is what replaced them: every square the bar
    /// draws is a slot, and every key the bar answers is a slot's `press`.
    #[test]
    fn the_keyboard_reads_the_squares_that_are_drawn() {
        let lib = library(3);
        let bar = slots(&lib, true);

        for slot in &bar {
            if let Press::Shift(d) = slot.press {
                assert_eq!(shifted_for_digit(d, &lib), Some(slot.key), "shift and {d}");
            }
        }
        // Every digit is spoken for once: two squares on one key is a square
        // that cannot be reached.
        let mut pressed: Vec<Press> =
            bar.iter().map(|s| s.press).filter(|p| *p != Press::None).collect();
        let before = pressed.len();
        pressed.sort_by_key(|p| format!("{p:?}"));
        pressed.dedup();
        assert_eq!(pressed.len(), before, "two squares share a key");
    }

    /// **The clock is a laboratory's**, and its squares are the difference —
    /// running, stepping, the rules, and emptying it, which are the four
    /// things only a laboratory lets anybody do.
    #[test]
    fn a_bar_without_the_clock_has_none_of_it() {
        let lib = library(0);
        let ours = slots(&lib, true);
        let theirs = slots(&lib, false);
        assert_eq!(ours.len(), theirs.len() + 4);
        assert!(theirs.iter().all(|s| s.group != Group::Clock));
        for key in [Key::Run, Key::Step, Key::Rules, Key::Wipe] {
            assert!(ours.iter().any(|s| s.key == key), "{key:?} is missing");
        }
    }

    use crate::client::views::game::stamp::{Stamp, ON_THE_BAR};

    /// A `Look` with nothing in it that these tests read but the two flags
    /// they are about — the rest is a theme and a pair of keyboard callbacks.
    fn look<'a>(theme: &'a Theme, _library: &Library, free: bool) -> Look<'a> {
        Look {
            theme,
            what: crate::net::Placement::Life,
            sheet: None,
            player: PlayerId(1),
            typed: &|d| Some(d.to_string()),
            plain: &|d| Some(d.to_string()),
            own_clock: true,
            paused: false,
            showing_rules: false,
            free,
        }
    }

    fn library(n: usize) -> Library {
        let mut library = Library::default();
        for i in 0..n {
            library.keep(Stamp {
                name: format!("s{i}"),
                cells: vec![(0, 0)],
                size: (1, 1),
                on_bar: false,
            });
        }
        library
    }

    /// The bar reads left to right whatever is on it, and Ice is always the
    /// last thing — so the key that walls somebody off does not move when you
    /// capture a pattern.
    /// The kinds keep their keys however many patterns are captured, which is
    /// the whole reason the bar is split.
    #[test]
    fn the_kind_keys_never_move() {
        for n in [0, 1, ON_THE_BAR, ON_THE_BAR + 5] {
            let keys = shifted(&library(n));
            let expected: Vec<Key> = (0..kinds().len()).map(Key::Kind).collect();
            assert_eq!(&keys[..expected.len()], &expected[..], "{n} stamps");
        }
    }

    /// and they keep the order they sit in.
    #[test]
    fn shift_picks_the_kinds_in_the_order_they_sit() {
        let few = library(1);
        // Every kind, in the order it sits on the bar. Ice is among them now:
        // it used to sit apart because it came with a different stroke, and
        // the stroke is the other axis.
        for (i, tool) in kinds().iter().enumerate() {
            assert_eq!(
                shifted_for_digit(i as u32 + 1, &few),
                Some(Key::Kind(i)),
                "shift and {} should pick {}",
                i + 1,
                tool.name
            );
        }
        // Then the shape square, which used to be the one control on the bar
        // with a key outside this row -- so the bar read left to right and the
        // keyboard skipped a square in the middle of it. Capture and more
        // shuffled along to make room.
        let after = kinds().len() as u32;
        assert_eq!(shifted_for_digit(after + 1, &few), Some(Key::Flip));
        assert_eq!(shifted_for_digit(after + 2, &few), Some(Key::Capture));
        assert_eq!(shifted_for_digit(after + 3, &few), Some(Key::More));
        assert_eq!(shifted_for_digit(after + 4, &few), None);

        // And the row fits the digits it is named by, which is what the help
        // screen prints across: one keycap per square, none left over.
        assert!(
            shifted(&few).len() <= 10,
            "the shifted row has outgrown the digits, so some square has no key"
        );

        // And not one of them moves when a pattern is captured.
        let many = library(ON_THE_BAR + 1);
        assert_eq!(shifted(&many), shifted(&few));
    }

    /// **The two axes are independent, which is the whole of the change.**
    ///
    /// Every combination is reachable now rather than the dozen somebody
    /// happened to list: a pane of factories and a pencil of ice were both
    /// unsayable, because a tool carried its stroke with it.
    #[test]
    fn a_shape_and_a_kind_are_chosen_separately() {
        let ice = kinds().iter().position(|k| k.placement == Placement::Ice).unwrap();
        let drawn_ice = Held { shape: Shape::Draw, kind: ice, turn: Default::default() };
        assert_eq!(drawn_ice.stroke(), Stroke::Pencil, "a line of ice was unsayable");
        assert_eq!(drawn_ice.placement(), Some(Placement::Ice));

        let panes_of_factory = Held {
            shape: Shape::Rect,
            kind: kinds().iter().position(|k| k.placement == Placement::Factory).unwrap(),
            turn: Default::default(),
        };
        assert_eq!(
            panes_of_factory.stroke(),
            Stroke::Rectangle,
            "a pane of factories was unsayable"
        );
        assert_eq!(panes_of_factory.placement(), Some(Placement::Factory));

        // A stamp is a shape, so it keeps whatever it is being made of --
        // which is what "remove the stamps know what they are made of" means
        // at the point of placing one.
        let stamped = Held { shape: Shape::Stamp(0), kind: ice, turn: Default::default() };
        assert_eq!(stamped.placement(), Some(Placement::Ice), "a stamp lost the held kind");
        assert_eq!(stamped.stroke(), Stroke::Rectangle, "a drag with a stamp still captures");
    }

    /// Capturing is a rectangle, and it has a square of its own — there has to
    /// be a way to take the first stamp, and "drag with a stamp held" is not
    /// one when you have none.
    #[test]
    fn capturing_is_reachable_with_nothing_captured() {
        assert!(Held { shape: Shape::Capture, kind: 0, turn: Default::default() }.captures());
        assert!(
            Held { shape: Shape::Stamp(3), kind: 0, turn: Default::default() }.captures(),
            "a stamp takes the next one"
        );
        assert!(!Held { shape: Shape::Draw, kind: 0, turn: Default::default() }.captures());
        assert!(!Held { shape: Shape::Rect, kind: 0, turn: Default::default() }.captures());
        assert!(shifted(&library(0)).contains(&Key::Capture));
    }

    /// One square for two shapes, and it says which is current rather than
    /// offering both: a click gives the other one, and from a stamp gives
    /// drawing back.
    #[test]
    fn the_shape_square_offers_the_other_one() {
        assert_eq!(Shape::Draw.other(), Shape::Rect);
        assert_eq!(Shape::Rect.other(), Shape::Draw);
        // From anything on the shape axis that is not one of the two, the way
        // out is drawing -- the same place the key goes for a material that
        // draws, and never a dead end.
        assert_eq!(Shape::Stamp(3).other(), Shape::Draw);
        assert_eq!(Shape::Capture.other(), Shape::Draw);
    }

    /// **The one key lands in one place**, which is what makes it usable
    /// without looking: a toggle's meaning depends on what was pressed last,
    /// and this always puts the shape back to whatever the held material is
    /// usually wanted in.
    #[test]
    fn the_reset_key_goes_to_what_the_kind_is_usually_wanted_in() {
        let kind = |what: Placement| kinds().iter().position(|k| k.placement == what).unwrap();

        for (what, expected) in [
            (Placement::Life, Shape::Draw),
            (Placement::Factory, Shape::Draw),
            (Placement::Turret, Shape::Draw),
            (Placement::Ice, Shape::Rect),
        ] {
            let held = Held { shape: Shape::Capture, kind: kind(what), turn: Default::default() };
            assert_eq!(held.defaulted().shape, expected, "{what:?}");
            // Twice is the same place, which a toggle could not promise.
            assert_eq!(
                held.defaulted().defaulted().shape,
                expected,
                "{what:?} moved on a second press"
            );
            assert_eq!(held.defaulted().kind, held.kind, "resetting a shape changed the material");
        }

        // And it is the way out of a stamp, whatever is held.
        let stamped =
            Held { shape: Shape::Stamp(4), kind: kind(Placement::Ice), turn: Default::default() };
        assert_eq!(stamped.defaulted().shape, Shape::Rect);
    }
}
