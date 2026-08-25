//! The views, and the interface they are drawn with.
//!
//! A view is a screen: [`battle`] is the game, and a menu or a lobby would sit
//! beside it. [`Views`] is the egui plumbing they share.
//!
//! Lives under `client` rather than `render` because what to show is policy,
//! not plumbing — `render` stays generic wgpu and winit, and knows nothing
//! about egui. The client feeds it events and hands it the pass.
//!
//! egui draws into the same render pass as the world, so there is no second
//! surface and no compositing step.
//!
//! Input is translated from winit by hand rather than by `egui-winit`, which
//! does not compile for wasm32 at 0.36: `egui::DroppedFile` declares
//! `bytes_async` under `cfg(wasm32)` and egui-winit's implementation only
//! provides the native `bytes`. Translating here keeps one code path for both
//! targets, and a HUD needs only pointer, wheel and modifiers — the IME and
//! clipboard handling that egui-winit exists for is not in play.

pub mod battle;
pub mod camera;
pub mod clock;
pub mod help;
pub mod hotbar;
pub mod hud;
pub mod icons;
pub mod lobby;
pub mod menu;
pub mod overlay;
pub mod record;
pub mod stamp;
pub mod theme;
pub mod words;

use crate::render::context::GpuState;

pub struct Views {
    ctx: egui::Context,
    /// Events gathered since the last frame.
    events: Vec<egui::Event>,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    /// What each panel covered last frame, in points.
    ///
    /// Consumption is decided from this rather than from egui's own
    /// `wants_pointer`, which depends on interaction state this integration
    /// feeds by hand: if any of that is wrong the answer sticks true and the
    /// world silently stops receiving clicks, with nothing to show why. A
    /// rectangle can be reasoned about, printed, and seen.
    ///
    /// One rectangle per panel, never their union. Two panels' union is their
    /// bounding box, and the panels are in opposite corners — the box between
    /// a HUD at the top left and a hotbar at the bottom centre is most of the
    /// window, so the world only received the strip beyond it.
    claimed: Vec<egui::Rect>,
    /// True while a widget is being dragged, so a drag that leaves the panel
    /// still belongs to the panel.
    dragging_widget: bool,
    start: f64,
    /// What each digit key types with shift held.
    ///
    /// Starts as what the **common layout** types, and is corrected the moment
    /// a key says otherwise. Both halves matter: seeding means the great
    /// majority see the right label on the first frame without pressing
    /// anything, and correcting means somebody on Programmer Dvorak — where
    /// the digits are shifted to begin with, so shift and `1` is not `!` — is
    /// only shown the wrong one until they use it.
    ///
    /// Guessed rather than asked, because there is no portable way to ask. On
    /// the web `navigator.keyboard.getLayoutMap()` would answer properly and is
    /// Chrome-only and asynchronous; natively there is nothing. What is *not*
    /// guessed is the binding: keys are bound by physical position, so every
    /// layout has the same key in the same place and only the label is ever in
    /// question.
    shifted_digits: [Option<String>; 9],
    pub theme: theme::Theme,
    renderer: egui_wgpu::Renderer,
}

/// What shift and the digits type on the layout most people have. A starting
/// guess, corrected by [`Views::shifted_digits`] as soon as a key disagrees.
const COMMON_SHIFTED_DIGITS: [&str; 9] = ["!", "@", "#", "$", "%", "^", "&", "*", "("];

/// Which of the nine shift-keyed squares a physical key is, if any. By
/// position on the board, not by what it prints, so it is the same key on
/// every layout.
fn digit_index(code: winit::keyboard::KeyCode) -> Option<usize> {
    use winit::keyboard::KeyCode as K;
    Some(match code {
        K::Digit1 => 0,
        K::Digit2 => 1,
        K::Digit3 => 2,
        K::Digit4 => 3,
        K::Digit5 => 4,
        K::Digit6 => 5,
        K::Digit7 => 6,
        K::Digit8 => 7,
        K::Digit9 => 8,
        _ => return None,
    })
}

/// Borrowed out so the match arm above reads as one thing. `KeyEvent::state`
/// is a field, and taking a reference to it inside the pattern would move the
/// event out of the borrow.
fn state_of(event: &winit::event::KeyEvent) -> &winit::event::ElementState {
    &event.state
}

