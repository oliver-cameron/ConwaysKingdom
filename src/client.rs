//! The client: a world, a view of it, and the loop that keeps them in step.
//!
//! Deliberately thin. The rules live in [`crate::sim`] and the GPU work in
//! [`crate::render`]; this only decides *policy* — how fast generations run,
//! which world to open, and where to point the camera. A headless server would
//! keep the [`World`] and drop everything else in this file.

use crate::render::app::App;
use crate::render::chunks::{
    chunk_instance_layout, world_bind_group_layout, CameraUniform, ChunkStore, SHADER_SOURCE,
};
use crate::render::context::{Draw, DrawCall, GpuState};
use crate::render::pipeline::{create_pipeline, PipelineDescriptor};
use crate::sim::{World, CHUNK_N};

use crate::net::link::Link;
use crate::net::ClientMessage;

/// Seconds of wall clock per generation.
pub const GENERATION_SPAN: f32 = 0.25;

/// Where the camera looks, in cells, as (x, y) — that is, (col, row).
const VIEW_CENTRE: (f32, f32) = (CHUNK_N as f32 / 2.0, CHUNK_N as f32 / 2.0);

/// Screen pixels per cell. Never below 1: point sampling drops sparse cells
/// under that.
const VIEW_ZOOM: f32 = 16.0;

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
    world: World,
    /// Last reported cursor position, in physical pixels.
    cursor: (f64, f64),
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
        let zoom = VIEW_ZOOM;
        let origin = [
            VIEW_CENTRE.0 - vw / (2.0 * zoom),
            VIEW_CENTRE.1 - vh / (2.0 * zoom),
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

    /// Where a screen position lands in the world, in absolute cell
    /// coordinates. The inverse of what the vertex shader does.
    fn cell_under_cursor(&self, gpu: &GpuState, (px, py): (f64, f64)) -> (i32, i32) {
        let (vw, vh) = (gpu.size.0 as f32, gpu.size.1 as f32);
        let origin = (
            VIEW_CENTRE.0 - vw / (2.0 * VIEW_ZOOM),
            VIEW_CENTRE.1 - vh / (2.0 * VIEW_ZOOM),
        );
        let x = origin.0 + px as f32 / VIEW_ZOOM;
        let y = origin.1 + py as f32 / VIEW_ZOOM;
        (y.floor() as i32, x.floor() as i32) // (row, col)
    }
}

impl App for BattleApp {
    fn init(gpu: &GpuState) -> Self {
        let world = match WORLD {
            WorldMode::Infinite => World::infinite(),
            WorldMode::Torus => World::toroidal(TORUS_CHUNKS.0, TORUS_CHUNKS.1),
        };
        let mut chunks = ChunkStore::new(&gpu.device);
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
            world,
            cursor: (0.0, 0.0),
            pending_click: None,
            link: open_link(),
        };
        app.world.dirty = false;
        app.write_camera(gpu);
        app
    }

    fn resize(&mut self, gpu: &GpuState) {
        self.write_camera(gpu);
    }

    fn update(&mut self, gpu: &GpuState, dt: f32) {
        if let Some(link) = &mut self.link {
            for msg in link.drain() {
                // Received and logged. Applying them needs the client to
                // hold a tick of its own, which is the next step.
                log::info!("server: {msg:?}");
            }
            if link.is_closed() {
                log::warn!("link closed; continuing offline");
                self.link = None;
            }
        }

        if let Some(at) = self.pending_click.take() {
            let (row, col) = self.cell_under_cursor(gpu, at);
            // Received, resolved, and deliberately ignored. A net::Action is
            // built here once there is somewhere to send it.
            log::info!("click at cell ({row}, {col})");
        }

        self.world.update(dt, GENERATION_SPAN);
        if self.world.dirty {
            self.chunks.sync(&gpu.queue, &self.world, TORUS_REPEATS);
            self.world.dirty = false;
            self.write_camera(gpu);
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
        self.cursor = (x, y);
    }

    /// Clicks are received and resolved to a cell, but deliberately do nothing
    /// yet. This is where a `net::Action` will be built and sent.
    fn on_click(&mut self, button: winit::event::MouseButton, pressed: bool) {
        if !pressed {
            return;
        }
        let _ = button;
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
    let link = Link::connect_to_origin("/ws")?;
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
