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
    /// Pixels per cell **in the world pass's own terms**, so already multiplied
    /// by `over`.
    zoom: f32,
    /// How many samples across one screen pixel the world was drawn at. Two
    /// normally; one where the target had to fall back to the screen's own
    /// size — see `render::context::Offscreen::offscreen_size`.
    over: f32,
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

/// One sample of the world, filtered against its neighbours by phase.
///
/// **This is what a single-sample buffer needs and a supersampled one does
/// not.** With one reading a pixel there is nothing inside the pixel to
/// average, so the only thing left is to ask how far the pixel's footprint
/// reaches past the texel it is centred in and take that much of the
/// neighbour. It is a reconstruction from too little, and it is why the
/// weighting has to be by phase rather than flat: a quarter of each of four
/// softens a sample sitting dead in the middle of a texel exactly as much as
/// one straddling two, and that is a blur.
fn phased(at: vec2<i32>, last: vec2<i32>, grid_zoom: f32) -> vec4<f32> {
    let here = textureLoad(world, at, 0);
    // Guarded, because a zero here is a divide by nought and a screen of NaN —
    // and the buffer is zero for exactly as long as it takes the first camera
    // to be written into it.
    let zoom = max(grid_zoom, 1e-4);
    let texels = (grid.origin + vec2<f32>(at) / zoom) * TILE_N;
    let f = fract(texels);
    let half = 0.5 * TILE_N / zoom;

    // How much of the footprint spills into the texel before and the one after.
    let before = max(vec2<f32>(0.0), vec2<f32>(half) - f);
    let after = max(vec2<f32>(0.0), f + half - 1.0);
    // Chosen in floats and converted, rather than as `select(vec2<i32>, ...)`.
    // A component-wise `select` lowers to GLSL's `mix(x, y, bvec)`, and that
    // overload exists for floats in GLSL ES 3.00 and for integers only from
    // 3.20 — so the integer form compiles everywhere except WebGL2, which is
    // the one backend that reaches this shader through GLSL at all.
    let toward = vec2<i32>(select(vec2<f32>(-1.0), vec2<f32>(1.0), after > before));
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(world)) - vec2<i32>(1, 1);
    let over = max(i32(grid.over), 1);

    // **The real samples, averaged, which is the only thing here that adds
    // information rather than rearranging it.**
    //
    // The world pass draws at `over` times the screen's size, so each screen
    // pixel has `over^2` readings of the world underneath it and the honest
    // answer is their mean. That is what antialiasing is; everything the
    // resolve did before this was a reconstruction from a single reading,
    // making the best of a sample that had already thrown the detail away.
    if over > 1 {
        let base = vec2<i32>(floor(in.clip.xy)) * over;
        var sum = vec4<f32>(0.0);
        for (var y = 0; y < over; y = y + 1) {
            for (var x = 0; x < over; x = x + 1) {
                sum = sum + textureLoad(world, clamp(base + vec2<i32>(x, y), vec2<i32>(0), last), 0);
            }
        }
        return sum / f32(over * over);
    }

    // No room for a larger target on this display, so the old reconstruction
    // is still the best available — see `phased`.
    return phased(vec2<i32>(floor(in.clip.xy)), last, grid.zoom);
}
