// One instanced quad per visible chunk. Cells come from an R16Uint array
// texture read with textureLoad, so scaling is nearest-neighbour by
// construction and there is no sampler to configure.

// MUST MATCH `sim::cell::bits`. The shader cannot read Rust constants, so this
// block is the one thing kept in step by hand; changing the split means
// changing both.
//
//  15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
// |   player    |F |G |       kind        | A |
const ALIVE_BIT:    u32 = 1u;
const KIND_SHIFT:   u32 = 1u;   const KIND_MASK: u32 = 255u;
const FLAG_ICE:   u32 = 512u; // 1 << 9
const PLAYER_SHIFT: u32 = 11u;  // top field, so no mask is needed

// See render::atlas. One layer per cell state; a cell's own UV picks the tile
// within that layer's sheet.
const TILE_N: u32 = 16u;         // texels per tile, and cells per chunk
const SHEET_TILES: f32 = 16.0;   // tiles across a sheet
const STATES: u32 = 4u;          // dead, alive, dead+ice, alive+ice
const KIND_BACKDROP: u32 = 1u;   // a quad standing in for every unloaded chunk

struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the top-left pixel
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    chunk_n:  f32,         // cells per chunk edge
    _pad:     vec2<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks: texture_2d_array<u32>;   // rg = cell bits, ba = uv
@group(0) @binding(2) var sprites: texture_2d_array<f32>;
@group(0) @binding(3) var sprite_sampler: sampler;

// --- colour -----------------------------------------------------------------
//
// The atlas carries no hue: R is saturation, G lightness, A coverage. Hue comes
// from the cell's player, so one sheet serves every player and two players'
// cells are the same shape in different colours.
//
// OKLab rather than HSV, because HSV's hues are not evenly spaced perceptually:
// its yellows and cyans read far brighter than its blues at equal "value", so
// players would not look equally prominent. Output is linear, which is what the
// sRGB surface format expects to convert itself.

const TAU: f32 = 6.283185307;
// Golden ratio: consecutive player numbers land far apart on the hue circle,
// so neighbouring players never share a colour.
const HUE_STEP: f32 = 0.6180339887;

fn oklab_to_linear_srgb(lab: vec3<f32>) -> vec3<f32> {
    let l_ = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m_ = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s_ = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return vec3<f32>(
         4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    );
}

fn in_gamut(rgb: vec3<f32>) -> bool {
    return all(rgb >= vec3<f32>(-0.0005)) && all(rgb <= vec3<f32>(1.0005));
}

