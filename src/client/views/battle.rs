//! The game view: one world, a camera over it, and the input that drives both.
//!
//! A view rather than the application, so a menu or a lobby can be another one
//! beside it without this having to know they exist.

use std::cell::RefCell;

use crate::render::app::App;
use super::{hotbar, hud, overlay, Views};
use crate::render::atlas::Atlas;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, CHUNK_N};

use crate::net::link::Link;
use crate::net::{Action, ClientMessage, Placement, ServerMessage, Stamped};
use crate::sim::{Player, PlayerId};

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

/// Where the camera starts looking, in cells, as (x, y) — that is, (col, row).
const START_CENTRE: (f32, f32) = (CHUNK_N as f32 / 2.0, CHUNK_N as f32 / 2.0);

/// Screen pixels per cell at startup.
const START_ZOOM: f32 = 16.0;

/// Zoom is clamped to this. Never below 1: point sampling drops sparse cells
/// under one pixel per cell, so they would flicker out rather than shrink.
const ZOOM_RANGE: (f32, f32) = (1.0, 64.0);

/// Cells per second when panning with the keyboard, at one pixel per cell.
/// Divided by the zoom, so a keypress moves the same distance on screen
/// whatever the zoom is.
const PAN_SPEED: f32 = 600.0;

/// A press that travels further than this many points from where it landed is
/// a drag rather than a click.
///
/// Points, not physical pixels: a hand shakes by a distance on the glass, not
/// by a number of pixels, so on a display at twice the density the same shake
/// covers twice as many of them.
const DRAG_SLOP: f64 = 4.0;

/// Multiplies the keyboard pan while shift is held.
const PAN_FAST: f32 = 3.0;

/// Seconds for a released pan to decay to a third of its speed. A flick
/// coasts roughly `speed * PAN_GLIDE` cells and stops. Zero turns it off.
const PAN_GLIDE: f32 = 0.15;

/// Below this, in cells per second, letting go is a stop rather than a flick.
const PAN_GLIDE_MIN: f32 = 3.0;

/// How much of a frame's measured speed carries into the glide. Smoothed,
/// because one short frame at the end of a drag reports a speed the hand never
/// had, and the glide would take it literally.
const PAN_SMOOTHING: f32 = 0.35;

/// The hover box is not drawn below this many pixels per cell. A box around a
/// two-pixel cell claims a precision the pointer does not have.
const HOVER_MIN_ZOOM: f32 = 4.0;

/// The most cells one drag may cover.
///
/// A drag at one pixel per cell can sweep millions, and every one of them
/// would be listed, priced, applied and put on the wire. The cap is what keeps
/// a careless sweep from stalling the client; the price is what keeps a
/// deliberate one honest.
const MAX_FILL_CELLS: i64 = 4096;

/// Cells of slack around the viewport when subscribing, so life entering from
/// off screen is already held rather than popping in a chunk late.
const VIEW_MARGIN: i32 = CHUNK_N as i32;

/// How many copies of a toroidal world to draw either side of the original, so
/// the tiling can be seen tiling. Ignored for infinite worlds.
const TORUS_REPEATS: i32 = 1;

/// Which world the app opens.
const WORLD: WorldMode = WorldMode::Infinite;

/// A toroidal world's size, as (chunks high, chunks wide).
const TORUS_CHUNKS: (i32, i32) = (16, 16);

#[derive(Clone, Copy, PartialEq, Eq)]
// `WORLD` is a const, so whichever arm is not selected reads as dead.
#[allow(dead_code)]
pub enum WorldMode {
    Infinite,
    Torus,
}

/// Set before the event loop starts, because `App::init` takes no arguments
/// of its own. A one-shot rather than a config store: it is read once.
#[cfg(not(target_arch = "wasm32"))]
static CONNECTION: std::sync::Mutex<Option<(Option<String>, String)>> =
    std::sync::Mutex::new(None);

/// Point the client at a server before launching it. `None` runs offline.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_connection(url: Option<String>, name: String) {
    *CONNECTION.lock().unwrap() = Some((url, name));
}

/// What the pointer is doing.
///
/// One thing at a time by construction. Drawing and panning were two
/// independent flags, so a press could be both at once and the release of
/// either ended neither cleanly.
#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    None,
    /// The left button, or one finger, over the world: a click if it never
    /// travels, a rectangle to fill if it does.
    Drawing(Drag),
    /// The view follows the pointer. `button` is what has to come up again to
    /// end it, and is `None` for fingers.
    Panning { button: Option<winit::event::MouseButton> },
}

/// A press that may yet become a drag.
#[derive(Clone, Copy, PartialEq)]
struct Drag {
    /// Where the press landed, in cells, as (row, col).
    from: (i32, i32),
    /// And in pixels, which is what decides a drag from a click.
    from_px: (f64, f64),
    moved: bool,
}

impl Drag {
    fn begin(px: (f64, f64), cell: (i32, i32)) -> Self {
        Self { from: cell, from_px: px, moved: false }
    }

    /// Note where the press has got to. `slop` is in the same physical pixels
    /// the positions are.
    fn reached(&mut self, px: (f64, f64), slop: f64) {
        self.moved |= travelled(self.from_px, px, slop);
    }
}

/// Whether a press that landed at `from` and has reached `to` is a drag.
///
/// Measured from where the press landed, not between one pointer event and the
/// next. That was the bug: a slow, deliberate sweep arrives as a stream of
/// one-pixel moves, no single one of them clears any threshold worth setting,
/// and the whole gesture collapsed into a click at the release point — so a
/// dragged pane came out as a single cell.
fn travelled(from: (f64, f64), to: (f64, f64), slop: f64) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    dx * dx + dy * dy > slop * slop
}

/// A finished gesture waiting to be resolved to cells. Input callbacks are not
/// handed the `GpuState`, and the screen-to-world mapping needs the viewport,
/// so it waits for the next `update` rather than guessing here.
struct Pending {
    drag: Drag,
    to_px: (f64, f64),
}

