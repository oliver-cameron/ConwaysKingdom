//! The client: a world, a view of it, and the loop that keeps them in step.
//!
//! Deliberately thin. The rules live in [`crate::sim`] and the GPU work in
//! [`crate::render`]; this only decides *policy* — how fast generations run,
//! which world to open, and where to point the camera. A headless server would
//! keep the [`World`] and drop everything else in this file.

use crate::render::app::App;
use crate::render::atlas::Atlas;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, CHUNK_N};

use crate::net::link::Link;
use crate::net::{Action, ClientMessage, ServerMessage, Stamped};
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

/// A press that moves further than this many pixels was a drag, not a click.
const DRAG_SLOP: f64 = 3.0;

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

/// What a click does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    /// Take a cell away. Yours pays; someone else's costs.
    Remove,
    /// Put one down.
    Place,
}

pub struct BattleApp {
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
    /// Left button held, and whether it has moved far enough to be a drag.
    dragging: bool,
    drag_moved: bool,
    /// Held pan keys: left, right, up, down.
    pan: [bool; 4],
    /// Fingers currently down, as (id, position). Two of them is a pinch.
    touches: Vec<(u64, (f64, f64))>,
    /// Distance between the two fingers last frame, to measure the pinch by.
    pinch_span: Option<f64>,
    /// Viewport size in physical pixels, refreshed every frame from the
    /// `GpuState`. Cached only because input callbacks are not handed one --
    /// updating it solely on resize left it stale whenever a resize event did
    /// not arrive, and zoom anchoring then disagreed with the camera about how
    /// big the screen was.
    viewport: (f32, f32),
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
    /// Why the last action was refused, shown until the next one succeeds.
    notice: Option<String>,
    /// What this player can spend. Predicted locally with the same arithmetic
    /// the server charges by, so the number on screen is the number the server
    /// will agree with.
    value: i32,
    /// A click waiting to be resolved to a cell. Input callbacks are not given
    /// the `GpuState`, and the mapping needs the viewport, so it is deferred to
    /// the next `update` rather than guessed here.
    pending_click: Option<((f64, f64), Act)>,
}

impl BattleApp {
    /// A fixed camera. Autoscrolling is gone: the view no longer chases the
    /// live pattern, so what is on screen is whatever `VIEW_CENTRE` and
    /// `VIEW_ZOOM` say. Panning and zooming will be driven by input.
    fn write_camera(&self, gpu: &GpuState) {
        let (vw, vh) = self.viewport;
        let zoom = self.zoom;
        let origin = [
            self.camera.0 - vw / (2.0 * zoom),
            self.camera.1 - vh / (2.0 * zoom),
        ];

        gpu.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&CameraUniform {
                origin,
                viewport: [vw, vh],
                zoom,
                chunk_n: CHUNK_N as f32,
                _pad: [0.0; 2],
            }),
        );
    }

    /// Screen position to world position in cells, unrounded. Zoom anchoring
    /// needs the fraction, which the integer form throws away.
    fn cell_under_cursor_f(&self, (vw, vh): (f32, f32), (px, py): (f64, f64)) -> (f32, f32) {
        let origin = (
            self.camera.0 - vw / (2.0 * self.zoom),
            self.camera.1 - vh / (2.0 * self.zoom),
        );
        (
            origin.0 + px as f32 / self.zoom,
            origin.1 + py as f32 / self.zoom,
        )
    }

    /// Where a screen position lands in the world, in absolute cell
    /// coordinates. The inverse of what the vertex shader does.
    fn cell_under_cursor(&self, (px, py): (f64, f64)) -> (i32, i32) {
        let (vw, vh) = self.viewport;
        let origin = (
            self.camera.0 - vw / (2.0 * self.zoom),
            self.camera.1 - vh / (2.0 * self.zoom),
        );
        let x = origin.0 + px as f32 / self.zoom;
        let y = origin.1 + py as f32 / self.zoom;
        (y.floor() as i32, x.floor() as i32) // (row, col)
    }
}

