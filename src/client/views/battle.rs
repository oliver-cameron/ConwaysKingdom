//! The game view: one world, a camera over it, and the input that drives both.
//!
//! A view rather than the application, so a menu or a lobby can be another one
//! beside it without this having to know they exist.

use std::cell::RefCell;

use crate::render::app::App;
use super::{camera, hotbar, hud, menu, overlay, Views};
use crate::render::atlas::Atlas;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, WorldKind, CHUNK_N};

use crate::net::link::Link;
use crate::net::{Action, ClientMessage, Placement, ServerMessage, Stamped};
use crate::sim::{Player, PlayerId};

/// How large a purely vertical pixel scroll has to be, in a browser, to be a
/// wheel notch rather than a trackpad swipe. Chrome sends 100 or 120 for a
/// notch; a swipe is a stream of much smaller values, and a fast one only
/// gets near this at its peak.
const WHEEL_NOTCH: f64 = 60.0;

/// How often a client asks the server whether they still agree, in
/// generations. Four a second, so this is every few seconds.
const CHECKPOINT_EVERY: u64 = 12;

/// The most chunks one checkpoint carries. Sixteen bytes each, so even the
/// cap is a small message; it exists so a client holding an enormous world
/// cannot send an enormous one.
const MAX_CHECKPOINT_CHUNKS: usize = 512;

/// The most generations a client will step at once to catch up before giving
/// up and taking the server's number. A stall long enough to exceed this has
/// already cost more than the catching up would fix.
const CATCH_UP: u64 = 32;

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

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

/// Set before the event loop starts, like the connection and for the same
/// reason: `App::init` takes no arguments of its own.
#[cfg(not(target_arch = "wasm32"))]
static WORLD: std::sync::Mutex<WorldKind> = std::sync::Mutex::new(WorldKind::Infinite);

/// Choose the world before launching. Native only — a browser has no command
/// line, and its world comes from the server anyway.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_world(mode: WorldKind) {
    *WORLD.lock().unwrap() = mode;
}

#[cfg(not(target_arch = "wasm32"))]
fn chosen_world() -> WorldKind {
    *WORLD.lock().unwrap()
}

/// A browser gets the infinite world, and then the server's if it connects.
#[cfg(target_arch = "wasm32")]
fn chosen_world() -> WorldKind {
    WorldKind::Infinite
}

/// Where to connect, as whom, and to which room. Set before the event loop
/// starts, because `App::init` takes no arguments of its own. A one-shot
/// rather than a config store: it is read once.
#[cfg(not(target_arch = "wasm32"))]
static CONNECTION: std::sync::Mutex<Option<Connection>> = std::sync::Mutex::new(None);

/// What a client needs to reach a server: an address, a name, and a room.
#[cfg(not(target_arch = "wasm32"))]
pub struct Connection {
    /// `None` runs offline.
    pub url: Option<String>,
    pub name: String,
    /// Which world on that server. `None` takes whatever the server calls its
    /// default, so a player with nothing to say about rooms still lands
    /// somewhere.
    pub room: Option<String>,
}

/// Point the client at a server before launching it.
#[cfg(not(target_arch = "wasm32"))]
pub fn set_connection(connection: Connection) {
    *CONNECTION.lock().unwrap() = Some(connection);
}

/// Which screen the client is on.
///
/// Not two `App`s. The event loop calls one, and the world, the pipeline and
/// the atlas belong to the game whether or not it is being looked at — so the
/// menu is a state the app is in rather than a second app with its own copy
/// of the GPU.
enum Screen {
    /// Choosing a server and a room, or choosing to play alone.
    Menu(menu::Menu),
    /// In a world. The only screen that takes input from the world.
    Playing,
}

/// How long to wait for a server to say what rooms it has before giving up.
///
/// Generous, because it covers a connection being made as well as answered,
/// and short enough that a wrong address is a mistake you correct rather than
/// a page you reload.
const ROOM_LIST_TIMEOUT: f64 = 8.0;

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
    /// How much of the path has been laid down already.
    ///
    /// A stroke is laid as it is drawn rather than when the button comes up.
    /// Holding it back meant every line appeared a moment after the hand that
    /// drew it, which reads as lag however fast the rest is — and it is a
    /// pencil, so it should behave like one.
    laid: usize,
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
            laid: 0,
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
    fn reached(&mut self, px: (f64, f64), slop: f64) {
        self.moved |= travelled(self.from_px, px, slop);
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

    /// How many cells this drag covers, without listing them. More than one is
    /// what makes it a drag rather than a click, and that has to be decided
    /// before the cells are priced -- otherwise a click that lands somewhere
    /// it may not build is refused in a drag's words.
    fn cell_count(&self, to: (i32, i32)) -> i64 {
        match self.stroke {
            hotbar::Stroke::Pencil => self.path.len() as i64,
            hotbar::Stroke::Rectangle => {
                let (rows, cols) = span(self.from, to);
                rows * cols
            }
        }
    }
}

