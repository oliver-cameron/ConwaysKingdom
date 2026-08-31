//! A press, a drag, a pinch — and the arithmetic that says which is which.
//!
//! **Already testable without a window**, which is why it is a file of its
//! own: nothing here touches the GPU, egui or a socket. It takes positions and
//! returns cells and spans, and the tests at the bottom of the game view have
//! always run on a machine with no display.
//!
//! One gesture at a time by construction, which is the whole design. Drawing
//! and panning used to be two independent flags, so a press could be both at
//! once and releasing either ended neither cleanly.

use super::*;

/// One thing at a time by construction. Drawing and panning were two
/// independent flags, so a press could be both at once and the release of
/// either ended neither cleanly.
pub(crate) enum Gesture {
    None,
    /// The left button, or one finger, over the world: a click if it never
    /// travels, a rectangle to fill if it does.
    Drawing(Drag),
    /// The view follows the pointer. `button` is what has to come up again to
    /// end it, and is `None` for fingers.
    Panning {
        button: Option<winit::event::MouseButton>,
    },
}

/// A press that may yet become a drag.
pub(crate) struct Drag {
    /// Where the press landed, in cells, as (row, col).
    pub(crate) from: (i32, i32),
    /// And in pixels, which is what decides a drag from a click.
    pub(crate) from_px: (f64, f64),
    pub(crate) moved: bool,
    /// What this drag lays, taken from the slot held when it began. Fixed at
    /// the press rather than read each frame, so changing slot mid-stroke does
    /// not change the shape of a line already half drawn.
    pub(crate) stroke: hotbar::Stroke,
    /// Every cell the pointer has crossed, in order. A pencil only.
    pub(crate) path: Vec<(i32, i32)>,
    /// The same cells as a set. A stroke that crosses itself would otherwise
    /// list a cell twice, and the pricing compares each entry against the
    /// world rather than against the entries before it — so the crossing
    /// would be charged for twice and paid for once.
    pub(crate) seen: std::collections::HashSet<(i32, i32)>,
}

impl Drag {
    pub(crate) fn begin(px: (f64, f64), cell: (i32, i32), stroke: hotbar::Stroke) -> Self {
        let mut drag = Self {
            from: cell,
            from_px: px,
            moved: false,
            stroke,
            path: Vec::new(),
            seen: std::collections::HashSet::new(),
        };
        if stroke == hotbar::Stroke::Pencil {
            // The press marks where it landed whatever part of the cell it hit:
            // you aimed at it. Everything after has to pass through a middle.
            drag.mark(cell);
        }
        drag
    }

    /// Note where the press has got to. `slop` is in the same physical pixels
    /// the positions are.
    pub(crate) fn reached(&mut self, px: (f64, f64), slop: f64) {
        self.moved |= travelled(self.from_px, px, slop);
    }

    pub(crate) fn mark(&mut self, cell: (i32, i32)) {
        if self.seen.insert(cell) {
            self.path.push(cell);
        }
    }

    /// Whether the stroke has reached its limit and stopped growing.
    pub(crate) fn full(&self) -> bool {
        self.path.len() as i64 >= MAX_DRAG_CELLS
    }

    /// How many cells this drag covers, without listing them. More than one is
    /// what makes it a drag rather than a click, and that has to be decided
    /// before the cells are priced -- otherwise a click that lands somewhere
    /// it may not build is refused in a drag's words.
    pub(crate) fn cell_count(&self, to: (i32, i32)) -> i64 {
        match self.stroke {
            hotbar::Stroke::Pencil => self.path.len() as i64,
            hotbar::Stroke::Rectangle => {
                let (rows, cols) = span(self.from, to);
                rows * cols
            }
        }
    }
}