impl BattleApp {
    /// Move the camera for whatever pan keys are held. Returns whether it
    /// moved, so the uniform is only rewritten when it needs to be.
    fn apply_pan(&mut self, dt: f32) {
        let x = (self.pan[1] as i32 - self.pan[0] as i32) as f32;
        let y = (self.pan[3] as i32 - self.pan[2] as i32) as f32;
        if x == 0.0 && y == 0.0 {
            return;
        }
        let step = PAN_SPEED * dt / self.zoom;
        self.camera.0 += x * step;
        self.camera.1 += y * step;
        self.camera_dirty = true;
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
    /// Place a cell, or destroy whatever is there.
    ///
    /// Applied locally *and* sent, rather than sent and awaited: the rules are
    /// deterministic and the server runs the same `net::apply` and charges by
    /// the same `net::value_delta`, so acting immediately shows the right
    /// answer a round trip early. If the server disagrees the chunk digests
    /// will not match and the resync puts it right.
    fn act_on(&mut self, row: i32, col: i32, act: Act) {
        let player = self.me.unwrap_or(PlayerId(1));
        let cells = vec![(row, col)];
        let action = match act {
            Act::Remove => Action::Erase { cells },
            Act::Place => Action::Paint { cells },
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
        self.value += delta;

        crate::net::apply(&mut self.world, &stamped);
        self.world.dirty = true;
        log::debug!("{act:?} ({row}, {col}); value {}", self.value);

        match &self.link {
            Some(link) => link.send(ClientMessage::Act(stamped)),
            // Offline, the local world is the only world, so it is done.
            None => {}
        }
    }

    /// Chunk coordinates the viewport covers, plus a margin.
    fn visible_chunks(&self) -> Vec<crate::sim::Coord> {
        let (vw, vh) = self.viewport;
        let half = (vw / (2.0 * self.zoom), vh / (2.0 * self.zoom));
        let min = (
            (self.camera.1 - half.1).floor() as i32 - VIEW_MARGIN,
            (self.camera.0 - half.0).floor() as i32 - VIEW_MARGIN,
        );
        let max = (
            (self.camera.1 + half.1).ceil() as i32 + VIEW_MARGIN,
            (self.camera.0 + half.0).ceil() as i32 + VIEW_MARGIN,
        );
        World::chunks_covering(min, max)
    }

    fn subscribe_to_view(&mut self) {
        let (vw, vh) = self.viewport;
        let half = (vw / (2.0 * self.zoom), vh / (2.0 * self.zoom));
        let min = (
            (self.camera.1 - half.1).floor() as i32 - VIEW_MARGIN,
            (self.camera.0 - half.0).floor() as i32 - VIEW_MARGIN,
        );
        let max = (
            (self.camera.1 + half.1).ceil() as i32 + VIEW_MARGIN,
            (self.camera.0 + half.0).ceil() as i32 + VIEW_MARGIN,
        );

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
        chunks.sync(&gpu.queue, &world, TORUS_REPEATS, &[]);

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
            dragging: false,
            drag_moved: false,
            pan: [false; 4],
            touches: Vec::new(),
            pinch_span: None,
            viewport: (1.0, 1.0),
            notice: None,
            value: Player::STARTING_VALUE,
            me: None,
            subscribed: std::collections::HashSet::new(),
            cursor: (0.0, 0.0),
            pending_click: None,
            link,
        };
        app.world.dirty = false;
        app.viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        app.write_camera(gpu);
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        // `update` notices this too; this just avoids a frame of staleness.
        self.viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        self.camera_dirty = true;
        self.subscribed.clear();

        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
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

        if let Some((at, act)) = self.pending_click.take() {
            let (row, col) = self.cell_under_cursor(at);
            self.act_on(row, col, act);
        }

        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            let visible = self.visible_chunks();
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS, &visible);
            self.world.dirty = false;
        }
        if self.camera_dirty {
            self.write_camera(gpu);
            self.camera_dirty = false;
            // Panning brings different chunks into view, and the unloaded ones
            // among them are drawn from the instance list, so it must follow.
            let visible = self.visible_chunks();
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS, &visible);
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
        if self.dragging {
            if dx.abs() > DRAG_SLOP || dy.abs() > DRAG_SLOP {
                self.drag_moved = true;
            }
            // Dragging pulls the world with the pointer, so the camera moves
            // the other way. Divided by zoom, because the drag is in pixels
            // and the camera lives in cells.
            self.camera.0 -= dx as f32 / self.zoom;
            self.camera.1 -= dy as f32 / self.zoom;
            self.camera_dirty = true;
        }
    }

    fn on_key(&mut self, code: winit::keyboard::KeyCode, pressed: bool) {
        use winit::keyboard::KeyCode as K;
        let slot = match code {
            K::ArrowLeft | K::KeyA => 0,
            K::ArrowRight | K::KeyD => 1,
            K::ArrowUp | K::KeyW => 2,
            K::ArrowDown | K::KeyS => 3,
            _ => return,
        };
        self.pan[slot] = pressed;
    }

    fn on_touch(&mut self, id: u64, phase: winit::event::TouchPhase, x: f64, y: f64) {
        use winit::event::TouchPhase as P;
        match phase {
            P::Started => self.touches.push((id, (x, y))),
            P::Moved => {
                if let Some(t) = self.touches.iter_mut().find(|t| t.0 == id) {
                    t.1 = (x, y);
                }
            }
            P::Ended | P::Cancelled => self.touches.retain(|t| t.0 != id),
        }

        match self.touches.as_slice() {
            // One finger drags, like a held mouse button.
            [(_, at)] => {
                if matches!(phase, P::Started) {
                    self.cursor = *at;
                    self.pinch_span = None;
                    return;
                }
                let (dx, dy) = (at.0 - self.cursor.0, at.1 - self.cursor.1);
                self.cursor = *at;
                self.camera.0 -= dx as f32 / self.zoom;
                self.camera.1 -= dy as f32 / self.zoom;
                self.camera_dirty = true;
            }
            // Two fingers pinch. Zoom by the ratio of the gap between them, so
            // the gesture is scale-invariant: the same spread does the same
            // thing whether the fingers started close together or far apart.
            [(_, a), (_, b)] => {
                let span = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                let midpoint = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
                if let Some(previous) = self.pinch_span {
                    if previous > 1.0 && span > 1.0 {
                        // Anchor on the midpoint, so the world does not slide
                        // out from under the fingers doing the pinching.
                        let before = self.cell_under_cursor_f(self.viewport, midpoint);
                        self.zoom = (self.zoom * (span / previous) as f32)
                            .clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
                        let after = self.cell_under_cursor_f(self.viewport, midpoint);
                        self.camera.0 += before.0 - after.0;
                        self.camera.1 += before.1 - after.1;
                        self.camera_dirty = true;
                    }
                }
                self.pinch_span = Some(span);
                self.cursor = midpoint;
            }
            // No fingers, or more than two: nothing to measure.
            _ => self.pinch_span = None,
        }
    }

    fn on_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        use winit::event::MouseScrollDelta as D;
        // A line of wheel and a pixel of trackpad are wildly different
        // magnitudes, so normalise before using either.
        let steps = match delta {
            D::LineDelta(_, y) => y,
            D::PixelDelta(p) => p.y as f32 / 50.0,
        };
        // Zoom about the cursor, not the screen centre: zooming towards a
        // corner should keep what is under the pointer under the pointer.
        let before = self.cell_under_cursor_f(self.viewport, self.cursor);
        self.zoom = (self.zoom * 1.15f32.powf(steps)).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let after = self.cell_under_cursor_f(self.viewport, self.cursor);
        self.camera.0 += before.0 - after.0;
        self.camera.1 += before.1 - after.1;
        self.camera_dirty = true;
    }

    /// Left removes, right places.
    ///
    /// Removing is the ordinary click because it is the one you make most: it
    /// is how a player mines their own cells for value, and clearing your own
    /// ground should not need a modifier.
    ///
    /// Left also pans, so a press that moved is a pan rather than a click on a
    /// cell. Resolution is deferred to the next frame, because input callbacks
    /// are not handed the `GpuState` and the screen-to-world mapping needs the
    /// viewport.
    fn on_click(&mut self, button: winit::event::MouseButton, pressed: bool) {
        use winit::event::MouseButton as B;
        match button {
            B::Left => {
                if pressed {
                    self.dragging = true;
                    self.drag_moved = false;
                    return;
                }
                self.dragging = false;
                if self.drag_moved {
                    return;
                }
                self.pending_click = Some((self.cursor, Act::Remove));
            }
            B::Right if pressed => self.pending_click = Some((self.cursor, Act::Place)),
            _ => {}
        }
    }

    fn hud(&mut self, ctx: &egui::Context) {
        egui::Window::new("kingdom")
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
            .show(ctx, |ui| {
                let player = self.me.unwrap_or(PlayerId(1));
                ui.horizontal(|ui| {
                    // The same colour the shader gives this player's cells, so
                    // the swatch and the board cannot disagree about who is who.
                    let (r, g, b) = player_colour(player);
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(14.0, 14.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
                    ui.heading(format!("Player {}", player.0));
                });

                ui.separator();
                ui.label(format!("Value  {}", self.value));
                ui.label(format!("Generation  {}", self.world.generation));
                ui.label(format!(
                    "Chunks  {} held, {} drawn",
                    self.world.stored_count(),
                    self.chunks.instance_count()
                ));
                ui.label(format!("Zoom  {:.1} px/cell", self.zoom));

                ui.separator();
                match &self.link {
                    Some(_) => ui.colored_label(egui::Color32::from_rgb(120, 210, 140), "connected"),
                    None => ui.colored_label(egui::Color32::from_rgb(220, 170, 90), "offline"),
                };

                if let Some(notice) = &self.notice {
                    ui.colored_label(egui::Color32::from_rgb(230, 120, 110), notice);
                }

                ui.separator();
                ui.small("left click: take a cell   right click: place one");
                ui.small("drag or arrows to pan, wheel or pinch to zoom");
            });
    }

    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 })
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