/// Every cell on the line between two cells, both ends included.
///
/// Bresenham, which is what a pen tool does and what every raster editor
/// draws with: exactly one cell per step along the longer axis, stepping
/// sideways wherever the line does. **Connected, and never two cells thick.**
///
/// It replaced a rule that asked whether the pointer had passed through the
/// middle of a cell, and only marked it if so. That drew a clean diagonal —
/// at 45° the samples land on cell centres — and it fell apart at every other
/// angle: a shallow stroke enters most of the cells it crosses near their top
/// or bottom edge, so they were dropped and the line came out as scattered
/// dots. Measured at nine cells swept and three placed.
///
/// Filling in every cell any sample touches is the other failure and the
/// reason that rule existed: near a corner it catches the cells either side,
/// so a diagonal comes out thick. Bresenham is neither — it picks one cell per
/// step, so there is nothing to be thick with and nothing to fall through.
///
/// What is given up is drawing a shape with deliberate holes in one stroke: a
/// glider is five cells with gaps between them, and it now takes more than one
/// stroke or a few clicks. That is how a pen behaves, and a line you cannot
/// draw is worse than a glider you must lift the pen for.
fn line(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut row, mut col) = from;
    let dc = (to.1 - col).abs();
    let dr = -(to.0 - row).abs();
    let sc = if col < to.1 { 1 } else { -1 };
    let sr = if row < to.0 { 1 } else { -1 };
    let mut err = dc + dr;

    // One entry per step along the longer axis, so this is exactly as long as
    // the line and cannot run away even if the pointer jumps the screen.
    let mut out = Vec::with_capacity((dc.max(-dr) + 1) as usize);
    loop {
        out.push((row, col));
        if (row, col) == to {
            return out;
        }
        let twice = 2 * err;
        if twice >= dr {
            err += dr;
            col += sc;
        }
        if twice <= dc {
            err += dc;
            row += sr;
        }
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
    /// Menu or game. Everything the world does with input asks this first: a
    /// click that lands beside the menu panel must not draw on the world
    /// behind it.
    screen: Screen,
    /// When the room list was asked for, so a server that never answers
    /// becomes a message rather than a menu that says "asking" forever.
    asked_at: Option<f64>,
    /// Which room the server put us in, once it has said.
    ///
    /// Taken from the `Welcome` rather than from what was asked for: a client
    /// may have named no room at all, and the rejoin token is filed under this
    /// name, so a guess here is a token that comes back to the wrong world.
    /// `None` while offline, where there is no room to be in.
    room: Option<crate::net::RoomName>,
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

    /// Whether what the hotbar holds is already on this cell. Taking it away
    /// would change something, which is exactly what "already there" means.
    fn already_there(&self, row: i32, col: i32) -> bool {
        let placement = hotbar::SLOTS[self.slot].placement;
        let existing = self.world.cell_at(row, col).unwrap_or(crate::sim::Cell::DEAD);
        placement.remove_from(existing) != existing
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

    /// Step the world up to the generation the server is on.
    ///
    /// Normally exactly one step: the server sends one of these per
    /// generation, and a websocket does not lose or reorder them. Anything
    /// else means this client and the server disagree about where in the
    /// sequence they are, which is not something to paper over quietly — the
    /// worlds have already diverged, and the honest thing is to say so and
    /// take the server's number, because it is the one everybody else has.
    fn advance_to(&mut self, tick: crate::net::Tick) {
        let here = self.world.generation;
        if tick == here + 1 {
            self.world.step();
            return;
        }
        if tick > here && tick - here <= CATCH_UP {
            log::debug!("{} generations behind; catching up", tick - here);
            for _ in here..tick {
                self.world.step();
            }
            return;
        }
        log::warn!("out of step: the server is at {tick} and this client at {here}");
        self.world.set_generation(tick);
        self.world.dirty = true;
    }

    /// Drain the socket and fold what arrived into the local world.
    fn pump_link(&mut self) {
        let Some(link) = &mut self.link else { return };
        let messages = link.drain();
        let closed = link.is_closed();

        for msg in messages {
            match msg {
                ServerMessage::Welcome { you, tick, spawn, token, value, room, world } => {
                    // Kept first, before anything else can go wrong: the whole
                    // value of it is being able to come back, and a client that
                    // crashes on its first frame is exactly the case that needs
                    // to.
                    //
                    // Filed under the room the server says we are in, not the
                    // one that was asked for: a client may have named none, and
                    // a token stored against the wrong room brings you back to
                    // the wrong world.
                    crate::net::keep::store_token(&room, &token);
                    log::info!(
                        "joined room \"{room}\" as {you:?} at tick {tick}, in a {} world",
                        match world {
                            crate::sim::WorldKind::Infinite => "boundless".to_string(),
                            crate::sim::WorldKind::Toroidal { rows, cols } =>
                                format!("{rows}x{cols} wrapping"),
                        }
                    );
                    // Taken from the server, not assumed: a player coming
                    // back has a value already, and guessing the starting
                    // figure would have this client offering to spend money
                    // the server knows is gone.
                    self.value = value;
                    self.me = Some(you);
                    self.room = Some(room);
                    // Into the game. Not before: until a Welcome arrives there
                    // is no world to be in, and a menu that closed on the
                    // click would leave the player staring at ground they
                    // could not build on while the socket was still opening.
                    self.screen = Screen::Playing;
                    self.asked_at = None;
                    // Now, and only now, drop the local world. Until Welcome
                    // arrives there is nothing authoritative to replace it
                    // with, and an empty screen is worse than a local game.
                    //
                    // Built to the shape the server named. A client that
                    // assumed an infinite plane against a wrapping server
                    // folded no coordinates: chunks the server calls the same
                    // one were several to the client, digests were taken
                    // against coordinates it had never heard of, and the seam
                    // showed the moment anything crossed it. Nothing a client
                    // can see says whether the ground ends, so this is the only
                    // way it can know.
                    self.world = world.build();
                    // A birth's owner is seeded from the generation, so a
                    // client simulating at a different tick would make
                    // different choices from identical cells.
                    self.world.set_generation(tick);
                    self.subscribed.clear();
                    // Look at our own ground, which is the only place we may
                    // build. Derived rather than sent: `spawn_for` is the same
                    // function on both sides, so the client can work out where
                    // it was put without being told.
                    self.camera.centre = middle_of(spawn);
                    self.camera.dirty = true;
                }
                ServerMessage::Rejected { reason } => {
                    log::error!("server refused the connection: {reason}");
                    // Shown rather than logged and dropped. The refusal names
                    // the rooms that do exist, which with no other listing is
                    // the most useful thing on the screen -- and the link is
                    // kept, so the next choice is a click rather than a
                    // reconnect.
                    self.show_menu(menu::Stage::Failed(reason));
                    self.link.as_ref().inspect(|l| l.send(ClientMessage::Rooms));
                    self.asked_at = Some(self.elapsed);
                    return;
                }
                ServerMessage::Rooms { rooms } => {
                    log::info!("the server has {} room(s)", rooms.len());
                    self.asked_at = None;
                    if let Screen::Menu(m) = &mut self.screen {
                        // A refusal already on screen is carried over rather
                        // than replaced: the reason and the list of rooms that
                        // do exist are two halves of one answer, and a list
                        // arriving on its own reads as the click having done
                        // nothing.
                        let note = match std::mem::replace(&mut m.stage, menu::Stage::Idle) {
                            menu::Stage::Failed(why) => Some(why),
                            menu::Stage::Choosing { note, .. } => note,
                            _ => None,
                        };
                        m.stage = menu::Stage::Choosing { rooms, note };
                    }
                }
                ServerMessage::ChunkData { tick, chunk, cells } => {
                    match bytemuck::try_from_bytes::<crate::sim::Chunk>(&cells) {
                        Ok(c) => {
                            // The generation is not taken from here. A chunk
                            // reply and the step broadcast reach the socket by
                            // different routes, so a chunk can arrive from a
                            // tick either side of the one this client is on --
                            // and setting the clock from it without stepping
                            // would leave the world's state and its label
                            // disagreeing, quietly, for good. The step stream
                            // owns the clock; this only carries cells.
                            if tick != self.world.generation {
                                log::debug!(
                                    "chunk {chunk:?} is from tick {tick}, and this client is \
                                     on {}",
                                    self.world.generation
                                );
                            }
                            self.world.put_chunk(chunk, *c);
                        }
                        Err(e) => log::warn!("chunk {chunk:?} was the wrong size: {e}"),
                    }
                }
                ServerMessage::Step { tick, actions } => {
                    // Applied at the generation the server applied them at,
                    // then stepped to the generation it stepped to. Order and
                    // timing both matter: the step is a pure function of state
                    // and tick, so doing this a generation early or late is
                    // the same as doing something else.
                    for stamped in &actions {
                        crate::net::apply(&mut self.world, stamped);
                    }
                    self.advance_to(tick);

                    // Every so often, ask whether we still agree. Cheap
                    // enough to do often, and the sooner a divergence is found
                    // the less of the world has been built on top of it.
                    if self.world.generation % CHECKPOINT_EVERY == 0 {
                        self.send_checkpoint();
                    }
                }
                ServerMessage::Resync { tick, chunks } => {
                    log::warn!("desynced at tick {tick}; refetching {} chunks", chunks.len());
                    // Asked for again at once rather than left to the viewport
                    // to notice: a wrong chunk off screen is still wrong, and
                    // it will be back on screen eventually.
                    for c in &chunks {
                        self.subscribed.remove(c);
                    }
                    if let Some(link) = &self.link {
                        link.send(ClientMessage::Subscribe { chunks });
                    }
                }
            }
        }

        if closed {
            self.link = None;
            self.asked_at = None;
            match &self.screen {
                // Nothing was ever reached. The address is the likely reason
                // and the only thing the player can act on, so it is what the
                // message names.
                Screen::Menu(m) => {
                    let address = m.address.clone();
                    self.show_menu(menu::Stage::Failed(format!("no server answered at {address}")));
                }
                // Mid-game. The simulation is deterministic, so the world
                // carries on locally rather than stopping -- offline is a
                // solitary game, not a broken one.
                Screen::Playing => log::warn!("link closed; continuing offline"),
            }
            return;
        }

        if matches!(self.screen, Screen::Playing) {
            self.subscribe_to_view();
        }
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
        let (stamped, delta) = self.quote(cells, false);

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
        self.commit(&stamped);
        self.last_action = Some(format!("{shape}, {delta:+}"));
    }

    /// What a drag would lay, and how to describe it.
    ///
    /// One function for both shapes and for both callers, so the preview
    /// cannot draw one thing and the release lay another.
    fn drag_cells(&self, drag: &Drag, to: (i32, i32)) -> Result<(Vec<(i32, i32)>, String), String> {
        let name = hotbar::SLOTS[self.slot].name;
        let outside = |cells: &[(i32, i32)]| {
            cells
                .iter()
                .filter(|&&(r, c)| !crate::net::may_place(&self.world, self.player(), r, c))
                .count()
        };
        match drag.stroke {
            hotbar::Stroke::Pencil => {
                let stray = outside(&drag.path);
                if stray > 0 {
                    return Err(format!("{stray} of those cells are not your territory"));
                }
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
                let cells: Vec<(i32, i32)> = (r0..=r1)
                    .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
                    .collect();
                let stray = outside(&cells);
                if stray > 0 {
                    return Err(format!("{stray} of those cells are not your territory"));
                }
                Ok((cells, format!("laid {rows}x{cols} of {name}")))
            }
        }
    }

    /// Apply an action here, and send it if there is anyone to send it to.
    ///
    /// Applied straight away, connected or not, so what you draw appears under
    /// your hand rather than a quarter of a second later. The rules are
    /// deterministic and the server runs the same `net::apply`, so acting
    /// immediately shows the right answer a round trip early.
    ///
    /// Usually. The server applies it whenever the message lands, which is
    /// this generation if it arrives before the next step and the one after if
    /// it arrives later — so a click is a coin flip, and on the losing side
    /// this world has evolved those cells a generation earlier than the
    /// server's. That is what `Checkpoint` is for: the divergence is real,
    /// rare, and found by comparing digests rather than prevented by waiting.
    fn commit(&mut self, stamped: &Stamped) {
        crate::net::apply(&mut self.world, stamped);
        self.world.dirty = true;
        if let Some(link) = &self.link {
            link.send(ClientMessage::Act(stamped.clone()));
        }
    }

    /// Mark every cell the pointer passed through the middle of on its way
    /// here.
    ///
    /// Sampled along the segment rather than read off its ends, because
    /// pointer events arrive far apart when the hand moves quickly: a fast
    /// stroke crosses several cells between two of them, and a pencil that
    /// only marked where it was told would draw a dotted line. Sampling finely
    /// enough not to step over a collider, and then counting only the cells
    /// whose middle was actually crossed, is what draws a clean diagonal
    /// rather than a thick one.
    fn extend_stroke(&mut self, from_px: (f64, f64), to_px: (f64, f64)) {
        if !matches!(&self.gesture, Gesture::Drawing(d) if d.stroke == hotbar::Stroke::Pencil) {
            return;
        }
        // Straight from one reported position to the next, in cells. Sampling
        // the segment in pixels and asking what each sample was over is the
        // thing this replaced: how many samples to take is a guess, and every
        // answer to it is wrong at some angle or some zoom. A line between two
        // cells has no such parameter.
        let from = self.camera.cell_at(from_px);
        let to = self.camera.cell_at(to_px);
        for cell in line(from, to) {
            if let Gesture::Drawing(drag) = &mut self.gesture {
                if drag.full() {
                    return;
                }
                drag.mark(cell);
            }
        }
    }

    /// Lay down whatever the pencil has crossed since the last frame.
    ///
    /// Priced and refused a batch at a time, so a stroke that runs out of
    /// value stops where the money did rather than being refused whole. That
    /// is the natural reading of a pencil: you draw until you cannot.
    fn flush_stroke(&mut self) {
        let Gesture::Drawing(drag) = &self.gesture else { return };
        if drag.stroke != hotbar::Stroke::Pencil || !drag.moved || drag.laid == drag.path.len() {
            return;
        }
        let fresh: Vec<(i32, i32)> = drag.path[drag.laid..].to_vec();
        let stray = fresh
            .iter()
            .filter(|&&(r, c)| !crate::net::may_place(&self.world, self.player(), r, c))
            .count();

        if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.laid = drag.path.len();
        }
        if stray > 0 {
            self.notice = Some(format!("{stray} of those cells are not your territory"));
            return;
        }

        let (stamped, delta) = self.quote(fresh, false);
        if self.value + delta < 0 {
            self.notice = Some(format!("costs {}, you have {}", -delta, self.value));
            return;
        }
        self.notice = None;
        self.value += delta;
        self.commit(&stamped);
        self.last_action = Some(format!("drew {} of {}", -delta, hotbar::SLOTS[self.slot].name));
    }

    /// Tell the server what this client thinks it holds, so the two can find
    /// out cheaply whether they agree.
    ///
    /// A chunk is 512 bytes and its digest is eight, so a whole world's worth
    /// of state fits in a message that costs nothing to send — which is the
    /// point: agreement can be checked constantly, and only the chunks that
    /// actually disagree are ever sent back.
    ///
    /// Stamped with the generation the digests were taken at, because a chunk
    /// compared against the wrong tick disagrees for a reason that is not a
    /// bug. The server ignores a checkpoint from any tick but its own, so one
    /// that arrives late is skipped rather than answered wrongly, and the next
    /// one is only seconds away.
    fn send_checkpoint(&self) {
        let Some(link) = &self.link else { return };
        let chunks: Vec<(crate::sim::Coord, u64)> = self
            .world
            .stored()
            .iter()
            .filter_map(|&(coord, _)| Some((coord, self.world.chunk_digest(coord)?)))
            .take(MAX_CHECKPOINT_CHUNKS)
            .collect();
        if chunks.is_empty() {
            return;
        }
        link.send(ClientMessage::Checkpoint { tick: self.world.generation, chunks });
    }

    /// Price an action on these cells    /// Price an action on these cells: what would be sent, and what it costs.
    ///
    /// Shared by the click, by a drag, and by the preview of a drag, so the
    /// preview cannot promise something the release then refuses and a drag
    /// cannot be priced differently from the click it is made of.
    fn quote(&self, cells: Vec<(i32, i32)>, taking: bool) -> (Stamped, i32) {
        let placement = hotbar::SLOTS[self.slot].placement;
        let action = if taking {
            Action::Erase { cells, placement }
        } else {
            Action::Paint { cells, placement }
        };
        let stamped = Stamped { tick: self.world.generation, player: self.player(), action };
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
        // A stroke is laid as it is drawn, so the cells are their own preview.
        // A wash over the top of them would only say a second time what is
        // already on the board. A rectangle does not exist until it is
        // released, so it still needs showing.
        if drag.stroke == hotbar::Stroke::Pencil {
            return None;
        }
        let to = self.cell_under_cursor(self.cursor);
        let slot = &hotbar::SLOTS[self.slot];

        let (cells, label, allowed) = match self.drag_cells(drag, to) {
            Err(why) => (Vec::new(), why, false),
            Ok((cells, shape)) => {
                let (_, delta) = self.quote(cells.clone(), false);
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
        let name = hotbar::SLOTS[self.slot].name;

        let existing = self.world.cell_at(row, col).unwrap_or(crate::sim::Cell::DEAD);
        let placement = hotbar::SLOTS[self.slot].placement;
        let already_there = self.already_there(row, col);

        // Placing is confined to a player's own territory, which grows where
        // their life goes. Refused here on the same terms the server refuses
        // it, so the answer is instant rather than a round trip away.
        if !already_there && !crate::net::may_place(&self.world, player, row, col) {
            self.notice = Some(format!("({row}, {col}) is not your territory"));
            self.last_action = Some(format!("({row}, {col}) is not yours to build on"));
            return;
        }

        // Ice is not liftable. A pane stops time over whatever it covers, and
        // being able to take one back at will would make it cheap to undo as
        // well as strong to place -- what removes ice is life reaching it.
        // Said rather than silently ignored: a click that does nothing looks
        // exactly like a click that never arrived.
        if already_there && !placement.can_be_taken() {
            self.notice = Some(format!("{name} cannot be taken back; life shatters it"));
            self.last_action = Some(format!("{name} at ({row}, {col}) stays; only life breaks it"));
            return;
        }

        // Priced against the world as it stands, before the action changes it,
        // and refused here on the same terms the server would refuse it. Doing
        // it locally means the refusal is instant rather than a round trip
        // away, and the two cannot disagree because it is the same function.
        let (stamped, delta) = self.quote(vec![(row, col)], already_there);
        if self.value + delta < 0 {
            self.notice = Some(format!("costs {}, you have {}", -delta, self.value));
            return;
        }
        self.notice = None;
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
        self.commit(&stamped);
        log::debug!("clicked ({row}, {col}); value {}", self.value);
    }

    /// Whether input from the world should be acted on at all.
    fn playing(&self) -> bool {
        matches!(self.screen, Screen::Playing)
    }

    /// Back to the menu, in this state, keeping whatever was typed into it.
    ///
    /// Rebuilt from what is remembered when there was no menu to return to,
    /// which is the case of a game started from a command line being refused.
    fn show_menu(&mut self, stage: menu::Stage) {
        match &mut self.screen {
            Screen::Menu(m) => m.stage = stage,
            Screen::Playing => {
                let address = self.address_hint();
                let mut m = menu::Menu::new(address, cfg!(target_arch = "wasm32"));
                m.stage = stage;
                self.screen = Screen::Menu(m);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn address_hint(&self) -> String {
        Link::origin_url("/ws").unwrap_or_else(|| "ws://localhost:8080/ws".into())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn address_hint(&self) -> String {
        crate::net::keep::server().unwrap_or_else(|| DEFAULT_ADDRESS.into())
    }

    /// Act on what the menu was clicked for.
    fn chose(&mut self, chose: menu::Chose) {
        match chose {
            menu::Chose::Nothing => {}
            // The local world is already built and already granted -- `init`
            // does that whether or not anyone connects -- so this is a change
            // of screen and nothing else.
            menu::Chose::Offline => {
                log::info!("playing alone");
                self.screen = Screen::Playing;
            }
            menu::Chose::Connect(address) => {
                if let Screen::Menu(m) = &mut self.screen {
                    crate::net::keep::remember_name(&m.name);
                }
                crate::net::keep::remember_server(&address);
                log::info!("asking {address} what rooms it has");
                // Any previous socket goes first. Two links would both be
                // draining into one client, and the second Welcome would
                // arrive into a world built for the first.
                self.link = dial(&address);
                match &self.link {
                    Some(link) => {
                        link.send(ClientMessage::Rooms);
                        self.asked_at = Some(self.elapsed);
                        self.show_menu(menu::Stage::Asking);
                    }
                    None => self
                        .show_menu(menu::Stage::Failed(format!("{address} is not an address"))),
                }
            }
            menu::Chose::Join(room) => {
                let Some(link) = &self.link else {
                    self.show_menu(menu::Stage::Failed("the connection went away".into()));
                    return;
                };
                let name = match &self.screen {
                    Screen::Menu(m) => m.name.clone(),
                    Screen::Playing => "player".into(),
                };
                crate::net::keep::remember_name(&name);
                log::info!("joining room \"{room}\" as \"{name}\"");
                link.send(ClientMessage::Join {
                    token: crate::net::keep::token_for_join(Some(&room)),
                    name,
                    room: Some(room),
                });
            }
        }
    }

    /// Give up on a server that has not answered.
    ///
    /// A menu that says "asking" forever is indistinguishable from one that is
    /// broken, and the two most likely causes -- a wrong address, and a server
    /// that is not running -- both look exactly like this.
    fn time_out_room_list(&mut self) {
        let Some(asked) = self.asked_at else { return };
        if self.elapsed - asked < ROOM_LIST_TIMEOUT {
            return;
        }
        self.asked_at = None;
        self.link = None;
        let address = self.address_hint();
        self.show_menu(menu::Stage::Failed(format!("{address} did not answer")));
    }

    fn subscribe_to_view(&mut self) {
        let (min, max) = self.camera.visible_cells(VIEW_MARGIN);
        // Folded onto the chunks that actually exist before anything is asked
        // for. On a wrapping world the viewport runs off the edge and comes
        // back, so the same chunk is covered under several global coordinates
        // -- and a `Resync` names the folded one. Asking under the unfolded
        // name would subscribe several times to one chunk and then fail to
        // match the name the server used when it said that chunk was wrong.
        let mut wanted: Vec<_> = World::chunks_covering(min, max)
            .into_iter()
            .map(|c| self.world.canonical(c))
            .filter(|c| !self.subscribed.contains(c))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
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
        // Where to go, or whether to ask. A destination stated on a command
        // line or in a link is a choice already made; anything else opens the
        // menu, which is the only way a room can be chosen without a terminal.
        let (screen, link) = match startup() {
            Start::Join { url, name, room } => {
                log::info!("connecting to {url}, asking for room {room:?}");
                let link = dial(&url).inspect(|link| {
                    link.send(ClientMessage::Join {
                        name,
                        token: crate::net::keep::token_for_join(room.as_deref()),
                        room,
                    })
                });
                (Screen::Playing, link)
            }
            Start::Menu { address } => {
                (Screen::Menu(menu::Menu::new(address, cfg!(target_arch = "wasm32"))), None)
            }
        };
        // Always start with something on screen. Holding an empty world until
        // the server answers means a client that never connects -- wrong port,
        // server down, a page served from somewhere else -- shows nothing at
        // all and looks broken. A socket object exists long before it
        // connects, and may never connect, so its mere existence is no reason
        // to blank the view. `Welcome` is what replaces this.
        let mut world = chosen_world().build();
        if crate::net::too_cramped_for_grants(&world) {
            log::warn!("this world is too small for every player to get a square of their own");
        }
        // Placing is confined to a player's own territory, so an offline game
        // needs the grant a server would have made. Without it there is no
        // opening move: nothing is owned, so nothing may be placed, so nothing
        // ever comes to own anything.
        crate::net::grant(&mut world, PlayerId(1));
        // And look at it. Where a grant lands depends on the shape of the
        // world, so this is read back rather than assumed -- the same reason
        // `Welcome` carries the spawn for a connected client.
        let home = middle_of(crate::net::spawn_for(PlayerId(1), &world));
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
            "client ready: one {}x{} sprite sheet, chunk {}x{} cells, cell {} bytes",
            crate::render::atlas::SHEET_N,
            crate::render::atlas::SHEET_N,
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
            camera: camera::Camera::new(home, START_ZOOM),
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
            screen,
            asked_at: None,
            me: None,
            room: None,
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

        if self.playing() {
            self.apply_pan(dt);
            self.flush_stroke();
        }

        if self.link.is_some() {
            self.pump_link();
        }
        self.time_out_room_list();

        if let Some(Pending { drag, to_px }) = self.pending.take() {
            let to = self.cell_under_cursor(to_px);
            // More than one cell is what makes it a drag rather than a click.
            // A press that travelled but stayed inside one cell would place
            // where a click would take, so which of the two happens must not
            // turn on a few pixels of hand shake at high zoom.
            let laid_already = drag.stroke == hotbar::Stroke::Pencil && drag.laid > 0;
            if drag.moved && drag.cell_count(to) > 1 {
                if laid_already {
                    // Already down, cell by cell, as it was drawn.
                } else {
                    match self.drag_cells(&drag, to) {
                        Ok((cells, shape)) => self.lay(cells, shape),
                        Err(why) => self.notice = Some(why),
                    }
                }
            } else if !laid_already {
                self.click(to.0, to.1);
            }
        }

        // Only offline. Connected, the world advances when the server says a
        // generation happened, and never on this client's own clock -- see
        // `advance_to`.
        if self.link.is_none() {
            self.world.update(dt, GENERATION_SPAN);
        }
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
            room: self.room.as_deref(),
            world: self.world.kind(),
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
        let mut chose = menu::Chose::Nothing;
        // Taken out for the frame, because the closure needs `&mut` on it and
        // `self` is already borrowed by `views`. Put back below, whatever the
        // menu did with it.
        let mut screen = std::mem::replace(&mut self.screen, Screen::Playing);
        let on_web = cfg!(target_arch = "wasm32");
        let output = self.views.borrow_mut().run(gpu, self.elapsed, |ctx| match &mut screen {
            // The world is still drawn behind it, and still running if this
            // client is offline. A menu over a dead grey rectangle says the
            // game has not started; a menu over a world says it is waiting for
            // you.
            Screen::Menu(m) => {
                let (picked_menu, rect) = menu::show(ctx, &theme, m, on_web);
                chose = picked_menu;
                rect.into_iter().collect()
            }
            Screen::Playing => {
                overlay::show(ctx, &theme, &marks);
                let hud_rect = hud::show(ctx, &theme, &status);
                let bar = hotbar::show(ctx, &theme, slot);
                picked = bar.picked;
                // Each panel on its own. Folding them together first would
                // claim everything between them, and they sit in opposite
                // corners.
                [hud_rect, bar.rect].into_iter().flatten().collect()
            }
        });
        self.screen = screen;
        self.chose(chose);
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
        let was = self.cursor;
        let (dx, dy) = (x - was.0, y - was.1);
        self.cursor = (x, y);
        self.hovering = true;

        let slop = self.slop();
        if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached((x, y), slop);
            self.extend_stroke(was, (x, y));
        } else if self.is_panning() {
            self.camera.pan_by_pixels(dx, dy);
        }
    }

    fn on_key(&mut self, code: winit::keyboard::KeyCode, pressed: bool) {
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
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
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
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
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
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

        let was = self.cursor;
        self.cursor = at;
        self.touch_count = self.touches.len();
        let slop = self.slop();
        if matches!(phase, P::Started) {
            self.begin_drawing(at);
        } else if let Gesture::Drawing(drag) = &mut self.gesture {
            drag.reached(at, slop);
            self.extend_stroke(was, at);
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
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
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
            // In a browser a mouse wheel is pixels too, so the unit no longer
            // separates it from a trackpad and a wheel panned instead of
            // zooming. Two things separate them, and neither alone is enough:
            //
            // - A wheel is **purely vertical**. A trackpad swipe almost always
            //   carries some sideways drift, because fingers do.
            // - A wheel arrives as one large jump a notch, 100 or 120 in
            //   Chrome, where a swipe is a stream of small ones.
            //
            // And the number of notches is clamped, because being wrong must
            // stay cheap: a fast swipe read as a wheel then nudges the zoom
            // instead of leaping several levels a frame. Firefox is not
            // affected either way -- it reports lines for a wheel, which the
            // arm above takes.
            //
            // A heuristic, and named as one. `ctrl` is not: a pinch always
            // zooms, so there is a way to zoom that never guesses.
            D::PixelDelta(p)
                if cfg!(target_arch = "wasm32")
                    && p.x == 0.0
                    && p.y.abs() >= WHEEL_NOTCH =>
            {
                let notches = (p.y / WHEEL_NOTCH).clamp(-1.0, 1.0) as f32;
                self.zoom_about_cursor(1.15f32.powf(notches))
            }
            D::PixelDelta(p) => self.camera.pan_by_pixels(p.x, p.y),
        }
    }

    /// Left draws: a click acts on one cell, a drag fills the rectangle it
    /// swept. Middle, right and space+left all pan, so drawing and moving the
    /// view are never the same gesture and neither has to guess which was
    /// meant — and every mouse and trackpad has at least one of the three.
    fn on_click(&mut self, button: winit::event::MouseButton, pressed: bool) {
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
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

/// What the client does before the first frame: go somewhere, or ask.
enum Start {
    /// Straight into a game, because something said where to go — `--ws` on a
    /// command line, or `?room=` on a page. A stated destination is a choice
    /// already made, and asking again would be the menu getting in the way.
    Join { url: String, name: String, room: Option<String> },
    /// Show the menu, with this address filled in.
    Menu { address: String },
}

/// Connect, and ask nothing yet.
///
/// Two shapes because the two links differ: a browser's socket may fail to be
/// constructed at all, and a native one is a thread that starts and may then
/// find nothing there.
#[cfg(target_arch = "wasm32")]
fn dial(url: &str) -> Option<Link> {
    Link::connect(url)
}

#[cfg(not(target_arch = "wasm32"))]
fn dial(url: &str) -> Option<Link> {
    Some(Link::connect(url.to_string()))
}

/// On the web nothing needs configuring: the page came from the server, so the
/// server is wherever the page came from. `wss` when the page is `https`, or
/// the browser blocks it as mixed content.
///
/// The room comes from the query string — `?room=lobby` — because that is the
/// one part a page cannot derive from where it was served, and naming it is
/// how a link takes somebody straight to a world. With none, the menu asks.
#[cfg(target_arch = "wasm32")]
fn startup() -> Start {
    let url = Link::origin_url("/ws").unwrap_or_else(|| "ws://localhost:8080/ws".into());
    let name = crate::net::keep::name().unwrap_or_else(|| "web".into());
    match room_in_query(&query_string()) {
        Some(room) => Start::Join { url, name, room: Some(room) },
        None => Start::Menu { address: url },
    }
}

#[cfg(target_arch = "wasm32")]
fn query_string() -> String {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default()
}

/// On native there is no page to have come from, so the URL is an argument —
/// and without one, the menu asks for it.
#[cfg(not(target_arch = "wasm32"))]
fn startup() -> Start {
    let taken = CONNECTION.lock().unwrap().take();
    let Some(Connection { url, name, room }) = taken else {
        return Start::Menu { address: DEFAULT_ADDRESS.into() };
    };
    crate::net::keep::remember_name(&name);
    match url {
        Some(url) => Start::Join { url, name, room },
        None => Start::Menu { address: DEFAULT_ADDRESS.into() },
    }
}

/// What the native menu offers when nothing has been typed before. The server
/// this repository tells you to run, on the port it tells you to run it on.
#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_ADDRESS: &str = "ws://127.0.0.1:8080/ws";

/// The `room` parameter out of a query string, given the string.
///
/// Reached only from the browser's startup, so off wasm32 nothing but the test
/// below calls it — which is the point of the split, and why the allow is
/// narrower than silencing the warning at the module.
///
/// Split from the lookup above so it can be tested at all: everything that
/// reaches a browser's `location` is unreachable off wasm32, and this is the
/// half with the decisions in it.
///
/// Parsed by hand rather than through `UrlSearchParams`, which would be
/// another web-sys feature for one lookup. No percent-decoding, deliberately:
/// a room name is letters, digits, `-` and `_`, so a name that needed decoding
/// was never a room name, and refusing it here would say only "no" where the
/// server can say what the rooms actually are.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn room_in_query(search: &str) -> Option<String> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "room")
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.is_empty())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// How a browser client says which world it wants. There is no command
    /// line on a page, and the socket comes from the origin, so the query
    /// string is the only thing left to carry it.
    #[test]
    fn the_room_comes_out_of_the_query_string() {
        assert_eq!(room_in_query("?room=lobby").as_deref(), Some("lobby"));
        assert_eq!(room_in_query("room=lobby").as_deref(), Some("lobby"), "with or without the ?");
        assert_eq!(
            room_in_query("?name=alice&room=arena&zoom=4").as_deref(),
            Some("arena"),
            "and wherever it sits among the others"
        );

        // Nothing to say means the server decides, which is what keeps a bare
        // URL a game.
        for none in ["", "?", "?room=", "?rooms=lobby", "?roomy=lobby", "?name=alice"] {
            assert_eq!(room_in_query(none), None, "{none:?}");
        }

        // Not validated here. A name that is not one goes to the server,
        // which refuses it with the list of rooms that do exist -- and that
        // list is the only way a player finds out what is there.
        assert_eq!(room_in_query("?room=NOT A ROOM").as_deref(), Some("NOT A ROOM"));
    }

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
        let mut drag = Drag::begin((100.0, 100.0), (0, 0), hotbar::Stroke::Rectangle);
        for at in [(102.0, 100.0), (98.0, 101.0), (100.0, 98.0), (101.0, 101.0)] {
            drag.reached(at, DRAG_SLOP);
        }
        assert!(!drag.moved);
    }

    /// Once a press is a drag it stays one. Coming back to where it started
    /// mid-sweep must not turn the gesture back into a click.
    #[test]
    fn a_drag_does_not_become_a_click_again() {
        let mut drag = Drag::begin((100.0, 100.0), (0, 0), hotbar::Stroke::Rectangle);
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
        assert!(span((0, 0), (i32::MAX, i32::MAX)).0 > MAX_DRAG_CELLS);
    }

    /// A cell is only marked when the pointer passes through the middle of
    /// it, and the gap around that middle is what lets a diagonal be drawn.
    ///
    /// Filling in every cell between one position and the next caught both
    /// cells either side of a corner, so a diagonal came out thick and a shape
    /// with holes in it could not be drawn at all.
    #[test]
    fn a_stroke_is_unbroken_at_every_angle() {
        // The bug this replaced: a shallow sweep placed three of the nine
        // cells it crossed, because the pointer entered most of them near an
        // edge rather than through the middle. A pen tool draws a line.
        let shallow = line((0, 0), (2, 9));
        assert_eq!(shallow.len(), 10, "one cell per column: {shallow:?}");
        assert_eq!(
            shallow.iter().map(|&(_, c)| c).collect::<Vec<_>>(),
            (0..=9).collect::<Vec<_>>(),
            "and no column skipped"
        );

        // Connected at every angle, and one cell thick at every angle.
        for &to in &[
            (9, 0),
            (0, 9),
            (9, 9),
            (2, 9),
            (9, 2),
            (-7, 4),
            (4, -7),
            (-6, -6),
            (0, 0),
        ] {
            let drawn = line((0, 0), to);
            assert_eq!(drawn.first(), Some(&(0, 0)), "{to:?} starts where the pen did");
            assert_eq!(drawn.last(), Some(&to), "{to:?} ends where the pen did");
            for pair in drawn.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let (dr, dc) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
                assert!(dr <= 1 && dc <= 1 && (dr + dc) > 0, "{to:?}: {a:?} to {b:?} is a jump");
            }
            let steps = (to.0.abs()).max(to.1.abs()) as usize + 1;
            assert_eq!(drawn.len(), steps, "{to:?}: one step per cell of the longer axis");
        }
    }

    /// A 45-degree stroke is a clean diagonal with nothing beside it. Filling
    /// in every cell a sample touched made this two cells thick near the
    /// corners, which is the other way to get it wrong.
    #[test]
    fn a_diagonal_sweep_marks_a_diagonal() {
        assert_eq!(
            line((0, 0), (4, 4)),
            vec![(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)],
        );
        // Negative coordinates behave the same: the world has no origin.
        assert_eq!(
            line((0, 0), (-3, -3)),
            vec![(0, 0), (-1, -1), (-2, -2), (-3, -3)],
        );
    }

    /// A stroke that crosses itself must list each cell once. The pricing
    /// compares every entry against the world rather than against the entries
    /// before it, so a repeat would be charged for twice and laid once.
    #[test]
    fn a_stroke_that_crosses_itself_lists_each_cell_once() {
        let mut drag = Drag::begin((0.0, 0.0), (0, 0), hotbar::Stroke::Pencil);
        // Out along a row, back along it, and out again.
        for col in (0..=6).chain((0..=5).rev()).chain(1..=6) {
            drag.mark((0, col));
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
        for col in 0..MAX_DRAG_CELLS as i32 * 2 {
            if drag.full() {
                break;
            }
            drag.mark((0, col));
        }
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

/// The middle of a granted patch, as the camera wants it: (x, y), which is
/// (col, row) the other way round.
fn middle_of((row, col): (i32, i32)) -> (f32, f32) {
    let half = crate::net::SPAWN_N as f32 / 2.0;
    (col as f32 + half, row as f32 + half)
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
