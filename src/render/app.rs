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

/// Whether the wheel event now being delivered is a zoom gesture.
///
/// On the web the ctrl in a trackpad pinch is **not a modifier state**. The
/// browser sets `ctrlKey` on the wheel event itself and no key is down —
/// which is the universal pinch gesture, and the same one that zooms a page.
/// Modelling it as a held key is the mistake this exists to undo.
///
/// winit does turn it into a `ModifiersChanged`, but only while the canvas has
/// focus, and a freshly loaded page has none. So every pinch arrived looking
/// exactly like a two-finger scroll: vertical, small deltas, no ctrl — and the
/// view panned. Clicking the page fixed it, which is why it kept coming back
/// as fixed and then broken again.
///
/// Read from the event instead, in the **capture** phase on `window`, so the
/// flag is set before the canvas's own listener has queued the event it
/// belongs to. Bubbling would run after it and be a gesture late.
#[cfg(target_arch = "wasm32")]
mod zoom_gesture {
    use std::cell::Cell;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    thread_local! {
        static CTRL: Cell<bool> = const { Cell::new(false) };
        static WATCHING: Cell<bool> = const { Cell::new(false) };
    }

    pub fn last() -> bool {
        CTRL.with(Cell::get)
    }

    /// Idempotent, because `resumed` may be called more than once and two
    /// listeners would be one too many.
    pub fn watch() {
        if WATCHING.with(|w| w.replace(true)) {
            return;
        }
        let Some(window) = web_sys::window() else { return };
        let handler = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(|e: web_sys::WheelEvent| {
            CTRL.with(|c| c.set(e.ctrl_key()));
        });
        let _ = window.add_event_listener_with_callback_and_bool(
            "wheel",
            handler.as_ref().unchecked_ref(),
            true,
        );
        // Leaked deliberately: it listens for as long as the page lives, and
        // dropping a `Closure` detaches it.
        handler.forget();
    }
}

/// Prevent the few browser defaults that would fight the game, and let every
/// other one through.
///
/// winit's blanket `prevent_default` is off, because it took the browser's own
/// shortcuts with it and a page you cannot open the inspector on is a page you
/// cannot debug. What it was doing that is worth keeping is narrow, and this
/// is the whole of it:
///
/// - **the arrows and space scroll a document**, and the game pans with both,
///   so a pan would scroll the page out from under the canvas;
/// - **right-click opens the context menu**, and right-drag pans;
/// - **middle-click starts autoscroll** on Firefox, and middle-drag pans.
///
/// Never when ctrl, meta or alt is held, so ctrl+shift+I, F12, ctrl+R and the
/// rest reach the browser whatever the game has bound. Touch needs nothing
/// here: `touch-action: none` in the page's CSS settles it before an event is
/// delivered at all, which is the only thing that works, since the browser
/// decides what a gesture means before anyone can cancel it.
#[cfg(target_arch = "wasm32")]
mod defaults {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    /// Attach a listener and let it live as long as the page does. Dropping a
    /// `Closure` detaches it, so each one is leaked deliberately.
    fn on<E: wasm_bindgen::convert::FromWasmAbi + 'static>(
        canvas: &web_sys::HtmlCanvasElement,
        event: &str,
        f: impl FnMut(E) + 'static,
    ) {
        let handler = Closure::<dyn FnMut(E)>::new(f);
        let _ = canvas.add_event_listener_with_callback(event, handler.as_ref().unchecked_ref());
        handler.forget();
    }

    pub fn guard(canvas: &web_sys::HtmlCanvasElement) {
        // **Every wheel over the canvas belongs to the game.** It is a zoom,
        // or it is a pan, and the browser's ideas about both are wrong here:
        // ctrl and a wheel is a trackpad pinch, which the page would take as
        // its own zoom, and a sideways wheel is a two-finger swipe, which
        // Chrome takes as going back a page. Losing a world because a pan ran
        // out of sideways is not a thing to leave to chance.
        //
        // On the canvas rather than on the window, and that is the whole
        // reason this works: browsers make wheel listeners on `window`,
        // `document` and `body` passive by default, and a passive listener
        // cannot prevent anything. On any other element they are not.
        on(canvas, "wheel", |e: web_sys::WheelEvent| e.prevent_default());
        on(canvas, "keydown", |e: web_sys::KeyboardEvent| {
            if e.ctrl_key() || e.meta_key() || e.alt_key() {
                return;
            }
            if matches!(
                e.key().as_str(),
                "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | " "
            ) {
                e.prevent_default();
            }
        });
        // Right-drag pans, so the menu would open on top of every pan.
        on(canvas, "contextmenu", |e: web_sys::MouseEvent| e.prevent_default());
        // Button 1 is the middle one, which pans and which Firefox otherwise
        // takes as the start of an autoscroll.
        on(canvas, "mousedown", |e: web_sys::MouseEvent| {
            if e.button() == 1 {
                e.prevent_default();
            }
        });
    }
}

