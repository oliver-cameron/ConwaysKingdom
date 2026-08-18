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

pub async fn run<A: App>() {
    #[cfg(not(target_arch = "wasm32"))]
    env_logger::init();

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

#[cfg(not(target_arch = "wasm32"))]
fn now_secs() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

#[cfg(target_arch = "wasm32")]
fn now_secs() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now() / 1000.0
}