/// The egui key a winit key means, where egui has one.
///
/// Only what a text field and a menu need: editing, moving the caret,
/// confirming, and leaving. Not the letters and digits — those reach a field
/// as `Text`, and egui only wants them as `Key` for shortcuts, which this
/// integration has no clipboard to serve.
fn egui_key(key: &winit::keyboard::Key) -> Option<egui::Key> {
    use winit::keyboard::{Key, NamedKey};
    let named = match key {
        Key::Named(named) => named,
        _ => return None,
    };
    Some(match named {
        NamedKey::Enter => egui::Key::Enter,
        NamedKey::Tab => egui::Key::Tab,
        NamedKey::Space => egui::Key::Space,
        NamedKey::Backspace => egui::Key::Backspace,
        NamedKey::Delete => egui::Key::Delete,
        NamedKey::Escape => egui::Key::Escape,
        NamedKey::ArrowLeft => egui::Key::ArrowLeft,
        NamedKey::ArrowRight => egui::Key::ArrowRight,
        NamedKey::ArrowUp => egui::Key::ArrowUp,
        NamedKey::ArrowDown => egui::Key::ArrowDown,
        NamedKey::Home => egui::Key::Home,
        NamedKey::End => egui::Key::End,
        NamedKey::PageUp => egui::Key::PageUp,
        NamedKey::PageDown => egui::Key::PageDown,
        NamedKey::Insert => egui::Key::Insert,
        _ => return None,
    })
}

