//! Where the world is being looked at from, and how that moves.
//!
//! Split out of the game view because it is the one part of it that is pure
//! arithmetic: a position, a scale, and the mapping between the screen and the
//! world. That mapping was written out at each of its four call sites, which
//! is four places for the camera to be understood differently, and none of it
//! could be tested without a window to put it in.
//!
//! Knows nothing about what is being drawn or about what the pointer means.
//! The view decides that a middle drag pans; this decides what panning is.

use crate::render::chunks::CameraUniform;
use crate::sim::CHUNK_N;

/// Zoom is clamped to this.
///
/// **The floor came down when there was something to draw at it.** It was one
/// pixel per cell, because below that a point sample drops sparse cells and
/// they flicker out rather than shrinking — and because the fine path cannot
/// be *resident* down there anyway: one chunk is one texture array layer, the
/// guaranteed floor is 256 of them, and a 1080p screen wants thousands under
/// about zoom five. So zooming out further only ever bought more backdrop.
///
/// Both are answered now. `render::chunks::CoarseTexture` draws the world as
/// one texel a cell out of one quad, so a wrapping world **collapses into
/// itself repeating** rather than into empty ground; and the antialiasing
/// averages the cells a pixel covers rather than picking one of them.
///
/// **One pixel a cell is the floor**, because below it a cell is smaller than
/// the thing drawing it and no filter downstream can put that back.
///
/// It was a quarter — four cells to a pixel — and the reason given was that a
/// quarter "is where four samples a side stop covering the footprint exactly".
/// That was true of a `k`x`k` supersample in the world shader, and there has
/// not been one since antialiasing moved to a screen-space pass of its own.
/// The world is point-sampled once a pixel now, so at a quarter three of every
/// four cells are sampled by nothing at all and which three changes as the
/// camera moves — which is the shimmer, and it is the same fault as
/// [texels nothing samples](../../../../docs/planned.md) one level up.
///
/// A cap and not a fix. What is under it is unchanged, and getting further out
/// honestly wants the level of detail to keep going rather than the camera to
/// keep going without one.
pub const ZOOM_RANGE: (f32, f32) = (1.0, 64.0);

/// Seconds for a released pan to decay to a third of its speed. A flick
/// coasts roughly `speed * GLIDE` cells and stops. Zero turns it off.
const GLIDE: f32 = 0.15;

/// Below this, in cells per second, letting go is a stop rather than a flick.
const GLIDE_MIN: f32 = 3.0;

/// How much of a frame's measured speed carries into the glide. Smoothed,
/// because one short frame at the end of a drag reports a speed the hand never
/// had, and the glide would take it literally.
const SMOOTHING: f32 = 0.35;

pub struct Camera {
    /// What is at the middle of the screen, in cells, as (x, y).
    pub centre: (f32, f32),
    /// Screen pixels per cell.
    pub zoom: f32,
    /// Viewport size in physical pixels.
    pub viewport: (f32, f32),
    /// Physical pixels per point, which is the only thing standing between
    /// this and egui's coordinates.
    pub scale: f32,
    /// Set by anything that moves or scales the view. Zoom used to change the
    /// field without anything uploading it, so scrolling did nothing.
    pub dirty: bool,
    /// Cells dragged since the last frame, and the speed that came of it.
    /// Accumulated rather than measured in the pointer callback, which is
    /// given no time step — the same movement over two frames and over twenty
    /// is not the same flick.
    step: (f32, f32),
    velocity: (f32, f32),
}

impl Camera {
    pub fn new(centre: (f32, f32), zoom: f32) -> Self {
        Self {
            centre,
            zoom,
            viewport: (1.0, 1.0),
            scale: 1.0,
            dirty: true,
            step: (0.0, 0.0),
            velocity: (0.0, 0.0),
        }
    }

    /// The cell at the top-left of the screen, as (x, y). Every mapping
    /// between the screen and the world starts here.
    pub fn origin(&self) -> (f32, f32) {
        let (vw, vh) = self.viewport;
        (self.centre.0 - vw / (2.0 * self.zoom), self.centre.1 - vh / (2.0 * self.zoom))
    }

    /// Screen position to world position in cells, unrounded. Zoom anchoring
    /// needs the fraction, which the integer form throws away.
    pub fn cell_at_f(&self, (px, py): (f64, f64)) -> (f32, f32) {
        let origin = self.origin();
        (origin.0 + px as f32 / self.zoom, origin.1 + py as f32 / self.zoom)
    }

    /// Where a screen position lands in the world, as (row, col). The inverse
    /// of what the vertex shader does.
    pub fn cell_at(&self, at: (f64, f64)) -> (i32, i32) {
        let (x, y) = self.cell_at_f(at);
        (y.floor() as i32, x.floor() as i32)
    }

