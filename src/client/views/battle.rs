//! The game view: one world, a camera over it, and the input that drives both.
//!
//! A view rather than the application, so a menu or a lobby can be another one
//! beside it without this having to know they exist.

use std::cell::RefCell;

use crate::render::app::App;
use super::{camera, hotbar, hud, overlay, Views};
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

/// The hover box is not drawn below this many pixels per cell. A box around a
/// two-pixel cell claims a precision the pointer does not have.
const HOVER_MIN_ZOOM: f32 = 4.0;

/// The most cells one drag may lay.
///
/// A rectangle at one pixel per cell can cover millions, and every one of them
/// would be listed, priced, applied and put on the wire. A stroke stops
/// growing when it reaches this and says so, rather than being trimmed at the
/// end where nobody would see what was lost.
const MAX_DRAG_CELLS: i64 = 4096;

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
struct Drag {
    /// Where the press landed, in cells, as (row, col).
    from: (i32, i32),
    /// And in pixels, which is what decides a drag from a click.
    from_px: (f64, f64),
    moved: bool,
    /// What this drag lays, taken from the slot held when it began. Fixed at
    /// the press rather than read each frame, so changing slot mid-stroke does
    /// not change the shape of a line already half drawn.
    stroke: hotbar::Stroke,
    /// Every cell the pointer has crossed, in order. A pencil only.
    path: Vec<(i32, i32)>,
    /// The same cells as a set. A stroke that crosses itself would otherwise
    /// list a cell twice, and the pricing compares each entry against the
    /// world rather than against the entries before it — so the crossing
    /// would be charged for twice and paid for once.
    seen: std::collections::HashSet<(i32, i32)>,
}

impl Drag {
    fn begin(px: (f64, f64), cell: (i32, i32), stroke: hotbar::Stroke) -> Self {
        let mut drag = Self {
            from: cell,
            from_px: px,
            moved: false,
            stroke,
            path: Vec::new(),
            seen: std::collections::HashSet::new(),
        };
        if stroke == hotbar::Stroke::Pencil {
            drag.mark(cell);
        }
        drag
    }

    /// Note where the press has got to. `slop` is in the same physical pixels
    /// the positions are, and `cell` is what is under it now.
    fn reached(&mut self, px: (f64, f64), slop: f64, cell: (i32, i32)) {
        self.moved |= travelled(self.from_px, px, slop);
        if self.stroke != hotbar::Stroke::Pencil {
            return;
        }
        // Every cell between the last one and this, not just this one. Pointer
        // events arrive far apart when the hand moves quickly — a fast stroke
        // can cross twenty cells between two of them — so a pencil that marked
        // only where the pointer was reported would draw a dotted line.
        let last = self.path.last().copied().unwrap_or(self.from);
        for step in line(last, cell) {
            if self.path.len() as i64 >= MAX_DRAG_CELLS {
                return;
            }
            self.mark(step);
        }
    }

    fn mark(&mut self, cell: (i32, i32)) {
        if self.seen.insert(cell) {
            self.path.push(cell);
        }
    }

    /// Whether the stroke has reached its limit and stopped growing.
    fn full(&self) -> bool {
        self.path.len() as i64 >= MAX_DRAG_CELLS
    }
}

/// The cells a straight line from `from` to `to` passes through, `from`
/// excluded. Bresenham, so it is the same set whichever end it starts from.
fn line(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut r, mut c) = from;
    let (dr, dc) = ((to.0 - r).abs(), -(to.1 - c).abs());
    let (sr, sc) = (if r < to.0 { 1 } else { -1 }, if c < to.1 { 1 } else { -1 });
    let mut err = dr + dc;
    let mut out = Vec::new();
    while (r, c) != to {
        let e2 = 2 * err;
        if e2 >= dc {
            err += dc;
            r += sr;
        }
        if e2 <= dr {
            err += dr;
            c += sc;
        }
        out.push((r, c));
    }
    out
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
    /// Where the world is being looked at from, and how that moves.
    camera: camera::Camera,
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
        gpu.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&self.camera.uniform()));
    }

    fn cell_under_cursor(&self, at: (f64, f64)) -> (i32, i32) {
        self.camera.cell_at(at)
    }

    /// Take the viewport and the pixel density from the frame, and say whether
    /// the visible area changed.
    ///
    /// Cached on the camera because input callbacks are handed no `GpuState`,
    /// and refreshed every frame rather than on resize alone: updating it only
    /// on resize left it stale whenever no resize event arrived, and zoom
    /// anchoring then disagreed with the camera about how big the screen was.
    fn fit(&mut self, gpu: &GpuState) -> bool {
        let viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        self.camera.scale = gpu.scale_factor;
        if viewport == self.camera.viewport {
            return false;
        }
        self.camera.viewport = viewport;
        self.camera.dirty = true;
        true
    }

    /// Who we are. Before the server has said, we are player one — offline is
    /// a game of one rather than a game of nobody.
    fn player(&self) -> PlayerId {
        self.me.unwrap_or(PlayerId(1))
    }
}

