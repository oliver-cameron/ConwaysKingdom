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

/// What the match in this room is doing, once the server has said.
///
/// A struct rather than a tuple, which it outgrew the moment it carried more
/// than three things — and every one of them is read by name at the far end.

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
pub(crate) fn digit(code: winit::keyboard::KeyCode) -> Option<u32> {
    use winit::keyboard::KeyCode as K;
    Some(match code {
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