pub struct BattleApp {
    /// The interface, and the shapes it produced this frame.
    ///
    /// Behind a cell because the overlay is recorded while the frame holds an
    /// immutable borrow of the app: `draw_calls` returns references into it,
    /// so the pass cannot also take `&mut`.
    views: RefCell<Views>,
    ui_output: RefCell<Option<super::Output>>,
    pipeline: wgpu::RenderPipeline,
    bind_groups: Vec<wgpu::BindGroup>,
    vertex_buffers: Vec<wgpu::Buffer>,
    camera_buffer: wgpu::Buffer,
    chunks: ChunkStore,
    /// Held for as long as the bind group refers to it.
    _atlas: Atlas,
    world: World,
    /// Set by anything that moves or scales the camera. Zoom used to change
    /// the field without anything uploading it, so scrolling did nothing.
    camera_dirty: bool,
    /// Camera centre, in cells, as (x, y).
    camera: (f32, f32),
    /// Screen pixels per cell.
    zoom: f32,
    /// What the pointer is doing.
    gesture: Gesture,
    /// Space held, which turns the left button into a pan. The convention in
    /// every drawing tool, and the only mouse pan available on a trackpad with
    /// no middle button.
    space: bool,
    /// Shift held, which hurries the keyboard pan.
    shift: bool,
    /// Held pan keys: left, right, up, down.
    pan: [bool; 4],
    /// Cells the view has been dragged since the last frame. Accumulated here
    /// rather than measured in the pointer callback, which is given no time
    /// step — the same movement over two frames and over twenty is not the
    /// same flick.
    pan_step: (f32, f32),
    /// Cells per second the view keeps once a pan is let go.
    pan_velocity: (f32, f32),
    /// Fingers currently down, as (id, position).
    touches: Vec<(u64, (f64, f64))>,
    /// How many were down last time they were looked at. A finger arriving or
    /// leaving moves their centre without the hand moving, so the count is
    /// what says whether a jump in it was a gesture or an arithmetic artefact.
    touch_count: usize,
    /// Distance between two fingers last frame, to measure a pinch by.
    pinch_span: Option<f64>,
    /// Where the fingers' centre was last frame, to measure a two-finger pan
    /// by. Also the point a pinch scales about.
    view_anchor: Option<(f64, f64)>,
    /// Set once a touch has had a second finger join it, and held until every
    /// finger lifts. Without it, lifting one finger out of a pinch turns the
    /// other into a drawing gesture and paints a line across the world.
    touch_view: bool,
    /// Whether a pointer is hovering over the world at all. A finger is not
    /// hovering — there is nothing under it once it lifts — so the hover box
    /// would otherwise be left behind wherever the last touch ended.
    hovering: bool,
    /// Viewport size in physical pixels, refreshed every frame from the
    /// `GpuState`. Cached only because input callbacks are not handed one --
    /// updating it solely on resize left it stale whenever a resize event did
    /// not arrive, and zoom anchoring then disagreed with the camera about how
    /// big the screen was.
    viewport: (f32, f32),
    /// Physical pixels per point, refreshed beside the viewport. Cached for
    /// the same reason: input callbacks are handed no `GpuState`.
    scale: f32,
    /// Last reported cursor position, in physical pixels.
    cursor: (f64, f64),
    /// Our own player number, once the server has issued one.
    me: Option<crate::sim::PlayerId>,
    /// Chunks already asked for, so a moving viewport only asks for what is new.
    subscribed: std::collections::HashSet<crate::sim::Coord>,
    /// The server connection, if there is one. A client with no link still
    /// simulates: the rules are deterministic, so offline is a game of one
    /// rather than a broken game.
    link: Option<Link>,
    /// Seconds since the client started. The interface animates against this,
    /// so it must be real time -- the generation counter only moves four times
    /// a second and would make every hover and fade crawl.
    elapsed: f64,
    /// Why the last action was refused, shown until the next one succeeds.
    notice: Option<String>,
    /// What the last click did, kept so the interface can show it. Without
    /// this a click that lands on empty ground is indistinguishable from a
    /// click that never arrived.
    last_action: Option<String>,
    /// What this player can spend. Predicted locally with the same arithmetic
    /// the server charges by, so the number on screen is the number the server
    /// will agree with.
    value: i32,
    /// A finished gesture waiting to be resolved to cells.
    pending: Option<Pending>,
    /// Which hotbar slot is selected.
    slot: usize,
}

impl BattleApp {
    /// A fixed camera. Autoscrolling is gone: the view no longer chases the
    /// live pattern, so what is on screen is whatever `VIEW_CENTRE` and
    /// `VIEW_ZOOM` say. Panning and zooming will be driven by input.
    fn write_camera(&self, gpu: &GpuState) {
        let (vw, vh) = self.viewport;
        let (ox, oy) = self.origin();

        gpu.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                origin: [ox, oy],
                viewport: [vw, vh],
                zoom: self.zoom,
                chunk_n: CHUNK_N as f32,
                _pad: [0.0; 2],
            }),
        );
    }

    /// The cell at the top-left of the screen, as (x, y). Every mapping
    /// between the screen and the world starts here, and it used to be written
    /// out at each of them — four copies of two lines is four places for the
    /// camera to be understood differently.
    fn origin(&self) -> (f32, f32) {
        let (vw, vh) = self.viewport;
        (
            self.camera.0 - vw / (2.0 * self.zoom),
            self.camera.1 - vh / (2.0 * self.zoom),
        )
    }

    /// Screen position to world position in cells, unrounded. Zoom anchoring
    /// needs the fraction, which the integer form throws away.
    fn cell_under_cursor_f(&self, (px, py): (f64, f64)) -> (f32, f32) {
        let origin = self.origin();
        (
            origin.0 + px as f32 / self.zoom,
            origin.1 + py as f32 / self.zoom,
        )
    }

    /// Where a screen position lands in the world, in absolute cell
    /// coordinates. The inverse of what the vertex shader does.
    fn cell_under_cursor(&self, at: (f64, f64)) -> (i32, i32) {
        let (x, y) = self.cell_under_cursor_f(at);
        (y.floor() as i32, x.floor() as i32) // (row, col)
    }

    /// A block of cells as a rectangle on screen, in points.
    ///
    /// Points, not pixels: egui works in points and the camera in physical
    /// pixels, and this is the one place the two meet. `to` is included, so a
    /// cell and itself is one cell wide.
    fn cell_rect(&self, scale: f32, from: (i32, i32), to: (i32, i32)) -> egui::Rect {
        let origin = self.origin();
        let point = |x: f32, y: f32| {
            egui::pos2(
                (x - origin.0) * self.zoom / scale,
                (y - origin.1) * self.zoom / scale,
            )
        };
        let (r0, r1) = (from.0.min(to.0) as f32, from.0.max(to.0) as f32 + 1.0);
        let (c0, c1) = (from.1.min(to.1) as f32, from.1.max(to.1) as f32 + 1.0);
        egui::Rect::from_min_max(point(c0, r0), point(c1, r1))
    }

    /// Who we are. Before the server has said, we are player one — offline is
    /// a game of one rather than a game of nobody.
    fn player(&self) -> PlayerId {
        self.me.unwrap_or(PlayerId(1))
    }
}

