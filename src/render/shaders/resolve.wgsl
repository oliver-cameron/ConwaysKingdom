// The last thing that happens to the world before the interface is drawn on
// top of it: a box filter one pixel wide, in screen space.
//
// **Why here and not in the world shader.** Filtering the *content* means
// asking what a cell is at four places at once, and a cell is a sprite in an
// atlas — so a filter that crosses a cell boundary has to resolve two
// different tiles, and one that stays inside a tile cannot smooth the boundary
// at all, which is the line the eye actually follows. Every arrangement of it
// either misses cell edges or filters twice and blurs.
//
// In screen space there are no tiles. A pixel is a pixel, its neighbour is its
// neighbour, and whether the two came from the same cell, two cells or the
// backdrop makes no difference to what the answer should be. One rule, one
// place, and nothing in the world shader has to know it exists.
//
// **Flat stays flat**, which is the whole of "still pixelated": inside a block
// of one colour all four taps agree and the average is that colour. Only an
// edge — where they disagree — gets an intermediate pixel, and it gets exactly
// one, because the kernel is one pixel across.

@group(0) @binding(0) var world: texture_2d<f32>;

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

    // This pixel and the two beside it, and the corner between them. Clamped,
    // so the edge of the screen averages with itself rather than reading off
    // the end and coming back black.
    //
    // Up and right specifically: the sample the world shader took sits at a
    // corner of the pixel rather than its middle, so the four that surround
    // that corner are these. Which corner it is decides which two neighbours,
    // and nothing else about this changes.
    let right = vec2<i32>(min(at.x + 1, last.x), at.y);
    let up = vec2<i32>(at.x, max(at.y - 1, 0));
    let both = vec2<i32>(right.x, up.y);

    let sum = textureLoad(world, at, 0)
        + textureLoad(world, right, 0)
        + textureLoad(world, up, 0)
        + textureLoad(world, both, 0);
    return sum * 0.25;
}
