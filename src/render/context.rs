//! The GPU context and the per-frame submission path.
//!
//! Client-only: nothing here is shared with the server.

use std::ops::Range;
use std::sync::Arc;

use winit::window::Window;

/// Owns the wgpu device/queue/surface. This is the one place that
/// knows about the native-vs-web split: everything above it (pipeline
/// builder, frame submission) is platform-agnostic.
pub struct GpuState {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: (u32, u32),
    /// Physical pixels per logical point, so the overlay can size itself the
    /// way the platform expects rather than in raw device pixels.
    pub scale_factor: f32,
    pub window: Arc<Window>,
    /// **What the world is drawn into**, before the one-pixel filter that puts
    /// it on the screen — see [`Resolve`] and `shaders/resolve.wgsl`.
    pub offscreen: Offscreen,
}

/// The texture the world is drawn into, and the pipeline that resolves it onto
/// the surface.
///
/// A pass of its own rather than filtering in the world shader, because in
/// screen space there are no tiles: a pixel's neighbour is its neighbour
/// whether the two came from one cell, two cells or the backdrop, so one rule
/// covers every edge in the picture. See `shaders/resolve.wgsl`.
///
/// The interface is **not** in it. It is drawn in the second pass, onto the
/// surface, after the resolve — text and panel edges are already exactly where
/// they should be and filtering them would only soften them.
pub struct Offscreen {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// Where the world's texel grid falls on the screen, so the resolve can
    /// weight its blend by **phase** rather than averaging blindly. See
    /// [`Self::set_grid`].
    grid: wgpu::Buffer,
    /// Samples across one screen pixel, after the size clamp.
    over: f32,
}

impl Offscreen {
    /// **Where the texel grid sits, in world terms**: the camera's origin in
    /// cells and its zoom in pixels per cell.
    ///
    /// Without it the resolve can only average its neighbours by a fixed
    /// amount, which softens a pixel sitting dead in the middle of a texel
    /// exactly as much as one straddling two — a blur rather than
    /// antialiasing. With it, a pixel whose footprint lies inside one texel
    /// keeps its own colour and only one that overlaps a neighbour takes any
    /// of it, in proportion to how much.
    pub fn set_grid(&self, queue: &wgpu::Queue, origin: (f32, f32), zoom: f32) {
        let grid = [origin.0, origin.1, zoom, self.over];
        queue.write_buffer(&self.grid, 0, bytemuck::cast_slice(&grid));
    }

    /// How many samples across one screen pixel the world was drawn at.
    ///
    /// One when the target had to fall back to the screen's own size, which is
    /// a large display against the device's texture limit — see
    /// [`Self::offscreen_size`]. The resolve reads this and does what it can.
    pub fn over(&self) -> f32 {
        self.over
    }

    /// **How much larger than the screen the world is drawn.**
    ///
    /// The world pass takes one sample a pixel, so anything finer than a pixel
    /// is decided by which side of the sample it happened to fall — and at low
    /// zoom that is most of the picture. Drawing at twice the width and height
    /// takes four samples where there was one and lets the resolve average
    /// them, which is the only thing here that adds information rather than
    /// rearranging it. `docs/planned.md#texels-nothing-samples` calls this the
    /// cheap one to try, and it is: the offscreen target already existed and
    /// this is its size.
    ///
    /// Four times the fragment cost of the world pass, which is why it is a
    /// named number and not a two.
    pub const SUPERSAMPLE: u32 = 2;

    /// The size of the offscreen target for a given surface.
    ///
    /// Clamped, because the product is what gets allocated: a 4K display at two
    /// is a 7680x4320 texture, and the limit a device guarantees is 8192. Above
    /// the cap the world is drawn at the screen's own size, which is where it
    /// was before this existed.
    pub(crate) fn offscreen_size(device: &wgpu::Device, size: (u32, u32)) -> (u32, u32) {
        let most = device.limits().max_texture_dimension_2d;
        let (w, h) = (size.0.max(1), size.1.max(1));
        let (big_w, big_h) = (w * Self::SUPERSAMPLE, h * Self::SUPERSAMPLE);
        if big_w <= most && big_h <= most {
            (big_w, big_h)
        } else {
            (w, h)
        }
    }