impl BattleApp {
    /// Scale the zoom about a screen position, keeping what is under it in
    /// place. Shared by the wheel, the trackpad and two fingers, so all three
    /// behave identically rather than each drifting its own way.
    fn zoom_about(&mut self, factor: f32, at: (f64, f64)) {
        let before = self.cell_under_cursor_f(at);
        self.zoom = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let after = self.cell_under_cursor_f(at);
        self.camera.0 += before.0 - after.0;
        self.camera.1 += before.1 - after.1;
        self.camera_dirty = true;
    }

    fn zoom_about_cursor(&mut self, factor: f32) {
        self.zoom_about(factor, self.cursor);
    }

    /// Move the view by a pointer movement in pixels.
    ///
    /// The world follows the pointer, so the camera goes the other way, and
    /// the drag is in pixels while the camera lives in cells.
    fn pan_by_pixels(&mut self, dx: f64, dy: f64) {
        let step = (dx as f32 / self.zoom, dy as f32 / self.zoom);
        self.camera.0 -= step.0;
        self.camera.1 -= step.1;
        self.pan_step.0 -= step.0;
        self.pan_step.1 -= step.1;
        self.camera_dirty = true;
    }

    fn begin_pan(&mut self, button: Option<winit::event::MouseButton>) {
        self.gesture = Gesture::Panning { button };
        self.pan_velocity = (0.0, 0.0);
        self.pan_step = (0.0, 0.0);
    }

    /// Let go, and let it coast if it was still moving.
    fn end_pan(&mut self) {
        self.gesture = Gesture::None;
        if self.pan_velocity.0.hypot(self.pan_velocity.1) < PAN_GLIDE_MIN {
            self.pan_velocity = (0.0, 0.0);
        }
    }

    /// The drag threshold in the physical pixels positions are reported in.
    fn slop(&self) -> f64 {
        DRAG_SLOP * self.scale.max(1.0) as f64
    }

    /// Whether a gesture the world owns is in progress.
    fn gesture_active(&self) -> bool {
        self.gesture != Gesture::None || self.touch_view
    }

    fn is_panning(&self) -> bool {
        matches!(self.gesture, Gesture::Panning { .. }) || self.touch_view
    }

    /// Drop whatever the pointer was doing, without acting on it.
    ///
    /// Escape, and anything that takes the focus away: the release that would
    /// have ended the gesture goes to whoever took the focus, not to us, and a
    /// gesture nothing ends leaves the view stuck to a pointer that is
    /// somewhere else entirely.
    fn cancel_gesture(&mut self) {
        self.gesture = Gesture::None;
        self.pending = None;
        self.pan = [false; 4];
        self.space = false;
        self.shift = false;
        self.touches.clear();
        self.touch_count = 0;
        self.touch_view = false;
        self.pinch_span = None;
        self.view_anchor = None;
        self.pan_velocity = (0.0, 0.0);
    }

    /// Move the camera for whatever pan keys are held, then either measure the
    /// pan in progress or carry a released one on.
    fn apply_pan(&mut self, dt: f32) {
        let x = (self.pan[1] as i32 - self.pan[0] as i32) as f32;
        let y = (self.pan[3] as i32 - self.pan[2] as i32) as f32;
        if x != 0.0 || y != 0.0 {
            let step = PAN_SPEED * if self.shift { PAN_FAST } else { 1.0 } * dt / self.zoom;
            self.camera.0 += x * step;
            self.camera.1 += y * step;
            self.camera_dirty = true;
            // A key and a glide pulling at once would be two answers to where
            // the view is going.
            self.pan_velocity = (0.0, 0.0);
        }

        if self.is_panning() {
            self.measure_pan(dt);
        } else {
            self.pan_step = (0.0, 0.0);
            self.glide(dt);
        }
    }

    /// Turn this frame's dragging into a speed, smoothed. One short frame at
    /// the end of a drag reports a speed the hand never had, and an unsmoothed
    /// glide would take it literally.
    fn measure_pan(&mut self, dt: f32) {
        let (dx, dy) = std::mem::take(&mut self.pan_step);
        if dt <= 0.0 {
            return;
        }
        let (vx, vy) = self.pan_velocity;
        self.pan_velocity = (
            vx + (dx / dt - vx) * PAN_SMOOTHING,
            vy + (dy / dt - vy) * PAN_SMOOTHING,
        );
    }

    /// Carry a released pan on, decaying, so a flick covers ground without a
    /// second drag.
    fn glide(&mut self, dt: f32) {
        let (vx, vy) = self.pan_velocity;
        if vx == 0.0 && vy == 0.0 {
            return;
        }
        self.camera.0 += vx * dt;
        self.camera.1 += vy * dt;
        self.camera_dirty = true;

        let decay = if PAN_GLIDE > 0.0 { (-dt / PAN_GLIDE).exp() } else { 0.0 };
        self.pan_velocity = (vx * decay, vy * decay);
        // Stop rather than approach zero forever, or the camera never settles
        // and every frame rewrites the uniform and resyncs the instance list.
        if self.pan_velocity.0.hypot(self.pan_velocity.1) < 0.5 {
            self.pan_velocity = (0.0, 0.0);
        }
    }