/// Hand every texture change to the renderer, then empty the delta.
///
/// Emptying it is not tidiness. `TexturesDelta` asserts on drop that it is
/// empty, and reading it through a reference leaves it full, so the assert
/// fires however faithfully the deltas were handled. Split out so the emptying
/// can be tested without a GPU, since the bug is in the bookkeeping rather
/// than in the upload.
fn consume_textures(delta: &mut egui::TexturesDelta, mut sink: impl FnMut(Change<'_>)) {
    // A texture can arrive as several partial updates in one frame, so each id
    // carries a list rather than a single delta.
    for (id, deltas) in &delta.set {
        for d in deltas {
            sink(Change::Set(*id, d));
        }
    }
    for id in &delta.free {
        sink(Change::Free(*id));
    }
    delta.clear();
}

/// One texture change. A single callback rather than two, because both need
/// the renderer and two closures cannot borrow it at once.
enum Change<'a> {
    Set(egui::TextureId, &'a egui::epaint::ImageDelta),
    Free(egui::TextureId),
}

/// Whether any panel covers the pointer.
///
/// A list rather than one rectangle, and this is why: the panels sit in
/// different corners, and anything that folds them into a single rectangle
/// first claims all the world between them.
fn claims(panels: &[egui::Rect], pointer: egui::Pos2) -> bool {
    panels.iter().any(|panel| panel.contains(pointer))
}

/// The shapes a frame of interface produced.
///
/// Deliberately holds no `TexturesDelta`. egui panics if one is dropped with
/// deltas unapplied, and a frame is not always drawn — the surface can report
/// Skip while it settles, which is exactly when the font atlas first arrives.
/// Uploading textures when they are produced rather than when they are drawn
/// removes the failure case instead of guarding it.
pub struct Output {
    primitives: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
}

impl Views {
    pub fn new(gpu: &GpuState) -> Self {
        let ctx = egui::Context::default();
        let theme = theme::Theme::default();
        // Once, not per frame: egui keeps its style between passes.
        theme.apply(&ctx);
        Self {
            ctx,
            theme,
            events: Vec::new(),
            pointer: egui::Pos2::ZERO,
            modifiers: egui::Modifiers::default(),
            claimed: Vec::new(),
            dragging_widget: false,
            start: 0.0,
            shifted_digits: COMMON_SHIFTED_DIGITS.map(|c| Some(c.to_string())),
            // No depth buffer and one sample, matching the world's pipeline;
            // egui has to agree with it because they share a pass.
            renderer: egui_wgpu::Renderer::new(
                &gpu.device,
                gpu.config.format,
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    depth_stencil_format: None,
                    ..Default::default()
                },
            ),
        }
    }

    /// The egui context, for anything that has to be registered with it —
    /// a texture, say — before a frame is built.
    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Whether the interface, rather than the world, should get the pointer.
    pub fn wants_pointer(&self) -> bool {
        self.dragging_widget || claims(&self.claimed, self.pointer)
    }

    /// What shift and this digit type on the keyboard in front of the player,
    /// once they have pressed it. `None` until then.
    pub fn shifted_digit(&self, digit: u32) -> Option<&str> {
        let index = (digit as usize).checked_sub(1)?;
        self.shifted_digits.get(index)?.as_deref()
    }

    /// Whether the interface, rather than the world, should get the keyboard.
    ///
    /// Asked of egui directly, unlike [`Self::wants_pointer`], and the reason
    /// the two differ is worth stating. A pointer is claimed by *where it is*,
    /// which this integration can answer from the rectangles each panel
    /// reported, and answering it that way avoids depending on interaction
    /// state fed by hand. The keyboard is claimed by *what has focus*, which
    /// is egui's own bookkeeping and not something a rectangle can express:
    /// there is nowhere else to ask.
    pub fn wants_keyboard(&self) -> bool {
        self.ctx.egui_wants_keyboard_input()
    }

    /// Translate a window event. Returns whether the world should ignore it.
    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent, scale: f32) -> bool {
        use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                // egui works in points; winit reports physical pixels.
                self.pointer = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                self.events.push(egui::Event::PointerMoved(self.pointer));
                // Never withheld. The client tracks the cursor for its own
                // hover and drag handling, and a position is not an action.
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.events.push(egui::Event::PointerGone);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = (match button {
                    MouseButton::Left => Some(egui::PointerButton::Primary),
                    MouseButton::Right => Some(egui::PointerButton::Secondary),
                    MouseButton::Middle => Some(egui::PointerButton::Middle),
                    _ => None,
                }) else {
                    return false;
                };
                self.events.push(egui::Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: *state == ElementState::Pressed,
                    modifiers: self.modifiers,
                });
                self.wants_pointer()
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, d) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(p) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(p.x as f32 / scale, p.y as f32 / scale),
                    ),
                };
                self.events.push(egui::Event::MouseWheel {
                    unit,
                    delta: d,
                    // winit gives no phase for a wheel, and egui's own advice
                    // when it is unknown is Move.
                    phase: egui::TouchPhase::Move,
                    modifiers: self.modifiers,
                });
                self.wants_pointer()
            }
            WindowEvent::ModifiersChanged(state) => {
                let s = state.state();
                self.modifiers = egui::Modifiers {
                    alt: s.alt_key(),
                    ctrl: s.control_key(),
                    shift: s.shift_key(),
                    mac_cmd: false,
                    command: s.control_key(),
                };
                false
            }
            // Typing. Nothing needed it until there was a field to type into,
            // and a menu is two text fields, so this is where the keyboard
            // stops being the game's alone.
            //
            // Two events, not one. `Text` is what a character key contributes
            // to a field; `Key` is what backspace, the arrows, enter and escape
            // do to one. A key that produces text produces both, because egui
            // routes shortcuts off `Key` and content off `Text`, and a field
            // that got only text could never be corrected.
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let pressed = *state_of(event) == ElementState::Pressed;
                if let Some(key) = egui_key(&event.logical_key) {
                    self.events.push(egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers: self.modifiers,
                    });
                }
                // Watch what shift and a digit actually types, so the hotbar
                // can label its keys with what is on the keyboard rather than
                // with what a US layout would have printed.
                if pressed && self.modifiers.shift {
                    if let (winit::keyboard::PhysicalKey::Code(code), Some(text)) =
                        (event.physical_key, event.text.as_ref())
                    {
                        if let Some(index) = digit_index(code) {
                            let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                            if !typed.is_empty() {
                                self.shifted_digits[index] = Some(typed);
                            }
                        }
                    }
                }
                // Only on the way down, and never while a command modifier is
                // held: ctrl+V is a paste, and inserting a literal "v" beside
                // it is the sort of thing that only shows up in somebody's
                // password field.
                if pressed
                    && !self.modifiers.command
                    && !self.modifiers.alt
                    && let Some(text) = event.text.as_ref()
                {
                    // Control characters arrive here as text -- enter is
                    // "\r", escape is "\u{1b}" -- and inserting them into a
                    // field puts an invisible character in a room name.
                    let printable: String = text.chars().filter(|c| !c.is_control()).collect();
                    if !printable.is_empty() {
                        self.events.push(egui::Event::Text(printable));
                    }
                }
                self.wants_keyboard()
            }
            _ => false,
        }
    }

    /// Build the frame, upload whatever textures it produced, and hand back
    /// the shapes to draw.
    pub fn run(
        &mut self,
        gpu: &GpuState,
        now: f64,
        build: impl FnOnce(&egui::Context) -> Vec<egui::Rect>,
    ) -> Output {
        if self.start == 0.0 {
            self.start = now;
        }
        let pixels_per_point = gpu.scale_factor;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(
                    gpu.size.0 as f32 / pixels_per_point,
                    gpu.size.1 as f32 / pixels_per_point,
                ),
            )),
            time: Some(now - self.start),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.ctx.set_pixels_per_point(pixels_per_point);

        self.ctx.begin_pass(input);
        self.claimed = build(&self.ctx);
        let mut full = self.ctx.end_pass();

        self.dragging_widget = self.ctx.egui_is_using_pointer();

        let renderer = &mut self.renderer;
        consume_textures(&mut full.textures_delta, |change| match change {
            Change::Set(id, delta) => renderer.update_texture(&gpu.device, &gpu.queue, id, delta),
            Change::Free(id) => renderer.free_texture(&id),
        });

        Output { primitives: self.ctx.tessellate(full.shapes, pixels_per_point), pixels_per_point }
    }

    /// Record the interface into the pass the world was just drawn into.
    pub fn render(
        &mut self,
        gpu: &GpuState,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut wgpu::RenderPass<'static>,
        output: &Output,
    ) {
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.size.0, gpu.size.1],
            pixels_per_point: output.pixels_per_point,
        };
        self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            encoder,
            &output.primitives,
            &descriptor,
        );
        self.renderer.render(pass, &output.primitives, &descriptor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure that took the client down on startup: egui hands over a
    /// `TexturesDelta`, and dropping one that still holds deltas asserts. It
    /// happened on the first frames, when the font atlas arrives and the
    /// surface is still reporting Skip, so nothing had drawn yet.
    ///
    /// No GPU here: the bug is in the bookkeeping, and the bookkeeping is what
    /// this checks.
    #[test]
    fn a_frames_textures_are_consumed_and_the_delta_emptied() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 240.0),
            )),
            ..Default::default()
        });
        egui::Area::new("test".into()).show(&ctx, |ui| ui.label("Player 1"));
        let mut full = ctx.end_pass();

        // Drawing text builds a font atlas, so there is something to lose.
        assert!(
            !full.textures_delta.is_empty(),
            "no textures produced, so this would pass for the wrong reason"
        );

        let mut uploaded = 0;
        consume_textures(&mut full.textures_delta, |change| {
            if matches!(change, Change::Set(..)) {
                uploaded += 1;
            }
        });
        assert!(uploaded > 0, "the delta should have reached the renderer");
        assert!(
            full.textures_delta.is_empty(),
            "handling the deltas is not enough; the delta must be emptied too"
        );
        // Dropping `full` here is the actual assertion: it panics if not empty.
    }

    /// The panels were folded into one rectangle with `Rect::union`, which is
    /// their bounding box. A HUD at the top left and a hotbar at the bottom
    /// centre bound most of the window between them, so the world only ever
    /// received the strip to the right of the hotbar — and every gesture
    /// anywhere else was swallowed with nothing on screen to say why.
    #[test]
    fn panels_claim_themselves_and_not_the_space_between_them() {
        let hud = egui::Rect::from_min_size(egui::pos2(14.0, 14.0), egui::vec2(220.0, 300.0));
        let hotbar = egui::Rect::from_min_size(egui::pos2(600.0, 700.0), egui::vec2(110.0, 50.0));
        let panels = [hud, hotbar];

        assert!(claims(&panels, egui::pos2(100.0, 100.0)), "on the HUD");
        assert!(claims(&panels, egui::pos2(640.0, 720.0)), "on the hotbar");

        // Between the two, and the case the union got wrong.
        assert!(!claims(&panels, egui::pos2(400.0, 400.0)), "open world");
        assert!(!claims(&panels, egui::pos2(100.0, 690.0)), "below the HUD");
        assert!(!claims(&panels, egui::pos2(590.0, 60.0)), "above the hotbar");
        assert!(
            claims(&[hud.union(hotbar)], egui::pos2(400.0, 400.0)),
            "the union swallowed open world, which is the bug this replaced"
        );
    }
}
