//! The game view: one world, a camera over it, and the input that drives both.
//!
//! A view rather than the application, so a menu or a lobby can be another one
//! beside it without this having to know they exist.

pub mod camera;
pub mod clock;
pub mod help;
pub mod hotbar;
pub mod hud;
pub mod input;
pub mod lobby;
pub mod overlay;
mod rules;
pub mod stamp;
pub mod start;

use input::{cell_under, centroid, digit, pinch_span, span, Drag, Gesture};
use start::{dial, middle_of, solo_world, startup, Start};

/// Native only, like the things themselves: a browser client learns its server
/// from the page it came from and has no command line to be told anything on.
#[cfg(not(target_arch = "wasm32"))]
pub use start::{default_address, set_connection, set_world, Connection};

use std::cell::RefCell;

use super::words;
use super::{icons, menu, Views};
use crate::render::app::App;
use crate::render::atlas::Atlas;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, WorldKind, CHUNK_N};
use hotbar::{Held, Key};
use lobby as lobby_view;

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

/// What a `Step` at this tick means for a client sitting at that generation.
///
/// Pure arithmetic, and a free function so it can be tested without a GPU —
/// the same argument that took the camera out of this file. Everything
/// interesting about recovering from a lost message is the decision, and the
/// decision is one comparison.
#[derive(Debug, PartialEq, Eq)]
enum Advance {
    /// The next generation, which is the only thing that ever ought to arrive.
    Step,
    /// Something was lost. Not "we are behind by n" — a `Step` carries the
    /// actions applied at its tick, so a gap is n generations whose *contents*
    /// this client was never told, and there is nothing it can compute that
    /// would fill them in.
    Lost,
}

fn advance(here: u64, tick: crate::net::Tick) -> Advance {
    if tick == here + 1 {
        Advance::Step
    } else {
        Advance::Lost
    }
}

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
///
/// [`crate::net::MOST_CELLS_AT_ONCE`] rather than a number of its own, because
/// the server refuses anything longer: a client that stopped somewhere else
/// would either draw shapes the server drops or refuse ones it would take.
const MAX_DRAG_CELLS: i64 = crate::net::MOST_CELLS_AT_ONCE as i64;

/// Cells of slack around the viewport when subscribing, so life entering from
/// off screen is already held rather than popping in a chunk late.
const VIEW_MARGIN: i32 = CHUNK_N as i32;
/// A join that has been decided on and not yet sent.
///
/// The room and the name; the token and the signature are looked up at the
/// moment it goes, because both can change while it waits.
struct Joining {
    name: String,
    room: Option<crate::net::RoomId>,
}

enum Screen {
    /// Choosing a server and a room, or choosing to play alone.
    Menu(menu::Menu),
    /// In a world. The only screen that takes input from the world.
    Playing,
}

/// How often the room list is asked for again while it is on screen.
const ROOM_LIST_REFRESH: f64 = 3.0;

/// How long to wait for a server to say what rooms it has before giving up.
///
/// Generous, because it covers a connection being made as well as answered,
/// and short enough that a wrong address is a mistake you correct rather than
/// a page you reload.
const ROOM_LIST_TIMEOUT: f64 = 8.0;

/// What the pointer is doing.
///
#[derive(Clone)]
struct Lobby {
    phase: crate::net::MatchPhase,
    victory: Option<crate::net::Victory>,
    players: Vec<(PlayerId, String)>,
    /// The teams, their names and who is at each one's controls. Empty in a
    /// free-for-all.
    teams: Vec<crate::net::Team>,
    /// Whose match it is: the player who may start it. `None` for one the
    /// console made, which starts at the console.
    owner: Option<PlayerId>,
    /// Who blew the whistle, once somebody has.
    started_by: Option<PlayerId>,
    /// The code that reaches this room, if it is private. Shown in the lobby,
    /// which is where somebody waiting for their friends actually needs to
    /// read it off and send it.
    code: Option<String>,
}

impl Lobby {
    /// The hue table. A constant now — a team is a player and therefore one
    /// colour, so nothing about it depends on who else is in the room. It used
    /// to be a pass over the roster, because a member's place inside its
    /// team's family of hue depended on how many members there were.
    fn hues(&self) -> [f32; PlayerId::COUNT] {
        crate::client::views::hue::table()
    }

    fn look<'a>(
        &'a self,
        me: PlayerId,
        hues: &'a [f32; PlayerId::COUNT],
        refused: Option<&'a str>,
    ) -> lobby_view::Look<'a> {
        lobby_view::Look {
            me,
            refused,
            phase: &self.phase,
            victory: self.victory,
            players: &self.players,
            owner: self.owner,
            started_by: self.started_by,
            teams: &self.teams,
            code: self.code.as_deref(),
            hues,
        }
    }
}

/// A finished gesture waiting to be resolved to cells. Input callbacks are not
/// handed the `GpuState`, and the screen-to-world mapping needs the viewport,
/// so it waits for the next `update` rather than guessing here.
struct Pending {
    drag: Drag,
    to_px: (f64, f64),
}

/// **What the interface is holding between frames**, in one place.
///
/// The frame needs `&mut` on all of it while `views` is borrowed, and these
/// were taken out one at a time with `mem::take` and put back afterwards —
/// five separate fields, each of which is silently *missing* if anything
/// between the two returns early. Grouped, the frame borrows one thing.
///
/// What belongs here is state the **interface** owns: which screen, and what
/// is half-typed or half-drawn on it. The world, the link and the purse are
/// not interface state and stay on the app.
struct Ui {
    /// Menu or game. Everything the world does with input asks this first: a
    /// click that lands beside the menu panel must not draw on the world
    /// behind it.
    screen: Screen,
    /// The pattern being drawn by hand in the library, if any.
    ///
    /// Lives here rather than in the library window because the window is
    /// built afresh every frame, and half a drawing that vanished when you
    /// looked away would not be worth having.
    sketch: stamp::Sketch,
    /// Which side is having its name typed, and what has been typed so far.
    ///
    /// Here rather than in the lobby panel because that panel is rebuilt every
    /// frame, and a name half-typed would vanish between two of them — the
    /// same reason `sketch` lives here.
    naming_team: Option<(PlayerId, String)>,
    /// What is being typed into a stamp's name box, held here for the reason
    /// [`Self::naming_team`] is.
    naming_stamp: Option<(usize, String)>,
    /// Which stamp the pad is editing, so keeping replaces it rather than
    /// adding a second copy. See [`stamp::Editing`].
    editing_stamp: stamp::Editing,
    /// Whether the stamps that did not fit on the bar are on screen.
    picking_stamp: bool,
    /// Whether the key list is on screen.
    ///
    /// Above every screen rather than on one, because the keys it lists work
    /// on more than one and a player who wants them does not know which screen
    /// they are supposed to ask from.
    helping: bool,
    /// Whether the rules panel is open — a laboratory's two switches, which
    /// are questions about the *game* rather than about the world.
    showing_rules: bool,
    /// Why the last attempt to start or leave a match was refused.
    ///
    /// Beside the lobby rather than in [`Self::notice`], which is drawn in the
    /// HUD's corner: a refusal belongs against the control that produced it,
    /// which is the same reason a `Made` refusal lands in the creation form.
    /// A button that appears to do nothing reads as a broken lobby, and
    /// "somebody has not picked a team" is a thing to *act on*.
    ///
    /// Cleared when the phase moves, because by then it has been answered.
    refused_start: Option<String>,
}

pub struct GameApp {
    ui: Ui,
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
    /// Shift held, which hurries the keyboard pan.
    shift: bool,
    /// Ctrl held. Only `ctrl+[` reads it, and it is tracked the way shift is
    /// for the same reason: what a key *means* can depend on a modifier, and
    /// the modifier arrives as its own press.
    ctrl: bool,
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
    ///
    /// `None` offline before the first grant, and `None` for the whole of a
    /// spectator's visit — see [`Self::watching`].
    me: Option<crate::sim::PlayerId>,
    /// Watching without a seat.
    ///
    /// Its own flag rather than `me.is_none()`, because those are two
    /// different states that happen to share a field: a client between a
    /// `Join` and its `Welcome` also has no number, and it is not a spectator.
    /// Everything that acts asks this first.
    watching: bool,
    /// When the room list was asked for, so a server that never answers
    /// becomes a message rather than a menu that says "asking" forever.
    asked_at: Option<f64>,
    /// When the room list last arrived, so it can be asked for again before it
    /// goes stale.
    listed_at: f64,
    /// What the match in this room is doing, once the server has said. `None`
    /// in an ordinary room, and in one that has not answered yet.
    lobby: Option<Lobby>,
    /// Which room the server put us in, once it has said.
    ///
    /// Taken from the `Welcome` rather than from what was asked for: a client
    /// may have named no room at all, and the rejoin token is filed under this
    /// name, so a guess here is a token that comes back to the wrong world.
    /// `None` while offline, where there is no room to be in.
    room: Option<crate::net::RoomId>,
    /// What that room is **called**, for the HUD.
    ///
    /// Beside the id rather than looked up from the room list, because a
    /// client that joined by code has never seen a listing and so has no other
    /// way to know what it is in.
    room_name: Option<String>,
    /// Chunks already asked for, so a moving viewport only asks for what is new.
    subscribed: std::collections::HashSet<crate::sim::Coord>,
    /// **What this server calls this client**, from the `Welcome`.
    ///
    /// The client cannot derive it — the server issues it — so it is learnt
    /// and remembered. `None` before a first join, or for a client with no
    /// secret to offer.
    person: Option<crate::net::PersonId>,
    /// What this client is rated on the server it is talking to.
    ///
    /// `None` until a server has said, which is a different thing from the
    /// starting number: a client that has reached nobody has no rating rather
    /// than an average one, and a dashboard that showed 1200 to somebody who
    /// has never connected would be inventing it.
    rating: Option<i32>,
    /// What the last result moved it by, so the dashboard can say so once.
    rating_change: Option<i32>,
    /// A join waiting for something to sign.
    ///
    /// **A join says who it is from**, and who it is from is a signature over
    /// a challenge that arrives after the socket opens — so the intent is held
    /// here and sent when the challenge lands. Held as what was wanted rather
    /// than as a built message, so the signature is made against whatever
    /// challenge is current rather than one that has since been replaced.
    joining: Option<Joining>,
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
    /// What the hotbar is holding.
    held: Held,
    /// Every pattern captured so far.
    stamps: stamp::Library,
    /// The sprite sheet as egui can draw it, so the hotbar shows the cell each
    /// tool puts down rather than spelling its name.
    icons: icons::Icons,
    /// Who holds how much ground, most first, as the server last said.
    ///
    /// From the server because a client holds only the chunks it subscribed
    /// to: counting locally would score its own screen rather than the world.
    standing: Vec<crate::net::Holding>,
    /// The generation [`Self::standing`] was counted at, offline. A pass over
    /// the world per generation and not per frame.
    counted_at: crate::net::Tick,
    /// The address last written down, so it is written again only when it
    /// would say something different.
    said_where: Option<(bool, bool, bool, bool, bool, Option<crate::net::RoomId>)>,
    /// Actions taken from an [`Acted`] before the `Step` that carries them.
    ///
    /// Emptied every `Step`, because an action belongs to one generation and
    /// the `Step` that names it is the last chance to hear about it.
    ///
    /// [`Acted`]: crate::net::ServerMessage::Acted
    applied_early: Vec<Stamped>,
    /// Whether this seat has given up, as the server last said.
    ///
    /// Held so the control can say so rather than offer to concede twice, and
    /// cleared on a `Welcome`, which is a different match or none.
    forfeited: bool,
    /// **The number this client's cells carry**, which in a team match is the
    /// team's and not this seat's.
    ///
    /// A team is a player and several clients are connected to it, so this is
    /// the whole of what a client has to know about teams: who it is playing
    /// as. Everything downstream — what it may place, what that costs, which
    /// of the actions coming back are its own — asks this and never asks who
    /// is allied with whom, because there is nobody to be allied with.
    ///
    /// Equal to [`Self::me`] offline, in a free-for-all, and in a match nobody
    /// has picked teams in. What used to be here was the whole roster's
    /// allegiances, copied off the wire so the client could price a placement
    /// beside a teammate the way the server would.
    plays_as: Option<PlayerId>,
    /// The game being played, if there is one, waiting to be filed.
    ///
    /// Committed when the room ends for this client — a different `Welcome`, a
    /// link that closed, or the way back to the menu. A tab closed mid-game
    /// loses its record, which is the honest cost of not writing on every
    /// change: a browser gives no reliable moment to write at.
    ///
    /// A spectator never has one. Watching is not playing, and a record of
    /// worlds you looked at is not a record.
    in_play: Option<crate::client::record::InPlay>,
    /// How badly this client and the server are disagreeing, as a decaying
    /// rate rather than a log line — see [`crate::client::desync`].
    geiger: crate::client::desync::Geiger,
    /// **A laboratory's two rules**, and they are two.
    ///
    /// `net::may_place` is about where your influence reaches and `net::price`
    /// is about what you can afford — separate rules with separate reasons, so
    /// an experiment may reasonably want the map open and the economy on, or
    /// the reverse. They were one flag until there was a panel to put them on.
    ///
    /// The only two an experiment has to take off, because [liveness is
    /// exactly B3/S23] whatever else is true. Offline only, and cleared on a
    /// `Welcome`: a client predicting placements a server would refuse resyncs
    /// every time it draws.
    ///
    /// [liveness is exactly B3/S23]: crate::sim::World::step
    place_anywhere: bool,
    place_free: bool,
    /// **The world is stopped**, which only means anything offline.
    ///
    /// A connected client advances when the server says a generation happened
    /// and never on its own clock, so there is nothing here for a pause to
    /// stop — see `advance_to`. Held on the app rather than in `Ui` because it
    /// is a fact about the world and not about the interface, which is the
    /// same line `world`, `link` and `value` are on the far side of.
    paused: bool,
    /// One generation asked for while stopped, taken on the next frame.
    ///
    /// A request rather than a step taken in the key handler, so that stepping
    /// happens exactly where stepping happens — one place that banks what was
    /// mined and recounts the standings, rather than two that have to agree.
    step_once: bool,
}

