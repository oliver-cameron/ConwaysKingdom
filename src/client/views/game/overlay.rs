//! What the pointer is about to do, drawn over the world.
//!
//! Read-only, and given everything in screen points: the camera arithmetic
//! belongs to whoever owns the camera, and a view doing its own would be a
//! second place for it to be wrong. Same arrangement as [`super::hud`].
//!
//! Painted into `Order::Background`, which is above the world — the world is
//! drawn before egui in the same pass — and below every panel, so a rectangle
//! swept under the hotbar does not cover it.
//!
//! Nothing here is interactive. It is a layer painter rather than an `Area`
//! precisely so it cannot claim the pointer: an `Area` under the cursor would
//! register as a widget being dragged, and the world would stop receiving the
//! very gesture this exists to show.

use crate::client::views::theme::Theme;

/// Everything the overlay draws, assembled by the client each frame.
pub struct Marks {
    /// The cell under the pointer. Absent when the pointer is over a panel,
    /// when the view is being moved, or when cells are too small to point at
    /// one — a box around a two-pixel cell claims a precision the pointer
    /// does not have.
    pub hover: Option<egui::Rect>,
    /// The player's own colour, which is what every mark on the world is
    /// drawn in. The accent belongs to the panels; using it out here would
    /// say the interface owns the cell rather than the player about to take
    /// it, and two players would point at cells in the same colour.
    pub tint: egui::Color32,
    /// The rectangle a drag has swept so far.
    pub selection: Option<Selection>,
    /// **Fireballs**, in screen points, newest last — see [`Fireball`].
    pub blasts: Vec<Fireball>,
    /// **Where a generation happens twice**, one per player — see [`Halo`].
    pub halos: Vec<Halo>,
}

/// The edge of the ground an overclocker runs the rule twice over.
///
/// **Drawn because nothing else says it is there.** A disc runs at double rate
/// and the cells in it mostly do not look it: the shapes worth building are
/// period one or two, and stepping a period-two pattern twice lands it on the
/// phase it started from, so the fastest ground on the board reads as the
/// stillest. The edge is the one honest mark — it says where the rule changes,
/// which is the thing a player has to know and cannot infer.
///
/// **The edge of the union, per player, not a ring per machine.** Two
/// overclockers side by side make one patch of fast ground with one border;
/// drawing a circle round each would say there are two regions and draw a line
/// through the middle of ground that has no line in it.
///
/// Cells rather than a curve, for the reason [`Fireball`] gives: everything on
/// this board is a square on a grid.
pub struct Halo {
    /// One rectangle per cell on the border, already in screen points.
    pub tiles: Vec<egui::Rect>,
    /// Whose clock it is. A halo is drawn in its owner's colour and not in the
    /// viewer's, because whose ground runs fast is the whole of what it says.
    pub tint: egui::Color32,
}

/// How strongly a halo's edge is drawn, out of 255.
///
/// Faint on purpose: it is a boundary on ground that is still ground, and the
/// cells inside it are what somebody is looking at.
const HALO_ALPHA: u8 = 90;

/// A detonation, part way through burning out.
///
/// **Cells, not a shape.** Everything on this board is a square on a grid, and
/// a smooth circle drawn over it belongs to a different game — so a blast burns
/// as the tiles it turned over, each its own colour, aligned to the same grid
/// the cells are on.
pub struct Fireball {
    /// One rectangle per burning cell, already in screen points, with the heat
    /// of that particular tile. The camera arithmetic belongs to whoever owns
    /// the camera — same arrangement as the hover box.
    pub tiles: Vec<(egui::Rect, egui::Color32)>,
}

/// How many **generations** a blast burns for.
///
/// Generations rather than seconds, so the fire keeps time with the board
/// underneath it: a world running at half speed burns for twice as long in
/// wall clock and exactly as long in the only clock the game has. A blast is
/// a thing the simulation did, and an effect on a timer of its own would drift
/// away from it every time somebody moved the slider.
///
/// Six is long enough to be a fire rather than a flash — at the default rate
/// that is a second and a half — and short enough that two blasts in a row are
/// two things.
pub const BURNS_FOR: u64 = 6;

pub struct Selection {
    /// What would be laid: one rectangle for a pane, one per cell for a
    /// stroke. A stroke doubles back on itself, so there is no outline that
    /// describes it — the cells are the shape.
    pub cells: Vec<egui::Rect>,
    /// Everything the drag spans, which is where the label hangs from. For a
    /// pane it is the pane; for a stroke it is what the hand covered.
    pub bounds: egui::Rect,
    /// Whether to draw an edge round `bounds`. A pane has one; a line does
    /// not, and a box round a scribble says nothing true about it.
    pub outlined: bool,
    /// The player's own colour. Ice has no colour of its own — the shader
    /// tints all four cell states with the owner's hue and tells them apart by
    /// sprite — so the preview does the same and hatches instead.
    pub tint: egui::Color32,
    /// Whether what is being laid is a pane, which is drawn hatched. The flat
    /// stand-in for a texture: the same colour, a different surface.
    pub hatched: bool,
    /// Size and price, as `Ice 6x4 · 24 cells · −120`.
    pub label: String,
    /// Whether the drag would be allowed. A refused drag is drawn as refused
    /// *while the button is still down*, so the answer arrives before the
    /// commitment rather than after it — and a fill is all or nothing, so a
    /// refusal means no cells at all rather than as many as could be paid for.
    pub allowed: bool,
}

