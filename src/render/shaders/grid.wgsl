// One instanced quad per visible chunk. Cells come from an R16Uint array
// texture read with textureLoad, so scaling is nearest-neighbour by
// construction and there is no sampler to configure.

// MUST MATCH `sim::cell::bits`. The shader cannot read Rust constants, so this
// block is the one thing kept in step by hand; changing the split means
// changing both.
//
//  byte 0 (R)                byte 1 (G)
// | player |level|H|       |    kind     |I |A |
//  7 6 5 4  3 2 1  0        7 6 5 4 3 2   1  0
//
// Byte 2 (B) is **not** a cell byte. It is the neighbour mask -- which of the
// four sides have a cell that would draw the same sprite -- computed on the
// way to the GPU by `render::chunks::neighbours`, because a fragment knows
// only its own array layer and a cell on a chunk edge cannot reach the layer
// its neighbour is in. It is derived and it is not in `sim::Cell`: appearance
// must not be a thing two clients can disagree about and desync over.
//
// Byte 3 (A) is spare.
//
// Only the player is read here. The level decides where a border ends up, and
// it is not drawn: ground fading with it made every claim look like a
// different kind of cell, and a strength nobody can act on separately is one
// the map does not need to spell out. Alive, ice and kind are read as one
// number -- byte 1 is the tile index into the sheet, so this shader never
// takes them apart, and that is the point of the layout.
const PLAYER_SHIFT: u32 = 4u;   // top of its byte, so no mask is needed

// See render::atlas. One sheet; a cell's tile byte is the index into it.
const TILE_N: u32 = 16u;         // texels per tile, and cells per chunk
const SHEET_TILES: f32 = 16.0;   // tiles across the sheet
const KIND_BACKDROP: u32 = 1u;   // a quad standing in for every unloaded chunk

// How much of a cell the outline takes, as a fraction of its width, and how
// far down it takes the colour there. An eighth is one texel at this tile
// size, which is what makes it read as a line drawn on the art rather than as
// a border added around it.
const EDGE: f32 = 0.0625;
const EDGE_SHADE: f32 = 0.55;