    /// Move and scale the view to follow the fingers.
    ///
    /// One gesture rather than two: fingers that spread while they travel do
    /// both at once, which is what keeps the world under them. Zooming about
    /// where they were and then moving by how far they went is what puts the
    /// cell that was under them back under them.
    fn follow_touches(&mut self) {
        let focus = centroid(&self.touches);
        let span = pinch_span(&self.touches);

        // The centre of two fingers is nowhere near the centre of one, so a
        // finger arriving or leaving moves it without the hand moving. Take
        // the new centre as the anchor instead of panning by the jump.
        let settled = self.touch_count == self.touches.len();
        self.touch_count = self.touches.len();

        if settled {
            if let (Some(anchor), Some(before), Some(now)) =
                (self.view_anchor, self.pinch_span, span)
            {
                if before > 1.0 && now > 1.0 {
                    self.zoom_about((now / before) as f32, anchor);
                }
            }
            if let Some(anchor) = self.view_anchor {
                self.pan_by_pixels(focus.0 - anchor.0, focus.1 - anchor.1);
            }
        }

        self.pinch_span = span;
        self.view_anchor = Some(focus);
        self.cursor = focus;
    }

    /// Drain the socket and fold what arrived into the local world.
    fn pump_link(&mut self) {
        let Some(link) = &mut self.link else { return };
        let messages = link.drain();
        let closed = link.is_closed();

        for msg in messages {
            match msg {
                ServerMessage::Welcome { you, tick } => {
                    log::info!("joined as {you:?} at tick {tick}; adopting the server's world");
                    self.value = Player::STARTING_VALUE;
                    self.me = Some(you);
                    // Now, and only now, drop the local world. Until Welcome
                    // arrives there is nothing authoritative to replace it
                    // with, and an empty screen is worse than a local game.
                    self.world = World::infinite_empty();
                    // A birth's owner is seeded from the generation, so a
                    // client simulating at a different tick would make
                    // different choices from identical cells.
                    self.world.set_generation(tick);
                    self.subscribed.clear();
                }
                ServerMessage::Rejected { reason } => {
                    log::error!("server refused the connection: {reason}");
                    self.link = None;
                    return;
                }
                ServerMessage::ChunkData { tick, chunk, cells } => {
                    match bytemuck::try_from_bytes::<crate::sim::Chunk>(&cells) {
                        Ok(c) => {
                            self.world.set_generation(tick);
                            self.world.put_chunk(chunk, *c);
                        }
                        Err(e) => log::warn!("chunk {chunk:?} was the wrong size: {e}"),
                    }
                }
                ServerMessage::Actions(actions) => {
                    for stamped in &actions {
                        crate::net::apply(&mut self.world, stamped);
                    }
                }
                ServerMessage::Resync { tick, chunks } => {
                    log::warn!("desynced at tick {tick}; refetching {} chunks", chunks.len());
                    for c in chunks {
                        self.subscribed.remove(&c);
                    }
                }
            }
        }

        if closed {
            log::warn!("link closed; continuing offline");
            self.link = None;
            return;
        }

        self.subscribe_to_view();
    }

    /// Ask for any visible chunk not already requested. The camera is fixed for
    /// now, so this settles after the first frame; it is written against the
    /// viewport so panning needs no new code.
    /// Fill a rectangle with whatever the hotbar has selected.
    ///
    /// Always places, never takes: a drag across occupied ground is far more
    /// likely to be building over it than a request to clear it cell by cell,
    /// and an accidental sweep that wiped a structure would be unforgiving.
    /// Taking stays a deliberate single click.
    fn fill(&mut self, from: (i32, i32), to: (i32, i32)) {
        let (rows, cols) = span(from, to);
        let cells = match self.rectangle(from, to) {
            Ok(cells) => cells,
            Err(why) => {
                self.notice = Some(why);
                return;
            }
        };
        let count = cells.len();
        let (stamped, delta) = self.quote(cells);

        // All or nothing. A rectangle laid as far as the value stretched would
        // be a different shape from the one that was drawn, and the player
        // would be left working out where it stopped and why.
        if self.value + delta < 0 {
            self.notice = Some(format!(
                "{count} cells costs {}, you have {}",
                -delta, self.value
            ));
            return;
        }
        self.notice = None;
        self.value += delta;
        crate::net::apply(&mut self.world, &stamped);
        self.world.dirty = true;
        self.last_action = Some(format!(
            "filled {rows}x{cols} with {}, -{}",
            hotbar::SLOTS[self.slot].name,
            -delta
        ));

        if let Some(link) = &self.link {
            link.send(ClientMessage::Act(stamped));
        }
    }

    /// The cells a rectangle covers, or why it may not be laid at all.
    fn rectangle(&self, from: (i32, i32), to: (i32, i32)) -> Result<Vec<(i32, i32)>, String> {
        let (rows, cols) = span(from, to);
        let area = rows * cols;
        if area > MAX_FILL_CELLS {
            return Err(format!("{area} cells is more than one drag may lay"));
        }
        let (r0, r1) = (from.0.min(to.0), from.0.max(to.0));
        let (c0, c1) = (from.1.min(to.1), from.1.max(to.1));
        Ok((r0..=r1)
            .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
            .collect())
    }

    /// Price a paint of these cells: the action that would be sent, and what
    /// it would cost. Shared by the fill and by the preview of it, so the
    /// preview cannot promise something the release then refuses.
    fn quote(&self, cells: Vec<(i32, i32)>) -> (Stamped, i32) {
        let stamped = Stamped {
            tick: self.world.generation,
            player: self.player(),
            action: Action::Paint {
                cells,
                placement: hotbar::SLOTS[self.slot].placement,
            },
        };
        let delta = crate::net::value_delta(&self.world, &stamped);
        (stamped, delta)
    }