pub fn show(ctx: &egui::Context, theme: &Theme, marks: &Marks) {
    if marks.hover.is_none()
        && marks.selection.is_none()
        && marks.blasts.is_empty()
        && marks.halos.is_empty()
    {
        return;
    }
    let p = theme.palette;
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("world-marks")));

    // Under everything, and under the fire especially: a halo is a standing
    // fact about the ground and a blast is something that just happened to it.
    for halo in &marks.halos {
        let edge = halo.tint.gamma_multiply(HALO_ALPHA as f32 / 255.0);
        for tile in &halo.tiles {
            painter.rect_filled(*tile, 0.0, edge);
        }
    }

    // Under the pointer's marks, because what you are about to do matters more
    // than what just happened.
    for blast in &marks.blasts {
        for (tile, heat) in &blast.tiles {
            painter.rect_filled(*tile, 0.0, *heat);
        }
    }

    if let Some(rect) = marks.hover {
        painter.rect_filled(rect, 0.0, marks.tint.gamma_multiply(0.14));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, marks.tint.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
    }

    let Some(selection) = &marks.selection else { return };
    let edge = if selection.allowed { selection.tint } else { p.bad };
    // A stroke's cells are drawn solid enough to read as cells; a pane's one
    // rectangle is a wash, because what matters there is the extent.
    let wash = if selection.outlined { 0.16 } else { 0.45 };
    for cell in &selection.cells {
        painter.rect_filled(*cell, 0.0, edge.gamma_multiply(wash));
    }
    if selection.hatched {
        hatch(&painter, selection.bounds, edge.gamma_multiply(0.45));
    }
    if selection.outlined {
        painter.rect_stroke(
            selection.bounds,
            0.0,
            egui::Stroke::new(1.5, edge),
            egui::StrokeKind::Inside,
        );
    }
    chip(&painter, theme, selection);
}

/// Diagonals across a rectangle, clipped to it.
///
/// Drawn as a fixed number of lines rather than one every `SPACING` points, so
/// a rectangle swept across the whole screen costs the same as a small one:
/// at one pixel per cell a drag can cover a few thousand cells, and a line
/// per six points of that is a few hundred draw calls for a decoration.
fn hatch(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    const SPACING: f32 = 7.0;
    const MOST: usize = 96;

    let reach = rect.width() + rect.height();
    let step = (reach / MOST as f32).max(SPACING);
    let stroke = egui::Stroke::new(1.0, colour);
    let painter = painter.with_clip_rect(rect);

    let mut offset = 0.0;
    while offset < reach {
        painter.line_segment(
            [
                egui::pos2(rect.left() + offset, rect.top()),
                egui::pos2(rect.left(), rect.top() + offset),
            ],
            stroke,
        );
        offset += step;
    }
}

/// The size and price, in a small panel above the rectangle's top-left corner.
///
/// Above rather than inside: a rectangle can be one cell tall, and a label
/// inside one would be unreadable. Clamped to the screen, because a drag that
/// starts at the top of the window has nothing above it.
fn chip(painter: &egui::Painter, theme: &Theme, selection: &Selection) {
    let p = theme.palette;
    let m = theme.metrics;
    let colour = if selection.allowed { p.text } else { p.bad };
    let galley =
        painter.layout_no_wrap(selection.label.clone(), egui::FontId::proportional(11.0), colour);

    let padding = egui::vec2(6.0, 3.0);
    let size = galley.size() + padding * 2.0;
    let screen = painter.clip_rect();
    let wanted = egui::pos2(selection.bounds.left(), selection.bounds.top() - size.y - 4.0);
    let at = egui::pos2(
        wanted.x.clamp(screen.left() + 4.0, (screen.right() - size.x - 4.0).max(screen.left())),
        wanted.y.clamp(screen.top() + 4.0, (screen.bottom() - size.y - 4.0).max(screen.top())),
    );
    let rect = egui::Rect::from_min_size(at, size);

    painter.rect_filled(rect, m.rounding, p.surface);
    painter.rect_stroke(
        rect,
        m.rounding,
        egui::Stroke::new(1.0, if selection.allowed { p.line } else { p.bad }),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + padding, galley, colour);
}

