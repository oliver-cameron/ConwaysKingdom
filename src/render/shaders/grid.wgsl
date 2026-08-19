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
const FLAG_GLASS:   u32 = 512u; // 1 << 9
const PLAYER_SHIFT: u32 = 11u;  // top field, so no mask is needed

// Texels along one edge of a sprite, and the layer holding the pane. See
// render::atlas -- one sprite per layer, so a sprite index is a layer index.
const SPRITE_N: u32 = 16u;
const LAYER_GLASS: i32 = 1;

struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the top-left pixel
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    chunk_n:  f32,         // cells per chunk edge
    _pad:     vec2<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks: texture_2d_array<u32>;
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
    /// because texels are what the sprites are addressed in.
    @location(0) local: vec2<f32>,
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
    // rect.zw is the chunk's size in cells; times SPRITE_N gives texels.
    out.local = corner * rect.zw * f32(SPRITE_N);
    out.layer = attrs.x;
    return out;
}

/// Sample one sprite layer at a position within a cell, given in texels.
fn sprite_at(layer: i32, texel_in_cell: vec2<f32>) -> vec4<f32> {
    return textureSample(sprites, sprite_sampler, texel_in_cell / f32(SPRITE_N), layer);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // local is in texels across the chunk; the cell is the texel divided by a
    // sprite's width, and where we are inside that cell is the remainder.
    let cell_coord = vec2<i32>(floor(in.local / f32(SPRITE_N)));
    let within = in.local % f32(SPRITE_N);
    let cell = textureLoad(chunks, cell_coord, i32(in.layer), 0).r;

    // Faint grid on the chunk's outer ring, so chunk loading stays visible.
    let n = cam.chunk_n;
    let cell_f = vec2<f32>(cell_coord);
    let on_edge = cell_f.x < 1.0 || cell_f.y < 1.0
        || cell_f.x >= n - 1.0 || cell_f.y >= n - 1.0;
    var colour = vec3<f32>(0.0);
    if on_edge {
        colour = vec3<f32>(0.012, 0.012, 0.02);
    }

    let player = cell >> PLAYER_SHIFT;
    let saturation = player_saturation(player);
    let hue = player_hue(player);

    // The living cell, if there is one. Its kind is its sprite layer, so a
    // kind cannot name art that does not exist.
    if (cell & ALIVE_BIT) != 0u {
        let texel = sprite_at(i32((cell >> KIND_SHIFT) & KIND_MASK), within);
        if texel.a > 0.02 {
            colour = mix(colour, shade(texel.g, texel.r * saturation, hue), texel.a);
        }
    }

    // The pane over it, if there is one. Drawn after, and independently of
    // whether the cell is alive: a cell may be alive, glassed, both or neither.
    if (cell & FLAG_GLASS) != 0u {
        let pane = sprite_at(LAYER_GLASS, within);
        if pane.a > 0.02 {
            colour = mix(colour, shade(pane.g, pane.r * saturation, hue), pane.a);
        }
    }

    // Composited against the background rather than alpha-blended, so the
    // pipeline needs no blend state and draw order stays irrelevant.
    return vec4<f32>(colour, 1.0);
}
