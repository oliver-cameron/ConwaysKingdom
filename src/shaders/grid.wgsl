// One instanced quad per visible chunk. Cells come from an Rgba8Uint array
// texture read with textureLoad, so scaling is nearest-neighbour by
// construction and there is no sampler to configure.

struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the top-left pixel
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    _pad0:    f32,
    _pad1:    vec2<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks: texture_2d_array<u32>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,                       // cell coords in the chunk
    @location(1) @interpolate(flat) layer: u32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,                        // x, y, w, h in world cells
    @location(1) attrs: vec4<u32>,                        // x = array layer
) -> VsOut {
    // Unit quad as a triangle strip: (0,0) (1,0) (0,1) (1,1).
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let world = rect.xy + corner * rect.zw;
    let px = (world - cam.origin) * cam.zoom;

    var out: VsOut;
    out.clip = vec4<f32>(
        px / cam.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0),
        0.0, 1.0,
    );
    out.local = corner * rect.zw;
    out.layer = attrs.x;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let coord = vec2<i32>(floor(in.local));
    let cell = textureLoad(chunks, coord, i32(in.layer), 0);
    // R = kind (0 is dead), G = player, B = age, A = flags.
    if cell.r == 0u {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);            // dead: black
    }
    return vec4<f32>(1.0, 0.85, 0.1, 1.0);               // alive: yellow
}