/// Off the web there is no such thing: a pinch either arrives as a
/// `PinchGesture` or as a genuinely held ctrl, and both are already handled.
#[cfg(not(target_arch = "wasm32"))]
mod zoom_gesture {
    pub fn last() -> bool {
        false
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
    /// Frame intervals since the last report, for `frame_report`.
    frames: Vec<f32>,
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
            frames: Vec::new(),
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

        // winit's web backend calls `preventDefault` on everything it handles,
        // and that takes the browser's own shortcuts with it -- F12 and
        // ctrl+shift+I among them, so the page swallowed the key that opens
        // the inspector and there was no way to read the console of a build
        // that was misbehaving. A page has no business doing that.
        //
        // Off, and the handful of defaults actually worth preventing are
        // prevented by hand in `defaults` below.
        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes.with_prevent_default(false)
        };
        let window =
            Arc::new(event_loop.create_window(attributes).expect("failed to create window"));

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            web_sys::window()
                .and_then(|win| win.document())
                .and_then(|doc| {
                    let canvas = window.canvas()?;
                    canvas.set_id("render-canvas");
                    defaults::guard(&canvas);
                    doc.body()?.append_child(&canvas).ok()?;
                    // Focused, or the keyboard goes nowhere. winit gives the
                    // canvas a `tabindex` so it *can* take focus, and then
                    // waits for something to give it: key events go to the
                    // focused element, and until the player has clicked the
                    // page there is not one. So WASD, the arrows, the digits
                    // and escape all did nothing on a freshly loaded page and
                    // began working the moment you clicked -- which, since a
                    // click also draws, looked like the keyboard needing the
                    // game to be "started".
                    let _ = canvas.focus();
                    Some(())
                })
                .expect("couldn't append canvas to document body");
            zoom_gesture::watch();
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

