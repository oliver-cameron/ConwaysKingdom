//! Browser tests for the GPU setup path.
//!
//!     wasm-pack test --headless --firefox      # or --chrome
//!
//! These exercise the same resource creation `BattleApp::init` performs, but
//! against an offscreen canvas rather than a winit window, so they can run
//! without a display. Validation errors are caught with error scopes: wgpu
//! reports most binding mistakes that way rather than by panicking, so without
//! the scopes a broken bind group would pass silently.
#![cfg(target_arch = "wasm32")]

use conwayskingdom::{
    chunk_instance_layout, create_pipeline_with, world_bind_group_layout, Chunk, ChunkTexture,
    PipelineDescriptor, SHADER_SOURCE,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
}

/// Mirrors `GpuState::new`, but backed by a detached canvas. WebGL2 has no
/// surfaceless path — the backend needs a canvas to get a context at all — so
/// a test cannot simply pass `compatible_surface: None`.
async fn gpu() -> Gpu {
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(64);
    canvas.set_height(64);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        flags: Default::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });

    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .expect("create surface from canvas");

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        })
        .await
        .expect("no adapter: WebGPU and WebGL2 both unavailable");

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("test device"),
            required_features: wgpu::Features::empty(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: Default::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .expect("request device");

    let format = surface.get_capabilities(&adapter).formats[0];
    Gpu { device, queue, format }
}

#[wasm_bindgen_test]
async fn an_adapter_is_reachable() {
    let g = gpu().await;
    assert!(g.device.limits().max_texture_array_layers >= 2);
}

/// The regression guard for the bug this file was written for. The GL backend
/// derives its texture target from the texture descriptor, so a chunk store
/// allocated with `depth_or_array_layers == 1` becomes a `TEXTURE_2D` and a
/// `D2Array` view over it mismatches:
///
///   "wgpu-hal heuristics assumed that the view dimension will be equal to
///    `D2` rather than `D2Array`"
///
/// wgpu reports that through `log::error!`, not a panic, so this asserts the
/// layer count directly as well as binding the thing for real.
#[wasm_bindgen_test]
async fn chunk_store_binds_as_a_d2_array() {
    let g = gpu().await;
    let scope = g.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let chunks = ChunkTexture::new(&g.device, ChunkTexture::LAYER_BUDGET);
    assert!(
        chunks.layers > 1,
        "a single-layer array texture is a TEXTURE_2D on the GL backend"
    );

    let camera = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("camera"),
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bgl = world_bind_group_layout(&g.device);
    let _bind_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("world"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: camera.as_entire_binding() },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&chunks.view),
            },
        ],
    });

    let err = scope.pop().await;
    assert!(err.is_none(), "validation error: {err:?}");
}

#[wasm_bindgen_test]
async fn a_chunk_uploads_into_a_layer() {
    let g = gpu().await;
    let scope = g.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let chunks = ChunkTexture::new(&g.device, ChunkTexture::LAYER_BUDGET);
    let chunk = Chunk::zeroed();
    chunks.upload(&g.queue, 0, &chunk);
    g.queue.submit(std::iter::empty());

    let err = scope.pop().await;
    assert!(err.is_none(), "validation error: {err:?}");
}

/// Builds the real pipeline from the real shader and the real layouts, so a
/// WGSL change that breaks the binding contract fails here.
#[wasm_bindgen_test]
async fn the_pipeline_compiles_and_validates() {
    let g = gpu().await;
    let scope = g.device.push_error_scope(wgpu::ErrorFilter::Validation);

    let bgl = world_bind_group_layout(&g.device);
    let _pipeline = create_pipeline_with(
        &g.device,
        g.format,
        &PipelineDescriptor {
            label: "chunk pipeline",
            shader_source: SHADER_SOURCE,
            vertex_buffers: &[chunk_instance_layout()],
            bind_group_layouts: &[Some(&bgl)],
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
    );

    let err = scope.pop().await;
    assert!(err.is_none(), "validation error: {err:?}");
}