struct Camera {
    origin:   vec2<f32>,   // world position, in cells, of the top-left pixel
    viewport: vec2<f32>,   // framebuffer size in physical pixels
    zoom:     f32,         // screen pixels per cell
    chunk_n:  f32,         // cells per chunk edge
    encode:   f32,         // non-zero when this shader must encode sRGB itself
    _pad:     f32,
    // A hue per player, as a turn, worked out on the client -- see
    // client::views::hue. Packed four to a vec4 because a uniform array of
    // scalars has a 16-byte stride in WGSL, so `array<f32, 16>` would be 256
    // bytes to carry 64.
    hues:     array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks: texture_2d_array<u32>;   // r = owner, g = tile, b = sides
@group(0) @binding(2) var sprites: texture_2d<f32>;
@group(0) @binding(3) var sprite_sampler: sampler;

// --- colour -----------------------------------------------------------------
//
// The atlas carries no hue: R is saturation, G lightness, A coverage. Hue comes
// from the cell's player, so one sheet serves every player and two players'
// cells are the same shape in different colours.
//
// OKLab rather than HSV, because HSV's hues are not evenly spaced perceptually:
// its yellows and cyans read far brighter than its blues at equal "value", so
// players would not look equally prominent.
//
// Everything here works in linear light, and an sRGB surface format encodes it
// on the way out. Where no sRGB format was offered -- WebGL2, whose default
// framebuffer has no encode-on-write to give -- `cam.encode` is set and
// `fs_main` does it instead. See `linear_to_srgb` at the bottom.

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

/// A player's hue, looked up rather than worked out.
///
/// It used to be `fract(player * HUE_STEP)`, which is right for a free-for-all
/// and cannot be right for teams: allies need a **family** of hue, and where a
/// member sits in their family depends on who else is on their team — which is
/// not a thing one player's number can answer. So the client works out the
/// whole table and hands it over, and the shader does the one thing a shader
/// should do with it.
///
/// Nothing else about a cell changes with the player. The sprite, its
/// lightness and its coverage all come from the sheet; the player contributes
/// a hue and a saturation tier and nothing more.
fn player_hue(player: u32) -> f32 {
    return cam.hues[player / 4u][player % 4u] * TAU;
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
    // Player zero is nobody. Unclaimed ground has no colour of its own, so it
    // is grey, and territory reads as colour against it -- which is the whole
    // of how a player sees what is theirs. Without this, unowned cells take
    // hue zero at the muted tier and unclaimed ground is a dull red field.
    if player == 0u {
        return 0.0;
    }
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

    // Unloaded ground is ground where every cell is dead, and `chunks` holds a
    // layer of exactly that -- see ChunkStore::init_unloaded_layer. So the
    // backdrop is not a special case with a pattern of its own: it is one quad
    // spanning thousands of chunks, with the world position wrapped onto that
    // one dead chunk. Everything below then runs unchanged, which is what puts
    // a dead cell's sprite on unloaded ground. Drawing the chunk ring and
    // nothing else left it blank however the dead sprite was drawn.
    var local = in.local;
    if in.kind == KIND_BACKDROP {
        local = (in.world - floor(in.world / n) * n) * f32(TILE_N);
    }

    // local is in texels across the chunk; the cell is that divided by a
    // tile's width, and where we are inside the cell is the remainder.
    let cell_coord = vec2<i32>(floor(local / f32(TILE_N)));
    let within = local % f32(TILE_N);

    // r is the owner byte, g the tile byte -- and the tile byte *is* the index
    // into the sheet, kind and alive and ice already folded into it. Nothing
    // to look up and nothing to branch on, which is what keeps this one
    // unconditional sample: WGSL forbids implicit derivatives in non-uniform
    // control flow, so a sample inside an `if` on what a cell is would be
    // undefined on the GL path and rejected outright by Tint.
    let texel = textureLoad(chunks, cell_coord, i32(in.layer), 0);
    let tile = f32(texel.g);

    // Low nibble across the sheet, high nibble down it.
    let tile_xy = vec2<f32>(tile % SHEET_TILES, floor(tile / SHEET_TILES));
    let sheet_uv = (tile_xy + within / f32(TILE_N)) / SHEET_TILES;
    let sprite = textureSample(sprites, sprite_sampler, sheet_uv);

    var colour = grid_tint(vec2<f32>(cell_coord), n);

    let player = texel.r >> PLAYER_SHIFT;

    colour = mix(
        colour,
        shade(sprite.g, sprite.r * player_saturation(player), player_hue(player)),
        sprite.a,
    );

    // **The edge of a region, drawn from the neighbour mask.**
    //
    // What autotiling is for in a game like this is making a mass of cells
    // read as one shape, and the usual way there is a sheet of sixteen
    // variants per material with the mask choosing between them. That is a
    // question of art rather than of arithmetic: the byte holds the mask and
    // the sheet has two hundred and forty free tiles, so the day those
    // variants exist this becomes `tile + variant[mask]` and nothing else
    // changes.
    //
    // Until then the same mask draws the same information without any: a side
    // with nothing like this cell on it gets a line, and a side that continues
    // into a neighbour does not. A block of ice becomes one slab with an
    // outline instead of sixteen tiles that happen to touch, which is the
    // whole of what the mask was wanted for.
    //
    // Nothing at all on a dead, ice-free cell -- empty ground has no shape to
    // outline, and the backdrop is exactly that, so the mask is ignored there
    // rather than ringing every square of nothing.
    if sprite.a > 0.0 {
        let sides = texel.b;
        let edge = f32(TILE_N) * EDGE;
        let open =
            (f32((sides & 1u) == 0u) * step(within.y, edge))
            + (f32((sides & 2u) == 0u) * step(f32(TILE_N) - edge, within.x))
            + (f32((sides & 4u) == 0u) * step(f32(TILE_N) - edge, within.y))
            + (f32((sides & 8u) == 0u) * step(within.x, edge));
        colour = mix(colour, colour * EDGE_SHADE, min(open, 1.0) * sprite.a);
    }

    // Encoded here only when the surface will not do it. On an sRGB surface
    // this is skipped and the hardware converts; on a plain Unorm one the
    // linear numbers would otherwise reach the display as though they were
    // already encoded, which costs a mid grey more than half the light it
    // should emit and reads as a dark, muddy picture.
    if cam.encode != 0.0 {
        colour = linear_to_srgb(colour);
    }

    // Composited against the background rather than alpha-blended, so the
    // pipeline needs no blend state and draw order stays irrelevant.
    return vec4<f32>(colour, 1.0);
}

/// Linear light to sRGB: the transfer function a surface would apply itself.
///
/// The piecewise sRGB curve rather than a plain 1/2.2 gamma, because the two
/// disagree most in the darks -- which is exactly the range a mistake here is
/// most visible in -- and because it must be the inverse of what the display
/// does, not an approximation of it.
///
/// `max` guards the `pow`: a negative base is undefined in WGSL, and while
/// `shade` clips its output to the gamut, nothing in the type system says so.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