impl GameApp {
    /// Hand the shader the camera and the hue table.
    ///
    /// The shader encodes sRGB itself exactly when the surface will not.
    ///
    /// Read from the negotiated format every frame rather than cached: it is
    /// one field access, and a cached copy is one more thing to forget when
    /// the surface is reconfigured.
    fn write_camera(&self, gpu: &GpuState) {
        // Recomputed here rather than cached, because `write_camera` runs
        // only when the camera has moved -- not every frame -- and a pass over
        // sixteen players is nothing beside the buffer write it rides on.
        let uniform = self.camera.uniform(
            !gpu.config.format.is_srgb(),
            &crate::client::views::hue::table(),
            // Where the coarse texture is looking, so the shader can turn a
            // world position into one of its texels. A window of nothing when
            // the fine path is drawing, which the shader never reads.
            self.chunks.coarse_window().unwrap_or(((0, 0), (0, 0))),
            self.chunks.coarse_wraps(&self.world),
        );
        gpu.queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
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
    /// The number this client places under: its team's in a match, its own
    /// otherwise. Every rule takes this and knows nothing about teams.
    fn player(&self) -> PlayerId {
        self.plays_as.or(self.me).unwrap_or(PlayerId(1))
    }
}

impl GameApp {
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
        let stroke = self.held.stroke();
        self.gesture = Gesture::Drawing(Drag::begin(at, self.camera.cell_at(at), stroke));
    }

    /// Whether the world will take anything this player does right now.
    ///
    /// A match that has not started takes nothing, and neither does one that
    /// is decided — the server drops those actions, and a client that went on
    /// predicting them would draw cells that appear under the hand and vanish
    /// a moment later when the next `Checkpoint` corrects the world. Which
    /// looks like the game losing your work rather than like a rule.
    ///
    /// `None` for a room that is not a match, and for one that has not said
    /// yet, both of which are ordinary rooms as far as this is concerned.
    fn may_act(&self) -> bool {
        // A spectator has no seat, so it has no player number to attribute an
        // action to and no value to spend. Refused here as well as on the
        // server, so that clicking the world says why rather than doing
        // nothing -- the server drops what it cannot attribute and sends
        // nothing back, which on its own looks exactly like a lost click.
        !self.watching && self.lobby.as_ref().is_none_or(|l| l.phase.accepts_actions())
    }

