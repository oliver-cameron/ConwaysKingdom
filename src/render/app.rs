use std::sync::Arc;
use winit::{event::*, event_loop::{ControlFlow, EventLoop}, window::WindowBuilder};

use crate::render::context::{DrawCall, Frame, FrameAcquire};
use crate::render::context::GpuState;

/// Implement this to plug your own resources/pipelines into the event
/// loop. The core knows nothing about what you draw — it just calls
/// `draw_calls` each frame and submits whatever you return.
pub trait App: 'static {
    fn init(gpu: &GpuState) -> Self
    where
        Self: Sized;

    /// Called after the surface has been reconfigured for a new size.
    fn resize(&mut self, _gpu: &GpuState) {}

    /// Called once per frame before `draw_calls`, with the time in
    /// seconds since the previous frame.
    fn update(&mut self, _gpu: &GpuState, _dt: f32) {}

    /// Called once per frame. Return every draw call you want recorded
    /// into this frame's single render pass, in order.
    fn draw_calls(&self) -> Vec<DrawCall<'_>>;

    /// Clear color for the frame, or `None` to load existing contents.
    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 })
    }

    fn on_key(&mut self, _code: winit::keyboard::KeyCode, _pressed: bool) {}
    fn on_scroll(&mut self, _delta: MouseScrollDelta) {}

    /// Cursor moved, in physical pixels from the top-left of the surface.
    fn on_cursor(&mut self, _x: f64, _y: f64) {}

    /// A mouse button went down or up, at the last reported cursor position.
    fn on_click(&mut self, _button: MouseButton, _pressed: bool) {}
}

/// Run the event loop. Logging is the caller's business: initialising it here
/// as well as in `main` panics with `SetLoggerError`, since a logger can only
/// be installed once.
pub async fn run<A: App>() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("wgpu core")
            .build(&event_loop)
            .expect("failed to create window"),
    );

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let canvas = window.canvas()?;
                canvas.set_id("render-canvas");
                doc.body()?.append_child(&canvas).ok()?;
                Some(())
            })
            .expect("couldn't append canvas to document body");

        // winit leaves the canvas at the HTML default of 300x150, and
        // `inner_size` reads the element, so without this the surface is
        // configured at 300x150 and stretched across the viewport by the CSS.
        fit_canvas_to_window(&window);
    }

    let mut gpu = GpuState::new(window.clone()).await;
    let mut app = A::init(&gpu);
    let mut last_frame = now_secs();

    // Kick off the redraw chain. From here each frame asks for the next one,
    // so the loop is paced by exactly one scheduler.
    window.request_redraw();

    event_loop
        .run(move |event, elwt| {
            // Wait, not Poll. `request_redraw` already maps to
            // requestAnimationFrame on the web, whereas Poll schedules a
            // second loop through requestIdleCallback -- which winit's own
            // docs note "might be affected by browser throttling". Running
            // both means two schedulers competing to drive one renderer.
            elwt.set_control_flow(ControlFlow::Wait);

            match event {
                Event::WindowEvent { event, window_id } if window_id == window.id() => match event {
                    WindowEvent::CloseRequested => elwt.exit(),
                    WindowEvent::Resized(size) => {
                        gpu.resize(size.width, size.height);
                        app.resize(&gpu);
                    }
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                physical_key: winit::keyboard::PhysicalKey::Code(code),
                                state,
                                ..
                            },
                        ..
                    } => app.on_key(code, state == ElementState::Pressed),
                    WindowEvent::MouseWheel { delta, .. } => app.on_scroll(delta),
                    WindowEvent::CursorMoved { position, .. } => {
                        app.on_cursor(position.x, position.y)
                    }
                    WindowEvent::MouseInput { button, state, .. } => {
                        app.on_click(button, state == ElementState::Pressed)
                    }
                    WindowEvent::RedrawRequested => {
                        // The browser gives no resize event for the canvas
                        // element, only for its own window, and the devtools
                        // pane changes that without any page event a canvas
                        // would see. Checking each frame costs two property
                        // reads and covers every case: window resize, devtools
                        // opening, zoom, a rotated phone.
                        #[cfg(target_arch = "wasm32")]
                        fit_canvas_to_window(&window);

                        let now = now_secs();
                        let dt = (now - last_frame).max(0.0) as f32;
                        last_frame = now;

                        app.update(&gpu, dt);

                        match Frame::begin(&gpu) {
                            FrameAcquire::Ready(frame) => {
                                let calls = app.draw_calls();
                                frame.submit(&gpu, app.clear_color(), &calls);
                            }
                            FrameAcquire::Skip => {}
                            FrameAcquire::Reconfigure => {
                                gpu.resize(gpu.size.0, gpu.size.1);
                            }
                            FrameAcquire::Lost => {
                                log::error!("GPU device lost");
                                elwt.exit();
                            }
                        }

                        // Ask for the next frame now that this one is done.
                        window.request_redraw();
                    }
                    _ => {}
                },
                _ => {}
            }
        })
        .expect("event loop error");
}

/// Keep the canvas the size of the window it sits in, at the device's real
/// pixel density.
///
/// The backing store is sized in physical pixels and the element in CSS
/// pixels, so a HiDPI display renders at its own resolution instead of being
/// upscaled. Only called when something actually changed, since setting the
/// size unconditionally would clear the canvas every frame.
#[cfg(target_arch = "wasm32")]
fn fit_canvas_to_window(window: &winit::window::Window) {
    use winit::dpi::PhysicalSize;
    let Some(win) = web_sys::window() else { return };
    let dpr = win.device_pixel_ratio().max(1.0);
    let w = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let h = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(600.0);
    let want = PhysicalSize::new((w * dpr).round() as u32, (h * dpr).round() as u32);
    if want.width == 0 || want.height == 0 {
        return;
    }
    if window.inner_size() != want {
        let _ = window.request_inner_size(want);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

#[cfg(target_arch = "wasm32")]
fn now_secs() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now() / 1000.0
}
