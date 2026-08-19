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
use crate::net::{ClientMessage, ServerMessage};

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
    /// Viewport size in physical pixels, cached because input callbacks are
    /// not handed the `GpuState`.
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
    /// A click waiting to be resolved to a cell. Input callbacks are not given
    /// the `GpuState`, and the mapping needs the viewport, so it is deferred to
    /// the next `update` rather than guessed here.
    pending_click: Option<(f64, f64)>,
}

impl BattleApp {
    /// A fixed camera. Autoscrolling is gone: the view no longer chases the
    /// live pattern, so what is on screen is whatever `VIEW_CENTRE` and
    /// `VIEW_ZOOM` say. Panning and zooming will be driven by input.
    fn write_camera(&self, gpu: &GpuState) {
        let (vw, vh) = (gpu.size.0 as f32, gpu.size.1 as f32);
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
    fn cell_under_cursor(&self, gpu: &GpuState, (px, py): (f64, f64)) -> (i32, i32) {
        let (vw, vh) = (gpu.size.0 as f32, gpu.size.1 as f32);
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
    fn pump_link(&mut self, gpu: &GpuState) {
        let Some(link) = &mut self.link else { return };
        let messages = link.drain();
        let closed = link.is_closed();

        for msg in messages {
            match msg {
                ServerMessage::Welcome { you, tick } => {
                    log::info!("joined as {you:?} at tick {tick}; adopting the server's world");
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

        self.subscribe_to_view(gpu);
    }

    /// Ask for any visible chunk not already requested. The camera is fixed for
    /// now, so this settles after the first frame; it is written against the
    /// viewport so panning needs no new code.
    fn subscribe_to_view(&mut self, gpu: &GpuState) {
        let (vw, vh) = (gpu.size.0 as f32, gpu.size.1 as f32);
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
        chunks.sync(&gpu.queue, &world, TORUS_REPEATS);

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
            viewport: (1.0, 1.0),
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
        self.viewport = (gpu.size.0 as f32, gpu.size.1 as f32);
        self.camera_dirty = true;
        self.subscribed.clear(); // the viewport changed; ask again

        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        self.apply_pan(dt);

        if self.link.is_some() {
            self.pump_link(gpu);
        }

        if let Some(at) = self.pending_click.take() {
            let (row, col) = self.cell_under_cursor(gpu, at);
            // Resolved and deliberately ignored. Sending it is one line --
            // ClientMessage::Act with a Paint at this cell -- once there is a
            // decision about what a click should mean.
            log::info!("click at cell ({row}, {col})");
        }

        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS);
            self.world.dirty = false;
        }
        if self.camera_dirty {
            self.write_camera(gpu);
            self.camera_dirty = false;
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

    fn on_scroll(&mut self, delta: winit::event::MouseScrollDelta) {
        let gpu_size = self.viewport;
        use winit::event::MouseScrollDelta as D;
        // A line of wheel and a pixel of trackpad are wildly different
        // magnitudes, so normalise before using either.
        let steps = match delta {
            D::LineDelta(_, y) => y,
            D::PixelDelta(p) => p.y as f32 / 50.0,
        };
        // Zoom about the cursor, not the screen centre: zooming towards a
        // corner should keep what is under the pointer under the pointer.
        let before = self.cell_under_cursor_f(gpu_size, self.cursor);
        self.zoom = (self.zoom * 1.15f32.powf(steps)).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let after = self.cell_under_cursor_f(gpu_size, self.cursor);
        self.camera.0 += before.0 - after.0;
        self.camera.1 += before.1 - after.1;
        self.camera_dirty = true;
    }

    /// Clicks are received and resolved to a cell, but deliberately do nothing
    /// yet. This is where a `net::Action` will be built and sent.
    fn on_click(&mut self, button: winit::event::MouseButton, pressed: bool) {
        if button != winit::event::MouseButton::Left {
            return;
        }
        if pressed {
            self.dragging = true;
            self.drag_moved = false;
            return;
        }
        self.dragging = false;
        // A press that moved was a pan, not a click on a cell.
        if self.drag_moved {
            return;
        }
        // `gpu` is not passed to input callbacks, so resolve on the next frame
        // instead of guessing the viewport here.
        self.pending_click = Some(self.cursor);
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