/// Lightness, saturation and a hue angle to linear RGB.
///
/// Asking for more chroma than sRGB can show is the normal case, not an edge
/// one: at this chroma most hues leave the gamut at some lightness. Clamping
/// the result would fix the range but bend the hue -- red clips before blue,
/// so two players drift towards each other. Instead the chroma is bisected
/// down until it fits, which keeps hue and lightness exactly and gives up only
/// the saturation that could not be shown. That is what OKHSL does, and the
/// part worth having here.
fn shade(lightness: f32, saturation: f32, hue: f32) -> vec3<f32> {
    let dir = vec2<f32>(cos(hue), sin(hue));
    // Taper towards black and white, where no hue has any chroma to spare.
    let chroma = 0.30 * saturation * (1.0 - abs(2.0 * lightness - 1.0));

    var rgb = oklab_to_linear_srgb(vec3<f32>(lightness, chroma * dir.x, chroma * dir.y));
    if in_gamut(rgb) {
        return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    var lo = 0.0;
    var hi = 1.0;
    for (var i = 0; i < 8; i = i + 1) {
        let mid = (lo + hi) * 0.5;
        let c = chroma * mid;
        if in_gamut(oklab_to_linear_srgb(vec3<f32>(lightness, c * dir.x, c * dir.y))) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let c = chroma * lo;
    rgb = oklab_to_linear_srgb(vec3<f32>(lightness, c * dir.x, c * dir.y));
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn player_hue(player: u32) -> f32 {
    return fract(f32(player) * HUE_STEP) * TAU;
}

/// Saturation is a second axis for telling players apart, because five bits of
/// player leaves 31 of them and hue alone gets crowded: the closest pair ends
/// up 0.026 apart in OKLab, which is not much.
///
/// Two tiers, alternating, so neighbouring player numbers differ in saturation
/// as well as hue. Measured over 31 players that lifts the closest pair to
/// 0.037, and over the first eight -- the case that actually happens -- to
/// 0.119. Spreading saturation smoothly instead is worse than doing nothing,
/// since lowering it shrinks the chroma radius and pulls colours together.
fn player_saturation(player: u32) -> f32 {
    if (player & 1u) == 1u {
        return 1.0;
    }
    return 0.55;
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    /// Position within the chunk in **texels**, 0..256 -- a u8 on each axis.
    /// A chunk is 16 cells of 16 texels, so the cell is `local / 16` and the
    /// position inside it is `local % 16`. Held in texels rather than cells
    /// because texels are what the tiles are addressed in.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    /// Position in the world, in cells. The backdrop spans thousands of chunks,
    /// so it works from this rather than from a texel offset that would run out
    /// of precision.
    @location(2) world: vec2<f32>,
    @location(3) @interpolate(flat) kind: u32,
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
    // rect.zw is the quad's size in cells; times TILE_N gives texels.
    out.local = corner * rect.zw * f32(TILE_N);
    out.world = world;
    out.kind = attrs.y;
    out.layer = attrs.x;
    return out;
}

/// The faint ring drawn on a chunk's outer cells, so chunk boundaries stay
/// visible and loading is something you can watch.
fn grid_tint(cell_in_chunk: vec2<f32>, n: f32) -> vec3<f32> {
    if cell_in_chunk.x < 1.0 || cell_in_chunk.y < 1.0
        || cell_in_chunk.x >= n - 1.0 || cell_in_chunk.y >= n - 1.0 {
        return vec3<f32>(0.012, 0.012, 0.02);
    }
    return vec3<f32>(0.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = cam.chunk_n;

    // Unloaded ground: every chunk of it looks the same, so it is one quad and
    // the pattern comes from the world position rather than from a texture.
    if in.kind == KIND_BACKDROP {
        let cell = floor(in.world);
        let within_chunk = cell - floor(cell / n) * n;
        return vec4<f32>(grid_tint(within_chunk, n), 1.0);
    }

    // local is in texels across the chunk; the cell is that divided by a
    // tile's width, and where we are inside the cell is the remainder.
    let cell_coord = vec2<i32>(floor(in.local / f32(TILE_N)));
    let within = in.local % f32(TILE_N);

    // rg holds the cell's sixteen bits, ba the tile it draws.
    let texel = textureLoad(chunks, cell_coord, i32(in.layer), 0);
    let cell = texel.r | (texel.g << 8u);
    let uv = vec2<f32>(f32(texel.b), f32(texel.a));

    let alive = (cell & ALIVE_BIT) != 0u;
    let ice = (cell & FLAG_ICE) != 0u;

    // Every combination of alive and ice has its own picture, so this is one
    // sample with no branch. That matters beyond tidiness: sampling inside a
    // conditional is sampling in non-uniform control flow, which WGSL forbids
    // for anything using implicit derivatives.
    let kind = (cell >> KIND_SHIFT) & KIND_MASK;
    let state = u32(alive) + u32(ice) * 2u;
    let layer = i32(kind * STATES + state);

    // Tile within the sheet, then the texel within that tile.
    let sheet_uv = (uv + within / f32(TILE_N)) / SHEET_TILES;
    let sprite = textureSample(sprites, sprite_sampler, sheet_uv, layer);

    var colour = grid_tint(vec2<f32>(cell_coord), n);

    let player = cell >> PLAYER_SHIFT;
    colour = mix(
        colour,
        shade(sprite.g, sprite.r * player_saturation(player), player_hue(player)),
        sprite.a,
    );

    // Composited against the background rather than alpha-blended, so the
    // pipeline needs no blend state and draw order stays irrelevant.
    return vec4<f32>(colour, 1.0);
}
