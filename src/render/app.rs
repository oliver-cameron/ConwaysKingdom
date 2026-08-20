//! The window, the event loop, and the frame.
//!
//! winit 0.30 replaced the closure-driven loop with [`ApplicationHandler`], and
//! moved window creation inside it — a window only exists once the platform has
//! resumed. GPU setup follows the window, and it is async, so [`Harness`] holds
//! three states: no window, waiting for the device, running. On native the wait
//! is a `block_on`; in a browser it cannot be, so the device arrives through a
//! shared slot that is checked each time the loop wakes.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::render::context::{Frame, FrameAcquire, GpuState};

/// Implement this to plug your own resources and pipelines into the loop. The
/// core knows nothing about what you draw — it calls `draw_calls` each frame
/// and submits whatever comes back.
pub trait App: 'static {
    fn init(gpu: &GpuState) -> Self
    where
        Self: Sized;

    /// Called after the surface has been reconfigured for a new size.
    fn resize(&mut self, _gpu: &GpuState) {}

    /// Called once per frame before `draw_calls`, with seconds since the last.
    fn update(&mut self, _gpu: &GpuState, _dt: f32) {}

    /// Every draw call to record into this frame's render pass, in order.
    fn draw_calls(&self) -> Vec<crate::render::context::DrawCall<'_>>;

    /// Clear colour, or `None` to keep what is already there.
    fn clear_color(&self) -> Option<wgpu::Color> {
        Some(wgpu::Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 })
    }

    /// A raw window event, before it is dispatched to anything below.
    /// Returning true means the app took it and the typed callbacks should not
    /// also fire — an interface layer uses this to keep a click on a button
    /// from also acting on the world.
    fn on_window_event(&mut self, _event: &WindowEvent, _scale: f32) -> bool {
        false
    }

    /// Record anything that should sit on top of the world, into the same
    /// pass. Runs after `draw_calls`.
    ///
    /// `render` knows nothing about what draws here; an interface is the
    /// client's business, and keeping it out of this module is what lets the
    /// module stay generic wgpu and winit.
    ///
    /// Takes `&self` because the frame holds an immutable borrow of the app
    /// for the whole pass — `draw_calls` returns references into it. Anything
    /// here that needs to mutate does so behind its own cell.
    fn overlay(
        &self,
        _gpu: &GpuState,
        _encoder: &mut wgpu::CommandEncoder,
        _pass: &mut wgpu::RenderPass<'static>,
    ) {
    }

    fn on_key(&mut self, _code: winit::keyboard::KeyCode, _pressed: bool) {}
    /// A wheel or trackpad scroll. `zoom_gesture` is set when the platform
    /// reports it as a pinch rather than a scroll — browsers and most desktop
    /// environments send a trackpad pinch as ctrl+wheel.
    fn on_scroll(&mut self, _delta: MouseScrollDelta, _zoom_gesture: bool) {}

    /// A trackpad pinch, where the platform reports one as a gesture rather
    /// than as ctrl+wheel. macOS and iOS do; nothing else in winit does.
    fn on_pinch(&mut self, _delta: f64) {}

    /// Cursor moved, in physical pixels from the top-left of the surface.
    fn on_cursor(&mut self, _x: f64, _y: f64) {}

    /// A mouse button went down or up, at the last reported cursor position.
    fn on_click(&mut self, _button: MouseButton, _pressed: bool) {}

    /// A finger touched, moved, or left. `id` distinguishes fingers, which is
    /// what makes a pinch tellable from a drag.
    fn on_touch(&mut self, _id: u64, _phase: TouchPhase, _x: f64, _y: f64) {}

    /// What the pointer should look like. Read after every `update`, and sent
    /// to the window only when it changes — a cursor set every frame flickers
    /// on some platforms and is a call into the compositor on all of them.
    ///
    /// Here rather than in the app because only the loop holds the window, and
    /// handing the window out would let anything resize or retitle it.
    fn cursor_icon(&self) -> winit::window::CursorIcon {
        winit::window::CursorIcon::Default
    }
}

pub async fn run<A: App>() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let harness = Harness::<A>::default();

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut harness = harness;
        event_loop.run_app(&mut harness).expect("event loop error");
    }

    // `run_app` never returns, which a browser will not tolerate; `spawn_app`
    // hands the loop to the page instead.
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(harness);
    }
}

struct Running<A> {
    window: Arc<Window>,
    gpu: GpuState,
    app: A,
    last_frame: f64,
    /// Control held. A trackpad pinch arrives as ctrl+wheel nearly everywhere,
    /// so the wheel needs to know.
    ctrl: bool,
    /// What the cursor was last set to, so it is only set again on a change.
    cursor: winit::window::CursorIcon,
}

struct Harness<A> {
    running: Option<Running<A>>,
    /// Where the device lands when it is built off-thread, which is the only
    /// option in a browser.
    pending: Rc<RefCell<Option<(Arc<Window>, GpuState)>>>,
    /// Set once the window exists, so a second `resumed` does not build another.
    started: bool,
}

impl<A> Default for Harness<A> {
    fn default() -> Self {
        Self { running: None, pending: Rc::new(RefCell::new(None)), started: false }
    }
}

