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
}

/// A detonation, part way through going off.
pub struct Fireball {
    /// Where it went off and how far it reaches, both already in screen
    /// points: the camera arithmetic belongs to whoever owns the camera.
    pub at: egui::Pos2,
    pub reach: f32,
    /// How far through its life, nought to one.
    pub age: f32,
}

/// How long a fireball lasts, in seconds.
///
/// **Longer than the generation it belongs to**, and that is the point. At the
/// default rate a generation is a quarter of a second, so an effect that lived
/// exactly as long as the event would be under the threshold at which anybody
/// notices *what* happened as against that something did. Short enough that
/// two blasts in a row are two things.
pub const FIREBALL: f32 = 0.75;

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
    if marks.hover.is_none() && marks.selection.is_none() && marks.blasts.is_empty() {
        return;
    }
    let p = theme.palette;
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("world-marks")));

    // Under the pointer's marks, because what you are about to do matters more
    // than what just happened.
    for blast in &marks.blasts {
        fireball(&painter, blast);
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

/// **A fireball**, drawn as a mesh rather than as circles.
///
/// A fire is a *gradient* — white at the core, orange at the middle, red and
/// then nothing at the edge — and egui has no radial fill. A fan does it
/// exactly: one bright vertex at the centre and a ring of transparent ones
/// around it, with the colour interpolated between. Concentric circles were
/// the other way and they band, which at this size reads as a target rather
/// than as a flame.
///
/// Three things move over its life and they move at different rates, which is
/// most of what makes it read as fire rather than as a shape being animated:
///
/// - **It expands fast and then stops.** `sqrt` of the age, so most of the
///   growth is in the first third. Something that grew steadily would read as
///   a circle being resized.
/// - **It cools.** White through yellow and orange to a deep red, which is
///   what a fire does and what an eye already knows how to read as "this is
///   ending".
/// - **It thins.** The fade is late and sharp — `(1 - age)` squared — so it
///   burns at full strength and then goes, rather than being half transparent
///   for half its life.
fn fireball(painter: &egui::Painter, blast: &Fireball) {
    let age = blast.age.clamp(0.0, 1.0);
    // Fast then settling, so the first frames are the explosion and the rest
    // is it burning out at roughly the size it reached.
    let radius = blast.reach * (0.35 + 0.65 * age.sqrt());
    if radius < 1.0 {
        return;
    }

    // White-hot, cooling. Held in the middle a while rather than swept evenly,
    // because a fire is orange for most of its life and only briefly white.
    let core = match age {
        a if a < 0.15 => lerp(WHITE_HOT, YELLOW, a / 0.15),
        a if a < 0.5 => lerp(YELLOW, ORANGE, (a - 0.15) / 0.35),
        a => lerp(ORANGE, EMBER, (a - 0.5) / 0.5),
    };
    // Late and sharp: it burns and then goes.
    let strength = (1.0 - age).powi(2);

    let mut mesh = egui::Mesh::default();
    let middle =
        egui::Color32::from_rgba_unmultiplied(core.0, core.1, core.2, (235.0 * strength) as u8);
    // The rim is the same colour with nothing left of it, so the falloff is a
    // fade rather than an edge.
    let rim = egui::Color32::from_rgba_unmultiplied(core.0, core.1, core.2, 0);
    mesh.colored_vertex(blast.at, middle);
    // Enough sides that the rim is a circle at the size a blast is drawn --
    // reach is six cells, so this is tens of pixels rather than hundreds.
    const SIDES: usize = 28;
    for n in 0..=SIDES {
        let turn = n as f32 / SIDES as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(blast.at + egui::vec2(turn.cos(), turn.sin()) * radius, rim);
    }
    for n in 1..=SIDES {
        mesh.add_triangle(0, n as u32, n as u32 + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The four temperatures a fireball passes through, as plain sRGB.
///
/// Not from the theme: a fire is not a piece of interface and does not take
/// the palette's accent. These are what a flame is, and the player's own
/// colour is already what the *ground* it turned over will be drawn in.
type Heat = (u8, u8, u8);
const WHITE_HOT: Heat = (255, 250, 224);
const YELLOW: Heat = (255, 214, 102);
const ORANGE: Heat = (240, 130, 44);
const EMBER: Heat = (150, 44, 30);

fn lerp(from: Heat, to: Heat, t: f32) -> Heat {
    let t = t.clamp(0.0, 1.0);
    let one = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    (one(from.0, to.0), one(from.1, to.1), one(from.2, to.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(age: f32) -> Fireball {
        Fireball { at: egui::pos2(100.0, 100.0), reach: 40.0, age }
    }

    /// **It expands fast and then settles**, so the first frames are the
    /// explosion and the rest is it burning out at roughly the size it
    /// reached. Something that grew steadily would read as a circle being
    /// resized.
    #[test]
    fn a_fireball_does_most_of_its_growing_early() {
        let radius = |age: f32| 40.0 * (0.35 + 0.65 * age.sqrt());
        let (start, third, end) = (radius(0.0), radius(0.33), radius(1.0));
        assert!(third > start + (end - start) * 0.5, "it grew evenly: {start} {third} {end}");
        assert!(end <= 40.0, "it grew past the ground it turned over");
    }

    /// **It cools**, white through yellow and orange to a deep red, which is
    /// what an eye already reads as something ending.
    #[test]
    fn a_fireball_cools_as_it_goes() {
        let heat = |age: f32| match age {
            a if a < 0.15 => lerp(WHITE_HOT, YELLOW, a / 0.15),
            a if a < 0.5 => lerp(YELLOW, ORANGE, (a - 0.15) / 0.35),
            a => lerp(ORANGE, EMBER, (a - 0.5) / 0.5),
        };
        let (new, old) = (heat(0.0), heat(1.0));
        assert_eq!(new, WHITE_HOT);
        assert_eq!(old, EMBER);
        // Redder and darker all the way, which is what cooling is.
        let mut last = heat(0.0);
        for n in 1..=20 {
            let now = heat(n as f32 / 20.0);
            assert!(now.1 <= last.1, "green went up at {n}: {last:?} then {now:?}");
            assert!(now.2 <= last.2, "blue went up at {n}");
            last = now;
        }
    }

    /// **It burns and then goes**, rather than being half transparent for half
    /// its life.
    #[test]
    fn a_fireball_fades_late() {
        let strength = |age: f32| (1.0f32 - age).powi(2);
        assert!(strength(0.5) < 0.5, "it was still half there at halfway");
        assert!(strength(0.25) > 0.5, "it faded before it had burnt");
        assert_eq!(strength(1.0), 0.0);
    }

    /// A fireball smaller than a point is not drawn, which is what a blast far
    /// enough away to be one pixel is.
    #[test]
    fn a_fireball_too_small_to_see_is_not_drawn() {
        let ctx = egui::Context::default();
        let painter =
            ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("test")));
        // No panic and nothing added: the guard is a `return`, and what this
        // holds is that it is taken before any arithmetic on a zero radius.
        fireball(&painter, &Fireball { at: egui::pos2(0.0, 0.0), reach: 0.5, age: 0.0 });
    }
}