/// How much of a cell counts as being on it, across the middle.
///
/// The rest is a gap, and **the gap is the point**. Filling in every cell the
/// pointer passes over draws a solid line, and a solid line is not what you
/// want to draw: the patterns worth placing have holes in them. A glider is
/// five cells with gaps between them, and it should be one motion of the hand
/// rather than five clicks.
///
/// So a cell counts only if the pointer went through the middle of it, and
/// this is how much of the middle. What that buys is that a stroke passing
/// diagonally between two cells does not catch the two beside the corner —
/// which is what makes a diagonal a diagonal, and a glider a glider.
///
/// **Measured against drawing a glider in one motion**, with a hand that
/// wobbles a quarter of a cell and cuts its corners:
///
/// ```text
///   0.35    2% land it exactly, 2.4 of the five cells missed
///   0.55   57%                  0.4 missed
///   0.70   96%                  none missed, and none extra
///   0.80   64%                  none missed, 0.35 cells extra
/// ```
///
/// Below 0.7 the misses are the problem: you have to pass nearer the centre
/// of every cell than a hand reliably can, and the shape comes out with holes
/// in the wrong places. Above it the extras are, because the band grows wide
/// enough to catch the cells beside a corner and the gaps close up.
///
/// A 45° stroke is one cell thick and unbroken at every value in that range,
/// so it is not what this number is for. Angled strokes **do** break here, and
/// that is wanted rather than tolerated — an unbroken angled line is a thing
/// you can draw with two strokes, and a shape with holes is not.
///
/// Not every pattern is one motion. A lightweight spaceship has nine cells,
/// one of them not touching the other eight and three of them with a single
/// neighbour, so no tolerance makes it a single stroke: a stroke is a path,
/// and a path has two ends.
pub(crate) const CELL_COLLIDER: f32 = 0.7;

/// The cell a world position is on, if it is far enough inside one to count.
///
/// Fractional cell coordinates in, so it is the same arithmetic at every zoom
/// and can be tested without a camera to point at anything.
pub(crate) fn cell_under((x, y): (f32, f32)) -> Option<(i32, i32)> {
    let edge = (1.0 - CELL_COLLIDER) / 2.0;
    let inside = |v: f32| {
        let fraction = v - v.floor();
        fraction >= edge && fraction <= 1.0 - edge
    };
    (inside(x) && inside(y)).then(|| (y.floor() as i32, x.floor() as i32))
}

/// Whether a press that landed at `from` and has reached `to` is a drag.
///
/// Measured from where the press landed, not between one pointer event and the
/// next. That was the bug: a slow, deliberate sweep arrives as a stream of
/// one-pixel moves, no single one of them clears any threshold worth setting,
/// and the whole gesture collapsed into a click at the release point — so a
/// dragged pane came out as a single cell.
pub(crate) fn travelled(from: (f64, f64), to: (f64, f64), slop: f64) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    dx * dx + dy * dy > slop * slop
}

/// How many rows and columns a rectangle covers, both ends included.
///
/// In `i64` because a drag at one pixel per cell can span most of an `i32`,
/// and the product of two of those still has to be a number the cap can be
/// compared against rather than an overflow.
pub(crate) fn span(from: (i32, i32), to: (i32, i32)) -> (i64, i64) {
    ((from.0 as i64 - to.0 as i64).abs() + 1, (from.1 as i64 - to.1 as i64).abs() + 1)
}

/// The middle of however many fingers are down. One finger's middle is itself,
/// which is what lets a pinch that has lost a finger carry on panning.
pub(crate) fn centroid(touches: &[(u64, (f64, f64))]) -> (f64, f64) {
    let n = touches.len().max(1) as f64;
    let sum = touches.iter().fold((0.0, 0.0), |a, t| (a.0 + t.1 .0, a.1 + t.1 .1));
    (sum.0 / n, sum.1 / n)
}