impl<A: App> Harness<A> {
    /// Promote a delivered device into a running app.
    fn take_pending(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Some((window, gpu)) = self.pending.borrow_mut().take() else {
            return;
        };
        let app = A::init(&gpu);
        let last_frame = now_secs();
        self.running = Some(Running {
            window,
            gpu,
            app,
            last_frame,
            ctrl: false,
            cursor: winit::window::CursorIcon::Default,
        });
        if let Some(r) = &self.running {
            r.window.request_redraw();
        }
    }
}

impl<A: App> ApplicationHandler for Harness<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;

        let attributes = Window::default_attributes().with_title("Conway's Kingdom");
        let window = Arc::new(
            event_loop
                .create_window(attributes)
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

        let pending = self.pending.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let gpu = pollster::block_on(GpuState::new(window.clone()));
            *pending.borrow_mut() = Some((window, gpu));
        }

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let gpu = GpuState::new(window.clone()).await;
            *pending.borrow_mut() = Some((window.clone(), gpu));
            // Nothing else will wake the loop, so ask for the first frame here.
            window.request_redraw();
        });

        self.take_pending();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.take_pending();
        let Some(r) = &mut self.running else { return };

        // The app sees the raw event first, and anything it takes does not
        // also fire the typed callbacks -- otherwise pressing a button would
        // take the cell behind it.
        let consumed = r.app.on_window_event(&event, r.gpu.scale_factor);

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                r.gpu.resize(size.width, size.height);
                r.app.resize(&r.gpu);
            }
            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key: winit::keyboard::PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } if !consumed => r.app.on_key(code, state == ElementState::Pressed),
            WindowEvent::ModifiersChanged(state) => r.ctrl = state.state().control_key(),
            WindowEvent::MouseWheel { delta, .. } if !consumed => r.app.on_scroll(delta, r.ctrl),
            WindowEvent::PinchGesture { delta, .. } if !consumed => r.app.on_pinch(delta),
            WindowEvent::CursorMoved { position, .. } => r.app.on_cursor(position.x, position.y),
            WindowEvent::MouseInput { button, state, .. } if !consumed => {
                r.app.on_click(button, state == ElementState::Pressed)
            }
            WindowEvent::Touch(t) if !consumed => {
                r.app.on_touch(t.id, t.phase, t.location.x, t.location.y)
            }
            WindowEvent::RedrawRequested => {
                // Keep the surface the size of the box the canvas is shown
                // in, checked every frame because that box changes with no
                // event this would otherwise see -- a devtools pane opening,
                // for one.
                //
                // The *surface* rather than the canvas, because wgpu's WebGL
                // backend sets the canvas's backing store to match whatever
                // the surface is configured to: write the canvas and it is
                // undone on the next frame, write the surface and the canvas
                // follows. winit's own `inner_size` is no use here either --
                // it starts at zero, so the surface is configured 1x1 before
                // any resize observation lands, and the whole game is drawn
                // into one pixel and stretched over the window.
                #[cfg(target_arch = "wasm32")]
                if let Some(want) = wanted_canvas_size(&r.window) {
                    if want != r.gpu.size {
                        log::info!("surface {:?} -> {want:?}", r.gpu.size);
                        r.gpu.resize(want.0, want.1);
                        r.app.resize(&r.gpu);
                    }
                }

                let now = now_secs();
                let dt = (now - r.last_frame).max(0.0) as f32;
                r.last_frame = now;

                r.app.update(&r.gpu, dt);

                let cursor = r.app.cursor_icon();
                if cursor != r.cursor {
                    r.cursor = cursor;
                    r.window.set_cursor(cursor);
                }

                match Frame::begin(&r.gpu) {
                    FrameAcquire::Ready(frame) => {
                        let calls = r.app.draw_calls();
                        let app = &r.app;
                        let gpu = &r.gpu;
                        frame.submit(gpu, app.clear_color(), &calls, |encoder, pass| {
                            app.overlay(gpu, encoder, pass);
                        });
                    }
                    FrameAcquire::Skip => {}
                    FrameAcquire::Reconfigure => {
                        let (w, h) = r.gpu.size;
                        r.gpu.resize(w, h);
                    }
                    FrameAcquire::Lost => {
                        log::error!("GPU device lost");
                        event_loop.exit();
                    }
                }

                // Ask for the next frame now that this one is done. One
                // scheduler, paced by the display.
                r.window.request_redraw();
            }
            _ => {}
        }
    }
}

/// The backing store the canvas ought to have, in physical pixels.
///
/// A canvas has two sizes: the `width`/`height` attributes, which are the
/// pixels drawn into, and the CSS box those pixels are stretched across. The
/// page styles the canvas `100vw` by `100vh`, so the box is right from the
/// start; this is the size the pixels have to match it with.
///
/// Measured from the canvas's own client box rather than from
/// `window.innerWidth`, because the box is what the pixels are stretched
/// across. `None` before it has been laid out, when there is nothing to match.
#[cfg(target_arch = "wasm32")]
fn wanted_canvas_size(window: &Window) -> Option<(u32, u32)> {
    use winit::platform::web::WindowExtWebSys;

    let (win, canvas) = (web_sys::window()?, window.canvas()?);
    let dpr = win.device_pixel_ratio().max(1.0);
    let (w, h) = (canvas.client_width(), canvas.client_height());
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(((w as f64 * dpr).round() as u32, (h as f64 * dpr).round() as u32))
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