    /// Ask for the next frame here rather than at the end of the last one.
    ///
    /// This is what keeps the loop running, and where it is asked from decides
    /// whether it runs *evenly*. A `request_redraw` made while handling
    /// `RedrawRequested` may be folded into the redraw already being processed
    /// — winit says so, and several backends do it — so the request is dropped,
    /// nothing is left to wake the loop, and under `ControlFlow::Wait` it
    /// sleeps until some input arrives. Then the redraw fires immediately
    /// behind that input: a stall, then two frames almost together, and worst
    /// when the pointer is still because nothing else is waking it.
    ///
    /// `about_to_wait` runs after the queue has drained and before the loop
    /// sleeps, so a request made here is always outstanding when it sleeps and
    /// there is always exactly one frame pending. Pacing then comes from the
    /// present queue, which is `Fifo` and is the thing that should be setting
    /// it.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // **Nothing has started yet, and nothing will unless the loop keeps
        // turning.** On the web `GpuState::new` is async: `resumed` puts the
        // window and a future aside, and `take_pending` collects the result
        // the next time anything happens. Under `ControlFlow::Wait` that "next
        // time" may never come — there is no window to request a redraw on
        // yet, so this used to request nothing, and the loop went to sleep
        // with the adapter still being negotiated. Whether it ever woke came
        // down to whether some unrelated event arrived: a mouse move, a
        // resize, a socket. Nothing arrives on a page nobody has touched.
        //
        // Polling until the GPU is in hand costs a few frames of doing nothing
        // once, at startup, and is the difference between a client that starts
        // and one that sits on a blank canvas for ever.
        let Some(r) = &self.running else {
            event_loop.set_control_flow(ControlFlow::Poll);
            self.take_pending();
            return;
        };
        event_loop.set_control_flow(ControlFlow::Wait);
        r.window.request_redraw();
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
            // A window dragged to a display of a different density, or a
            // browser zoomed. The size in physical pixels may not change at
            // all, so this cannot be left to `Resized` -- and `GpuState`
            // re-reads the factor, so putting the size it already has back
            // through the same path is the whole of it.
            WindowEvent::ScaleFactorChanged { .. } => {
                let (width, height) = r.gpu.size;
                r.gpu.resize(width, height);
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
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                r.app.on_scroll(delta, r.ctrl || zoom_gesture::last())
            }
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
                // Clamped, because frame pacing here is lumpy by construction
                // and the world should not be.
                //
                // The loop runs on `ControlFlow::Wait` and re-arms itself with
                // a `request_redraw` at the end of each frame, so it sleeps
                // between frames -- and `Frame::begin` blocks the same thread
                // on the present queue, since the surface is `Fifo`. Input
                // arriving during that block queues up, and is then drained in
                // one go before the next redraw fires: a wait, then a burst.
                // Every `dt` inherits that jitter, and a long stall -- a
                // window drag, devtools opening, a tab in the background --
                // hands `World::update` a whole second at once, which it turns
                // into `MAX_CATCHUP_STEPS` generations in a single frame.
                //
                // A quarter of a second is one generation at the default rate.
                // Offline that means a stalled frame costs at most one step
                // rather than eight; connected the server is the clock and
                // this only paces the interface.
                const LONGEST_FRAME: f32 = 0.25;
                let dt = ((now - r.last_frame).max(0.0) as f32).min(LONGEST_FRAME);
                r.last_frame = now;

                r.frames.push(dt);
                frame_report(&mut r.frames);

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
            }
            _ => {}
        }
    }
}

/// Say how evenly frames are arriving, every few seconds and only at debug.
///
/// Worth having permanently, because "the frame rate feels lumpy" is a
/// complaint no screenshot can settle and the shape of the distribution says
/// which of two very different things is happening: a low median is not
/// keeping up, and a low median with a long tail is keeping up and stalling.
fn frame_report(frames: &mut Vec<f32>) {
    const EVERY: usize = 240;
    if frames.len() < EVERY {
        return;
    }
    let mut sorted = frames.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q) as usize] * 1000.0;
    let mean = frames.iter().sum::<f32>() / frames.len() as f32;
    log::debug!(
        "frames: {:.0}/s mean, ms p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}, shortest {:.1}",
        1.0 / mean,
        at(0.50),
        at(0.90),
        at(0.99),
        at(1.0),
        at(0.0),
    );
    frames.clear();
}

/// The most device pixels per point the canvas will ask for. See
/// `wanted_canvas_size` for why it is capped rather than followed.
#[cfg(target_arch = "wasm32")]
const MAX_PIXEL_RATIO: f64 = 2.0;

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
    // Capped. A phone at three device pixels a point asks for nine times the
    // fragments of one at a point apiece -- on an emulated iPhone that is a
    // backing store of 2940x5004, fifteen million pixels to fill every frame
    // for a picture made of flat 16x16 tiles. Two is past the point where
    // anyone can see the difference in pixel art and costs less than half.
    let dpr = win.device_pixel_ratio().clamp(1.0, MAX_PIXEL_RATIO);
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
