use crate::render::context::GpuState;

/// Everything needed to build a `wgpu::RenderPipeline` from a WGSL
/// source string. Deliberately generic — it doesn't know anything
/// about tiles, sprites, meshes, etc. Fill it in per pipeline you need
/// (one per distinct shader/vertex-layout combination).
pub struct PipelineDescriptor<'a> {
    pub label: &'a str,
    /// Raw WGSL source. Load it however you like (include_str!, a file
    /// read, a fetched string on web) and pass it in here.
    pub shader_source: &'a str,
    pub vs_entry: &'a str,
    pub fs_entry: &'a str,
    pub vertex_buffers: &'a [wgpu::VertexBufferLayout<'a>],
    pub bind_group_layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    pub topology: wgpu::PrimitiveTopology,
    pub cull_mode: Option<wgpu::Face>,
    /// None = opaque (no blending). Some(BlendState::ALPHA_BLENDING) is
    /// the usual choice for anything with transparency.
    pub blend: Option<wgpu::BlendState>,
}

impl<'a> Default for PipelineDescriptor<'a> {
    fn default() -> Self {
        Self {
            label: "unnamed pipeline",
            shader_source: "",
            vs_entry: "vs_main",
            fs_entry: "fs_main",
            vertex_buffers: &[],
            bind_group_layouts: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            blend: None,
        }
    }
}

/// Compiles `desc.shader_source` and builds a render pipeline targeting
/// the surface's current format. Call this once per pipeline at setup
/// time (it's not cheap enough to call per-frame).
pub fn create_pipeline(gpu: &GpuState, desc: &PipelineDescriptor) -> wgpu::RenderPipeline {
    create_pipeline_with(&gpu.device, gpu.config.format, desc)
}

/// Same, but without a `GpuState` — so a headless test (no window, no surface)
/// can build the real pipeline from the real descriptor.
pub fn create_pipeline_with(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    desc: &PipelineDescriptor,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(desc.label),
        source: wgpu::ShaderSource::Wgsl(desc.shader_source.into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(desc.label),
        bind_group_layouts: desc.bind_group_layouts,
        // Push-constant-style "immediate" data — unused here.
        immediate_size: 0,
    });

    // wgpu 29+ made VertexState::buffers a slice of Option<VertexBufferLayout>
    // (to allow "gap"/unbound slots), so wrap ours before handing them over.
    let buffers: Vec<Option<wgpu::VertexBufferLayout>> =
        desc.vertex_buffers.iter().map(|l| Some(l.clone())).collect();

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(desc.label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(desc.vs_entry),
            buffers: &buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(desc.fs_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: desc.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: desc.topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: desc.cull_mode,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
