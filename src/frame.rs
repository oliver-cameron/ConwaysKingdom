use std::ops::Range;

use crate::gpu::GpuState;

pub struct IndexBufferBinding<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub format: wgpu::IndexFormat,
}

pub enum Draw {
    /// Non-indexed draw: `pass.draw(vertices, instances)`.
    Vertices {
        vertices: Range<u32>,
        instances: Range<u32>,
    },
    /// Indexed draw: `pass.draw_indexed(indices, base_vertex, instances)`.
    /// Requires `DrawCall::index_buffer` to be set.
    Indexed {
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    },
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

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        FrameAcquire::Ready(Self { output, view, encoder })
    }

    /// Records every draw call into a single render pass, then submits
    /// the command buffer and presents. `clear_color` of `None` loads
    /// the existing contents of the target instead of clearing it
    /// (useful if you're compositing multiple passes elsewhere).
    pub fn submit(mut self, gpu: &GpuState, clear_color: Option<wgpu::Color>, calls: &[DrawCall]) {
        {
            let mut pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
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
            });

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
                    Draw::Indexed {
                        indices,
                        base_vertex,
                        instances,
                    } => {
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

        gpu.queue.submit(std::iter::once(self.encoder.finish()));
        // Presentation moved from SurfaceTexture::present() to
        // Queue::present(surface_texture) in newer wgpu.
        gpu.queue.present(self.output);
    }
}
