// One instanced quad per visible chunk. Cells come from an R16Uint array
// texture read with textureLoad, so scaling is nearest-neighbour by
// construction and there is no sampler to configure.

// MUST MATCH `sim::cell::bits`. The shader cannot read Rust constants, so this
// block is the one thing kept in step by hand; changing the split means
// changing both.
//
//  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
// |   player    |      metadata / flags      | A |
const ALIVE_BIT:    u32 = 1u;
const META_SHIFT:   u32 = 1u;   const META_MASK: u32 = 1023u;
const PLAYER_SHIFT: u32 = 11u;  // top field, so no mask is needed

struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the top-left pixel
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    chunk_n:  f32,         // cells per chunk edge
    _pad:     vec2<f32>,
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
    let cell = textureLoad(chunks, coord, i32(in.layer), 0).r;

    if (cell & ALIVE_BIT) == 0u {
        // Tint the outermost ring of dead cells so chunk boundaries are
        // visible: that is what makes chunk loading something you can watch.
        let n = cam.chunk_n;
        if in.local.x < 1.0 || in.local.y < 1.0
            || in.local.x >= n - 1.0 || in.local.y >= n - 1.0 {
            return vec4<f32>(0.06, 0.06, 0.09, 1.0);     // chunk grid
        }
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);            // dead: black
    }
    return vec4<f32>(1.0, 0.85, 0.1, 1.0);               // alive: yellow
}
