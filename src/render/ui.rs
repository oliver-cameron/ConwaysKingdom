//! The overlay.
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

use crate::render::context::GpuState;

pub struct Ui {
    ctx: egui::Context,
    /// Events gathered since the last frame.
    events: Vec<egui::Event>,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    /// Whether egui claimed the pointer last frame. Used to decide whether a
    /// click belongs to the interface or the world; one frame stale, which no
    /// one can perceive and which avoids running the UI twice per frame.
    wants_pointer: bool,
    start: f64,
    renderer: egui_wgpu::Renderer,
}

/// What a frame of UI produced: the shapes to draw and the textures to upload.
pub struct Output {
    primitives: Vec<egui::ClippedPrimitive>,
    textures_delta: egui::TexturesDelta,
    pixels_per_point: f32,
}

impl Ui {
    pub fn new(gpu: &GpuState) -> Self {
        Self {
            ctx: egui::Context::default(),
            events: Vec::new(),
            pointer: egui::Pos2::ZERO,
            modifiers: egui::Modifiers::default(),
            wants_pointer: false,
            start: 0.0,
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

    /// Whether the interface, rather than the world, should get the next click.
    pub fn wants_pointer(&self) -> bool {
        self.wants_pointer
    }

    /// Translate a window event. Returns whether the world should ignore it.
    pub fn on_window_event(&mut self, event: &winit::event::WindowEvent, scale: f32) -> bool {
        use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                // egui works in points; winit reports physical pixels.
                self.pointer = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                self.events.push(egui::Event::PointerMoved(self.pointer));
                self.wants_pointer
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
                self.wants_pointer
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
                self.wants_pointer
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
            _ => false,
        }
    }

    /// Build the frame, then hand back what to draw.
    pub fn run(&mut self, gpu: &GpuState, now: f64, build: impl FnOnce(&egui::Context)) -> Output {
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
        build(&self.ctx);
        let full = self.ctx.end_pass();

        self.wants_pointer = self.ctx.egui_wants_pointer_input();

        Output {
            primitives: self.ctx.tessellate(full.shapes, pixels_per_point),
            textures_delta: full.textures_delta,
            pixels_per_point,
        }
    }

    /// Upload what changed, then record the overlay into the pass the world was
    /// just drawn into.
    pub fn render(
        &mut self,
        gpu: &GpuState,
        encoder: &mut wgpu::CommandEncoder,
        pass: &mut wgpu::RenderPass<'static>,
        output: &Output,
    ) {
        // A texture can arrive as several partial updates in one frame, so each
        // id carries a list rather than a single delta.
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }

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

        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