    /// A block of cells as a rectangle on screen, in points.
    ///
    /// Points, not pixels: egui works in points and this in physical pixels,
    /// and this is the one place the two meet. `to` is included, so a cell and
    /// itself is one cell wide.
    pub fn cell_rect(&self, from: (i32, i32), to: (i32, i32)) -> egui::Rect {
        let origin = self.origin();
        let point = |x: f32, y: f32| {
            egui::pos2(
                (x - origin.0) * self.zoom / self.scale,
                (y - origin.1) * self.zoom / self.scale,
            )
        };
        let (r0, r1) = (from.0.min(to.0) as f32, from.0.max(to.0) as f32 + 1.0);
        let (c0, c1) = (from.1.min(to.1) as f32, from.1.max(to.1) as f32 + 1.0);
        egui::Rect::from_min_max(point(c0, r0), point(c1, r1))
    }

    /// The region on screen, in absolute cells, as (min, max), with a margin
    /// of `margin` cells so life entering from off screen is already held.
    pub fn visible_cells(&self, margin: i32) -> ((i32, i32), (i32, i32)) {
        let (vw, vh) = self.viewport;
        let (ox, oy) = self.origin();
        let (w, h) = (vw / self.zoom, vh / self.zoom);
        (
            (oy.floor() as i32 - margin, ox.floor() as i32 - margin),
            ((oy + h).ceil() as i32 + margin, (ox + w).ceil() as i32 + margin),
        )
    }

    /// Scale about a screen position, keeping what is under it in place.
    /// Shared by the wheel, the trackpad and two fingers, so all three behave
    /// identically rather than each drifting its own way.
    pub fn zoom_about(&mut self, factor: f32, at: (f64, f64)) {
        let before = self.cell_at_f(at);
        self.zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let after = self.cell_at_f(at);
        self.centre.0 += before.0 - after.0;
        self.centre.1 += before.1 - after.1;
        self.dirty = true;
    }

    /// Move by a pointer movement in pixels.
    ///
    /// The world follows the pointer, so the camera goes the other way, and
    /// the drag is in pixels while the camera lives in cells.
    pub fn pan_by_pixels(&mut self, dx: f64, dy: f64) {
        let step = (dx as f32 / self.zoom, dy as f32 / self.zoom);
        self.centre.0 -= step.0;
        self.centre.1 -= step.1;
        self.step.0 -= step.0;
        self.step.1 -= step.1;
        self.dirty = true;
    }

    /// Move by a number of cells, at a speed that is the same on screen
    /// whatever the zoom.
    pub fn nudge(&mut self, x: f32, y: f32, cells_per_second: f32, dt: f32) {
        let step = cells_per_second * dt / self.zoom;
        self.centre.0 += x * step;
        self.centre.1 += y * step;
        self.dirty = true;
        // A key and a glide pulling at once would be two answers to where the
        // view is going.
        self.velocity = (0.0, 0.0);
    }

    /// Forget any coasting. A press, a key or a scroll is aiming at something,
    /// and a view still sliding would take the target away.
    pub fn halt(&mut self) {
        self.velocity = (0.0, 0.0);
    }

    pub fn begin_drag(&mut self) {
        self.velocity = (0.0, 0.0);
        self.step = (0.0, 0.0);
    }

    /// Let go, and let it coast if it was still moving.
    pub fn end_drag(&mut self) {
        if self.velocity.0.hypot(self.velocity.1) < GLIDE_MIN {
            self.velocity = (0.0, 0.0);
        }
    }

    /// Called once a frame: measure the drag in progress, or carry a released
    /// one on. `dragging` is whether the pointer is still moving the view.
    pub fn advance(&mut self, dt: f32, dragging: bool) {
        if dragging {
            self.measure(dt);
        } else {
            self.step = (0.0, 0.0);
            self.glide(dt);
        }
    }

    fn measure(&mut self, dt: f32) {
        let (dx, dy) = std::mem::take(&mut self.step);
        if dt <= 0.0 {
            return;
        }
        let (vx, vy) = self.velocity;
        self.velocity = (vx + (dx / dt - vx) * SMOOTHING, vy + (dy / dt - vy) * SMOOTHING);
    }

    fn glide(&mut self, dt: f32) {
        let (vx, vy) = self.velocity;
        if vx == 0.0 && vy == 0.0 {
            return;
        }
        self.centre.0 += vx * dt;
        self.centre.1 += vy * dt;
        self.dirty = true;

        let decay = if GLIDE > 0.0 { (-dt / GLIDE).exp() } else { 0.0 };
        self.velocity = (vx * decay, vy * decay);
        // Stop rather than approach zero forever, or the view never settles
        // and every frame rewrites the uniform and resyncs the instance list.
        if self.velocity.0.hypot(self.velocity.1) < 0.5 {
            self.velocity = (0.0, 0.0);
        }
    }