/// The gap between exactly two fingers. One has no span to measure, and three
/// or more is not a pinch anybody means.
pub(crate) fn pinch_span(touches: &[(u64, (f64, f64))]) -> Option<f64> {
    let [(_, a), (_, b)] = touches else { return None };
    Some(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
}

/// The digit a key stands for, if it is one.
///
/// Positional, because the digit row is a **shape**: its meaning is the order
/// the keys sit in, and on a French keyboard the top row types `&é"'(-è_ç`
/// unshifted while still being the same ten keys in the same order.
///
/// `0` is here because the tenth stamp is picked with it — [`stamp_for_digit`]
/// maps it and the hotbar draws "0" in that square's corner. It was missing,
/// so the tenth stamp had a label naming a key that did nothing.
///
/// [`stamp_for_digit`]: super::hotbar::stamp_for_digit
pub(crate) fn digit(code: winit::keyboard::KeyCode) -> Option<u32> {
    use winit::keyboard::KeyCode as K;
    Some(match code {
        K::Digit0 | K::Numpad0 => 0,
        K::Digit1 | K::Numpad1 => 1,
        K::Digit2 | K::Numpad2 => 2,
        K::Digit3 | K::Numpad3 => 3,
        K::Digit4 | K::Numpad4 => 4,
        K::Digit5 | K::Numpad5 => 5,
        K::Digit6 | K::Numpad6 => 6,
        K::Digit7 | K::Numpad7 => 7,
        K::Digit8 | K::Numpad8 => 8,
        K::Digit9 | K::Numpad9 => 9,
        _ => return None,
    })
}

/// A key bound to what it **says** rather than to where it sits.
///
/// Four of the client's keys are mnemonics: `R` for rotate, `F` for flip, `?`
/// for the help screen and `` ` `` for the shape reset. Their meaning is the
/// character, not the place — the help screen prints `?` and the hotbar square
/// prints `` ` ``, so a positional binding would make both labels lie on any
/// layout that puts those characters elsewhere. On Dvorak the `R` *position*
/// types `p`, which would hide rotate under a key nothing mentions and leave
/// `r` inert.
///
/// **The shape reset is `` ` `` and not `~`.** They are the same key on a US
/// keyboard and the backtick is the unshifted half of it, so it is one press
/// rather than two — and, which matters more, `~` is a **dead key** on the
/// Spanish, Portuguese and Nordic layouts: it produces no text at all on its
/// own, waiting for a vowel to put a tilde over, so a client bound to the
/// character never saw it. A backtick is a plain character everywhere. `~` is
/// still accepted, because it is the same key and somebody who learnt the old
/// label should not find it dead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mnemonic {
    Help,
    Turn,
    Mirror,
    /// Run, or stop running.
    ///
    /// **Space**, which is what a pause key is everywhere and what Golly's
    /// nearest equivalent is. It was the drag-to-pan modifier and that is the
    /// weaker claim on it: panning has a middle drag, the arrows and the walk
    /// cluster, and a pause has nowhere else obvious to be. Return was tried
    /// first and `help`'s own test refused it, since return already takes what
    /// a list has picked.
    Play,
    /// One generation, and stay stopped.
    StepOne,
}

/// Whether this key types a Latin letter — which is the question that decides
/// whether a mnemonic can be bound by character at all.
fn types_a_letter(typed: Option<&str>) -> bool {
    typed.is_some_and(|t| t.chars().any(|c| c.is_ascii_alphabetic()))
}