    fn target(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        grid: &wgpu::Buffer,
    ) -> (wgpu::TextureView, wgpu::BindGroup) {
        let (width, height) = Self::offscreen_size(device, size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("world"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The surface's own format, so the resolve is a filter and not a
            // conversion: whatever the world shader decided about encoding has
            // already been decided.
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resolve"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry { binding: 1, resource: grid.as_entire_binding() },
            ],
        });
        (view, bind_group)
    }

    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resolve"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // Read with `textureLoad`, so there is no sampler and
                        // nothing to configure: the four taps are named, not
                        // filtered for.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = crate::render::pipeline::create_pipeline_with(
            device,
            format,
            &crate::render::pipeline::PipelineDescriptor {
                label: "resolve",
                shader_source: include_str!("shaders/resolve.wgsl"),
                bind_group_layouts: &[Some(&layout)],
                ..Default::default()
            },
        );
        let grid = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resolve grid"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (view, bind_group) = Self::target(device, &layout, format, size, &grid);
        let over = Self::over_for(device, size);
        Self { view, bind_group, layout, pipeline, grid, over }
    }

    pub(crate) fn resize(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) {
        let (view, bind_group) = Self::target(device, &self.layout, format, size, &self.grid);
        self.view = view;
        self.bind_group = bind_group;
        // Recomputed, because the clamp depends on the size: a window dragged
        // onto a 4K display can cross the device's texture limit.
        self.over = Self::over_for(device, size);
    }

    /// What the clamp actually left, as a number the shader can use.
    fn over_for(device: &wgpu::Device, size: (u32, u32)) -> f32 {
        let (w, _) = Self::offscreen_size(device, size);
        (w as f32 / size.0.max(1) as f32).max(1.0)
    }
}

/// Whether the browser will actually hand over a WebGPU adapter.
///
/// `navigator.gpu` existing is not enough. On a secure origin — which
/// `localhost` is — Chrome exposes it and then returns **null** from
/// `requestAdapter` whenever no GPU is usable: a blocklisted driver, a crashed
/// GPU process, a virtual machine, a headless browser. wgpu hands that null
/// back as an `Adapter` all the same, and the first method called on it throws
///
/// ```text
/// TypeError: Cannot read properties of null (reading 'info')
/// ```
///
/// which kills the page before the WebGL2 fallback is ever reached. So ask the
/// browser ourselves, and only name the backend if the answer is yes.
///
/// Reached reflectively through `js_sys` rather than through web-sys, whose
/// WebGPU bindings sit behind `--cfg=web_sys_unstable_apis` and would put a
/// build flag between this crate and compiling at all.
#[cfg(target_arch = "wasm32")]
async fn webgpu_usable() -> bool {
    use wasm_bindgen::JsCast;

    let get = |on: &wasm_bindgen::JsValue, name: &str| {
        js_sys::Reflect::get(on, &wasm_bindgen::JsValue::from_str(name)).ok()
    };

    let Some(navigator) = web_sys::window().map(|w| w.navigator()) else {
        return false;
    };
    let Some(gpu) = get(navigator.as_ref(), "gpu") else { return false };
    if gpu.is_undefined() || gpu.is_null() {
        return false;
    }
    let Some(request) =
        get(&gpu, "requestAdapter").and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    else {
        return false;
    };
    let Ok(promise) = request.call0(&gpu).and_then(|p| p.dyn_into::<js_sys::Promise>()) else {
        return false;
    };
    match wasm_bindgen_futures::JsFuture::from(promise).await {
        Ok(adapter) => !adapter.is_null() && !adapter.is_undefined(),
        Err(_) => false,
    }
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let scale_factor = window.scale_factor();
        let (width, height) = (size.width.max(1), size.height.max(1));

        // Native: let wgpu pick the best of Vulkan / Metal / DX12.
        #[cfg(not(target_arch = "wasm32"))]
        let backends = wgpu::Backends::PRIMARY;

        // Wasm: WebGPU when the browser will really give us one, and WebGL2
        // otherwise — which needs the `webgl` feature on the wgpu dependency
        // for the wasm32 target in Cargo.toml. Asking for WebGPU when it
        // cannot be had does not fall back, it crashes; see `webgpu_usable`.
        #[cfg(target_arch = "wasm32")]
        let backends = if webgpu_usable().await {
            wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
        } else {
            log::warn!("no WebGPU adapter offered; falling back to WebGL2");
            wgpu::Backends::GL
        };

        // InstanceDescriptor no longer implements Default as of wgpu
        // 29+, so every field is spelled out explicitly here.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance.create_surface(window.clone()).expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: true,
            })
            .await
            .expect("no suitable GPU adapter found (check that WebGPU/WebGL2 is available)");

        let info = adapter.get_info();
        log::info!("adapter: {} ({:?} via {:?})", info.name, info.device_type, info.backend);

        // WebGL2 only supports a reduced limit set (no storage buffers,
        // smaller texture sizes, etc). Requesting the downlevel defaults
        // keeps the same code path working on both backends; on native
        // it's a no-op since we use full defaults there.
        let limits = if cfg!(target_arch = "wasm32") {
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        } else {
            wgpu::Limits::default()
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mini-renderer device"),
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: limits,
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("failed to create device");

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            // Fifo is the only mode guaranteed to exist, and it is the one
            // that paces to the display instead of spinning. `present_modes[0]`
            // was whatever the driver happened to list first.
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        // The web target has no console to read a panic out of, so say what was
        // negotiated: a surface that is smaller or a different format than
        // expected is the usual reason a page renders wrong rather than not at
        // all.
        log::info!(
            "surface: {width}x{height} {:?}, present {:?}, {} format(s) offered, \
             shader encodes sRGB: {}",
            config.format,
            config.present_mode,
            surface_caps.formats.len(),
            !config.format.is_srgb()
        );
        // Every format the adapter offered, not just the one taken. Which of
        // them exist is the whole of what separates the backends here -- the
        // WebGL2 path offers sRGB formats and converts in its own present
        // blit, so a surface that came back Unorm there means something has
        // changed underneath us rather than that the browser cannot do it.
        log::debug!("formats offered: {:?}", surface_caps.formats);

        let offscreen = Offscreen::new(&device, config.format, (width, height));
        // **Said out loud, because it is invisible when it fails.** The world
        // is drawn into a target `over` times the screen and the resolve
        // averages each block of samples down — and if the device's texture
        // limit refuses the larger target, the factor silently comes back one
        // and the picture is exactly what it was before any of this. There is
        // no error and nothing looks broken, so this is the only way to know
        // which of the two happened.
        let (ow, oh) = Offscreen::offscreen_size(&device, (width, height));
        log::info!(
            "supersampling: world drawn at {ow}x{oh} for a {width}x{height} surface, \
             {}x per axis (limit {})",
            offscreen.over(),
            device.limits().max_texture_dimension_2d,
        );
        Self {
            surface,
            device,
            queue,
            config,
            size: (width, height),
            scale_factor: scale_factor as f32,
            window,
            offscreen,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimized window, etc.
        }
        // Read again rather than kept from window creation. It changes: a
        // window moved between two displays gets a new one, a browser zoom
        // changes `devicePixelRatio`, and on the web winit often reports the
        // real one only after the canvas has been attached and sized.
        //
        // Stale, it is the difference between text drawn at the display's
        // resolution and text drawn at half of it — egui lays out in points
        // and rasterises at this, so a value of one on a two-times display
        // gives glyphs rendered at half size and scaled up, which is what
        // aliased text on a sharp screen actually is.
        self.scale_factor = self.window.scale_factor() as f32;
        self.size = (width, height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        // The world is drawn at the screen's own resolution, so the target it
        // is drawn into follows the screen.
        self.offscreen.resize(&self.device, self.config.format, self.size);
    }
}