/// **How hot one tile is**, at this age, this far from the middle.
///
/// `None` for a tile that has gone out, so a fire eats itself from the edge in
/// rather than fading evenly: the outside of a blast cools first, which is
/// what a fire does and what stops the whole disc winking out in one frame.
///
/// Sustained rather than swept. The heat holds for the first half and then
/// falls away, because a fire that starts dying immediately reads as a flash
/// and this one is meant to last a few generations.
///
/// `noise` is the cell's own number — see `crate::sim::Roll` — so a tile keeps
/// its character from generation to generation rather than shimmering, and two
/// clients drawing the same blast draw the same fire.
pub fn heat(age: f32, out: f32, noise: u64) -> Option<egui::Color32> {
    // Ragged, so the disc has no drawn edge: a tile a little further out than
    // its neighbour may still be burning and one nearer may not.
    let jitter = (noise % 32) as f32 / 32.0 * 0.28;
    let gone = (out + jitter - 0.15).clamp(0.0, 1.0);
    if age >= 1.0 - gone * 0.75 {
        return None;
    }

    // Held, then falling. The centre stays hot longer than the rim.
    let left = ((1.0 - age) - gone * 0.55).clamp(0.0, 1.0);
    let hot = (left * 1.4).clamp(0.0, 1.0);

    // Red at the edges and through the cooling, orange and pale in the middle
    // while it is young. Two colours and a blend, because a fire drawn in more
    // than that at this size is a rainbow.
    let (r, g, b) = if hot > 0.55 {
        blend(ORANGE, PALE, (hot - 0.55) / 0.45)
    } else {
        blend(RED, ORANGE, hot / 0.55)
    };

    // Translucent throughout, so the ground it turned over reads through it —
    // the disc is *why* the fire is there and covering it would hide the thing
    // being announced.
    let alpha = (40.0 + 150.0 * left) * (0.75 + (noise % 8) as f32 / 32.0);
    Some(egui::Color32::from_rgba_unmultiplied(r, g, b, alpha.min(200.0) as u8))
}

/// The three temperatures a tile passes through, as plain sRGB.
///
/// Not from the theme: a fire is not a piece of interface and does not take the
/// palette's accent. The player's own colour is already what the ground it
/// turned over is drawn in.
type Heat = (u8, u8, u8);
const PALE: Heat = (255, 226, 150);
const ORANGE: Heat = (233, 126, 38);
const RED: Heat = (168, 38, 24);

fn blend(from: Heat, to: Heat, t: f32) -> Heat {
    let t = t.clamp(0.0, 1.0);
    let one = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    (one(from.0, to.0), one(from.1, to.1), one(from.2, to.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **It is sustained**, not a flash: still burning in the middle most of
    /// the way through, which is the difference between a fire and a frame of
    /// noise.
    #[test]
    fn the_middle_burns_for_most_of_it() {
        assert!(heat(0.0, 0.0, 0).is_some());
        assert!(heat(0.5, 0.0, 0).is_some(), "out by halfway");
        assert!(heat(0.8, 0.0, 0).is_some(), "out at four fifths");
        assert!(heat(1.0, 0.0, 0).is_none(), "still burning when it should be over");
    }

    /// **It eats itself from the edge in.** The outside cools first, so the
    /// disc shrinks rather than the whole of it winking out together.
    #[test]
    fn the_edge_goes_out_before_the_middle() {
        let half_way = 0.5;
        assert!(heat(half_way, 0.0, 0).is_some(), "the middle went first");
        assert!(heat(half_way, 1.0, 0).is_none(), "the rim was still burning at halfway");
    }

    /// **It cools towards red**, which is what an eye already reads as
    /// something ending, and it never brightens.
    #[test]
    fn a_tile_only_ever_cools() {
        let redness = |age: f32| {
            let c = heat(age, 0.0, 0).expect("out too early");
            (c.g() as i32, c.b() as i32)
        };
        let mut last = redness(0.0);
        for n in 1..=10 {
            let now = redness(n as f32 / 12.0);
            assert!(now.0 <= last.0, "green rose at {n}: {last:?} then {now:?}");
            assert!(now.1 <= last.1, "blue rose at {n}");
            last = now;
        }
    }

    /// **Translucent throughout**, because the ground it turned over is the
    /// thing being announced and covering it would hide it.
    #[test]
    fn a_tile_never_hides_the_ground_under_it() {
        for age in [0.0, 0.25, 0.5, 0.75] {
            for out in [0.0, 0.5, 1.0] {
                if let Some(c) = heat(age, out, 3) {
                    assert!(c.a() <= 200, "a tile at {age}/{out} was nearly solid");
                }
            }
        }
    }

    /// The same cell burns the same on every client, because the character
    /// comes from the cell's own number rather than from a frame.
    #[test]
    fn one_cell_burns_the_same_way_twice() {
        assert_eq!(heat(0.3, 0.4, 12345), heat(0.3, 0.4, 12345));
        assert_ne!(heat(0.3, 0.4, 1), heat(0.3, 0.4, 20), "every tile is the same fire");
    }
}