    /// What the shader needs to draw this view.
    ///
    /// `encode_srgb` is the surface's business rather than the camera's — the
    /// camera is arithmetic and has never known there is a GPU — so it is a
    /// parameter rather than a field. It rides here because the camera
    /// uniform is the only thing bound to the fragment stage that has room
    /// for it. `hues` rides here for the same reason and is nobody's business
    /// either: it is who is on whose team, which is the client's.
    /// `over` is how many times larger than the screen the world is being
    /// drawn — see [`crate::render::context::Offscreen::SUPERSAMPLE`].
    ///
    /// **Both the viewport and the zoom scale by it, and that is why nothing
    /// else has to.** Clip space is `position * zoom / viewport`, so doubling
    /// the two leaves every vertex exactly where it was and only the number of
    /// samples under it changes. The camera is still described in screen
    /// pixels everywhere else, which is what the pointer and the hover box and
    /// the visible-cell arithmetic all want.
    pub fn uniform(
        &self,
        encode_srgb: bool,
        hues: &[f32; crate::sim::PlayerId::COUNT],
        coarse: ((i32, i32), (i32, i32)),
        coarse_wraps: bool,
        over: f32,
    ) -> CameraUniform {
        let (ox, oy) = self.origin();
        let ((row, col), (rows, cols)) = coarse;
        CameraUniform {
            origin: [ox, oy],
            viewport: [self.viewport.0 * over, self.viewport.1 * over],
            zoom: self.zoom * over,
            chunk_n: CHUNK_N as f32,
            encode_srgb: if encode_srgb { 1.0 } else { 0.0 },
            coarse_wraps: if coarse_wraps { 1.0 } else { 0.0 },
            coarse: [col as f32, row as f32, cols as f32, rows as f32],
            // Four to a row, which is what the shader indexes and what a
            // uniform array's stride costs if it is not.
            hues: std::array::from_fn(|row| std::array::from_fn(|col| hues[row * 4 + col])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        let mut c = Camera::new((100.0, 50.0), 16.0);
        c.viewport = (800.0, 600.0);
        c
    }

    /// The screen-to-world mapping and the world-to-screen one are inverses,
    /// which is the property every gesture depends on and none of them could
    /// check: a click resolves through one and its preview is drawn with the
    /// other, so a disagreement puts the mark somewhere the click did not go.
    #[test]
    fn a_cell_maps_to_the_screen_and_back() {
        let c = camera();
        for at in [(0.0, 0.0), (400.0, 300.0), (799.0, 599.0), (37.0, 512.0)] {
            let cell = c.cell_at(at);
            let rect = c.cell_rect(cell, cell);
            // The rectangle drawn for that cell must contain the point that
            // chose it. Points, so undo the scale the rectangle is in.
            let point = egui::pos2(at.0 as f32 / c.scale, at.1 as f32 / c.scale);
            assert!(rect.contains(point), "{at:?} chose cell {cell:?}, drawn at {rect:?}");
        }
    }

    /// Zooming about a point keeps what is under it in place. That is what
    /// makes the wheel, the trackpad and a pinch feel like one gesture.
    #[test]
    fn zooming_holds_the_point_it_is_anchored_on() {
        let mut c = camera();
        let at = (620.0, 140.0);
        let before = c.cell_at_f(at);
        c.zoom_about(2.0, at);
        let after = c.cell_at_f(at);
        assert!((before.0 - after.0).abs() < 1e-3, "{before:?} vs {after:?}");
        assert!((before.1 - after.1).abs() < 1e-3, "{before:?} vs {after:?}");
    }

    /// Zoom clamps, and a clamped zoom must not shift the view sideways --
    /// the anchoring arithmetic runs either way, so a scale that did not
    /// change must produce a centre that did not either.
    #[test]
    fn zooming_past_the_limit_does_not_slide_the_view() {
        let mut c = camera();
        c.zoom = ZOOM_RANGE.1;
        let centre = c.centre;
        c.zoom_about(4.0, (10.0, 10.0));
        assert_eq!(c.zoom, ZOOM_RANGE.1);
        assert_eq!(c.centre, centre);
    }

    /// Panning pulls the world with the pointer, so the camera goes the other
    /// way -- and by a distance in cells, not in pixels.
    #[test]
    fn panning_moves_the_camera_against_the_pointer() {
        let mut c = camera();
        c.pan_by_pixels(32.0, -16.0);
        assert_eq!(c.centre, (100.0 - 2.0, 50.0 + 1.0));
    }

    /// A flick coasts; a slow release stops dead.
    #[test]
    fn a_flick_coasts_and_a_crawl_does_not() {
        let mut c = camera();
        c.begin_drag();
        c.pan_by_pixels(320.0, 0.0); // 20 cells in one frame
        c.advance(1.0 / 60.0, true);
        c.end_drag();
        let moved_by_coasting = {
            let before = c.centre;
            c.advance(1.0 / 60.0, false);
            (c.centre.0 - before.0).abs()
        };
        assert!(moved_by_coasting > 0.0, "a flick should carry on");

        let mut c = camera();
        c.begin_drag();
        c.pan_by_pixels(1.0, 0.0);
        c.advance(1.0 / 60.0, true);
        c.end_drag();
        let before = c.centre;
        c.advance(1.0 / 60.0, false);
        assert_eq!(c.centre, before, "a crawl should stop where it was let go");
    }
}