pub struct IndexBufferBinding<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub format: wgpu::IndexFormat,
}

pub enum Draw {
    /// Non-indexed draw: `pass.draw(vertices, instances)`.
    Vertices { vertices: Range<u32>, instances: Range<u32> },
    /// Indexed draw: `pass.draw_indexed(indices, base_vertex, instances)`.
    /// Requires `DrawCall::index_buffer` to be set.
    Indexed { indices: Range<u32>, base_vertex: i32, instances: Range<u32> },
}

/// One draw call: a pipeline, its bind groups, its vertex/index
/// buffers, and the range to draw. Build a list of these per frame and
/// hand them to `Frame::submit`. Nothing here assumes any particular
/// vertex format, shader, or resource layout — that's all defined by
/// whatever `PipelineDescriptor` you built.
pub struct DrawCall<'a> {
    pub pipeline: &'a wgpu::RenderPipeline,
    /// Bind group `i` in this slice is bound at `@group(i)`.
    pub bind_groups: &'a [wgpu::BindGroup],
    /// Vertex buffer `i` in this slice is bound at slot `i`, matching
    /// the order of `PipelineDescriptor::vertex_buffers` used to build
    /// `pipeline`.
    pub vertex_buffers: &'a [wgpu::Buffer],
    pub index_buffer: Option<IndexBufferBinding<'a>>,
    pub draw: Draw,
}

/// One in-flight frame: an acquired surface texture plus a command
/// encoder. Build with `Frame::begin`, then call `submit` once your
/// draw calls are ready.
pub struct Frame {
    output: wgpu::SurfaceTexture,
    view: wgpu::TextureView,
    encoder: wgpu::CommandEncoder,
}