    /// The box around the cell the pointer is on.
    ///
    /// Absent while the view is moving, while a rectangle is being swept —
    /// the rectangle is the answer then, and a box around its far corner as
    /// well is noise — and when the cells are too small to point at one.
    fn hover_mark(&self, scale: f32, on_ui: bool) -> Option<egui::Rect> {
        if on_ui || !self.hovering || self.is_panning() || self.zoom < HOVER_MIN_ZOOM {
            return None;
        }
        if matches!(self.gesture, Gesture::Drawing(drag) if drag.moved) {
            return None;
        }
        let at = self.cell_under_cursor(self.cursor);
        Some(self.cell_rect(scale, at, at))
    }

    /// The rectangle a drag has swept so far, with its size and its price.
    fn selection_mark(&self, scale: f32) -> Option<overlay::Selection> {
        let Gesture::Drawing(drag) = self.gesture else { return None };
        if !drag.moved {
            return None;
        }
        let to = self.cell_under_cursor(self.cursor);
        let (rows, cols) = span(drag.from, to);
        let slot = &hotbar::SLOTS[self.slot];

        let (label, allowed) = match self.rectangle(drag.from, to) {
            Err(why) => (why, false),
            Ok(cells) => {
                let (_, delta) = self.quote(cells);
                if self.value + delta < 0 {
                    (
                        format!(
                            "{} {rows}x{cols}   costs {}, you have {}",
                            slot.name, -delta, self.value
                        ),
                        false,
                    )
                } else {
                    (format!("{} {rows}x{cols}   {delta}", slot.name), true)
                }
            }
        };

        let (r, g, b) = hud::player_colour(self.player());
        Some(overlay::Selection {
            rect: self.cell_rect(scale, drag.from, to),
            tint: egui::Color32::from_rgb(r, g, b),
            hatched: slot.placement == Placement::Ice,
            label,
            allowed,
        })
    }

    /// One button does everything, and the cell under it decides which.
    ///
    /// Something living there means take it — your own for value, someone
    /// else's at a cost. Empty ground means put down whatever the hotbar has
    /// selected. There is nothing to hold and nothing to remember, which is
    /// what a clicker opening needs.
    ///
    /// Applied locally *and* sent, rather than sent and awaited: the rules are
    /// deterministic and the server runs the same `net::apply` and charges by
    /// the same `net::value_delta`, so acting immediately shows the right
    /// answer a round trip early. If the server disagrees the chunk digests
    /// will not match and the resync puts it right.
    fn click(&mut self, row: i32, col: i32) {
        let player = self.player();
        let cells = vec![(row, col)];
        let occupied = self
            .world
            .cell_at(row, col)
            .is_some_and(|c| c.is_alive() || c.is_ice());
        let action = if occupied {
            Action::Erase { cells }
        } else {
            Action::Paint { cells, placement: hotbar::SLOTS[self.slot].placement }
        };
        let stamped = Stamped { tick: self.world.generation, player, action };

        // Priced against the world as it stands, before the action changes it,
        // and refused here on the same terms the server would refuse it. Doing
        // it locally means the refusal is instant rather than a round trip
        // away, and the two cannot disagree because it is the same function.
        let delta = crate::net::value_delta(&self.world, &stamped);
        if self.value + delta < 0 {
            self.notice = Some(format!("costs {}, you have {}", -delta, self.value));
            return;
        }
        self.notice = None;

        let occupant = self.world.cell_at(row, col).filter(|c| c.is_alive());
        self.last_action = Some(match occupant {
            None if occupied => format!("cleared ice at ({row}, {col})"),
            None => format!("placed {} at ({row}, {col})", hotbar::SLOTS[self.slot].name),
            Some(c) if c.player() == player => format!("took ({row}, {col}), +1"),
            Some(c) => format!("destroyed player {}'s ({row}, {col}), -1", c.player().0),
        });
        self.value += delta;

        crate::net::apply(&mut self.world, &stamped);
        self.world.dirty = true;
        log::debug!("clicked ({row}, {col}); value {}", self.value);

        match &self.link {
            Some(link) => link.send(ClientMessage::Act(stamped)),
            // Offline, the local world is the only world, so it is done.
            None => {}
        }
    }

    /// The region on screen, in absolute cells, as (min, max), with a margin.
    fn visible_cells(&self) -> ((i32, i32), (i32, i32)) {
        let (vw, vh) = self.viewport;
        let (ox, oy) = self.origin();
        let (w, h) = (vw / self.zoom, vh / self.zoom);
        (
            (oy.floor() as i32 - VIEW_MARGIN, ox.floor() as i32 - VIEW_MARGIN),
            (
                (oy + h).ceil() as i32 + VIEW_MARGIN,
                (ox + w).ceil() as i32 + VIEW_MARGIN,
            ),
        )
    }

    fn subscribe_to_view(&mut self) {
        let (min, max) = self.visible_cells();
        let wanted: Vec<_> = World::chunks_covering(min, max)
            .into_iter()
            .filter(|c| !self.subscribed.contains(c))
            .collect();
        if wanted.is_empty() {
            return;
        }
        self.subscribed.extend(wanted.iter().copied());
        if let Some(link) = &self.link {
            link.send(ClientMessage::Subscribe { chunks: wanted });
        }
    }
}

impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let link = open_link();
        // Always start with something on screen. Holding an empty world until
        // the server answers means a client that never connects -- wrong port,
        // server down, a page served from somewhere else -- shows nothing at
        // all and looks broken. A socket object exists long before it
        // connects, and may never connect, so its mere existence is no reason
        // to blank the view. `Welcome` is what replaces this.
        let world = match WORLD {
            WorldMode::Infinite => World::demo(),
            WorldMode::Torus => World::toroidal(TORUS_CHUNKS.0, TORUS_CHUNKS.1),
        };
        let mut chunks = ChunkStore::new(&gpu.device);
        let atlas = Atlas::new(&gpu.device, &gpu.queue);
        chunks.init_unloaded_layer(&gpu.queue);
        chunks.sync(&gpu.queue, &world, TORUS_REPEATS, ((0, 0), (0, 0)));

        let camera_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = world_bind_group_layout(&gpu.device);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(chunks.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        let pipeline = create_pipeline(
            gpu,
            &PipelineDescriptor {
                label: "chunk pipeline",
                shader_source: SHADER_SOURCE,
                vertex_buffers: &[chunk_instance_layout()],
                bind_group_layouts: &[Some(&bgl)],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
        );

        let vertex_buffers = vec![chunks.instance_buffer().clone()];
        log::info!(
            "client ready: {} sprite layers, chunk {}x{} cells, cell {} bytes",
            crate::render::atlas::LAYERS,
            CHUNK_N,
            CHUNK_N,
            size_of::<crate::sim::Cell>(),
        );

        let mut app = Self {
            views: RefCell::new(Views::new(gpu)),
            ui_output: RefCell::new(None),
            pipeline,
            bind_groups: vec![bind_group],
            vertex_buffers,
            camera_buffer,
            chunks,
            _atlas: atlas,
            world,
            camera_dirty: true,
            camera: START_CENTRE,
            zoom: START_ZOOM,
            gesture: Gesture::None,
            space: false,
            shift: false,
            pan: [false; 4],
            pan_step: (0.0, 0.0),
            pan_velocity: (0.0, 0.0),
            touches: Vec::new(),
            touch_count: 0,
            pinch_span: None,
            view_anchor: None,
            touch_view: false,
            hovering: false,
            viewport: (1.0, 1.0),
            elapsed: 0.0,
            notice: None,
            last_action: None,
            value: Player::STARTING_VALUE,
            me: None,
            subscribed: std::collections::HashSet::new(),
            scale: 1.0,
            cursor: (0.0, 0.0),
            pending: None,
            slot: 0,
            link,
        };
        app.world.dirty = false;
        app.viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        app.scale = gpu.scale_factor;
        app.write_camera(gpu);
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        // `update` notices this too; this just avoids a frame of staleness.
        self.viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        self.scale = gpu.scale_factor;
        self.camera_dirty = true;
        self.subscribed.clear();

        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        self.scale = gpu.scale_factor;
        let viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        if viewport != self.viewport {
            self.viewport = viewport;
            self.camera_dirty = true;
            self.subscribed.clear(); // a different area is visible now
        }

        self.apply_pan(dt);

        if self.link.is_some() {
            self.pump_link();
        }

        if let Some(Pending { drag, to_px }) = self.pending.take() {
            let to = self.cell_under_cursor(to_px);
            // A press that travelled but stayed inside one cell is still a
            // click. A one-cell fill would place where a click would take, so
            // which of the two happens must not turn on a few pixels of hand
            // shake at high zoom.
            if drag.moved && to != drag.from {
                self.fill(drag.from, to);
            } else {
                self.click(to.0, to.1);
            }
        }

        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            let visible = self.visible_cells();
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS, visible);
            self.world.dirty = false;
        }
        self.elapsed += dt as f64;
        let status = hud::Status {
            player: self.player(),
            value: self.value,
            generation: self.world.generation,
            chunks_held: self.world.stored_count(),
            chunks_drawn: self.chunks.instance_count(),
            zoom: self.zoom,
            connected: self.link.is_some(),
            notice: self.notice.as_deref(),
            pointer_on_ui: self.views.borrow().wants_pointer(),
            cursor_cell: self.cell_under_cursor(self.cursor),
            last_action: self.last_action.as_deref(),
            holding: hotbar::SLOTS[self.slot].name,
        };
        let (slot, theme) = {
            let views = self.views.borrow();
            (self.slot, views.theme)
        };
        let marks = overlay::Marks {
            hover: self.hover_mark(gpu.scale_factor, status.pointer_on_ui),
            selection: self.selection_mark(gpu.scale_factor),
        };
        let mut picked = None;
        let output = self.views.borrow_mut().run(gpu, self.elapsed, |ctx| {
            overlay::show(ctx, &theme, &marks);
            let hud_rect = hud::show(ctx, &theme, &status);
            let bar = hotbar::show(ctx, &theme, slot);
            picked = bar.picked;
            // Either panel claims the pointer, so the union is what the world
            // must not receive.
            match (hud_rect, bar.rect) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            }
        });
        if let Some(index) = picked {
            self.slot = index;
        }
        *self.ui_output.borrow_mut() = Some(output);

        if self.camera_dirty {
            self.write_camera(gpu);
            self.camera_dirty = false;
            // Panning changes the region the backdrop has to cover, so the
            // instance list follows the camera.
            let visible = self.visible_cells();
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS, visible);
        }
    }

    fn draw_calls(&self) -> Vec<DrawCall<'_>> {
        vec![DrawCall {
            pipeline: &self.pipeline,
            bind_groups: &self.bind_groups,
            vertex_buffers: &self.vertex_buffers,
            index_buffer: None,
            draw: Draw::Vertices {
                vertices: 0..4,
                instances: 0..self.chunks.instance_count(),
            },
        }]
    }

    fn on_cursor(&mut self, x: f64, y: f64) {
        let (dx, dy) = (x - self.cursor.0, y - self.cursor.1);
        self.cursor = (x, y);
        self.hovering = true;

        let slop = self.slop();
        if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached((x, y), slop);
        } else if self.is_panning() {
            self.pan_by_pixels(dx, dy);
        }
    }

    fn on_key(&mut self, code: winit::keyboard::KeyCode, pressed: bool) {
        use winit::keyboard::KeyCode as K;
        if let Some(index) = digit(code).and_then(hotbar::slot_for_digit) {
            if pressed {
                self.slot = index;
            }
            return;
        }
        match code {
            K::Space => {
                self.space = pressed;
                return;
            }
            K::ShiftLeft | K::ShiftRight => {
                self.shift = pressed;
                return;
            }
            // Abandon whatever is being drawn. A rectangle you have decided
            // against otherwise has to be shrunk back to one cell to be made
            // harmless, and that still places a cell.
            K::Escape if pressed => {
                self.cancel_gesture();
                return;
            }
            _ => {}
        }
        let slot = match code {
            K::ArrowLeft | K::KeyA => 0,
            K::ArrowRight | K::KeyD => 1,
            K::ArrowUp | K::KeyW => 2,
            K::ArrowDown | K::KeyS => 3,
            _ => return,
        };
        self.pan[slot] = pressed;
    }

    /// A trackpad pinch, on the platforms that report one as a gesture.
    /// `delta` is a relative change, so it multiplies rather than adds.
    fn on_pinch(&mut self, delta: f64) {
        if !delta.is_finite() {
            return;
        }
        self.pan_velocity = (0.0, 0.0);
        self.zoom_about_cursor((1.0 + delta as f32).clamp(0.5, 2.0));
    }

    /// One finger draws; two move the view.
    ///
    /// The split that every touch drawing surface uses, and the one the mouse
    /// already uses here: the primary pointer draws, and a second gesture
    /// moves the view. Nothing to switch between, no timing to get right, and
    /// no mode to be in and forget.
    ///
    /// The alternative — one finger pans, as a map does — leaves nothing to
    /// draw with, and the hotbar has already promised the player is holding
    /// something.
    fn on_touch(&mut self, id: u64, phase: winit::event::TouchPhase, x: f64, y: f64) {
        use winit::event::TouchPhase as P;
        // A finger is not a hovering pointer. There is nothing under it once
        // it lifts, so the hover box would be left behind where it ended.
        self.hovering = false;
        let at = (x, y);

        match phase {
            P::Started => {
                self.touches.push((id, at));
                self.pan_velocity = (0.0, 0.0);
            }
            P::Moved => {
                if let Some(t) = self.touches.iter_mut().find(|t| t.0 == id) {
                    t.1 = at;
                }
            }
            P::Ended | P::Cancelled => self.touches.retain(|t| t.0 != id),
        }

        // A second finger means the view rather than the world, and it stays
        // that way until every finger lifts. Whatever the first finger had
        // started drawing is abandoned rather than resolved: the player was
        // reaching for a pinch, not finishing a rectangle.
        if self.touches.len() > 1 {
            self.touch_view = true;
            self.gesture = Gesture::None;
        }

        if self.touches.is_empty() {
            if let Gesture::Drawing(drag) = self.gesture {
                self.gesture = Gesture::None;
                self.pending = Some(Pending { drag, to_px: self.cursor });
            }
            self.touch_view = false;
            self.touch_count = 0;
            self.pinch_span = None;
            self.view_anchor = None;
            return;
        }

        if self.touch_view {
            self.follow_touches();
            return;
        }

        self.cursor = at;
        self.touch_count = self.touches.len();
        let slop = self.slop();
        if matches!(phase, P::Started) {
            self.gesture = Gesture::Drawing(Drag::begin(at, self.cell_under_cursor(at)));
        } else if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached(at, slop);
        }
    }

    /// A wheel zooms; a trackpad scrolls the view. They arrive as the same
    /// event, and the only thing separating them is the unit: a wheel reports
    /// discrete lines, a trackpad continuous pixels. Holding ctrl means zoom
    /// either way, which is how a trackpad pinch reaches a browser.
    ///
    /// Splitting on the unit is what makes the gestures consistent. Treating
    /// every scroll as zoom made a two-finger swipe on a trackpad lurch the
    /// zoom when every other application pans with it.
    fn on_scroll(&mut self, delta: winit::event::MouseScrollDelta, ctrl: bool) {
        use winit::event::MouseScrollDelta as D;
        // A trackpad sends its own momentum after the fingers lift, so ours
        // would be a second one running alongside it.
        self.pan_velocity = (0.0, 0.0);
        match delta {
            // A wheel notch: zoom, about the cursor rather than the screen
            // centre, so what is under the pointer stays under it.
            D::LineDelta(_, y) => self.zoom_about_cursor(1.15f32.powf(y)),
            // A trackpad. Pixels are already screen distance, so panning is a
            // straight division by zoom; zooming needs a much gentler factor
            // than a notch or the smallest pinch jumps several levels.
            D::PixelDelta(p) if ctrl => {
                self.zoom_about_cursor(1.15f32.powf(p.y as f32 / 140.0))
            }
            D::PixelDelta(p) => self.pan_by_pixels(p.x, p.y),
        }
    }

    /// Left draws: a click acts on one cell, a drag fills the rectangle it
    /// swept. Middle, right and space+left all pan, so drawing and moving the
    /// view are never the same gesture and neither has to guess which was
    /// meant — and every mouse and trackpad has at least one of the three.
    fn on_click(&mut self, button: winit::event::MouseButton, pressed: bool) {
        use winit::event::MouseButton as B;
        if pressed {
            // A press is aiming at something, so a glide left over from the
            // last one stops here rather than sliding the target away.
            self.pan_velocity = (0.0, 0.0);
            match button {
                B::Middle | B::Right => self.begin_pan(Some(button)),
                B::Left if self.space => self.begin_pan(Some(button)),
                B::Left => {
                    let at = self.cursor;
                    self.gesture = Gesture::Drawing(Drag::begin(at, self.cell_under_cursor(at)));
                }
                _ => {}
            }
            return;
        }
        match self.gesture {
            Gesture::Panning { button: held } if held == Some(button) => self.end_pan(),
            Gesture::Drawing(drag) if button == B::Left => {
                self.gesture = Gesture::None;
                self.pending = Some(Pending { drag, to_px: self.cursor });
            }
            _ => {}
        }
    }

    fn cursor_icon(&self) -> winit::window::CursorIcon {
        use winit::window::CursorIcon as C;
        if self.is_panning() {
            C::Grabbing
        } else if self.space {
            C::Grab
        } else if self.views.borrow().wants_pointer() {
            C::Default
        } else {
            C::Crosshair
        }
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent, scale: f32) -> bool {
        use winit::event::WindowEvent as E;
        match event {
            E::Focused(false) => self.cancel_gesture(),
            E::CursorLeft { .. } => self.hovering = false,
            _ => {}
        }

        let consumed = self.views.borrow_mut().on_window_event(event, scale);

        // A gesture that began on the world keeps the pointer until it ends,
        // even if it strays over a panel. Without this, a drag released over
        // the hotbar is swallowed: `on_click` never fires, the rectangle is
        // never filled, and the gesture stays open with nothing to close it.
        //
        // The interface still sees the event — only the answer changes — so
        // egui's own idea of where the pointer is stays right.
        if self.gesture_active() && matches!(event, E::MouseInput { .. } | E::CursorMoved { .. }) {
            return false;
        }
        consumed
    }

    fn overlay(
        &self,
        gpu: &GpuState,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut wgpu::RenderPass<'static>,
    ) {
        if let Some(output) = self.ui_output.borrow().as_ref() {
            self.views.borrow_mut().render(gpu, encoder, pass, output);
        }
    }

    fn clear_color(&self) -> Option<wgpu::Color> {
        // From the theme, so the world's ground and the panels beside it are
        // the same colour rather than two guesses at the same colour.
        Some(self.views.borrow().theme.clear_color())
    }
}