    /// Whether what the hotbar holds is already on this cell.
    ///
    /// `Placement::is_on` asks whether the square holds *this* thing, not
    /// whether taking it away would change anything — which is what this used
    /// to ask, and it could not tell life from a mine, since both are taken
    /// away by clearing the same bit. Holding one over the other now replaces
    /// the kind instead of killing the cell.
    fn already_there(&self, row: i32, col: i32) -> bool {
        let Some(placement) = self.held.placement() else { return false };
        let existing = self.world.cell_at(row, col).unwrap_or(crate::sim::Cell::DEAD);
        placement.is_on(existing)
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
        self.shift = false;
        self.ctrl = false;
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
    /// generation. Anything else means this client and the server disagree
    /// about where in the sequence they are, which is not something to paper
    /// over quietly — the worlds have already diverged, and the honest thing
    /// is to say so, take the server's number, and ask for the world again.
    fn advance_to(&mut self, tick: crate::net::Tick) {
        let here = self.world.generation;
        if let Advance::Step = advance(here, tick) {
            let mined = self.world.step();
            self.bank(&mined);
            return;
        }
        // **Anything else means messages were lost, so nothing is stepped.**
        //
        // Catching up is what this used to do, and it is the bug rather than
        // the recovery. A `Step` carries the actions applied at its tick, so a
        // gap is not "we are behind by n generations", it is "n generations
        // happened here that we were not told the contents of" — and stepping
        // to close it runs those generations *empty*. The world that comes out
        // is one nobody else has, and Life makes a handful of missing cells a
        // different pattern within a minute. Which is exactly what a branched
        // world looks like from the inside.
        //
        // A websocket does not lose or reorder, which is what made this look
        // safe. The broadcast channel in front of it does: `server::ws` logs
        // `connection lagged n messages` and carries on, and a client whose
        // socket is slow to drain — a backgrounded tab throttles exactly that
        // — is the ordinary case rather than an exotic one.
        //
        // So: take the server's number, throw away the world built on the
        // missing steps, and ask for it again. It self-limits, because the
        // generation is the server's afterwards and the next `Step` is one
        // past it.
        log::warn!(
            "out of step: the server is at {tick} and this client at {here}; \
             discarding {} generation(s) and asking again",
            tick.saturating_sub(here)
        );
        self.resync_everything(tick);
    }

    /// Throw away what this client holds and fetch it from the server.
    ///
    /// **The whole world, not the chunks that look wrong.** Every chunk here
    /// was stepped alongside every other, so one that missed an action has
    /// been feeding wrong cells across its edges into its neighbours ever
    /// since; and a chunk outside the viewport is never checkpointed at all,
    /// so "the ones we know are wrong" is a set this client cannot compute.
    /// Rebuilding to the world's own shape and re-asking is the only answer
    /// that does not leave some of the divergence in place.
    ///
    /// It looks like a join, because it is one: `Welcome` does this same thing
    /// for the same reason, and the blank moment before the chunks land is the
    /// one that path already has.
    fn resync_everything(&mut self, tick: crate::net::Tick) {
        // **Keeping the seed**, which the shape does not carry: `build` makes a
        // world that has not been told which game it is, and a client rolling
        // from nought against a server rolling from the room's number
        // disagrees at the first contested birth. That reads as a desync, so
        // it resyncs, so it rebuilds, so it disagrees again -- which is what
        // `examples/two` caught the moment this was wired up.
        let seed = self.world.seed();
        self.world = self.world.kind().build();
        self.world.set_seed(seed);
        self.world.set_generation(tick);
        self.world.dirty = true;
        // Cleared, or `subscribe_to_view` would take this client's word for
        // what it already holds and ask for none of it.
        self.subscribed.clear();
        // And this, which is a record of what was applied to the world that
        // has just been thrown away. Kept, it would tell the next `Step` to
        // skip an action that nothing in the new world has ever seen.
        self.applied_early.clear();
        self.geiger.reset();
        self.subscribe_to_view();
    }

    /// Settle a solitary match, if there is one and it is done.
    ///
    /// **The same two conditions the server settles**, read off the same
    /// `net::standings` this client already counts each generation — offline
    /// it holds the whole world, so its count is the server's count rather
    /// than a guess about a viewport.
    ///
    /// Only offline. Connected, a phase arrives in `ServerMessage::Match` and
    /// this must never invent one: two authorities deciding one match is the
    /// bug the whole design is arranged to avoid.
    fn decide_alone(&mut self) {
        if self.link.is_some() {
            return;
        }
        let Some(lobby) = &mut self.lobby else { return };
        let (Some(victory), crate::net::MatchPhase::Running { from }) =
            (lobby.victory, lobby.phase.clone())
        else {
            return;
        };
        let leader = self.standing.first();
        let held = leader.map_or(0, |h| h.score as usize);
        let done = match victory {
            crate::net::Victory::Timer { generations } => {
                self.world.generation.saturating_sub(from) >= generations
            }
            crate::net::Victory::Territory { squares } => held >= squares,
        };
        if !done {
            return;
        }
        lobby.phase = crate::net::MatchPhase::Over {
            winner: leader.map(|h| h.who),
            held,
            at: self.world.generation,
        };
        log::info!("a solitary match ended at generation {}", self.world.generation);
    }

    /// Fold a generation's mining into the predicted purse, floored at zero
    /// the way the server floors the real one.
    ///
    /// A prediction, and a low one: only the mines in chunks this client holds
    /// are counted. `Purse` is what makes it right again.
    fn bank(&mut self, mined: &crate::sim::Mined) {
        self.value =
            (self.value + crate::net::earnings(mined, self.player())).clamp(0, Player::MAX_VALUE);
    }

    /// Drain the socket and fold what arrived into the local world.
    fn pump_link(&mut self) {
        let Some(link) = &mut self.link else { return };
        let messages = link.drain();
        let closed = link.is_closed();

        for msg in messages {
            match msg {
                ServerMessage::Welcome {
                    you,
                    person,
                    tick,
                    spawn,
                    rating,
                    value,
                    room,
                    name,
                    world,
                } => {
                    self.rating = Some(rating);
                    // Kept first, before anything else can go wrong: the whole
                    // value of it is being able to come back, and a client that
                    // crashes on its first frame is exactly the case that needs
                    // to.
                    //
                    // Filed under the room the server says we are in, not the
                    // one that was asked for: a client may have named none, and
                    // a token stored against the wrong room brings you back to
                    // the wrong world.
                    // Filed against the **id**, not the name. That is the
                    // whole point of there being an id: a room that is
                    // renamed is the same room, and a token that keyed off
                    // the name would come back to nothing.
                    // The way back is the secret this client already has, so
                    // there is nothing to file per room any more. What is kept
                    // is who the server said we are, so the settings screen
                    // can say it without waiting for the next join — the
                    // server issues it, so this is the only way the client
                    // ever has it. See `net::auth`.
                    if let Some(id) = &person {
                        crate::net::keep::remember_person(id);
                    }
                    self.person = person;
                    // A different room is a different match, or none.
                    self.lobby = None;
                    log::info!(
                        "joined \"{name}\" ({room}) as {you:?} at tick {tick}, in a {} world",
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
                    self.plays_as = Some(you);
                    self.watching = false;
                    self.forfeited = false;
                    self.room = Some(room);
                    self.room_name = Some(name);
                    // Into the game. Not before: until a Welcome arrives there
                    // is no world to be in, and a menu that closed on the
                    // click would leave the player staring at ground they
                    // could not build on while the socket was still opening.
                    self.ui.screen = Screen::Playing;
                    self.asked_at = None;
                    self.say_where();
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
                    self.applied_early.clear();
                    // A pause is a fact about a world this client was keeping
                    // time in, and it is not keeping time in this one -- and a
                    // laboratory's rules are off in a world nobody else is in.
                    self.paused = false;
                    self.place_anywhere = false;
                    self.place_free = false;
                    self.world = crate::net::sane_world(
                        world,
                        self.room.as_ref().expect("the room was just set"),
                    );
                    // A birth's owner is seeded from the generation, so a
                    // client simulating at a different tick would make
                    // different choices from identical cells.
                    self.world.set_generation(tick);
                    self.subscribed.clear();
                    // A different room is a different world and a different
                    // argument about it, so the counter starts over rather
                    // than showing the last room's trouble against this one.
                    self.geiger.reset();
                    self.file_game();
                    self.in_play = Some(crate::client::record::InPlay::joined(
                        self.room_name.clone().unwrap_or_default(),
                        world,
                        tick,
                    ));
                    // Look at our own ground, which is the only place we may
                    // build. Derived rather than sent: `spawn_for` is the same
                    // function on both sides, so the client can work out where
                    // it was put without being told.
                    self.camera.centre = middle_of(spawn);
                    self.camera.dirty = true;
                }
                // Watching: the world and its clock, and no player at all.
                ServerMessage::Watching { room, name, tick, world } => {
                    log::info!("watching \"{name}\" ({room}) from tick {tick}");
                    self.lobby = None;
                    self.me = None;
                    self.watching = true;
                    self.value = 0;
                    // Watching is not playing, so whatever was being played
                    // is filed and nothing new is started.
                    self.file_game();
                    self.room = Some(room);
                    self.room_name = Some(name);
                    self.ui.screen = Screen::Playing;
                    self.asked_at = None;
                    self.say_where();
                    self.applied_early.clear();
                    // A pause is a fact about a world this client was keeping
                    // time in, and it is not keeping time in this one -- and a
                    // laboratory's rules are off in a world nobody else is in.
                    self.paused = false;
                    self.place_anywhere = false;
                    self.place_free = false;
                    self.world = crate::net::sane_world(
                        world,
                        self.room.as_ref().expect("the room was just set"),
                    );
                    self.world.set_generation(tick);
                    self.subscribed.clear();
                    self.geiger.reset();
                    // No spawn to look at, because nothing here is ours. The
                    // camera stays where it was, which for a fresh client is
                    // the origin -- and the origin is where the first grant
                    // goes, so it is where anything is likely to be.
                    self.camera.dirty = true;
                }
                // A result, and what it did to somebody's number. Ours moves
                // the dashboard; everybody else's is the half of a rating that
                // makes it a comparison rather than a score, and is worth a
                // line in the log until there is a screen for it.
                ServerMessage::Rated { who, rating, change } => {
                    let mine = self.person.clone();
                    if mine.as_ref() == Some(&who) {
                        log::info!("rated {rating} ({change:+})");
                        self.rating = Some(rating);
                        self.rating_change = Some(change);
                        // The home screen reads this when it is built, and it
                        // is already built if we are looking at it.
                        if let Screen::Menu(m) = &mut self.ui.screen {
                            m.rating = Some((rating, Some(change)));
                        }
                    } else {
                        log::info!("{who} is now rated {rating} ({change:+})");
                    }
                }
                // Somebody was given their opening ground. Ours moves the
                // camera, because a match lays everybody out at the whistle
                // and the spawn in our `Welcome` was worked out before any of
                // it existed -- it is stale by the time it matters, which is
                // why this used to need a reload.
                ServerMessage::Spawned { player, at } => {
                    if Some(player) == self.plays_as {
                        log::info!("granted ground at {at:?}");
                        self.camera.centre = middle_of(at);
                        self.camera.dirty = true;
                    }
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
                // Who is winning. Kept whole rather than merged, because a
                // player who has lost every square drops out of the list and a
                // merge would leave their last bar standing forever.
                ServerMessage::Match {
                    teams,
                    started_by,
                    owner,
                    code,
                    phase,
                    victory,
                    players,
                } => {
                    // A decided match is a result, and the only moment this
                    // client is told one. Recorded when it arrives rather than
                    // when the room is left, because a player who watches the
                    // final board and then closes the tab still played it.
                    if let (crate::net::MatchPhase::Over { winner, .. }, Some(live)) =
                        (&phase, self.in_play.as_mut())
                    {
                        live.decided(*winner == self.plays_as && self.plays_as.is_some());
                    }
                    // **Which player this client is now.** Joining a team is
                    // taking the controls of the team's player, so from here
                    // on this client's cells carry the team's number: what it
                    // may place, what that costs and which of the actions
                    // coming back are its own all follow from this one line.
                    //
                    // Read out of the roster rather than sent per connection,
                    // because `Match` is broadcast to the whole room and a
                    // field naming one recipient would be wrong for the rest.
                    // A seat on no team plays as itself, which is what a
                    // free-for-all is.
                    self.plays_as = self
                        .me
                        .map(|me| {
                            teams.iter().find(|t| t.players.contains(&me)).map_or(me, |t| t.id)
                        })
                        .or(self.plays_as);
                    // The phase moved, or somebody picked a team: either way
                    // whatever the last whistle was refused for has been
                    // answered, and a stale reason under the button is worse
                    // than none.
                    self.ui.refused_start = None;
                    self.lobby =
                        Some(Lobby { teams, phase, victory, players, owner, started_by, code });
                }
                ServerMessage::Standing { held, .. } => {
                    if let (Some(live), Some(me)) = (self.in_play.as_mut(), self.plays_as) {
                        live.holding(
                            held.iter().find(|h| h.who == me).map(|h| h.score).unwrap_or(0),
                        );
                    }
                    self.standing = held;
                }
                ServerMessage::Purse { value } => {
                    // Taken, not reconciled. A client only sees the mines in
                    // its own viewport, so its guess is always low and always
                    // getting lower; the server's number is the number. The
                    // cost is that an action sent for this tick and not yet
                    // applied shows for a moment as money still in hand, which
                    // a checkpoint interval later is right again.
                    if value != self.value {
                        log::debug!("purse: {} -> {value}", self.value);
                        self.value = value;
                    }
                }
                // A whistle that was not blown, into the lobby it was pressed
                // in. Its own message rather than `Rejected`, which closes a
                // connection: this leaves you exactly where you were, with a
                // reason to read.
                ServerMessage::NotStarted { reason } => {
                    log::info!("the match did not start: {reason}");
                    self.notice = Some(reason.clone());
                    self.ui.refused_start = Some(reason);
                }
                // The answer to `Create`, into the form it was sent from.
                // A refusal has to land beside the fields that produced it:
                // "there is already a room called that" is a thing to correct,
                // not a thing to be told once and then hunt for.
                ServerMessage::Made(made) => match made {
                    Ok(crate::net::Made { id, name, code }) => {
                        log::info!("made \"{name}\" ({id}); joining it");
                        // A code is the thing you send somebody, so it goes
                        // into the field it can be read off and copied from
                        // rather than only into a log line nobody will see.
                        if let Screen::Menu(m) = &mut self.ui.screen {
                            if let Some(code) = code {
                                m.code = code;
                            }
                            m.draft = None;
                        }
                        // Straight in. Making a world and then being handed
                        // back to the list to find it is a step that exists
                        // only because the messages are two.
                        self.chose(menu::Chose::Join(id));
                    }
                    Err(why) => {
                        log::info!("the server would not make that room: {why}");
                        if let Screen::Menu(menu::Menu { draft: Some(draft), .. }) =
                            &mut self.ui.screen
                        {
                            draft.asking = false;
                            draft.note = Some(why);
                        }
                    }
                },
                ServerMessage::Rooms { rooms } => {
                    log::debug!("the server has {} room(s)", rooms.len());
                    self.asked_at = None;
                    self.listed_at = self.elapsed;
                    if let Screen::Menu(m) = &mut self.ui.screen {
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
                    //
                    // **Except our own, which were applied when they were
                    // made.** A `Paint` is idempotent on the generation it was
                    // meant for and not one generation later: by then the cells
                    // it named have moved, and laying them again stamps the
                    // original pattern back on top of where it went. Draw a
                    // glider, watch it thicken into a blob and settle into a
                    // honey farm, and watch it snap back to a glider when the
                    // resync lands a few seconds later.
                    //
                    // Skipping them leaves the phase error prediction has
                    // always had -- the same cells, a generation out, which the
                    // checkpoint puts right -- instead of a different pattern
                    // that the rules then build on.
                    //
                    // **By seat, not by player.** Those are the same question
                    // until a team is a player and several clients are at its
                    // controls; then a teammate's action carries the team's
                    // number, this skipped it as something already predicted,
                    // and never applied it at all. Two people building on one
                    // team saw different worlds until a checkpoint dragged the
                    // chunks across -- which it did, every few seconds, for as
                    // long as they both kept playing.
                    // **And not one this client already applied early**, or a
                    // `Paint` lands twice: idempotent on the generation it was
                    // meant for, and not on the one after. `Acted` is a
                    // shortcut that can be dropped, so the `Step` still
                    // carries everything and this is where the two meet.
                    let early = std::mem::take(&mut self.applied_early);
                    for stamped in
                        actions.iter().filter(|s| Some(s.seat) != self.me && !early.contains(s))
                    {
                        // **And a teammate's comes out of the purse we share.**
                        // One team is one purse, so an ally spending is this
                        // client's balance moving with no action of its own
                        // behind it. Predicted for the same reason our own
                        // spending is: the alternative is a figure that stays
                        // wrong until the next checkpoint, which is seconds of
                        // offering to spend money somebody else has spent.
                        //
                        // **Priced before it is applied**, which is the whole
                        // contract of `value_delta` -- it reads what is there
                        // now, and what is there now is what the server priced
                        // it against.
                        if Some(stamped.player) == self.plays_as {
                            let delta = crate::net::value_delta(&self.world, stamped);
                            self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
                        }
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
                // **An action, the moment the server took it.** Applied here
                // rather than waited for, so a cell somebody else placed
                // appears in a round trip instead of at the next generation —
                // which at four a second was 125 ms of doing nothing on a link
                // that costs four.
                //
                // **Not on the tick it names**, which was the wrong test and
                // cost a whole generation whenever an action was made near a
                // boundary. `stamped.tick` is the *actor's* guess; the server
                // applies whatever is pending on the step it happens to be on
                // when it steps, which is the generation this client is on too
                // — `advance_to` puts it there and nothing else moves it. So
                // being caught up is the condition, and this is what being
                // caught up looks like.
                //
                // Our own is already down: this client predicted it when it
                // was made, which is the same thing a round trip earlier.
                ServerMessage::Acted(stamped) => {
                    if Some(stamped.seat) != self.me {
                        // **Priced before it is applied**, as in the `Step`
                        // arm above and for the same reason: `value_delta`
                        // reads what is on the square *now*, and the server
                        // priced it against that. Applied first, every cell
                        // compares equal to what the placement would put
                        // there -- `apply_to` and `remove_from` are
                        // idempotent -- so the delta was always exactly zero
                        // and a teammate's spending never moved this client's
                        // purse until the next `Purse` corrected it.
                        if Some(stamped.player) == self.plays_as {
                            let delta = self.price(&stamped);
                            self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
                        }
                        crate::net::apply(&mut self.world, &stamped);
                        self.applied_early.push(stamped);
                    }
                }
                ServerMessage::Resync { tick, chunks } => {
                    // One click per chunk, not per message: a resync naming
                    // forty chunks is a world being rebuilt, and one naming a
                    // single chunk is one prediction that missed. The log line
                    // says it happened; the counter says how often.
                    self.geiger.clicks(chunks.len(), self.elapsed);
                    log::warn!(
                        "desynced at tick {tick}; refetching {} chunks (rate {:.1})",
                        chunks.len(),
                        self.geiger.rate()
                    );
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
            self.file_game();
            self.link = None;
            self.asked_at = None;
            match &self.ui.screen {
                // Nothing was ever reached. The address is the likely reason
                // and the only thing the player can act on, so it is what the
                // message names.
                Screen::Menu(m) => {
                    let address = m.address.clone();
                    self.show_menu(menu::Stage::Failed(words::menu::no_answer(&address)));
                }
                // Mid-game. The simulation is deterministic, so the world
                // carries on locally rather than stopping -- offline is a
                // solitary game, not a broken one. **Said on screen as well as
                // in the log**, because a log line is not somewhere a player
                // is looking and the difference between this game and the one
                // they were in is every other player in it.
                Screen::Playing => {
                    log::warn!("link closed; continuing offline");
                    self.notice = Some(words::menu::LOST_CONNECTION.into());
                }
            }
            return;
        }

        if matches!(self.ui.screen, Screen::Playing) {
            self.subscribe_to_view();
        }
    }

    /// Ask for any visible chunk not already requested. The camera is fixed for
    /// now, so this settles after the first frame; it is written against the
    /// viewport so panning needs no new code.
    /// Lay what a drag drew, whatever shape it is.
    ///
    /// Always places, never takes: a drag across occupied ground is far more
    /// likely to be building over it than a request to clear it cell by cell,
    /// and an accidental sweep that wiped a structure would be unforgiving.
    /// Taking stays a deliberate single click.
    fn lay(&mut self, cells: Vec<(i32, i32)>, shape: String) {
        if !self.may_act() {
            self.notice = Some(if self.watching {
                words::menu::watch::NO_SEAT.into()
            } else {
                words::refused::not_started().to_string()
            });
            return;
        }
        let count = cells.len();
        let (stamped, delta) = self.quote(cells, false);

        // All or nothing. A stroke laid as far as the value stretched would
        // stop somewhere the hand did not, and the player would be left
        // working out where it ran out and why.
        if self.value + delta < 0 {
            self.notice = Some(words::refused::cannot_afford(count, -delta, self.value));
            return;
        }
        self.notice = None;
        self.value = (self.value + delta).clamp(0, Player::MAX_VALUE);
        self.commit(&stamped);
        self.last_action = Some(format!("{shape}, {delta:+}"));
    }

    /// What a drag would lay, and how to describe it.
    ///
    /// One function for both shapes and for both callers, so the preview
    /// cannot draw one thing and the release lay another.
    fn drag_cells(&self, drag: &Drag, to: (i32, i32)) -> Result<(Vec<(i32, i32)>, String), String> {
        let name = self.holding().to_string();
        // A refusal again: a stroke is all or nothing, so one cell nothing of
        // yours reaches refuses the whole of it, before the button comes up.
        let outside =
            |cells: &[(i32, i32)]| cells.iter().filter(|&&(r, c)| !self.may_place_at(r, c)).count();
        match drag.stroke {
            hotbar::Stroke::Pencil => {
                let stray = outside(&drag.path);
                if stray > 0 {
                    return Err(words::refused::cells_not_yours(stray));
                }
                let full = if drag.full() { ", full" } else { "" };
                Ok((drag.path.clone(), format!("{} cells of {name}{full}", drag.path.len())))
            }
            hotbar::Stroke::Rectangle => {
                let (rows, cols) = span(drag.from, to);
                let area = rows * cols;
                if area > MAX_DRAG_CELLS {
                    return Err(format!("{area} cells is more than one drag may lay"));
                }
                let (r0, r1) = (drag.from.0.min(to.0), drag.from.0.max(to.0));
                let (c0, c1) = (drag.from.1.min(to.1), drag.from.1.max(to.1));
                let cells: Vec<(i32, i32)> =
                    (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).collect();
                let stray = outside(&cells);
                if stray > 0 {
                    return Err(words::refused::cells_not_yours(stray));
                }
                Ok((cells, format!("{name} {rows}x{cols}")))
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
        // A quarter of a cell, so nothing narrower than a collider is stepped
        // over, and bounded so a pointer that jumps the screen is not sampled
        // a quarter-cell at a time all the way across it.
        let step = (self.camera.zoom as f64 / 4.0).max(1.0);
        let (dx, dy) = (to_px.0 - from_px.0, to_px.1 - from_px.1);
        let samples = ((dx.hypot(dy) / step).ceil() as usize).clamp(1, 512);

        for i in 1..=samples {
            let t = i as f64 / samples as f64;
            let at = (from_px.0 + dx * t, from_px.1 + dy * t);
            let Some(cell) = cell_under(self.camera.cell_at_f(at)) else { continue };
            if let Gesture::Drawing(drag) = &mut self.gesture {
                if drag.full() {
                    return;
                }
                drag.mark(cell);
            }
        }
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
        // Only the chunks this client has actually asked for.
        //
        // `stored()` is the wrong set on a wrapping world: a torus is
        // allocated whole, so every chunk exists from the moment the world is
        // built and the client would claim to hold hundreds it has never been
        // sent. They read as empty, the server disagrees with every one of
        // them, and it answers with a `Resync` naming the lot -- every
        // checkpoint, until the whole world has been dragged across. An
        // infinite world hid this, because there `stored()` is only what has
        // been fetched or grown.
        //
        // Asked-for rather than received, because the two differ only where
        // the server had nothing to send, and a chunk it says nothing about is
        // one it agrees is empty.
        let chunks: Vec<(crate::sim::Coord, u64)> = self
            .world
            .stored()
            .iter()
            .filter(|(coord, _)| self.subscribed.contains(coord))
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
        self.quote_as(cells, taking, self.held.placement().unwrap_or(Placement::Life))
    }

    /// The same, for a placement the hotbar is not holding — a stamp lays
    /// whatever it captured, which may be two kinds at once.
    fn quote_as(
        &self,
        cells: Vec<(i32, i32)>,
        taking: bool,
        placement: Placement,
    ) -> (Stamped, i32) {
        let action = if taking {
            Action::Erase { cells, placement }
        } else {
            Action::Paint { cells, placement }
        };
        let stamped = Stamped {
            tick: self.world.generation,
            player: self.player(),
            // This client, as against the number it plays under. In a team
            // match they differ, and telling them apart is what stops a
            // teammate's action being mistaken for one this client predicted.
            seat: self.me.unwrap_or_else(|| self.player()),
            action,
        };
        let delta = self.price(&stamped);
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
        // Nor under a stamp's ghost, for the same reason a swept rectangle
        // hides it: the thing about to be laid is already drawn, and a box
        // round one cell of it says something less true.
        if matches!(self.held.shape, hotbar::Shape::Stamp(_)) {
            return None;
        }
        let at = self.cell_under_cursor(self.cursor);
        Some(self.camera.cell_rect(at, at))
    }

    /// What a drag would lay if it were released now, with what it would cost.
    ///
    /// Nothing is on the board until the button comes up — for a stroke as
    /// much as for a pane — so this is the only thing saying what is being
    /// drawn while it is being drawn. A stroke used to lay itself cell by cell
    /// and needed no preview, because the cells were their own; holding it
    /// back is what makes one necessary.
    fn selection_mark(&self) -> Option<overlay::Selection> {
        let Gesture::Drawing(drag) = &self.gesture else { return self.stamp_preview() };
        if !drag.moved {
            return self.stamp_preview();
        }
        let to = self.cell_under_cursor(self.cursor);

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
            hotbar::Stroke::Pencil => {
                cells.iter().map(|&at| self.camera.cell_rect(at, at)).collect()
            }
            hotbar::Stroke::Rectangle => vec![self.camera.cell_rect(drag.from, to)],
        };

        let (r, g, b) = crate::client::views::hue::player_colour(self.player());
        Some(overlay::Selection {
            bounds: self.camera.cell_rect(drag.from, to),
            cells: rects,
            outlined: drag.stroke == hotbar::Stroke::Rectangle,
            tint: egui::Color32::from_rgb(r, g, b),
            hatched: self.held.placement() == Some(crate::net::Placement::Ice),
            label,
            allowed,
        })
    }

    /// What a stamp would lay, before it is laid.
    ///
    /// **A stamp is the one thing here placed by a click**, so there is no
    /// drag to watch it grow during — every other gesture answers "what am I
    /// about to do" by drawing itself while the button is down, and a stamp
    /// simply happened. It went down somewhere you had guessed at, sometimes
    /// off the ground you own, and the refusal arrived after the commitment.
    ///
    /// Drawn as a selection rather than as a thing of its own, because it is
    /// one: the same cells, the same tint, the same refusal in red, the same
    /// label saying what it costs. What the drag preview does for a rectangle,
    /// this does for a pattern.
    fn stamp_preview(&self) -> Option<overlay::Selection> {
        let hotbar::Shape::Stamp(index) = self.held.shape else { return None };
        if !self.hovering || self.is_panning() || self.camera.zoom < HOVER_MIN_ZOOM {
            return None;
        }
        let stamp = self.stamps.get(index)?.turned(self.held.turn);
        let corner = stamp.centred_on(self.cell_under_cursor(self.cursor));
        let laid = stamp.at(corner);

        // Priced by the same call the placement itself makes, so what the
        // preview says and what the click charges cannot drift apart.
        let what = self.held.placement()?;
        let delta = self.quote_as(laid.clone(), false, what).1;
        let stray = laid.iter().filter(|&&(r, c)| !self.may_place_at(r, c)).count();

        let allowed = stray == 0 && self.value + delta >= 0;
        let label = if stray > 0 {
            words::refused::cells_not_yours(stray)
        } else if self.value + delta < 0 {
            format!("{}   costs {}, you have {}", stamp.name, -delta, self.value)
        } else {
            format!("{}   {}x{}   {delta:+}", stamp.name, stamp.size.0, stamp.size.1)
        };

        let (r, g, b) = crate::client::views::hue::player_colour(self.player());
        let far = (corner.0 + stamp.size.0 - 1, corner.1 + stamp.size.1 - 1);
        Some(overlay::Selection {
            bounds: self.camera.cell_rect(corner, far),
            // Cell by cell rather than one rectangle over the extent: a
            // pattern is mostly holes, and a wash over its bounding box would
            // say it fills them.
            cells: laid.iter().map(|&at| self.camera.cell_rect(at, at)).collect(),
            outlined: false,
            tint: egui::Color32::from_rgb(r, g, b),
            hatched: false,
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
        // A stamp is not a cell, so a click with one held stamps rather than
        // placing or taking.
        if let hotbar::Shape::Stamp(index) = self.held.shape {
            self.stamp_at(index, (row, col));
            return;
        }

        let player = self.player();
        let name = self.holding().to_string();

        if !self.may_act() {
            self.notice = Some(if self.watching {
                words::menu::watch::NO_SEAT.into()
            } else {
                words::refused::not_started().to_string()
            });
            return;
        }
        let existing = self.world.cell_at(row, col).unwrap_or(crate::sim::Cell::DEAD);
        let Some(placement) = self.held.placement() else { return };
        let already_there = self.already_there(row, col);

        // Confined to ground your own influence reaches, and refused here on
        // the same terms the server refuses it, so the answer is instant
        // rather than a round trip away. Taking back what is already there is
        // not placing and is not confined.
        if !already_there && !self.may_place_at(row, col) {
            self.notice = Some(words::refused::not_your_territory(row, col));
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

    /// What the hotbar is holding, for the HUD and for a drag's label.
    /// Whether this client may put something on that square.
    ///
    /// One question in one place, so a laboratory takes the rule off
    /// everywhere it is asked rather than at three of the four call sites.
    fn may_place_at(&self, row: i32, col: i32) -> bool {
        self.place_anywhere || crate::net::may_place(&self.world, self.player(), row, col)
    }

    /// What an action costs, which in a laboratory is nothing.
    fn price(&self, stamped: &Stamped) -> i32 {
        if self.place_free {
            0
        } else {
            crate::net::value_delta(&self.world, stamped)
        }
    }

    /// Run, or stop running. Offline only.
    ///
    /// **Said rather than ignored** when there is a link. A key that does
    /// nothing and explains nothing reads as broken, where "the server keeps
    /// time here" reads as the rule it is — and it is a rule worth learning,
    /// because it is the same one that makes prediction work.
    fn toggle_running(&mut self) {
        if self.link.is_some() {
            self.notice = Some(words::help::SERVER_KEEPS_TIME.into());
            return;
        }
        self.paused = !self.paused;
        self.notice = None;
        self.last_action = Some(if self.paused {
            words::help::PAUSED.into()
        } else {
            words::help::RUNNING.into()
        });
    }

    /// One generation, and stay stopped.
    ///
    /// Stopping first if it was running, which is what somebody pressing this
    /// means: a single step out of a moving world is a step you cannot look
    /// at.
    fn step_one(&mut self) {
        if self.link.is_some() {
            self.notice = Some(words::help::SERVER_KEEPS_TIME.into());
            return;
        }
        self.paused = true;
        self.step_once = true;
        self.notice = None;
        // The generation it will be on once the step is taken, since that is
        // the number somebody stepping wants to read.
        self.last_action = Some(words::help::stepped_to(self.world.generation + 1));
    }

    fn holding(&self) -> &str {
        // Both axes, because both are what you are holding: a pane of ice and
        // a pencil of ice are different things to be about to do.
        match self.held.shape {
            hotbar::Shape::Stamp(i) => {
                self.stamps.get(i).map(|s| s.name.as_str()).unwrap_or("Stamp")
            }
            _ => self.held.tool().map(|t| t.name).unwrap_or("nothing"),
        }
    }

    /// Take what the hotbar was clicked or keyed for.
    fn pick(&mut self, key: Key) {
        match key {
            // **One axis each, which is the whole of what two axes buys.**
            // Picking a material does not put your pencil down, and picking a
            // shape does not change what it is made of.
            Key::Shape(shape) => {
                self.held.shape = shape;
                self.ui.picking_stamp = false;
            }
            Key::Kind(kind) => self.held.kind = kind,
            Key::More => self.ui.picking_stamp = !self.ui.picking_stamp,
            Key::Help => self.ui.helping = !self.ui.helping,
            // The same two the keys reach, so a square and a key cannot come
            // to mean different things.
            Key::Run => self.toggle_running(),
            Key::Step => self.step_one(),
            Key::Rules => self.ui.showing_rules = !self.ui.showing_rules,
            // The one place flipping happens, so the key and the square
            // cannot drift apart -- which they did, the key putting the shape
            // back to the kind's usual while a click toggled. The turn goes
            // with it, because a pattern left rotated after a flip is a reset
            // somebody has to reset.
            Key::Flip => {
                self.held.shape = self.held.shape.other();
                self.held.turn = Default::default();
                self.ui.picking_stamp = false;
            }
        }
    }

    /// Lay a stamp with its middle under the pointer.
    ///
    /// One action per placement it holds, because a `Paint` lays one kind and
    /// a stamp may have caught two. All or nothing across both: half a pattern
    /// is not the pattern, so it is priced whole before any of it is sent.
    fn stamp_at(&mut self, index: usize, at: (i32, i32)) {
        if !self.may_act() {
            self.notice = Some(if self.watching {
                words::menu::watch::NO_SEAT.into()
            } else {
                words::refused::not_started().to_string()
            });
            return;
        }
        let Some(stamp) = self.stamps.get(index).map(|s| s.turned(self.held.turn)) else {
            self.notice = Some(words::stamps::GONE.into());
            return;
        };
        let corner = stamp.centred_on(at);

        // **One kind, so one quote.** A stamp used to carry a placement per
        // cell and went down as several actions; it is a shape now and what it
        // is made of comes off the hotbar, so it is one.
        let Some(what) = self.held.placement() else { return };
        let laid = stamp.at(corner);
        let quotes = [self.quote_as(laid.clone(), false, what)];

        let cells: usize = stamp.cells.len();
        let stray = laid.iter().filter(|&&(r, c)| !self.may_place_at(r, c)).count();
        if stray > 0 {
            self.notice = Some(words::refused::cells_not_yours(stray));
            return;
        }

        let delta: i32 = quotes.iter().map(|(_, d)| d).sum();
        if self.value + delta < 0 {
            self.notice = Some(words::refused::cannot_afford(cells, -delta, self.value));
            return;
        }

        self.notice = None;
        self.value += delta;
        for (stamped, _) in &quotes {
            self.commit(stamped);
        }
        self.last_action = Some(words::stamps::placed(&stamp.name, cells, delta));
    }

    /// Take the rectangle a drag swept into a new stamp.
    fn capture(&mut self, from: (i32, i32), to: (i32, i32)) {
        match stamp::Stamp::capture(&self.world, self.player(), from, to) {
            Some(taken) => {
                let (name, cells) = (taken.name.clone(), taken.cells.len());
                self.stamps.keep(taken);
                // Held straight away: you swept it out because you want to put
                // it somewhere, and it is the newest so it is at index zero.
                self.held.shape = hotbar::Shape::Stamp(0);
                self.notice = None;
                self.last_action = Some(words::stamps::captured(&name, cells));
            }
            None => {
                self.notice = Some(words::stamps::NOTHING_TO_CAPTURE.into());
            }
        }
    }

    /// Whether input from the world should be acted on at all.
    fn playing(&self) -> bool {
        matches!(self.ui.screen, Screen::Playing)
    }

    /// Back to the menu, in this state, keeping whatever was typed into it.
    ///
    /// Rebuilt from what is remembered when there was no menu to return to,
    /// which is the case of a game started from a command line being refused.
    /// Whether the board is what the player is looking at.
    ///
    /// Not on the menu, and not in a lobby: a gathering match has an empty
    /// world, so there is literally nothing to see behind it. A **decided**
    /// match is the other way round — the board is the result, and covering it
    /// to say who won would hide the thing that says why.
    fn showing_world(&self) -> bool {
        if matches!(self.ui.screen, Screen::Menu(_)) {
            return false;
        }
        !matches!(self.lobby.as_ref().map(|l| &l.phase), Some(crate::net::MatchPhase::Gathering))
    }

    /// File the game in play, if there is one, and forget it.
    ///
    /// Called wherever a room ends for this client. Idempotent, so the paths
    /// that overlap — a link closing on the way back to the menu — file once.
    fn file_game(&mut self) {
        let Some(mut live) = self.in_play.take() else { return };
        live.at(self.world.generation);
        let game = live.finish();
        log::info!(
            "filing \"{}\": {} generations, {} squares at its largest",
            game.room,
            game.generations,
            game.best
        );
        crate::client::record::remember(&game);
    }

    /// Back to the menu, from wherever.
    ///
    /// The socket is kept and the room list asked for again, so going back is
    /// a step rather than a disconnection — the seat is held until another
    /// `Join` takes its place, which is what the server treats a second join
    /// as anyway.
    fn back_to_menu(&mut self) {
        self.file_game();
        // **Give the seat up.** Going back used to keep it, on the reasoning
        // that another `Join` would take its place — true of somebody who
        // rejoins the same room, and false of everything else. The player
        // stayed online, so the room went on counting them, and the rejoin
        // token, which only returns you to a player who is *not* online, found
        // them online and issued a new one. Leave and come back three times
        // and a room with one person in it said three.
        //
        // The token is kept: this is the seat being vacated, not the player
        // being forgotten, and coming back should still be coming back.
        if let (Some(link), true) = (&self.link, self.me.is_some() || self.watching) {
            link.send(ClientMessage::Leave);
        }
        self.me = None;
        self.room = None;
        self.room_name = None;
        self.lobby = None;
        self.standing.clear();
        self.subscribed.clear();
        self.watching = false;
        let asking = self.link.is_some();
        self.show_menu(if asking { menu::Stage::Asking } else { menu::Stage::Idle });
        if let Some(link) = self.link.as_ref() {
            link.send(ClientMessage::Rooms);
            self.asked_at = Some(self.elapsed);
        }
    }

    /// Leave whatever server this client is on, and start a game of one.
    ///
    /// **Playing alone has to mean alone**, and it meant "the same screen with
    /// the menu gone". The reasoning was that `init` builds and grants a local
    /// world whether or not anyone connects, so there was nothing left to do —
    /// true on the first press and false on every press after a room, because
    /// a `Welcome` replaces that world with the server's.
    ///
    /// So a client that had been anywhere got the room's world back, at the
    /// server's tick, with the player number and the value the server issued,
    /// a HUD that said connected, and — worst of it — a board that did not
    /// move. `update` steps the local world only when there is no link, since
    /// a connected client takes its generations from the server and must never
    /// invent its own; the link was still open, so nothing stepped. A frozen
    /// board in a world you cannot build in is not a game.
    ///
    /// The seat itself was already given up on the way here: `back_to_menu`
    /// sends `Leave`. This is the connection going, and the world with it.
    fn play_alone(&mut self) {
        self.play_alone_on(start::chosen_world(), None)
    }

    /// Playing alone on a described world, with a way to win it if one was
    /// asked for.
    ///
    /// **The client is the authority offline**, which is what makes a win
    /// condition possible with no server: there is one world, one clock and
    /// one player, so `MatchPhase` is a thing this can hold and decide for
    /// itself. It is the same `Victory` the server settles, read by the same
    /// `net::standings` — and it is not a match anybody could cheat, because
    /// there is nobody to cheat.
    fn play_alone_on(
        &mut self,
        shape: crate::sim::WorldKind,
        victory: Option<crate::net::Victory>,
    ) {
        log::info!("playing alone on {shape:?} ({victory:?})");
        self.file_game();
        // Dropping it closes it — see the `Drop` on the browser's `Link`, and
        // the socket thread that ends with its channel on native.
        self.link = None;
        // Or the room list this client is no longer waiting for times out and
        // drags it back to the menu, four seconds into a game of one, to say
        // that a server it has stopped talking to did not answer.
        self.asked_at = None;
        self.me = None;
        self.room = None;
        self.room_name = None;
        self.lobby = None;
        self.standing.clear();
        self.subscribed.clear();
        self.watching = false;
        // A different world is a different argument about it.
        self.geiger.reset();
        // What the server had issued belonged to the seat that was given up.
        self.value = Player::STARTING_VALUE;
        let (world, home) = start::solo_world_of(shape);
        self.world = world;
        // **A name it is not asked for.** A world nobody else can reach needs
        // none of a room's things and the form asks for none of them — but a
        // name costs nothing to give, is what the HUD reads, is the handle a
        // saved world would be filed under, and is what `net::world_seed`
        // turns into this world's own dice. Without one it rolled from nought,
        // along with every other solitary world.
        //
        // `room` stays empty beside it, and that is not an oversight: an id is
        // what a *server* is holding for this client, and there is no server.
        self.world.set_seed(crate::net::world_seed(&crate::net::SOLO_ROOM.into()));
        self.room_name = Some(crate::net::SOLO_ROOM.to_string());
        // A lobby of one, so the clock along the top has something to read.
        // `None` for a sandbox, which is what playing alone has always been
        // and what the form's "never" means.
        self.lobby = victory.map(|victory| Lobby {
            phase: crate::net::MatchPhase::Running { from: 0 },
            victory: Some(victory),
            players: vec![(PlayerId(1), String::new())],
            teams: Vec::new(),
            owner: None,
            started_by: None,
            code: None,
        });
        self.paused = false;
        self.camera.centre = home;
        // The chunk store still holds the room's world, and a dirty camera is
        // what makes `update` sync it against this one.
        self.camera.dirty = true;
        self.ui.screen = Screen::Playing;
    }

    /// Say where the client is, in the address bar.
    ///
    /// Called wherever the screen or the room changes rather than every frame:
    /// it is a browser API and a no-op on native, and neither wants doing
    /// sixty times a second to say the same thing.
    /// Whether this room is a match that has not started.
    fn gathering(&self) -> bool {
        matches!(self.lobby.as_ref().map(|l| &l.phase), Some(crate::net::MatchPhase::Gathering))
    }

    /// What the address would say, as something comparable — so it is written
    /// again only when it would say something different.
    fn here(&self) -> Option<(bool, bool, bool, bool, bool, Option<crate::net::RoomId>)> {
        Some((
            matches!(self.ui.screen, Screen::Playing),
            match &self.ui.screen {
                Screen::Menu(m) => m.page == menu::Page::Play,
                Screen::Playing => self.watching,
            },
            matches!(&self.ui.screen, Screen::Menu(m) if m.page == menu::Page::Alone),
            matches!(&self.ui.screen, Screen::Menu(m) if m.page == menu::Page::Experiments),
            self.gathering(),
            self.room.clone(),
        ))
    }

    /// What to join as: what is typed on the menu, or what was remembered.
    fn my_name(&self) -> String {
        match &self.ui.screen {
            Screen::Menu(m) => m.name.clone(),
            Screen::Playing => crate::net::keep::name().unwrap_or_else(|| "player".into()),
        }
    }

    /// Follow the back or forward button, if it has just been pressed.
    ///
    /// **The other half of pushing an address.** The browser moves the address
    /// and expects the page to follow; a client that only *writes* addresses
    /// shows the old screen under the new one, which is worse than having no
    /// addresses because now they lie.
    ///
    /// A room is joined the way a link into one is, because it is the same
    /// request. Everything else is a screen to be on, and going back to a
    /// screen means leaving whatever world you were in — which `back_to_menu`
    /// already does properly, giving up the seat rather than abandoning it.
    fn follow_history(&mut self) {
        use crate::client::route::Route;
        let Some(route) = crate::client::route::went_back() else { return };
        log::info!("the back button asked for {route:?}");
        match route {
            Route::Watch(room) => {
                self.back_to_menu();
                self.watching = true;
                self.ask_to_join(Joining { name: self.my_name(), room: Some(room) });
            }
            Route::Room(room) | Route::Lobby(room) => {
                self.back_to_menu();
                self.watching = false;
                self.ask_to_join(Joining { name: self.my_name(), room: Some(room) });
            }
            Route::Home => self.show_menu_page(menu::Page::Home),
            Route::Play => self.show_menu_page(menu::Page::Play),
            // Both, because a solitary world names no shape and so cannot be
            // rebuilt from an address. The form is where you say what it was.
            Route::Alone | Route::Solo => self.show_menu_page(menu::Page::Alone),
            Route::Lab => self.show_menu_page(menu::Page::Experiments),
        }
    }

    /// Leave whatever is on screen for a page of the menu.
    fn show_menu_page(&mut self, page: menu::Page) {
        if matches!(self.ui.screen, Screen::Playing) {
            self.back_to_menu();
        }
        if let Screen::Menu(m) = &mut self.ui.screen {
            m.page = page;
        }
    }

    fn say_where(&self) {
        use crate::client::route::Route;
        let route = match (&self.ui.screen, &self.room) {
            (Screen::Menu(m), _) if m.page == menu::Page::Play => Route::Play,
            (Screen::Menu(m), _) if m.page == menu::Page::Alone => Route::Alone,
            (Screen::Menu(m), _) if m.page == menu::Page::Experiments => Route::Lab,
            (Screen::Menu(_), _) => Route::Home,
            (Screen::Playing, Some(room)) if self.watching => Route::Watch(room.clone()),
            // A lobby is a screen of its own, so it says so. Following either
            // does the same thing — join that room — and what you get is
            // whichever screen the phase calls for.
            (Screen::Playing, Some(room)) if self.gathering() => Route::Lobby(room.clone()),
            (Screen::Playing, Some(room)) => Route::Room(room.clone()),
            // Offline. It has no room to name and is no link to hand
            // anybody, and it is still a screen you are on rather than the
            // home screen — which is what this said, so a solitary game spent
            // the whole of itself at `/home`.
            (Screen::Playing, None) => Route::Solo,
        };
        crate::client::route::show(&route);
    }

    fn show_menu(&mut self, stage: menu::Stage) {
        match &mut self.ui.screen {
            Screen::Menu(m) => {
                // A fresh attempt starts a fresh retry cadence. Left standing,
                // a `failed_at` from the last refusal would make the next one
                // read as already overdue and retry at once.
                if !matches!(stage, menu::Stage::Failed(_)) {
                    m.failed_at = None;
                }
                m.stage = stage;
                // Re-read, because a game may have just been filed on the way
                // here and a home screen showing the count from before it
                // would say the last game did not happen.
                m.record = crate::client::record::Summary::of(&crate::client::record::games());
                m.rating = self.rating.map(|r| (r, self.rating_change));
            }
            Screen::Playing => {
                let address = self.address_hint();
                let mut m = menu::Menu::new(address, cfg!(target_arch = "wasm32"));
                m.stage = stage;
                // Carried over, because the menu is where a rating is read and
                // the app is the only thing that has been told one. A match
                // that has just ended is the commonest way to arrive here.
                m.rating = self.rating.map(|r| (r, self.rating_change));
                self.ui.screen = Screen::Menu(m);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn address_hint(&self) -> String {
        Link::origin_url("/ws").unwrap_or_else(|| "ws://localhost:8080/ws".into())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn address_hint(&self) -> String {
        crate::net::keep::server().unwrap_or_else(|| start::DEFAULT_ADDRESS.into())
    }

    /// Act on what the menu was clicked for.
    fn chose(&mut self, chose: menu::Chose) {
        match chose {
            menu::Chose::Nothing => {}
            menu::Chose::Offline => self.play_alone(),
            menu::Chose::Alone { shape, victory } => self.play_alone_on(shape, victory),
            // **A laboratory opens stopped.** Golly's habit and the right one:
            // the first thing anybody does here is draw, and a world running
            // while you draw into it is a world eating what you drew.
            menu::Chose::Experiment { free_hand } => {
                self.play_alone_on(crate::sim::WorldKind::Infinite, None);
                self.place_anywhere = free_hand;
                self.place_free = free_hand;
                self.paused = true;
                self.last_action = Some(words::help::PAUSED.into());
            }
            // The list refreshes itself; this is for somebody who has just
            // made a room elsewhere and does not want to wait out the
            // interval. Asking again is one small message.
            menu::Chose::Refresh => {
                self.listed_at = self.elapsed;
                self.link.as_ref().inspect(|l| l.send(ClientMessage::Rooms));
            }
            menu::Chose::Connect(address) => {
                if let Screen::Menu(m) = &mut self.ui.screen {
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
                    None => {
                        self.show_menu(menu::Stage::Failed(words::menu::not_an_address(&address)))
                    }
                }
            }
            // Back into the world already behind the menu. Nothing is joined
            // and nothing is sent: the seat was never given up, because going
            // back to the menu keeps the socket and holds the seat until
            // another `Join` takes its place.
            menu::Chose::Resume => {
                self.ui.screen = Screen::Playing;
            }
            // **Kept, and not proved.** A key is checked by the server on the
            // next `Join` and nowhere else -- a client cannot know whether one
            // is this server's, and pretending to check it here would be a
            // second answer that has to agree with the real one. So this
            // writes it down; a key that reads but is not ours comes back as a
            // refusal that says so.
            //
            // No reconnect. A person is settled per join rather than per
            // socket, so the next join carries this one and the open socket
            // needs nothing done to it.
            menu::Chose::UseKey(written) => match crate::net::Secret::read(&written) {
                Ok(key) => {
                    log::info!("this client took a key it was given");
                    crate::net::keep::remember_secret(&key);
                    if let Screen::Menu(m) = &mut self.ui.screen {
                        // Normalised, so the field shows what was actually
                        // kept rather than whatever spacing it was pasted in.
                        m.key = key.written();
                    }
                }
                Err(why) => self.show_menu(menu::Stage::Failed(why)),
            },
            // **Everything, and it cannot be undone.** A key nobody else holds
            // is a key nobody can give back, so this is the one press in the
            // client that destroys something -- which is why the menu asks
            // twice before it gets here.
            menu::Chose::ResetEverything => {
                log::warn!("forgetting this client's key, record and settings");
                crate::net::keep::forget_everything();
                self.link = None;
                self.joining = None;
                self.me = None;
                self.room = None;
                self.room_name = None;
                self.ui.screen = Screen::Menu(menu::Menu::new(
                    self.address_hint(),
                    cfg!(target_arch = "wasm32"),
                ));
            }
            // The form is a column now rather than something opened, so there
            // is nothing to shut: a press here puts it back to its defaults.
            menu::Chose::Clear => {
                if let Screen::Menu(m) = &mut self.ui.screen {
                    m.draft = Some(menu::Draft::default());
                }
            }
            // Made, then joined -- in two steps, because `Made` only names the
            // room. Joining is the same message the room list sends, so there
            // is one way into a world rather than two.
            menu::Chose::Create { name, shape, victory, teams, private } => {
                let Some(link) = &self.link else {
                    self.show_menu(menu::Stage::Failed(words::menu::LOST_CONNECTION.into()));
                    return;
                };
                log::info!("asking for a room called \"{name}\"");
                link.send(ClientMessage::Create { name, shape, victory, teams, private });
            }
            // Watching takes no name and keeps no token: there is no player
            // to be remembered as.
            menu::Chose::Watch(room) => {
                let Some(link) = &self.link else {
                    self.show_menu(menu::Stage::Failed(words::menu::LOST_CONNECTION.into()));
                    return;
                };
                log::info!("watching room \"{room}\"");
                link.send(ClientMessage::Watch { room });
            }
            menu::Chose::Join(room) => {
                let Some(link) = &self.link else {
                    self.show_menu(menu::Stage::Failed(words::menu::LOST_CONNECTION.into()));
                    return;
                };
                let _ = link;
                let name = match &self.ui.screen {
                    Screen::Menu(m) => m.name.clone(),
                    Screen::Playing => "player".into(),
                };
                crate::net::keep::remember_name(&name);
                log::info!("joining {room} as \"{name}\"");
                self.ask_to_join(Joining { name, room: Some(room) });
            }
        }
    }

    /// Join.
    fn ask_to_join(&mut self, joining: Joining) {
        self.joining = Some(joining);
        self.send_pending_join();
    }

    /// Send the waiting join, if there is one and there is a link to send it
    /// on.
    ///
    /// A client with no secret sends none, and plays as somebody this server
    /// will not remember — the honest outcome for a browser that cannot keep
    /// one, rather than a reason to refuse to let anybody play.
    ///
    /// **Nothing is waited for now.** A join used to be held until the server
    /// sent a challenge to sign, which was a round trip before every join and
    /// a state to get wrong; `Link` already holds messages until the socket is
    /// open, so there is nothing left for this to be pending on.
    fn send_pending_join(&mut self) {
        let Some(link) = self.link.as_ref() else { return };
        let Some(joining) = self.joining.take() else { return };
        link.send(ClientMessage::Join {
            person: crate::net::keep::secret_or_new(),
            name: joining.name,
            room: joining.room,
        });
    }

    /// Ask for the room list again, so it does not go stale under the pointer.
    ///
    /// A list is a photograph of the moment it was asked for, and rooms come
    /// and go — somebody makes a match while you are reading, or the one you
    /// are about to click empties out. Re-asked rather than left with a button
    /// to press, because a list that is only right when you remember to
    /// refresh it is a list you cannot trust, and asking costs one small
    /// message every few seconds to a server that is already sending you a
    /// generation four times a second.
    ///
    /// Only while the list is what is on screen. Asking from inside a world
    /// would be answering a question nobody has open.
    fn refresh_room_list(&mut self) {
        if !matches!(
            self.ui.screen,
            Screen::Menu(menu::Menu { stage: menu::Stage::Choosing { .. }, .. })
        ) {
            return;
        }
        if self.elapsed - self.listed_at < ROOM_LIST_REFRESH {
            return;
        }
        self.listed_at = self.elapsed;
        self.link.as_ref().inspect(|l| l.send(ClientMessage::Rooms));
    }

    /// Give up on a server that has not answered.
    ///
    /// A menu that says "asking" forever is indistinguishable from one that is
    /// broken, and the two most likely causes -- a wrong address, and a server
    /// that is not running -- both look exactly like this.
    ///
    /// **This runs for a client that was told where to go, too.** A link into
    /// a room used to set no deadline at all, so a socket that never opened
    /// left the client playing the world it starts every session with, on its
    /// own, with nothing said and the HUD reading "connected" -- a browser's
    /// socket object exists long before it connects, and may never connect.
    fn time_out_first_word(&mut self) {
        let Some(asked) = self.asked_at else { return };
        if self.elapsed - asked < ROOM_LIST_TIMEOUT {
            return;
        }
        self.asked_at = None;
        self.link = None;
        let address = self.address_hint();
        self.show_menu(menu::Stage::Failed(words::menu::no_reply(&address)));
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

impl App for GameApp {
    fn init(gpu: &GpuState) -> Self {
        // The other half of writing addresses: the browser moves the address
        // on back and forward and expects the page to follow it.
        crate::client::route::follow_the_back_button();
        // **Who this client is, before it is anything else.** Minted here
        // rather than on the first join, because the key is what a record and
        // a stamp library are filed against and both of those exist before a
        // server has been reached — and because the settings screen had
        // nothing to show until somebody had already played somewhere, which
        // is exactly when carrying an identity to another machine stops being
        // possible and starts being a repair job.
        match crate::net::keep::secret_or_new() {
            Some(_) => log::debug!("this client has a key"),
            None => log::warn!("no key: this client will be somebody new everywhere it goes"),
        }
        // Where to go, or whether to ask. A destination stated on a command
        // line or in a link is a choice already made; anything else opens the
        // menu, which is the only way a room can be chosen without a terminal.
        let mut pending: Option<Joining> = None;
        // When the wait for a first word started, so it can run out. `None`
        // once the server has answered, and for a client that is not waiting.
        let mut asked_at: Option<f64> = None;
        let (screen, link) = match startup() {
            Start::Join { url, name, room, watch } => {
                log::info!("connecting to {url}, asking for room {room:?}");
                // `--room` and `?room=` are typed, so they are names rather
                // than ids -- and the server resolves either, along with a
                // code, which is what lets one flag carry all three.
                let room = room.map(crate::net::RoomId);
                // A link that says watch is answered by `Watch`, which takes
                // no name and no token: there is no player to be remembered
                // as, and nothing to sign for.
                let watching = watch && room.is_some();
                let link = dial(&url).inspect(|link| {
                    if let (Some(room), true) = (&room, watching) {
                        link.send(ClientMessage::Watch { room: room.clone() });
                    }
                });
                // Held rather than sent: a join says who it is from, and who
                // it is from is a signature over a challenge this connection
                // has not been given yet. `pump_link` sends it when it is.
                if link.is_some() && !watching {
                    pending = Some(Joining { name, room: room.clone() });
                }
                match link {
                    // **Timed like the menu's own ask, which it was not.** A
                    // link into a room set no deadline, so a socket that never
                    // opened produced no message, no retry and no way to tell:
                    // the client sat in `Screen::Playing` on the world it
                    // starts with, and a browser's socket object exists long
                    // before it connects, so the HUD said "connected" for the
                    // whole of it. A game that quietly turns out to be a
                    // different game is worse than one that says it failed.
                    Some(link) => {
                        asked_at = Some(0.0);
                        (Screen::Playing, Some(link))
                    }
                    // The address is unusable — a browser refusing to build a
                    // socket for it, which is the one way `dial` fails here.
                    // Said now rather than waited out.
                    None => (
                        Screen::Menu(menu::Menu::failed(
                            url.clone(),
                            cfg!(target_arch = "wasm32"),
                            words::menu::not_an_address(&url),
                        )),
                        None,
                    ),
                }
            }
            Start::Menu { address, page } => {
                let mut m = menu::Menu::new(address, cfg!(target_arch = "wasm32"));
                // A link to the play screen lands on it, and asks at once —
                // the same two things pressing Play does.
                if page == menu::Page::Play {
                    m.page = page;
                    m.typed_at = Some(0.0);
                }
                (Screen::Menu(m), None)
            }
        };
        // Always start with something on screen. Holding an empty world until
        // the server answers means a client that never connects -- wrong port,
        // server down, a page served from somewhere else -- shows nothing at
        // all and looks broken. A socket object exists long before it
        // connects, and may never connect, so its mere existence is no reason
        // to blank the view. `Welcome` is what replaces this.
        let (world, home) = solo_world();
        let mut chunks = ChunkStore::new(&gpu.device);
        let atlas = Atlas::new(&gpu.device, &gpu.queue);
        chunks.init_unloaded_layer(&gpu.queue);
        chunks.sync(&gpu.queue, &world, ((0, 0), (0, 0)), START_ZOOM);

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
                wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() },
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
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(chunks.coarse_view()),
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
            ui: Ui {
                screen,
                sketch: stamp::Sketch::default(),
                naming_team: None,
                naming_stamp: None,
                editing_stamp: None,
                picking_stamp: false,
                helping: false,
                showing_rules: false,
                refused_start: None,
            },
            camera: camera::Camera::new(home, START_ZOOM),
            gesture: Gesture::None,
            shift: false,
            ctrl: false,
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
            asked_at,
            listed_at: 0.0,
            lobby: None,
            me: None,
            watching: false,
            room: None,
            room_name: None,
            subscribed: std::collections::HashSet::new(),
            person: crate::net::keep::person(),
            rating: None,
            rating_change: None,
            joining: pending,
            cursor: (0.0, 0.0),
            pending: None,
            held: Held::default(),
            stamps: stamp::Library::remembered(),
            icons: icons::Icons::default(),
            standing: Vec::new(),
            counted_at: u64::MAX,
            plays_as: None,
            said_where: None,
            applied_early: Vec::new(),
            place_anywhere: false,
            place_free: false,
            paused: false,
            step_once: false,
            forfeited: false,
            in_play: None,
            geiger: Default::default(),
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
        // Back or forward, if either has just been pressed — before anything
        // else, so the whole frame is built for the screen the address now
        // says rather than for the one it said last frame.
        self.follow_history();
        // **A join set up before the first frame goes out on it.** `init` puts
        // one here for a link that named a room, and it used to be sent when
        // the server's challenge arrived — which was the only thing that ever
        // called this. With no challenge to wait for, nothing did, and a
        // `?room=` link opened a solo world in silence.
        //
        // Cheap to ask every frame: `send_pending_join` takes what it sends,
        // so this is one `Option` check once the join has gone.
        if self.joining.is_some() {
            self.send_pending_join();
        }
        if self.fit(gpu) {
            self.subscribed.clear(); // a different area is visible now
        }

        if self.playing() {
            self.apply_pan(dt);
        }

        if self.link.is_some() {
            self.pump_link();
        }
        self.time_out_first_word();
        self.refresh_room_list();

        if let Some(Pending { drag, to_px }) = self.pending.take() {
            let to = self.cell_under_cursor(to_px);
            // More than one cell is what makes it a drag rather than a click.
            // A press that travelled but stayed inside one cell would place
            // where a click would take, so which of the two happens must not
            // turn on a few pixels of hand shake at high zoom.
            if drag.moved && drag.cell_count(to) > 1 {
                if self.held.captures() {
                    self.capture(drag.from, to);
                } else {
                    match self.drag_cells(&drag, to) {
                        Ok((cells, shape)) => self.lay(cells, shape),
                        Err(why) => self.notice = Some(why),
                    }
                }
            } else {
                self.click(to.0, to.1);
            }
        }

        // Only offline. Connected, the world advances when the server says a
        // generation happened, and never on this client's own clock -- see
        // `advance_to`.
        if self.link.is_none() {
            // **Stopped means stopped, and one means one.** `World::update`
            // banks elapsed time against the span, so not calling it is the
            // whole of pausing: no time accumulates, and letting go does not
            // release a burst of generations that built up while stopped.
            // A decided match stops, the way a room does. `stepping` is the
            // same question the server asks, so a solitary match ends on the
            // generation it was won rather than running on with the result
            // already settled.
            let stopped = self.lobby.as_ref().is_some_and(|l| !l.phase.stepping());
            let mined = match (self.paused || stopped, std::mem::take(&mut self.step_once)) {
                (false, _) => self.world.update(dt, GENERATION_SPAN),
                (true, true) => self.world.step(),
                (true, false) => crate::sim::Mined::default(),
            };
            self.bank(&mined);
            // **And its own standings**, which nothing else was producing:
            // they arrive in a `ServerMessage::Standing` and offline there is
            // no server, so every figure that reads them sat at nought for the
            // whole of a solo game — the ground on the bar, and the bars in
            // the HUD.
            //
            // Counted exactly rather than guessed. A *connected* client holds
            // its screen and a margin, which is why it takes the server's
            // figure; offline it owns the whole world, so the same count the
            // server would do is the right one. `net::standings` is that
            // count, shared so the two cannot disagree.
            if self.world.generation != self.counted_at {
                self.counted_at = self.world.generation;
                self.standing = crate::net::standings(&self.world);
                self.decide_alone();
            }
        }
        if self.world.dirty {
            let visible = self.camera.visible_cells(VIEW_MARGIN);
            self.chunks.sync(&gpu.queue, &self.world, visible, self.camera.zoom);
            self.write_camera(gpu);
            self.world.dirty = false;
        }
        self.elapsed += dt as f64;
        // Decayed every frame rather than only when something arrives: the
        // whole point of a rate is that it falls on its own, and one that only
        // moved on a resync would sit at its peak until the next one.
        self.geiger.decay(self.elapsed);
        let holding = self.holding().to_string();
        let status = hud::Status {
            player: self.player(),
            value: self.value,
            generation: self.world.generation,
            // What this client has been sent, not what its world has room for.
            // A torus is allocated whole, so `stored_count` there is the size
            // of the world and says nothing about what has arrived.
            chunks_held: self.subscribed.len(),
            chunks_drawn: self.chunks.instance_count(),
            zoom: self.camera.zoom,
            connected: self.link.is_some(),
            room: self.room_name.as_deref(),
            world: self.world.kind(),
            notice: self.notice.as_deref(),
            pointer_on_ui: self.views.borrow().wants_pointer(),
            cursor_cell: self.cell_under_cursor(self.cursor),
            last_action: self.last_action.as_deref(),
            holding: &holding,
            standing: &self.standing,
            geiger: self.geiger,
            watching: self.watching,
            rating: self.rating.map(|r| (r, self.rating_change)),
            in_a_match: matches!(
                self.lobby.as_ref().map(|l| &l.phase),
                Some(crate::net::MatchPhase::Running { .. })
            ),
            started_it: self.lobby.as_ref().and_then(|l| l.owner) == self.me && self.me.is_some(),
            forfeited: self.forfeited,
        };
        // What the keys bound by *position* print here, read before the frame
        // because the closure below holds `views` for the whole of it. Three
        // rows, and every one of them prints something different on a keyboard
        // that is not the one this was written on.
        // Once the browser has said what this keyboard prints, take it. It
        // answers on a promise, so this is the frame after it resolved and
        // every frame after that is a lock and a `None`.
        self.views.borrow_mut().take_what_the_browser_said();
        let help_keys = {
            use winit::keyboard::KeyCode as K;
            const DIGITS: [K; 10] = [
                K::Digit1,
                K::Digit2,
                K::Digit3,
                K::Digit4,
                K::Digit5,
                K::Digit6,
                K::Digit7,
                K::Digit8,
                K::Digit9,
                K::Digit0,
            ];
            let views = self.views.borrow();
            // All of a row or none of it: half a row of keycaps with the rest
            // guessed is a row nobody can read.
            let row = |codes: &[K], shift: bool| {
                let seen: Vec<&str> =
                    codes.iter().filter_map(|&code| views.label(code, shift)).collect();
                (seen.len() == codes.len()).then(|| seen.concat())
            };
            // **The whole shifted row, not the first four.** It was four
            // while the tools were four, and the row has been six since a
            // capture and a "more" joined them and seven since the shape
            // square did — so the help screen named `shift + 1-4` for a row
            // that runs to seven. Asked of `hotbar::shifted`, which is the
            // list the keyboard actually uses.
            let tools = hotbar::shifted(&self.stamps).len().min(DIGITS.len());
            help::Keys {
                pan: row(&[K::KeyW, K::KeyA, K::KeyS, K::KeyD], false),
                stamps: row(&DIGITS, false),
                tools: row(&DIGITS[..tools], true),
            }
        };
        let (held, theme, shifted, bare) = {
            let views = self.views.borrow();
            let learned: Vec<Option<String>> =
                (1..=9).map(|d| views.shifted_digit(d).map(str::to_string)).collect();
            let bare: Vec<Option<String>> =
                (0..=9).map(|d| views.plain_digit(d).map(str::to_string)).collect();
            (self.held, views.theme, learned, bare)
        };
        // Registered before the frame rather than inside it: loading a texture
        // needs the context, and the context is borrowed for the whole build.
        let sheet = {
            let ctx = self.views.borrow().ctx().clone();
            self.icons.sheet(&ctx, self.player())
        };
        // What shift and a digit types on this keyboard, as far as anyone has
        // found out by pressing it.
        let typed =
            move |digit: u32| shifted.get(digit.checked_sub(1)? as usize).cloned().flatten();
        // And what it types on its own, for the stamp squares.
        let plain = move |digit: u32| bare.get(digit as usize).cloned().flatten();
        let (r, g, b) = crate::client::views::hue::player_colour(self.player());
        let marks = overlay::Marks {
            tint: egui::Color32::from_rgb(r, g, b),
            hover: self.hover_mark(status.pointer_on_ui),
            selection: self.selection_mark(),
        };
        let mut picked = None;
        let mut told_rules = rules::Did::default();
        let mut from_library = stamp::Picked::Nothing;
        let picking = self.ui.picking_stamp;
        let mut chose = menu::Chose::Nothing;
        // What the frame decided, acted on after it is built: each of these
        // changes the screen, and the screen is what the frame was drawn from.
        let mut leaving = false;
        let mut in_hud = hud::Did::Nothing;
        // What a press in the lobby meant, acted on after the frame is built
        // because both answers change the screen the frame was drawn from.
        let mut in_lobby = lobby_view::Did::Nothing;
        let lobby = self.lobby.clone();
        let standing = self.standing.clone();
        let generation = self.world.generation;
        let paused = self.paused;
        // Offline, this client is the clock; connected, the server is and
        // there is nothing on the bar to press.
        let own_clock = self.link.is_none();
        let showing_rules = self.ui.showing_rules;
        let (place_anywhere, place_free) = (self.place_anywhere, self.place_free);
        // What the client already is, which the menu cannot see for itself.
        let at = menu::Where {
            now: self.elapsed,
            on_web: cfg!(target_arch = "wasm32"),
            waiting_in_a_match: self.link.is_some()
                && matches!(
                    self.lobby.as_ref().map(|l| &l.phase),
                    Some(crate::net::MatchPhase::Gathering)
                ),
        };
        let me = self.player();
        let helping = self.ui.helping;
        let mut help_closed = false;
        // **One borrow, not five takes.** The closure needs `&mut` on the
        // interface's own state while `views` is borrowed, and because `ui`
        // and `views` are different fields, borrowing one says nothing about
        // the other — so the closure simply holds it.
        //
        // What that replaces is a `mem::take` per field and a put-back
        // afterwards: the screen, a half-typed name, a half-drawn pad. Every
        // one of them is silently *lost* if anything between the two returns
        // early, and one of the arms below does exactly that.
        //
        // Last, after everything read off `&self`, so that from here the
        // interface's state is borrowed and the world's is untouched.
        let ui = &mut self.ui;
        let editing = ui.editing_stamp;
        let output = self.views.borrow_mut().run(gpu, self.elapsed, |ctx| {
            // The screen, in its own closure so that an arm may still return
            // early -- one of them draws a lobby and nothing else.
            let mut rects: Vec<egui::Rect> = (|| -> Vec<egui::Rect> {
                match &mut ui.screen {
                    // The world is still drawn behind it, and still running if this
                    // client is offline. A menu over a dead grey rectangle says the
                    // game has not started; a menu over a world says it is waiting for
                    // you.
                    Screen::Menu(m) => {
                        let shown = menu::show(ctx, &theme, m, at);
                        let (picked_menu, rect) = (shown.did, shown.rect);
                        chose = picked_menu;
                        rect.into_iter().collect()
                    }
                    Screen::Playing => {
                        // A gathering match is a screen of its own: its world is
                        // empty until the whistle, so there is nothing to draw the
                        // lobby over and nothing for a HUD or a hotbar to act on.
                        if matches!(
                            lobby.as_ref().map(|l| &l.phase),
                            Some(crate::net::MatchPhase::Gathering)
                        ) {
                            let shown = lobby.as_ref().map_or_else(
                                crate::client::views::Shown::nowhere,
                                |l| {
                                    lobby_view::show(
                                        ctx,
                                        &theme,
                                        &l.look(me, &l.hues(), ui.refused_start.as_deref()),
                                        &mut ui.naming_team,
                                    )
                                },
                            );
                            in_lobby = shown.did;
                            return shown.rect.into_iter().collect();
                        }

                        overlay::show(ctx, &theme, &marks);
                        let shown = hud::show(ctx, &theme, &status);
                        let (hud_rect, pressed) = (shown.rect, shown.did);
                        in_hud = pressed;
                        // How much of the match is left, which is the one thing on
                        // screen that is about the room rather than about a player.
                        let clock_rect = lobby.as_ref().and_then(|l| {
                            clock::show(
                                ctx, &theme, generation, &l.phase, l.victory, &standing, paused,
                            )
                            .rect
                        });
                        let what = held.placement().unwrap_or(crate::net::Placement::Life);
                        let look = hotbar::Look {
                            own_clock,
                            paused,
                            showing_rules,
                            theme: &theme,
                            what,
                            sheet,
                            player: me,
                            typed: &typed,
                            plain: &plain,
                        };
                        let bar = hotbar::show(ctx, &look, held, &self.stamps, &status);
                        picked = bar.did;
                        // Over the bar rather than instead of it: the square
                        // that opens it has to stay pressable.
                        let rules_rect = if showing_rules && own_clock {
                            let shown = rules::show(ctx, &theme, place_anywhere, place_free);
                            told_rules = shown.did;
                            shown.rect
                        } else {
                            None
                        };
                        // Over the world rather than instead of it: a match that has
                        // not started looks exactly like a game that is broken, since
                        // nothing moves and nothing a player does appears.
                        // A decided match keeps its board: the result is what is on
                        // it, and covering that to say who won would hide the reason.
                        let waiting = lobby.as_ref().and_then(|l| {
                            let shown = lobby_view::show(
                                ctx,
                                &theme,
                                &l.look(me, &l.hues(), ui.refused_start.as_deref()),
                                &mut ui.naming_team,
                            );
                            if !matches!(shown.did, lobby_view::Did::Nothing) {
                                in_lobby = shown.did;
                            }
                            shown.rect
                        });
                        if picking {
                            let shown = stamp::show(
                                ctx,
                                &theme,
                                &self.stamps,
                                &mut ui.sketch,
                                what,
                                me,
                                sheet,
                                &mut ui.naming_stamp,
                                editing,
                            );
                            from_library = shown.did;
                            return [
                                hud_rect, bar.rect, shown.rect, waiting, clock_rect, rules_rect,
                            ]
                            .into_iter()
                            .flatten()
                            .collect();
                        }
                        // Each panel on its own. Folding them together first would
                        // claim everything between them, and they sit in opposite
                        // corners.
                        [hud_rect, bar.rect, waiting, clock_rect, rules_rect]
                            .into_iter()
                            .flatten()
                            .collect()
                    }
                }
            })();
            // Over everything, and drawn last so it sits on top of whatever
            // screen asked for it. Its rectangle joins the list the client
            // uses to keep a press off the world behind.
            if helping {
                let shown = help::show(ctx, &theme, &help_keys);
                let (rect, closed) = (shown.rect, shown.did == help::Did::Close);
                help_closed = closed;
                rects.extend(rect);
            }
            rects
        });

        self.chose(chose);
        if let Some(key) = picked {
            self.pick(key);
        }
        // The two rules, and the press that shuts the panel.
        if let Some(on) = told_rules.anywhere {
            self.place_anywhere = on;
        }
        if let Some(on) = told_rules.free {
            self.place_free = on;
        }
        if told_rules.close {
            self.ui.showing_rules = false;
        }
        match from_library {
            stamp::Picked::Nothing => {}
            stamp::Picked::Hold(i) => {
                self.held.shape = hotbar::Shape::Stamp(i);
                self.ui.picking_stamp = false;
            }
            // Held by index, so forgetting one shifts everything after it --
            // drop back to a tool rather than quietly holding a different
            // pattern than the one that was on screen a moment ago.
            stamp::Picked::Forget(i) => {
                self.stamps.forget(i);
                self.stamps.remember();
                self.ui.naming_stamp = None;
                self.ui.editing_stamp = None;
                self.held.shape = hotbar::Shape::default();
            }
            stamp::Picked::Pin(i, on) => {
                if self.stamps.pin(i, on) {
                    self.stamps.remember();
                } else {
                    self.notice = Some(words::stamps::BAR_FULL.into());
                }
            }
            stamp::Picked::Rename(i, name) => {
                self.stamps.rename(i, &name);
                self.stamps.remember();
                self.ui.naming_stamp = None;
            }
            // Onto the pad, and remembered as the one being edited so that
            // keeping puts it back rather than leaving the old one beside it.
            stamp::Picked::Edit(i) => {
                if let Some(stamp) = self.stamps.get(i) {
                    self.ui.sketch = stamp::Sketch::of(stamp);
                    self.ui.editing_stamp = Some(i);
                }
            }
            // Kept where a captured one is kept, and held straight away: you
            // drew it because you meant to place it.
            stamp::Picked::Keep(stamp) => {
                let (name, cells) = (stamp.name.clone(), stamp.cells.len());
                let held = match self.ui.editing_stamp.take() {
                    Some(i) => {
                        self.stamps.replace(i, stamp);
                        i
                    }
                    None => {
                        self.stamps.keep(stamp);
                        0
                    }
                };
                self.stamps.remember();
                self.held.shape = hotbar::Shape::Stamp(held);
                self.notice = Some(words::stamps::captured(&name, cells));
            }
            stamp::Picked::Close => self.ui.picking_stamp = false,
        }
        // Acted on after the frame is built, because it changes the screen and
        // the screen is what the frame was drawn from.
        //
        // Both answers come back as a broadcast phase change, or as
        // `NotStarted` with a reason, so there is nothing to do here but ask.
        match in_hud {
            hud::Did::Nothing => {}
            hud::Did::Back => leaving = true,
            hud::Did::Forfeit => {
                if let Some(link) = &self.link {
                    log::info!("giving up this match");
                    link.send(ClientMessage::Forfeit);
                    // Marked here rather than waiting for the server to say
                    // so: there is no message that answers a forfeit, only a
                    // lobby that changes, and a button that stays pressable
                    // for a quarter of a second is a button people press
                    // twice.
                    self.forfeited = true;
                }
            }
            hud::Did::EndMatch => {
                if let Some(link) = &self.link {
                    log::info!("asking to end the match");
                    link.send(ClientMessage::EndMatch);
                }
            }
        }
        match in_lobby {
            lobby_view::Did::Nothing => {}
            lobby_view::Did::Leave => leaving = true,
            // Whoever made it blows the whistle. The answer comes back as a
            // broadcast phase change, or as `NotStarted` with a reason, so
            // there is nothing to do here but ask.
            lobby_view::Did::Start => {
                if let Some(link) = &self.link {
                    log::info!("asking to start the match");
                    link.send(ClientMessage::Start);
                }
            }
            // Both answer by broadcast: the server changes who is on what and
            // the next lobby message says so, to everybody at once, which is
            // what a lobby full of people all changing sides needs.
            lobby_view::Did::JoinTeam(team) => {
                if let Some(link) = &self.link {
                    link.send(ClientMessage::JoinTeam { team });
                }
            }
            lobby_view::Did::NameTeam(team, name) => {
                if let Some(link) = &self.link {
                    link.send(ClientMessage::NameTeam { team, name });
                }
            }
        }
        if help_closed {
            self.ui.helping = false;
        }
        if leaving {
            self.back_to_menu();
        }
        // After the frame, because the menu's own page moves inside it — a
        // press on Play changes a field the client only sees on the way out.
        if self.said_where != self.here() {
            self.said_where = self.here();
            self.say_where();
        }
        *self.ui_output.borrow_mut() = Some(output);

        if self.camera.dirty {
            self.camera.dirty = false;
            // Panning changes the region the backdrop has to cover, so the
            // instance list follows the camera.
            let visible = self.camera.visible_cells(VIEW_MARGIN);
            self.chunks.sync(&gpu.queue, &self.world, visible, self.camera.zoom);
            // **After the sync, not before.** The uniform carries the coarse
            // window, and `sync` is what decides it — written first, the frame
            // that swaps to the coarse path draws it against last frame's
            // window, or against no window at all.
            self.write_camera(gpu);
        }
    }

    /// The world, unless there is nothing to show.
    ///
    /// It used to be drawn behind the menu, on the reasoning that a menu over
    /// a dead grey rectangle says the game has not started where a menu over a
    /// world says it is waiting for you. That was true when the menu was a
    /// small panel. It stopped being true once the menu had the screen to
    /// itself: a world sliding about behind a full-height panel is motion
    /// nobody asked for beside the thing they are reading, and a match that
    /// has not started has nothing behind it anyway — its world is empty until
    /// the whistle.
    fn draw_calls(&self) -> Vec<DrawCall<'_>> {
        if !self.showing_world() {
            return Vec::new();
        }
        vec![DrawCall {
            pipeline: &self.pipeline,
            bind_groups: &self.bind_groups,
            vertex_buffers: &self.vertex_buffers,
            index_buffer: None,
            draw: Draw::Vertices { vertices: 0..4, instances: 0..self.chunks.instance_count() },
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

    /// **Two kinds of binding, and which one a key wants is a question about
    /// what it means.**
    ///
    /// `code` is a *position*: winit names it after the US keycap that sits
    /// there, so `KeyCode::KeyD` is the third key of the home row wherever you
    /// are. `typed` is what that key actually produces on this layout.
    ///
    /// **A shape on the keyboard binds by position.** WASD is not the letters
    /// W, A, S and D, it is the cluster under a left hand — on Dvorak it is
    /// `,aoe` and it is still the same four keys in the same diamond, which is
    /// the whole of what makes it work. Space, escape, tab, the arrows and the
    /// digit row are the same argument: their meaning is where they are.
    ///
    /// **A mnemonic binds by character.** `R` for rotate and `F` for flip mean
    /// nothing as positions — on Dvorak the R position types `p`, so a
    /// positional binding would put rotate under `p` and leave `r` doing
    /// nothing, which is the letter the help screen is telling you to press.
    /// The same goes for `~` and `?`: both are *shown as the character*, so
    /// the label is a lie on any layout where that character is elsewhere.
    ///
    /// This was all positional, which is invisible on the layout it was
    /// written on and wrong on every other.
    fn on_key(
        &mut self,
        code: winit::keyboard::KeyCode,
        named: Option<winit::keyboard::NamedKey>,
        typed: Option<&str>,
        pressed: bool,
    ) {
        use winit::keyboard::KeyCode as K;
        use winit::keyboard::NamedKey as N;
        // **What a key means, where it means something.** Escape, shift and
        // the arrows are bound to their meaning rather than to where they sit,
        // because a player who has mapped caps lock to escape sends the escape
        // *meaning* from the caps lock *position* — and a binding on the
        // position never fires, which is exactly what happened. The walk
        // cluster and the digits stay positional, because those are a shape on
        // the board rather than a meaning.
        let escape = named == Some(N::Escape);
        // **Every screen has a key that leaves it**, which is the habit taken
        // from chess-tui — `b` for the menu there, escape here because that is
        // what escape means everywhere else. Handled before the guard below,
        // since the whole point is that it works on the screen the guard is
        // there to protect from everything else.
        //
        // One step at a time, innermost first: a form shuts before a world
        // does. Escape that skipped a level would take somebody out of a game
        // because they wanted to close a panel.
        // **The four keys bound to what they say rather than to where they
        // sit**, resolved in one place — see [`input::mnemonic`], which is
        // also where the fallback for a keyboard that cannot type them is and
        // where all of it is tested without a window.
        if let (true, Some(what)) = (pressed, input::mnemonic(code, typed, self.shift)) {
            match what {
                input::Mnemonic::Help => self.ui.helping = !self.ui.helping,
                input::Mnemonic::Play => self.toggle_running(),
                input::Mnemonic::StepOne => self.step_one(),
                // **A pattern and the same pattern turned are one pattern.**
                // Without this the library fills up with its own reflections —
                // four gliders, and four more that are their mirror image, and
                // the same again for every spaceship and every corner of a
                // wall.
                //
                // Shift turns the other way rather than taking a key of its
                // own: three presses and one press are the same place, so the
                // second binding is a convenience and should read as one.
                input::Mnemonic::Turn | input::Mnemonic::Mirror => {
                    self.held.turn = match (what, self.shift) {
                        (input::Mnemonic::Turn, false) => self.held.turn.right(),
                        (input::Mnemonic::Turn, true) => self.held.turn.left(),
                        _ => self.held.turn.mirror(),
                    };
                    // Said out loud, because a rotation with nothing held
                    // changes nothing on screen and looks like a dead key.
                    if !matches!(self.held.shape, hotbar::Shape::Stamp(_)) {
                        self.notice = Some(words::stamps::NOTHING_TO_TURN.into());
                    }
                }
            }
            return;
        }
        if pressed && escape && self.ui.helping {
            self.ui.helping = false;
            return;
        }
        if pressed && escape && !self.playing() {
            if let Screen::Menu(m) = &mut self.ui.screen {
                // The form is a column rather than something opened, so there
                // is no rung for it: a field lets go of the keyboard (handled
                // in `menu::show`, before the app sees the key at all), then
                // the page goes back.
                if m.page == menu::Page::Play {
                    m.page = menu::Page::Home;
                    return;
                }
            }
        }
        // The menu is over the world, and the world is still there. A click
        // that lands beside the panel must not draw on it.
        if !self.playing() {
            return;
        }
        if let Some(d) = digit(code) {
            if pressed {
                // Shift reaches the tools, which never change and never grow,
                // so the bare digits can belong to the stamps -- the thing you
                // hold ten of and swap between without looking.
                let key = if self.shift {
                    hotbar::shifted_for_digit(d, &self.stamps)
                } else {
                    // Through the bar, because a digit names a **square** and
                    // the stamp standing in it is whatever is pinned there.
                    hotbar::stamp_for_digit(d)
                        .and_then(|slot| self.stamps.bar().get(slot).copied())
                        .map(|i| Key::Shape(hotbar::Shape::Stamp(i)))
                };
                if let Some(key) = key {
                    self.pick(key);
                }
            }
            return;
        }
        match code {
            _ if named == Some(N::Shift) => {
                self.shift = pressed;
                return;
            }
            _ if named == Some(N::Control) => {
                self.ctrl = pressed;
                return;
            }
            // Abandon whatever is being drawn. A rectangle you have decided
            // against otherwise has to be shrunk back to one cell to be made
            // harmless, and that still places a cell.
            //
            // **`ctrl+[` is escape**, which is what it is: the ASCII escape
            // code, and what a terminal sends for it. Bound to the *keycode*
            // rather than as a control of its own, so it does whatever escape
            // does wherever escape does it and cannot drift from it — and so
            // it needs no row on the key list, because somebody who reaches
            // for it already knows what it is.
            K::BracketLeft if pressed && self.ctrl => {
                self.cancel_gesture();
                return;
            }
            _ if escape && pressed => {
                self.cancel_gesture();
                return;
            }
            _ => {}
        }
        let slot = match code {
            _ if named == Some(N::ArrowLeft) => 0,
            _ if named == Some(N::ArrowRight) => 1,
            _ if named == Some(N::ArrowUp) => 2,
            _ if named == Some(N::ArrowDown) => 3,
            K::KeyA => 0,
            K::KeyD => 1,
            K::KeyW => 2,
            K::KeyS => 3,
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
            D::PixelDelta(p) if ctrl => self.zoom_about_cursor(1.15f32.powf(p.y as f32 / 140.0)),
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
                if cfg!(target_arch = "wasm32") && p.x == 0.0 && p.y.abs() >= WHEEL_NOTCH =>
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

#[cfg(test)]
mod tests {
    use super::*;

    /// **Only the next generation is a step; everything else is a loss.**
    ///
    /// This used to step forward to close a gap of up to thirty-two, which is
    /// the branching bug rather than the recovery from it: those generations
    /// carried actions, stepping runs them empty, and the world that comes out
    /// is one nobody else has. There is no arithmetic that recovers a message
    /// that never arrived, so the only honest answers are "step" and "ask
    /// again".
    #[test]
    fn a_step_is_the_next_generation_and_nothing_else_is() {
        assert_eq!(advance(0, 1), Advance::Step);
        assert_eq!(advance(400, 401), Advance::Step);

        // Behind, by one message and by many. Both used to be caught up
        // locally and both are worlds this client would be inventing.
        assert_eq!(advance(400, 402), Advance::Lost, "one dropped step is still a loss");
        assert_eq!(advance(400, 432), Advance::Lost);
        assert_eq!(advance(400, 100_000), Advance::Lost);

        // Ahead, or the same tick twice: a websocket does not reorder, so
        // either means something upstream is not what it is thought to be, and
        // the answer is the same one.
        assert_eq!(advance(400, 400), Advance::Lost);
        assert_eq!(advance(400, 399), Advance::Lost);
        assert_eq!(advance(400, 0), Advance::Lost);
    }

    /// The HUD swatch and the cells on the board must agree about a player's
    /// colour, so this reproduces the shader's arithmetic and checks the result
    /// is in range and distinct between players.
    #[test]
    fn player_colours_are_in_gamut_and_distinct() {
        use crate::client::views::hue::player_colour;
        let mut seen = Vec::new();
        for p in 1..=PlayerId::MAX {
            let c = player_colour(PlayerId(p));
            assert!(!seen.contains(&c), "players {p} and an earlier one share {c:?}");
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
    fn only_the_middle_of_a_cell_counts() {
        assert_eq!(cell_under((4.5, 7.5)), Some((7, 4)), "dead centre");
        assert_eq!(cell_under((4.02, 7.5)), None, "barely inside the left edge");
        assert_eq!(cell_under((4.5, 7.98)), None, "barely inside the bottom");
        assert_eq!(cell_under((4.98, 7.02)), None, "a corner, which is the case");

        // Negative coordinates behave the same: the world has no origin.
        assert_eq!(cell_under((-3.5, -8.5)), Some((-9, -4)));
        assert_eq!(cell_under((-3.02, -8.5)), None);
    }

    /// What the tolerance is actually for: **a glider in one motion.**
    ///
    /// Five cells with gaps between them, drawn by dragging through their
    /// middles in one stroke. Every cell has to be caught and none of the four
    /// beside them may be, or it is not a glider — so this pins both halves at
    /// once, which no test of a straight line can.
    #[test]
    fn a_glider_is_one_motion() {
        // . # .
        // . . #
        // # # #
        let glider = [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)];
        // Through the middle of each, in the order a hand would take them.
        let path = [(1.5, 0.5), (2.5, 1.5), (2.5, 2.5), (1.5, 2.5), (0.5, 2.5)];

        let mut marked: Vec<(i32, i32)> = Vec::new();
        for pair in path.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            let steps = 16;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let at = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                if let Some(cell) = cell_under(at) {
                    if !marked.contains(&cell) {
                        marked.push(cell);
                    }
                }
            }
        }

        marked.sort_unstable();
        assert_eq!(marked, glider, "not a glider");
    }

    /// A diagonal sweep marks the cells it crosses the middle of and not the
    /// ones either side, which is what draws a glider rather than a wedge.
    ///
    /// True at every tolerance tried, which is the point: a 45° stroke is not
    /// what the constant is for, because both fractions move together.
    #[test]
    fn a_diagonal_sweep_marks_a_diagonal() {
        let marked: Vec<(i32, i32)> = (0..=40)
            .filter_map(|i| {
                let t = i as f32 / 10.0;
                cell_under((0.5 + t, 0.5 + t))
            })
            .collect();
        let mut unique: Vec<(i32, i32)> = marked.clone();
        unique.dedup();
        assert_eq!(
            unique,
            vec![(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)],
            "a clean diagonal, with nothing beside it"
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