/// Result of trying to acquire the next frame. wgpu represents this as
/// an enum (`CurrentSurfaceTexture`) rather than a plain `Result`,
/// since several of these cases are routine (occluded window, timed
/// out compositor) rather than actual errors — this mirrors that.
pub enum FrameAcquire {
    /// Got a texture to draw into (present-worthy, though `Suboptimal`
    /// means you should reconfigure soon).
    Ready(Frame),
    /// Nothing to draw this frame (timeout, occluded, or a validation
    /// hiccup) — just skip it, nothing is wrong.
    Skip,
    /// The surface needs reconfiguring before the next attempt. Call
    /// `gpu.resize(gpu.size.0, gpu.size.1)` (or a fresh size) and retry
    /// next frame.
    Reconfigure,
    /// The device was lost. Treat as fatal: recreate the GPU state or
    /// exit, same as you would for wgpu::SurfaceError::Lost previously.
    Lost,
}

impl Frame {
    /// Acquire the next swapchain image.
    pub fn begin(gpu: &GpuState) -> FrameAcquire {
        let output = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return FrameAcquire::Skip,
            wgpu::CurrentSurfaceTexture::Outdated => return FrameAcquire::Reconfigure,
            wgpu::CurrentSurfaceTexture::Lost => return FrameAcquire::Lost,
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame encoder"),
        });

        FrameAcquire::Ready(Self { output, view, encoder })
    }

    /// Records every draw call into a single render pass, then submits
    /// the command buffer and presents. `clear_color` of `None` loads
    /// the existing contents of the target instead of clearing it
    /// (useful if you're compositing multiple passes elsewhere).
    /// Records every draw call into a single render pass, then submits and
    /// presents.
    ///
    /// `overlay` runs after the draw calls, in the same pass, so an interface
    /// drawn on top needs no second surface and no compositing step. It gets
    /// the encoder too, since an overlay may need to upload buffers of its own
    /// before drawing.
    ///
    /// The pass is `'static` because a pass keeps its referenced resources
    /// alive itself; the only consequence is that touching the encoder while
    /// the pass is open becomes a runtime error rather than a compile one.
    pub fn submit(
        mut self,
        gpu: &GpuState,
        clear_color: Option<wgpu::Color>,
        calls: &[DrawCall],
        overlay: impl FnOnce(&mut wgpu::CommandEncoder, &mut wgpu::RenderPass<'static>),
    ) {
        {
            let mut pass = self
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("world"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &gpu.offscreen.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: match clear_color {
                                Some(c) => wgpu::LoadOp::Clear(c),
                                None => wgpu::LoadOp::Load,
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                })
                .forget_lifetime();

            for call in calls {
                pass.set_pipeline(call.pipeline);

                for (i, bind_group) in call.bind_groups.iter().enumerate() {
                    pass.set_bind_group(i as u32, bind_group, &[]);
                }
                for (i, buffer) in call.vertex_buffers.iter().enumerate() {
                    pass.set_vertex_buffer(i as u32, buffer.slice(..));
                }

                match &call.draw {
                    Draw::Vertices { vertices, instances } => {
                        pass.draw(vertices.clone(), instances.clone());
                    }
                    Draw::Indexed { indices, base_vertex, instances } => {
                        let ib = call
                            .index_buffer
                            .as_ref()
                            .expect("Draw::Indexed requires DrawCall::index_buffer to be set");
                        pass.set_index_buffer(ib.buffer.slice(..), ib.format);
                        pass.draw_indexed(indices.clone(), *base_vertex, instances.clone());
                    }
                }
            }
        }

        // **The world through the filter, then the interface on top of it.**
        // Two passes rather than one, and the split is the point: the world is
        // resolved through a one-pixel box — see `shaders/resolve.wgsl` — and
        // the interface is not, because text and panel edges are already
        // exactly where they should be and filtering them would only soften
        // them.
        {
            let mut pass = self
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("resolve and overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            // Every pixel is written by the triangle below, so
                            // there is nothing to clear and nothing to load.
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            pass.set_pipeline(&gpu.offscreen.pipeline);
            pass.set_bind_group(0, &gpu.offscreen.bind_group, &[]);
            pass.draw(0..3, 0..1);

            overlay(&mut self.encoder, &mut pass);
        }

        gpu.queue.submit(std::iter::once(self.encoder.finish()));
        // Presentation moved from SurfaceTexture::present() to
        // Queue::present(surface_texture) in newer wgpu.
        gpu.queue.present(self.output);
    }
}