/// Open a connection, if there is one to open.
///
/// On the web this needs no configuration: the page came from the server, so
/// the server is wherever the page came from. `wss` when the page is `https`,
/// or the browser blocks it as mixed content.
#[cfg(target_arch = "wasm32")]
fn open_link() -> Option<Link> {
    let url = Link::origin_url("/ws")?;
    log::info!("connecting to {url}");
    let link = Link::connect(&url)?;
    link.send(ClientMessage::Join { name: "web".into() });
    Some(link)
}

/// On native there is no page to have come from, so the URL is an argument.
#[cfg(not(target_arch = "wasm32"))]
fn open_link() -> Option<Link> {
    let (url, name) = CONNECTION.lock().unwrap().take()?;
    let link = Link::connect(url?);
    link.send(ClientMessage::Join { name });
    Some(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HUD swatch and the cells on the board must agree about a player's
    /// colour, so this reproduces the shader's arithmetic and checks the result
    /// is in range and distinct between players.
    #[test]
    fn player_colours_are_in_gamut_and_distinct() {
        use crate::client::views::hud::player_colour;
        let mut seen = Vec::new();
        for p in 1..=PlayerId::MAX {
            let c = player_colour(PlayerId(p));
            assert!(
                !seen.contains(&c),
                "players {p} and an earlier one share {c:?}"
            );
            seen.push(c);
        }
        // Player 1 is the saturated tier, player 2 the muted one.
        let (a, b) = (player_colour(PlayerId(1)), player_colour(PlayerId(2)));
        assert_ne!(a, b);
    }

    /// The bug this was written against. A drag was decided from the distance
    /// between one pointer event and the next, so a slow, deliberate sweep —
    /// which arrives as a stream of one-pixel moves — never counted as one,
    /// and a dragged pane came out as a single cell at the release point.
    #[test]
    fn a_slow_sweep_is_a_drag() {
        let mut drag = Drag::begin((100.0, 100.0), (0, 0));
        for step in 1..=60 {
            drag.reached((100.0 + step as f64, 100.0), DRAG_SLOP);
            assert!(
                !drag.moved || step as f64 > DRAG_SLOP,
                "a press should not become a drag inside the slop"
            );
        }
        assert!(drag.moved, "sixty one-pixel steps is a drag, not a click");
    }

    /// And the case the slop exists for: a hand that shakes on the button is
    /// still clicking, however many events it produces.
    #[test]
    fn a_shaky_press_is_a_click() {
        let mut drag = Drag::begin((100.0, 100.0), (0, 0));
        for at in [(102.0, 100.0), (98.0, 101.0), (100.0, 98.0), (101.0, 101.0)] {
            drag.reached(at, DRAG_SLOP);
        }
        assert!(!drag.moved);
    }

    /// Once a press is a drag it stays one. Coming back to where it started
    /// mid-sweep must not turn the gesture back into a click.
    #[test]
    fn a_drag_does_not_become_a_click_again() {
        let mut drag = Drag::begin((100.0, 100.0), (0, 0));
        drag.reached((400.0, 400.0), DRAG_SLOP);
        drag.reached((100.0, 100.0), DRAG_SLOP);
        assert!(drag.moved);
    }

    /// The cap that keeps a sweep at one pixel per cell from listing, pricing
    /// and sending millions of cells.
    #[test]
    fn a_rectangle_is_bounded() {
        let (rows, cols) = span((0, 0), (-3, 4));
        assert_eq!((rows, cols), (4, 5));
        assert!(span((0, 0), (i32::MAX, i32::MAX)).0 > MAX_FILL_CELLS);
    }

    /// A pinch is measured between two fingers and nothing else, but the
    /// centre is measured over however many are down — which is what lets a
    /// pinch that has lost a finger carry on panning with the other.
    #[test]
    fn fingers_have_a_centre_and_sometimes_a_span() {
        let two = [(1, (0.0, 0.0)), (2, (10.0, 0.0))];
        assert_eq!(centroid(&two), (5.0, 0.0));
        assert_eq!(pinch_span(&two), Some(10.0));

        let one = [(1, (4.0, 6.0))];
        assert_eq!(centroid(&one), (4.0, 6.0));
        assert_eq!(pinch_span(&one), None);
        assert_eq!(pinch_span(&[]), None);
    }
}

/// How many rows and columns a rectangle covers, both ends included.
///
/// In `i64` because a drag at one pixel per cell can span most of an `i32`,
/// and the product of two of those still has to be a number the cap can be
/// compared against rather than an overflow.
fn span(from: (i32, i32), to: (i32, i32)) -> (i64, i64) {
    (
        (from.0 as i64 - to.0 as i64).abs() + 1,
        (from.1 as i64 - to.1 as i64).abs() + 1,
    )
}

/// The middle of however many fingers are down. One finger's middle is itself,
/// which is what lets a pinch that has lost a finger carry on panning.
fn centroid(touches: &[(u64, (f64, f64))]) -> (f64, f64) {
    let n = touches.len().max(1) as f64;
    let sum = touches
        .iter()
        .fold((0.0, 0.0), |a, t| (a.0 + t.1 .0, a.1 + t.1 .1));
    (sum.0 / n, sum.1 / n)
}

/// The gap between exactly two fingers. One has no span to measure, and three
/// or more is not a pinch anybody means.
fn pinch_span(touches: &[(u64, (f64, f64))]) -> Option<f64> {
    let [(_, a), (_, b)] = touches else { return None };
    Some(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
}

/// The digit a key stands for, if it is one.
fn digit(code: winit::keyboard::KeyCode) -> Option<u32> {
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