impl BattleApp {
    fn zoom_about_cursor(&mut self, factor: f32) {
        self.camera.zoom_about(factor, self.cursor);
    }

    /// Whether a gesture the world owns is in progress.
    fn gesture_active(&self) -> bool {
        !matches!(self.gesture, Gesture::None) || self.touch_view
    }

    fn is_panning(&self) -> bool {
        matches!(self.gesture, Gesture::Panning { .. }) || self.touch_view
    }

    /// The drag threshold in the physical pixels positions are reported in.
    fn slop(&self) -> f64 {
        DRAG_SLOP * self.camera.scale.max(1.0) as f64
    }

    /// Start drawing, with the shape the held slot lays.
    fn begin_drawing(&mut self, at: (f64, f64)) {
        let stroke = hotbar::SLOTS[self.slot].stroke;
        self.gesture = Gesture::Drawing(Drag::begin(at, self.camera.cell_at(at), stroke));
    }

    fn begin_pan(&mut self, button: Option<winit::event::MouseButton>) {
        self.gesture = Gesture::Panning { button };
        self.camera.begin_drag();
    }

    fn end_pan(&mut self) {
        self.gesture = Gesture::None;
        self.camera.end_drag();
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
        self.camera.halt();
    }

    /// Move the view for whatever pan keys are held, then let the camera
    /// measure the drag in progress or carry a released one on.
    fn apply_pan(&mut self, dt: f32) {
        let x = (self.pan[1] as i32 - self.pan[0] as i32) as f32;
        let y = (self.pan[3] as i32 - self.pan[2] as i32) as f32;
        if x != 0.0 || y != 0.0 {
            let speed = PAN_SPEED * if self.shift { PAN_FAST } else { 1.0 };
            self.camera.nudge(x, y, speed, dt);
        }
        self.camera.advance(dt, self.is_panning());
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
                    self.camera.zoom_about((now / before) as f32, anchor);
                }
            }
            if let Some(anchor) = self.view_anchor {
                self.camera.pan_by_pixels(focus.0 - anchor.0, focus.1 - anchor.1);
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
    fn lay(&mut self, cells: Vec<(i32, i32)>, shape: String) {
        let count = cells.len();
        let (stamped, delta) = self.quote(cells);

        // All or nothing. A stroke laid as far as the value stretched would
        // stop somewhere the hand did not, and the player would be left
        // working out where it ran out and why.
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
        self.last_action = Some(format!("{shape}, {delta:+}"));

        if let Some(link) = &self.link {
            link.send(ClientMessage::Act(stamped));
        }
    }

    /// What a drag would lay, and how to describe it.
    ///
    /// One function for both shapes and for both callers, so the preview
    /// cannot draw one thing and the release lay another.
    fn drag_cells(&self, drag: &Drag, to: (i32, i32)) -> Result<(Vec<(i32, i32)>, String), String> {
        let name = hotbar::SLOTS[self.slot].name;
        match drag.stroke {
            hotbar::Stroke::Pencil => {
                let full = if drag.full() { ", full" } else { "" };
                Ok((
                    drag.path.clone(),
                    format!("drew {} cells of {name}{full}", drag.path.len()),
                ))
            }
            hotbar::Stroke::Rectangle => {
                let (rows, cols) = span(drag.from, to);
                let area = rows * cols;
                if area > MAX_DRAG_CELLS {
                    return Err(format!("{area} cells is more than one drag may lay"));
                }
                let (r0, r1) = (drag.from.0.min(to.0), drag.from.0.max(to.0));
                let (c0, c1) = (drag.from.1.min(to.1), drag.from.1.max(to.1));
                Ok((
                    (r0..=r1)
                        .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
                        .collect(),
                    format!("laid {rows}x{cols} of {name}"),
                ))
            }
        }
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
    fn hover_mark(&self, on_ui: bool) -> Option<egui::Rect> {
        if on_ui || !self.hovering || self.is_panning() || self.camera.zoom < HOVER_MIN_ZOOM {
            return None;
        }
        if matches!(&self.gesture, Gesture::Drawing(drag) if drag.moved) {
            return None;
        }
        let at = self.cell_under_cursor(self.cursor);
        Some(self.camera.cell_rect(at, at))
    }

    /// What a drag has laid out so far, with what it would cost.
    fn selection_mark(&self) -> Option<overlay::Selection> {
        let Gesture::Drawing(drag) = &self.gesture else { return None };
        if !drag.moved {
            return None;
        }
        let to = self.cell_under_cursor(self.cursor);
        let slot = &hotbar::SLOTS[self.slot];

        let (cells, label, allowed) = match self.drag_cells(drag, to) {
            Err(why) => (Vec::new(), why, false),
            Ok((cells, shape)) => {
                let (_, delta) = self.quote(cells.clone());
                if self.value + delta < 0 {
                    let why = format!("{shape}   costs {}, you have {}", -delta, self.value);
                    (cells, why, false)
                } else {
                    (cells, format!("{shape}   {delta:+}"), true)
                }
            }
        };

        let rects: Vec<egui::Rect> = match drag.stroke {
            // A stroke is its cells; there is no outline to draw round a line
            // that doubles back on itself.
            hotbar::Stroke::Pencil => cells
                .iter()
                .map(|&at| self.camera.cell_rect(at, at))
                .collect(),
            hotbar::Stroke::Rectangle => vec![self.camera.cell_rect(drag.from, to)],
        };

        let (r, g, b) = hud::player_colour(self.player());
        Some(overlay::Selection {
            bounds: self.camera.cell_rect(drag.from, to),
            cells: rects,
            outlined: drag.stroke == hotbar::Stroke::Rectangle,
            tint: egui::Color32::from_rgb(r, g, b),
            hatched: slot.placement == Placement::Ice,
            label,
            allowed,
        })
    }

    /// One button does everything, and the cell under it decides which — for
    /// whatever the hotbar is holding.
    ///
    /// The thing you are holding is already there, so take it back: your own
    /// for value, someone else's at a cost. It is not there, so put it down.
    /// There is nothing to hold and nothing to remember, which is what a
    /// clicker opening needs.
    ///
    /// Keyed on what is held rather than on whether the cell is occupied at
    /// all, because life and ice are independent. Clicking a living cell under
    /// a pane means killing the life, not taking the pane with it — and it is
    /// what gives a misplaced pane a way back, since holding Ice and clicking
    /// one lifts it.
    ///
    /// Applied locally *and* sent, rather than sent and awaited: the rules are
    /// deterministic and the server runs the same `net::apply` and charges by
    /// the same `net::value_delta`, so acting immediately shows the right
    /// answer a round trip early. If the server disagrees the chunk digests
    /// will not match and the resync puts it right.
    fn click(&mut self, row: i32, col: i32) {
        let player = self.player();
        let cells = vec![(row, col)];
        let slot = &hotbar::SLOTS[self.slot];
        let placement = slot.placement;

        let existing = self.world.cell_at(row, col).unwrap_or(crate::sim::Cell::DEAD);
        // Already there is exactly "taking it away would change something".
        let already_there = placement.remove_from(existing) != existing;
        let action = if already_there {
            Action::Erase { cells, placement }
        } else {
            Action::Paint { cells, placement }
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

        let name = slot.name;
        self.last_action = Some(match (already_there, existing.player()) {
            (false, _) => format!("placed {name} at ({row}, {col}), {delta:+}"),
            (true, owner) if owner == player => {
                format!("took your {name} at ({row}, {col}), {delta:+}")
            }
            (true, owner) => {
                format!("took player {}'s {name} at ({row}, {col}), {delta:+}", owner.0)
            }
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

    fn subscribe_to_view(&mut self) {
        let (min, max) = self.camera.visible_cells(VIEW_MARGIN);
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
            camera: camera::Camera::new(START_CENTRE, START_ZOOM),
            gesture: Gesture::None,
            space: false,
            shift: false,
            pan: [false; 4],
            touches: Vec::new(),
            touch_count: 0,
            pinch_span: None,
            view_anchor: None,
            touch_view: false,
            hovering: false,
            elapsed: 0.0,
            notice: None,
            last_action: None,
            value: Player::STARTING_VALUE,
            me: None,
            subscribed: std::collections::HashSet::new(),
            cursor: (0.0, 0.0),
            pending: None,
            slot: 0,
            link,
        };
        app.world.dirty = false;
        app.fit(gpu);
        app.write_camera(gpu);
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        // `update` notices this too; this just avoids a frame of staleness.
        self.fit(gpu);
        self.subscribed.clear();
        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        if self.fit(gpu) {
            self.subscribed.clear(); // a different area is visible now
        }

        self.apply_pan(dt);

        if self.link.is_some() {
            self.pump_link();
        }

        if let Some(Pending { drag, to_px }) = self.pending.take() {
            let to = self.cell_under_cursor(to_px);
            // More than one cell is what makes it a drag rather than a click.
            // A press that travelled but stayed inside one cell would place
            // where a click would take, so which of the two happens must not
            // turn on a few pixels of hand shake at high zoom.
            match self.drag_cells(&drag, to) {
                Ok((cells, shape)) if drag.moved && cells.len() > 1 => self.lay(cells, shape),
                Ok(_) => self.click(to.0, to.1),
                Err(why) => self.notice = Some(why),
            }
        }

        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            let visible = self.camera.visible_cells(VIEW_MARGIN);
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
            zoom: self.camera.zoom,
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
        let (r, g, b) = hud::player_colour(self.player());
        let marks = overlay::Marks {
            tint: egui::Color32::from_rgb(r, g, b),
            hover: self.hover_mark(status.pointer_on_ui),
            selection: self.selection_mark(),
        };
        let mut picked = None;
        let output = self.views.borrow_mut().run(gpu, self.elapsed, |ctx| {
            overlay::show(ctx, &theme, &marks);
            let hud_rect = hud::show(ctx, &theme, &status);
            let bar = hotbar::show(ctx, &theme, slot);
            picked = bar.picked;
            // Each panel on its own. Folding them together first would claim
            // everything between them, and they sit in opposite corners.
            [hud_rect, bar.rect].into_iter().flatten().collect()
        });
        if let Some(index) = picked {
            self.slot = index;
        }
        *self.ui_output.borrow_mut() = Some(output);

        if self.camera.dirty {
            self.write_camera(gpu);
            self.camera.dirty = false;
            // Panning changes the region the backdrop has to cover, so the
            // instance list follows the camera.
            let visible = self.camera.visible_cells(VIEW_MARGIN);
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

        let (slop, cell) = (self.slop(), self.cell_under_cursor((x, y)));
        if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached((x, y), slop, cell);
        } else if self.is_panning() {
            self.camera.pan_by_pixels(dx, dy);
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
        self.camera.halt();
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
                self.camera.halt();
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
            if let Gesture::Drawing(drag) = std::mem::replace(&mut self.gesture, Gesture::None) {
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
        let (slop, cell) = (self.slop(), self.cell_under_cursor(at));
        if matches!(phase, P::Started) {
            self.begin_drawing(at);
        } else if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached(at, slop, cell);
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
        self.camera.halt();
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
            D::PixelDelta(p) => self.camera.pan_by_pixels(p.x, p.y),
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
            self.camera.halt();
            match button {
                B::Middle | B::Right => self.begin_pan(Some(button)),
                B::Left if self.space => self.begin_pan(Some(button)),
                B::Left => self.begin_drawing(self.cursor),
                _ => {}
            }
            return;
        }
        // Taken out, so the drag can be moved into `pending` rather than
        // copied -- a stroke carries every cell it has crossed.
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Panning { button: held } if held == Some(button) => self.end_pan(),
            Gesture::Drawing(drag) if button == B::Left => {
                self.pending = Some(Pending { drag, to_px: self.cursor });
            }
            // Not this button's to end, so put it back.
            other => self.gesture = other,
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
        let mut drag = Drag::begin((100.0, 100.0), (0, 0), hotbar::Stroke::Rectangle);
        for step in 1..=60 {
            drag.reached((100.0 + step as f64, 100.0), DRAG_SLOP, (0, 0));
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
        let mut drag = Drag::begin((100.0, 100.0), (0, 0), hotbar::Stroke::Rectangle);
        for at in [(102.0, 100.0), (98.0, 101.0), (100.0, 98.0), (101.0, 101.0)] {
            drag.reached(at, DRAG_SLOP, (0, 0));
        }
        assert!(!drag.moved);
    }

    /// Once a press is a drag it stays one. Coming back to where it started
    /// mid-sweep must not turn the gesture back into a click.
    #[test]
    fn a_drag_does_not_become_a_click_again() {
        let mut drag = Drag::begin((100.0, 100.0), (0, 0), hotbar::Stroke::Rectangle);
        drag.reached((400.0, 400.0), DRAG_SLOP, (0, 0));
        drag.reached((100.0, 100.0), DRAG_SLOP, (0, 0));
        assert!(drag.moved);
    }

    /// The cap that keeps a sweep at one pixel per cell from listing, pricing
    /// and sending millions of cells.
    #[test]
    fn a_rectangle_is_bounded() {
        let (rows, cols) = span((0, 0), (-3, 4));
        assert_eq!((rows, cols), (4, 5));
        assert!(span((0, 0), (i32::MAX, i32::MAX)).0 > MAX_DRAG_CELLS);
    }

    /// A pencil that only marked where the pointer was reported would draw a
    /// dotted line: events arrive far apart when the hand moves quickly. Every
    /// step must touch the one before it.
    #[test]
    fn a_line_has_no_gaps() {
        for to in [(9, 2), (2, 9), (-7, 4), (0, 5), (5, 0), (-3, -8)] {
            let cells = line((0, 0), to);
            assert_eq!(*cells.last().unwrap(), to, "should arrive at {to:?}");
            let mut previous = (0, 0);
            for &at in &cells {
                let step = ((at.0 - previous.0).abs(), (at.1 - previous.1).abs());
                assert!(step.0 <= 1 && step.1 <= 1, "jumped from {previous:?} to {at:?}");
                previous = at;
            }
        }
        assert!(line((3, 3), (3, 3)).is_empty(), "going nowhere marks nothing");
    }

    /// A stroke that crosses itself must list each cell once. The pricing
    /// compares every entry against the world rather than against the entries
    /// before it, so a repeat would be charged for twice and laid once.
    #[test]
    fn a_stroke_that_crosses_itself_lists_each_cell_once() {
        let mut drag = Drag::begin((0.0, 0.0), (0, 0), hotbar::Stroke::Pencil);
        // Out along a row, back along it, and out again.
        for cell in [(0, 6), (0, 0), (0, 6)] {
            drag.reached((100.0, 100.0), DRAG_SLOP, cell);
        }
        let unique: std::collections::HashSet<_> = drag.path.iter().collect();
        assert_eq!(unique.len(), drag.path.len(), "a cell is listed twice");
        assert_eq!(drag.path.len(), 7, "seven cells from (0,0) to (0,6)");
    }

    /// The stroke stops at the cap rather than being trimmed later, so what
    /// is drawn is what is laid.
    #[test]
    fn a_stroke_stops_at_its_limit() {
        let mut drag = Drag::begin((0.0, 0.0), (0, 0), hotbar::Stroke::Pencil);
        drag.reached((100.0, 100.0), DRAG_SLOP, (0, MAX_DRAG_CELLS as i32 * 2));
        assert!(drag.full());
        assert_eq!(drag.path.len() as i64, MAX_DRAG_CELLS);
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
