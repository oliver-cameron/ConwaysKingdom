// The last thing that happens to the world before the interface is drawn on
// top of it: a box filter one pixel wide, in screen space.
//
// **Why here and not in the world shader.** Filtering the *content* means
// asking what a cell is at several places at once, and a cell is a sprite in
// an atlas — so a filter that crosses a cell boundary has to resolve two
// different tiles, and one that stays inside a tile cannot smooth the boundary
// at all, which is the line the eye actually follows. Every arrangement of it
// either missed cell edges or filtered twice and blurred.
//
// In screen space there are no tiles. A pixel is a pixel, its neighbour is its
// neighbour, and whether the two came from the same cell, two cells or the
// backdrop makes no difference to what the answer should be. One rule, one
// place, and nothing in the world shader has to know it exists.

/// Where the world's texel grid falls on the screen: the camera's origin in
/// cells, and its zoom in pixels per cell.
struct Grid {
    origin: vec2<f32>,
    zoom: f32,
    spare: f32,
};

@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var<uniform> grid: Grid;

// A cell is this many texels of art. The same number as `TILE_N` in
// `grid.wgsl`, and the one thing here kept in step with it by hand.
const TILE_N: f32 = 16.0;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
};

// One triangle covering the screen, from the vertex index alone — no buffer,
// no bind group, nothing to keep in step. A quad would be two triangles with a
// seam down the diagonal that some drivers rasterise twice.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var out: VsOut;
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;
    out.clip = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(in.clip.xy));
    let last = vec2<i32>(textureDimensions(world)) - vec2<i32>(1, 1);
    let here = textureLoad(world, at, 0);

    // **Weighted by where the pixel actually falls, not a flat average.**
    //
    // This took a quarter of each of four pixels, which softens a pixel
    // sitting dead in the middle of a texel exactly as much as one straddling
    // two — that is a blur, and it is what a blur is. What decides how much a
    // neighbour is worth is *phase*: how far this pixel's footprint reaches
    // past the texel it is centred in.
    //
    // So: where the pixel's centre sits inside its texel, and how wide the
    // pixel is in texels.
    // Guarded, because a zero here is a divide by nought and a screen of
    // NaN — and the buffer is zero for exactly as long as it takes the first
    // camera to be written into it.
    let zoom = max(grid.zoom, 1e-4);
    // `clip.xy` is in framebuffer pixels with y down, which is what
    // `vs_main` in grid.wgsl builds its clip position from, so the two agree
    // on both axes and neither needs flipping.
    let texels = (grid.origin + in.clip.xy / zoom) * TILE_N;
    let f = fract(texels);
    let half = 0.5 * TILE_N / zoom;

    // How much of the footprint spills into the texel before and the one
    // after. Below one pixel per texel only one of the two can be non-zero,
    // and a pixel wholly inside a texel spills into neither — which is the
    // case that has to cost nothing, because it is most of the screen.
    let before = max(vec2<f32>(0.0), vec2<f32>(half) - f);
    let after = max(vec2<f32>(0.0), f + half - 1.0);
    let toward = select(vec2<i32>(-1, -1), vec2<i32>(1, 1), after > before);
    let w = max(before, after) / (2.0 * half);

    // The neighbour in whichever direction the footprint reaches, and the
    // corner between the two. Clamped, so the edge of the screen leans on
    // itself rather than reading off the end and coming back black.
    let side = clamp(at + vec2<i32>(toward.x, 0), vec2<i32>(0), last);
    let over = clamp(at + vec2<i32>(0, toward.y), vec2<i32>(0), last);
    let corner = clamp(at + toward, vec2<i32>(0), last);

    // Separable: mix along x at both rows, then between the rows.
    let row = mix(here, textureLoad(world, side, 0), w.x);
    let next = mix(textureLoad(world, over, 0), textureLoad(world, corner, 0), w.x);
    return mix(row, next, w.y);
}
