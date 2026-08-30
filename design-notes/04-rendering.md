# Rendering: one pipeline, one bind group, one draw

A render pipeline is the compiled shader plus fixed-function state. Textures live in bind groups. Binding a different texture costs at most a `set_bind_group`, never a `set_pipeline` — and with an array texture it costs nothing at all, because every chunk is a layer of the same binding.

The whole frame is:

```rust
pass.set_pipeline(&pipeline);
pass.set_bind_group(0, &bind_group, &[]);
pass.set_vertex_buffer(0, instances.slice(..));
pass.draw(0..4, 0..(n_chunks + 1));      // triangle strip; last instance is the minimap
```

## Four ways to have many textures with one pipeline

**Multiple bindings in one group.** Different bindings may have different view dimensions, so `texture_2d_array<u32>` and `texture_2d<u32>` coexist. No feature flags, works on WebGL2. This is the world-plus-minimap case. Downlevel allows 16 sampled textures per stage.

**Array texture.** Many textures that are identical in size, format, mip count and sample count collapse to one binding indexed at runtime. This is the chunk store. Capped at 256 layers on the guaranteed minimums.

**Atlas.** One texture, sub-rects. Loses to the array on capacity — see [02-texture-residency.md](02-texture-residency.md).

**Binding arrays.** True bindless, genuinely different textures indexed at runtime. Needs `Features::TEXTURE_BINDING_ARRAY`, so native only, and the docs note that when any binding in a group is an array, no buffer in that group may be `Uniform` — the camera uniform would have to move groups. Not worth it here.

## Bind group

```rust
entries: &[
    // 0: camera uniform
    wgpu::BindGroupLayoutEntry {
        binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false, min_binding_size: None },
        count: None,
    },
    // 1: chunk store
    wgpu::BindGroupLayoutEntry {
        binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false },
        count: None,
    },
    // 2: minimap
    wgpu::BindGroupLayoutEntry {
        binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false },
        count: None,
    },
]
```

`Rg8Uint` maps to `TextureSampleType::Uint`, which means `texture_2d_array<u32>` and `textureLoad` — no sampler, no filter mode, and nearest-neighbour scaling by construction rather than by configuring a sampler correctly.

## Instances

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    /// kind 0: world-space cell rect. kind 1: screen-pixel rect.
    rect: [f32; 4],
    /// x = array layer, y = kind, z = tint/owner, w = pad
    meta: [u32; 4],
}
```

32 bytes, two vertex attributes — inside the downlevel caps of 16 attributes and a 255-byte stride. Storage buffers are unavailable on WebGL2 (`max_storage_buffers_per_shader_stage: 0`), so an instance vertex buffer is the only way to get per-chunk data across, and also the right way.

## Shaders

```wgsl
struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the viewport's top-left
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    _pad:     vec3<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks:  texture_2d_array<u32>;
@group(0) @binding(2) var minimap: texture_2d<u32>;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) meta: vec4<u32>,
) -> VsOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));   // triangle-strip quad
    let kind = meta.y;

    var px = rect.xy + corner * rect.zw;
    if kind == 0u { px = (px - cam.origin) * cam.zoom; }          // kind 1 is already screen-space

    var out: VsOut;
    out.clip  = vec4<f32>(px / cam.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv    = corner * select(vec2<f32>(MINIMAP_N), vec2<f32>(CHUNK_N), kind == 0u);
    out.layer = meta.x;
    out.kind  = kind;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = vec2<i32>(floor(in.uv));
    var t: vec2<u32>;
    if in.kind == 0u { t = textureLoad(chunks,  c, i32(in.layer), 0).rg; }
    else             { t = textureLoad(minimap, c, 0).rg; }
    if t.x == 0u { return DEAD; }
    return vec4<f32>(palette(t.y), 1.0);      // t.x = kind, t.y = player
}
```

That branch is legal specifically because `textureLoad` takes no derivatives. `textureSample` with implicit LOD could not be called under non-uniform control flow. `kind` is `@interpolate(flat)`, so the branch is coherent across every fragment of a quad.

The triangle-strip quad winds clockwise, which is fine because `PipelineDescriptor` defaults `cull_mode` to `None`. Palettes must be a function-local `var` array, not a module-level `const`, to be indexable by a runtime value.

## Do not contort for one pipeline

Pipeline count should be bounded by how many *kinds* of thing you draw, not by how much content there is. Two pipelines for "world" and "minimap" is fine forever; a `set_pipeline` twice a frame is irrelevant. The minimap will likely want genuinely different shading — aggregate territory colours, a viewport rectangle overlay, edge fade — and forcing that through one fragment shader via a `kind` branch is false economy the moment the two diverge. Take the unified version only while the minimap really is the same colours, smaller.

## The minimap

It is not a downscaled copy of the world, it is derived aggregate data at chunk granularity, which is why it never participates in the aliasing problem. Each chunk already tracks `all_dead` to skip simulation; extend that to a dominant-owner and live-count summary and the minimap is a byte or two per chunk, written on the CPU during the generation step and uploaded as one dirty rectangle per frame.

At one texel per chunk, a 512x512 minimap covers 512x512 chunks. Resolution is a free parameter precisely because the data is computed rather than sampled — summarise at one texel per 16x16 cells instead if a finer view is wanted.