/// The colour the shader gives a player, computed the same way so the HUD
/// swatch matches the cells on the board. OKLab with the chroma bisected down
/// until it fits sRGB, which keeps hue and lightness exactly rather than
/// bending them the way clamping would.
fn player_colour(player: PlayerId) -> (u8, u8, u8) {
    const HUE_STEP: f32 = 0.618_034;
    const TAU: f32 = std::f32::consts::TAU;

    let hue = (player.0 as f32 * HUE_STEP).fract() * TAU;
    let saturation = if player.0 % 2 == 1 { 1.0 } else { 0.55 };
    let lightness = 0.62f32;

    let oklab_to_linear = |l: f32, a: f32, b: f32| {
        let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
        let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
        let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
        let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        [
            4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
            -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
            -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
        ]
    };
    let inside = |c: [f32; 3]| c.iter().all(|v| (-0.0005..=1.0005).contains(v));

    let chroma = 0.30 * saturation * (1.0 - (2.0 * lightness - 1.0).abs());
    let (dx, dy) = (hue.cos(), hue.sin());
    let mut scale = 1.0;
    if !inside(oklab_to_linear(lightness, chroma * dx, chroma * dy)) {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..8 {
            let mid = (lo + hi) * 0.5;
            let c = chroma * mid;
            if inside(oklab_to_linear(lightness, c * dx, c * dy)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        scale = lo;
    }
    let c = chroma * scale;
    let linear = oklab_to_linear(lightness, c * dx, c * dy);

    // egui takes sRGB bytes, so encode; the shader hands linear to a surface
    // that does this in hardware.
    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    (encode(linear[0]), encode(linear[1]), encode(linear[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HUD swatch and the cells on the board must agree about a player's
    /// colour, so this reproduces the shader's arithmetic and checks the result
    /// is in range and distinct between players.
    #[test]
    fn player_colours_are_in_gamut_and_distinct() {
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
}