/// What a press means, if it means one of the four.
///
/// **Character first, and the US position as a fallback**, which is the half
/// that was missing. Binding purely by character is right on every layout that
/// can *type* the character and leaves the key unreachable on every layout
/// that cannot: on a Cyrillic, Greek, Hebrew or Thai keyboard the `R` key
/// types `к`, so rotate, flip and the help screen naming them had no key at
/// all — and `~` is worse than that, because it is a dead key on the Spanish,
/// Portuguese and Nordic layouts and so produces no text even where the
/// alphabet is Latin.
///
/// The fallback is narrow on purpose. `R` and `F` fall back to their positions
/// **only when that key types something that is not a Latin letter**, so
/// Dvorak — where the `R` position types a perfectly good `p` — keeps the
/// character binding and nothing is hidden under a key the help screen does
/// not name. `?` and `~` fall back unconditionally, because their US positions
/// are bound to nothing else and cannot collide.
///
/// **Direct-input layouts only.** A keyboard driven through an input method —
/// Pinyin, Japanese, Korean — composes text over several presses and hands it
/// over as a finished string, so `to_text` during composition says nothing
/// this could read and the game's single-key bindings are not a thing that
/// layout has. That is a separate problem and deliberately not attempted; see
/// docs/gotchas.md.
pub(crate) fn mnemonic(
    code: winit::keyboard::KeyCode,
    typed: Option<&str>,
    shift: bool,
) -> Option<Mnemonic> {
    use winit::keyboard::KeyCode as K;
    let says = |what: &str| typed.is_some_and(|t| t.eq_ignore_ascii_case(what));
    // A letter this keyboard cannot produce falls back to where it sits on the
    // one it is named after.
    let unreachable = !types_a_letter(typed);
    Some(match code {
        _ if says("?") => Mnemonic::Help,
        _ if says("r") => Mnemonic::Turn,
        _ if says("f") => Mnemonic::Mirror,
        _ if says(".") => Mnemonic::StepOne,
        K::Slash if shift => Mnemonic::Help,
        K::KeyR if unreachable => Mnemonic::Turn,
        K::KeyF if unreachable => Mnemonic::Mirror,
        // Positional, and there is no character to bind: the space bar prints
        // a space on every layout there is, so its position is never a
        // surprise and its label never has to be learned.
        K::Space => Mnemonic::Play,
        // A full stop falls back unconditionally: both positions are bound to
        // nothing else here and neither can collide.
        K::Period | K::NumpadDecimal => Mnemonic::StepOne,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode as K;

    /// Golly's two, and both are reachable on a keyboard this client cannot
    /// read a character from — return types nothing anywhere, and a layout
    /// whose full stop is somewhere else still has the key where `.` sits.
    #[test]
    fn the_clock_keys_are_reachable_on_any_layout() {
        assert_eq!(mnemonic(K::Space, Some(" "), false), Some(Mnemonic::Play));
        assert_eq!(mnemonic(K::Space, None, false), Some(Mnemonic::Play));
        assert_eq!(mnemonic(K::Period, Some("."), false), Some(Mnemonic::StepOne));
        // A full stop by character wherever it has moved to.
        assert_eq!(mnemonic(K::Semicolon, Some("."), false), Some(Mnemonic::StepOne));
        assert_eq!(mnemonic(K::Period, None, false), Some(Mnemonic::StepOne));
    }

    /// **Programmer Dvorak, where the digit row is shifted by default.** The
    /// letters sit where Dvorak puts them, so `r` and `f` are found by
    /// character; the top row types `&[{}()=*)+]!#` unshifted and the digits
    /// need shift, which the *bindings* do not care about because the row is
    /// bound by position — and which the help screen now says out loud,
    /// because it reads the labels off the keyboard rather than assuming.
    #[test]
    fn programmer_dvorak_finds_its_keys() {
        assert_eq!(mnemonic(K::KeyP, Some("r"), false), Some(Mnemonic::Turn));
        assert_eq!(mnemonic(K::KeyU, Some("f"), false), Some(Mnemonic::Mirror));
        // The digit row is positional, so it is untouched by any of this.
        assert_eq!(digit(K::Digit1), Some(1));
        assert_eq!(digit(K::Digit0), Some(0));
        // Where the backtick sits, that key types `$` — and nothing else in
        // the client wants it, so the position still resets the shape.
        // And so does whichever key does type a backtick.
    }

    /// The layout the game was written on, where character and position agree.
    #[test]
    fn a_us_keyboard_is_unchanged() {
        assert_eq!(mnemonic(K::KeyR, Some("r"), false), Some(Mnemonic::Turn));
        assert_eq!(mnemonic(K::KeyR, Some("R"), true), Some(Mnemonic::Turn));
        assert_eq!(mnemonic(K::KeyF, Some("f"), false), Some(Mnemonic::Mirror));
        assert_eq!(mnemonic(K::Slash, Some("?"), true), Some(Mnemonic::Help));
    }

    /// **Dvorak keeps the character binding**, which is what the fallback must
    /// not undo: the `R` position types `p` there, and rotate belongs under
    /// the key that types `r` rather than under one nothing tells you about.
    #[test]
    fn dvorak_binds_the_letter_and_not_the_place() {
        assert_eq!(mnemonic(K::KeyP, Some("r"), false), Some(Mnemonic::Turn), "r is r");
        assert_eq!(mnemonic(K::KeyR, Some("p"), false), None, "the R position types p there");
        assert_eq!(mnemonic(K::KeyU, Some("f"), false), Some(Mnemonic::Mirror));
        assert_eq!(mnemonic(K::KeyY, Some("f"), false), Some(Mnemonic::Mirror));
    }

    /// **And a keyboard with no Latin letters falls back to the place**, which
    /// is the case that had no key at all: on a Cyrillic layout `R` types `к`,
    /// so rotate, flip and the help screen naming them were unreachable.
    #[test]
    fn a_non_latin_keyboard_falls_back_to_where_the_key_sits() {
        assert_eq!(mnemonic(K::KeyR, Some("\u{43a}"), false), Some(Mnemonic::Turn));
        assert_eq!(mnemonic(K::KeyF, Some("\u{430}"), false), Some(Mnemonic::Mirror));
        // Greek and Hebrew are the same argument.
        assert_eq!(mnemonic(K::KeyR, Some("\u{3c1}"), false), Some(Mnemonic::Turn));
        assert_eq!(mnemonic(K::KeyF, Some("\u{5db}"), false), Some(Mnemonic::Mirror));
    }

    /// **A dead key types nothing**, which is what `~` is on the Spanish,
    /// Portuguese and Nordic layouts — so the shape control, bound to that
    /// character and then to the backtick beside it, was the least reachable
    /// key in the game on one of its most ordinary actions. It is a shifted
    /// digit now, with the rest of the bar, and this is what is left of that
    /// lesson: a binding by character is only as good as the character.
    #[test]
    fn a_key_that_is_somewhere_else_still_works_by_position() {
        // And where `?` is somewhere else entirely, the position still works.
        assert_eq!(mnemonic(K::Slash, None, true), Some(Mnemonic::Help));
        // Shift matters for `?`: the unshifted key is `/` and means nothing.
        assert_eq!(mnemonic(K::Slash, Some("/"), false), None);
    }

    /// Nothing else is one of the four. A key that means something is a key
    /// taken away from whatever else wanted it.
    #[test]
    fn an_ordinary_key_means_none_of_them() {
        for (code, typed) in
            // Space is play now, so it is no longer one of the ordinary keys.
            [(K::KeyA, "a"), (K::KeyW, "w"), (K::Digit1, "1"), (K::KeyG, "g")]
        {
            assert_eq!(mnemonic(code, Some(typed), false), None, "{typed}");
        }
    }

    /// The tenth stamp's square is drawn with a "0" in the corner, so the key
    /// has to reach it. It did not.
    #[test]
    fn zero_is_a_digit() {
        assert_eq!(digit(K::Digit0), Some(0));
        assert_eq!(digit(K::Numpad0), Some(0));
        assert_eq!(super::super::hotbar::stamp_for_digit(0), Some(9));
        // And the nine above it are unchanged.
        for n in 1..=9u32 {
            assert_eq!(super::super::hotbar::stamp_for_digit(n), Some(n as usize - 1));
        }
    }
}
