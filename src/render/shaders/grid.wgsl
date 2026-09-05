// One instanced quad per visible chunk. Cells come from an Rgba8Uint array
// texture read with textureLoad, so scaling is nearest-neighbour by
// construction and there is no sampler to configure.

// MUST MATCH `sim::cell::bits`. The shader cannot read Rust constants, so this
// block is the one thing kept in step by hand; changing the split means
// changing both.
//
//  byte 0 (R)                byte 1 (G)
// | player |level|H|       |K2| age  |K1 0|I |A |
//  7 6 5 4  3 2 1  0        7  6 5 4  3 2  1  0
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
// the map does not need to spell out.
//
// Byte 1 is what a cell **is**: alive, iced, its kind, and how far through
// whatever that kind counts. Every field is contiguous -- see
// `sim::cell::bits`, and the block below, which is the one thing kept in step
// with it by hand.
//
// It used to be the sheet index outright, which is why the kind sat in two
// pieces around the age. `sprite_index` is what pays for putting it back
// together, and it is four operations.
const PLAYER_SHIFT: u32 = 4u;   // top of its byte, so no mask is needed
const ALIVE: u32 = 1u;          // byte 1, bit 0
const ICE: u32 = 2u;            // byte 1, bit 1
const KIND_SHIFT: u32 = 2u;     // byte 1, bits 2..5
const KIND_MASK: u32 = 7u;
const AGE_SHIFT: u32 = 5u;      // byte 1, bits 5..8

// See render::atlas. One sheet, sixteen tiles each way.
const TILE_N: u32 = 16u;         // texels per tile, and cells per chunk
const SHEET_TILES: f32 = 16.0;   // tiles across the sheet
const SHEET_W: f32 = 256.0;      // texels across the sheet, TILE_N * SHEET_TILES
const SHEET_H: f32 = 384.0;      // and down it: the grid, plus the strip of levels
const LEVELS: f32 = 5.0;         // full size and four reductions
// --- levels of detail -------------------------------------------------------
//
// **A cell is sixteen texels of art**, so at sixteen pixels a cell a texel gets
// one pixel and below that some texels get no sample at all. Nothing later can
// put back what nothing sampled, so there is art for the size it is shown at,
// all the way down to one texel a cell.
//
// The reduced levels live in a **strip under the tile grid**, not in a corner
// of it — so every tile index is still a picture of its own and no kind index
// is spent on them. The strip packs left to right by halving: level L is a grid
// `256 >> L` wide starting at `256 - 512 / 2^L`, which is exactly where the
// level above it ended. MUST MATCH `render::atlas::LEVEL_ORIGIN`.

/// Where a level's grid starts and how big its tiles are: x, y, texels.
fn level_at(level: f32) -> vec3<f32> {
    if level < 0.5 {
        return vec3<f32>(0.0, 0.0, f32(TILE_N));
    }
    let scale = pow(2.0, level);
    return vec3<f32>(SHEET_W - SHEET_W * 2.0 / scale, SHEET_W, f32(TILE_N) / scale);
}

/// Which level this zoom wants, as a fraction between two of them.
///
/// Level `L` holds `16 >> L` texels a cell and so is exact at `16 >> L` pixels
/// a cell — one texel, one pixel. So the level is the log of the shortfall, and
/// a zoom between two of them lands between two levels rather than on one.
/// **This pass's own zoom, not the screen's** — which is the whole of what
/// supersampling buys. Drawn at twice the size, a cell covers twice as many
/// samples, so the level that is exact for it is one finer than the screen
/// would ask for, and the extra detail survives into the average the resolve
/// takes. Dividing by `over` here would throw that away and leave the larger
/// target costing four times as much to draw the same picture.
fn level_for(zoom: f32) -> f32 {
    return clamp(log2(f32(TILE_N) / max(zoom, 1e-4)), 0.0, LEVELS - 1.0);
}

/// Where the art gives out entirely, in pixels per cell.
///
/// **The last boundary, and the only one left that is not a level.** Below
/// `render::chunks::COARSE_BELOW` the world is drawn from the coarse texture as
/// one flat texel a cell out of a single quad — a different *quad*, not a
/// different sample, so it cannot be blended by mixing two taps the way the
/// levels are. And that threshold is there for **residency** rather than for
/// sampling: one chunk is one texture array layer and a screen wants more of
/// them than the guaranteed 256 below about zoom five, so it is not something
/// another level of art can push further down.
///
/// It does not have to be blended. The coarse path's colour is lightness from
/// the two state bits and hue from the owner, and the fine path is already
/// holding both for the cell it is drawing — so instead of blending two
/// pictures, the fine path **fades into the answer the coarse path would give**
/// and is already drawing it when the swap happens. No second quad, no blend
/// state, no extra texture read.
///
/// **Narrow, and deliberately.** This used to run from eight, which threw away
/// the art across the whole band the reduced levels exist to serve — the levels
/// were there and were fading out under a flat wash before anybody could see
/// them. It now covers the hysteresis window and nothing more: fully flat by
/// `COARSE_BELOW`, fully art again by a little over `FINE_ABOVE`, so the
/// handover looks the same going down as coming back up and every level above
/// it is drawn at full strength.
const FLAT_FROM: f32 = 3.0;
const FLAT_BY: f32 = 1.5;

/// How much of the cell's art to give up. One is the cell without it.
///
/// **In screen pixels a cell**, not this pass's. The thresholds it has to line
/// up with — `COARSE_BELOW` and `FINE_ABOVE` — are decided on the CPU from what
/// the screen is showing, and drawing the world larger than the screen moved
/// this one and not those. The art was still fully there when the coarse path
/// took over, which is the pop these numbers exist to remove.
fn flat_fade(zoom: f32) -> f32 {
    let on_screen = zoom / max(cam.over, 1.0);
    return clamp((FLAT_FROM - on_screen) / (FLAT_FROM - FLAT_BY), 0.0, 1.0);
}

/// A cell without its art: what the coarse path draws, from a cell in hand.
///
/// The same three constants and the same sum as `coarse_colour`, off the fine
/// path's own texel instead of the coarse texture — which is what lets the two
/// meet exactly rather than within a shade of each other.
fn flat_colour(owner: u32, tile: u32) -> vec3<f32> {
    let alive = (tile & ALIVE) != 0u;
    var light = COARSE_DEAD;
    if alive {
        light = COARSE_ALIVE;
    }
    if (tile & ICE) != 0u {
        light = light + COARSE_ICE;
    }
    let player = owner >> PLAYER_SHIFT;
    return shade(
        player_lightness(light, player, alive),
        player_chroma(player),
        player_hue(player),
    );
}

/// **The tile for ground nobody holds.** MUST MATCH `sim::cell::bits::NOBODY`.
///
/// Row 1 of the dead-nothing column, which the arithmetic below can address
/// and no cell can reach: column 0 is kind 0 dead and ice-free, and kind 0
/// never ages. Not rows 8-15, which look free and are the four unused kinds.
const NOBODY_TILE: f32 = 16.0;
const KIND_BACKDROP: u32 = 1u;   // a quad standing in for every unloaded chunk
const KIND_COARSE: u32 = 2u;     // one quad standing in for the whole world

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
    wraps:    f32,         // non-zero when the coarse window *is* the world
    // The palette, a column per table: every player's colour at the swatch
    // lightness, as OKLCH -- see client::views::hue::PALETTE. Packed four to a
    // vec4 because a uniform array of scalars has a 16-byte stride in WGSL,
    // so `array<f32, 16>` would be 256 bytes to carry 64.
    hues:      array<vec4<f32>, 4>,   // as a turn
    lightness: array<vec4<f32>, 4>,
    chroma:    array<vec4<f32>, 4>,
    // The world rect the coarse texture holds -- x, y, width, height, in
    // cells -- and `encode`'s neighbour says whether it wraps.
    coarse:   vec4<f32>,
    // **Samples across one screen pixel.** `zoom` above is this pass's own,
    // already multiplied by it; dividing gets back to what the screen shows.
    // Which level of detail to read is a question about the sample rate and
    // uses `zoom`; whether the art has given out is a question about the
    // screen, because it has to meet `render::chunks::COARSE_BELOW`.
    over:     f32,
    // No pad field. WGSL rounds a uniform struct up to a multiple of sixteen
    // on its own, so `over` at 240 makes it 256 — and a `vec3<f32>` written
    // here would *align* to 16 and land at 256 itself, making the struct 272
    // against Rust's 256. `the_camera_uniform_matches_the_shader` caught that,
    // which is the whole reason it reads the struct rather than trusting it.
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(0) @binding(1) var chunks: texture_2d_array<u32>;   // r = owner, g = tile, b = sides
@group(0) @binding(2) var sprites: texture_2d<f32>;
@group(0) @binding(3) var sprite_sampler: sampler;
// One texel a cell: R the owner byte, G the tile byte. See render::chunks.
@group(0) @binding(4) var coarse: texture_2d<u32>;

// --- colour -----------------------------------------------------------------
//
// The atlas carries no hue: R is saturation, G lightness, A coverage. The
// colour comes from the cell's player -- one row of a fixed table, chosen for
// separation and measured in OKLab, see `client::views::hue::PALETTE` -- so
// one sheet serves every player and two players' cells are the same shape in
// different colours. A row is what a full-saturation texel at `L_SWATCH`
// draws as; the sheet's saturation scales the row's chroma and its lightness
// is placed around the row's by `player_lightness`.
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
// The lightness a swatch is drawn at, and the one at which a full-saturation
// texel lands exactly on its row. MUST MATCH `client::views::hue::L_SWATCH`.
const L_SWATCH: f32 = 0.62;
// The least of the sheet's lightness held ground is drawn at. MUST MATCH
// `client::views::hue::HELD_FLOOR`.
const HELD_FLOOR: f32 = 0.85;

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

/// Lightness, chroma and a hue angle to linear RGB.
///
/// Asking for more chroma than sRGB can show is the normal case, not an edge
/// one: a row's chroma fits at the swatch lightness and most hues leave the
/// gamut somewhere above or below it. Clamping the result would fix the range
/// but bend the hue -- red clips before blue, so two players drift towards
/// each other. Instead the chroma is bisected down until it fits, which keeps
/// hue and lightness exactly and gives up only the saturation that could not
/// be shown. That is what OKHSL does, and the part worth having here.
///
/// MUST MATCH `client::views::hue::shade_at`.
fn shade(lightness: f32, chroma: f32, hue: f32) -> vec3<f32> {
    let dir = vec2<f32>(cos(hue), sin(hue));

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
/// A table rather than arithmetic here because the client draws the same
/// swatch beside a name in the lobby, and two derivations of one number are two
/// chances for the board and the lobby to disagree about who is who. Teams need
/// nothing extra: a team is a player, so it is one number and one row.
///
/// Nothing else about a cell changes with the player. The sprite, its shading
/// and its coverage all come from the sheet; the player contributes a row of
/// the palette and nothing more.
fn player_hue(player: u32) -> f32 {
    return cam.hues[player / 4u][player % 4u] * TAU;
}

/// The sheet's lightness, placed around the player's.
///
/// **Remapped rather than scaled.** A multiplier lands `L_SWATCH` on the
/// row's lightness only by pushing everything above it the same way, and the
/// sheet's brightest live texel is at 0.84: the palest rows would carry it
/// past white, where no chroma survives and `shade` has nothing left to give
/// up. So the map is linear from black to the reference and linear again from
/// the reference to white -- a multiplier below, where the art's shading lives
/// and an offset would crush it, and a compression above, where there is no
/// room for one. Nobody's row sits at `L_SWATCH`, so unclaimed ground keeps
/// the sheet's own lightness.
///
/// **A live cell takes the row in full and held ground does not.** Five rows
/// sit at 0.45, which below the reference is 0.73 of the sheet: above the two
/// thirds under which a cell stops reading as its own art, and wanted on a
/// live cell, where the separation the table was chosen for is needed. A dead
/// tile is a dark texel already and reads against the grey backdrop by hue and
/// chroma rather than by shading, and at 0.73 a dark player's territory sinks
/// into it. So held ground's reference is floored at `HELD_FLOOR` of the
/// swatch lightness -- a floor of `HELD_FLOOR` on the multiplier, with the
/// row's hue and chroma untouched -- and the player's live cells still carry
/// the whole of the darkness.
///
/// MUST MATCH `client::views::hue::player_lightness`.
fn player_lightness(lightness: f32, player: u32, alive: bool) -> f32 {
    var l_ref = cam.lightness[player / 4u][player % 4u];
    if !alive {
        l_ref = max(l_ref, L_SWATCH * HELD_FLOOR);
    }
    if lightness < L_SWATCH {
        return lightness * l_ref / L_SWATCH;
    }
    return l_ref + (lightness - L_SWATCH) * (1.0 - l_ref) / (1.0 - L_SWATCH);
}

/// The chroma a full-saturation texel reaches for this player; the sheet's
/// own saturation scales it.
///
/// Nought for player zero, which is nobody. Unclaimed ground has no colour of
/// its own, so it is grey, and territory reads as colour against it -- which
/// is the whole of how a player sees what is theirs. The table's first row
/// says so; nothing here has to.
fn player_chroma(player: u32) -> f32 {
    return cam.chroma[player / 4u][player % 4u];
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

/// What a cell looks like when there is no room to draw its art.
///
/// **Lightness carries the state and hue carries the owner**, which is the
/// same division the sprite sheet makes — a texel there is saturation and
/// lightness and the hue arrives from the player — so a coarse cell is that
/// with the sheet's variation dropped rather than a different colour scheme.
///
/// Player zero is unowned, and `player_chroma` already answers nought for it,
/// so unheld ground comes out grey at the dead lightness with no arm saying
/// so. Which is what the backdrop is: **an infinite world's coarse
/// texture holds only the chunks it has**, and everywhere else reads as a dead
/// unowned cell, so unloaded ground draws as unloaded ground for free.
const COARSE_DEAD: f32 = 0.16;
/// Below this many pixels a cell, the backdrop stops drawing cells at all.
const BACKDROP_FLAT: f32 = 2.0;

/// Ground nobody holds: no hue, no life, no ring. One colour, and the one both
/// paths use, so the coarse world and the fine backdrop cannot disagree about
/// what empty looks like.
fn ground() -> vec3<f32> {
    return shade(COARSE_DEAD, 0.0, 0.0);
}
const COARSE_ALIVE: f32 = 0.72;
/// Ice lifts whatever is under it, the way a pane reads as frosted rather than
/// as a colour of its own.
const COARSE_ICE: f32 = 0.18;

fn coarse_colour(at: vec2<f32>) -> vec3<f32> {
    // Where this world position lands in the coarse window, in cells.
    var rel = at - cam.coarse.xy;
    // **Wrapped where the window is the world**, which is the whole of how a
    // torus repeats: one texture, one quad, and the same fold `World::canonical`
    // makes on the other side of the wire. Not wrapped otherwise, because a
    // window on a boundless world tiles nothing -- there is nothing to tile.
    if cam.wraps != 0.0 {
        rel = rel - floor(rel / cam.coarse.zw) * cam.coarse.zw;
    }

    var texel = vec2<u32>(0u, 0u);
    // Outside the window reads as a dead unowned cell, which is what the
    // backdrop is. `textureLoad` would answer zero out of bounds anyway; this
    // says so rather than relying on it.
    if all(rel >= vec2<f32>(0.0)) && all(rel < cam.coarse.zw) {
        texel = textureLoad(coarse, vec2<i32>(floor(rel)), 0).rg;
    }

    // The two state bits, which are where they always were and are the only
    // part of the byte a cell without its art needs.
    let tile = texel.g;
    let alive = (tile & ALIVE) != 0u;
    var light = COARSE_DEAD;
    if alive {
        light = COARSE_ALIVE;
    }
    if (tile & ICE) != 0u {
        light = light + COARSE_ICE;
    }
    let player = texel.r >> PLAYER_SHIFT;
    return shade(
        player_lightness(light, player, alive),
        player_chroma(player),
        player_hue(player),
    );
}

/// The colour of one point, in texels across a chunk.
///
/// Everything that was the body of `fs_main` and is now called `k²` times.
/// Each sample recomputes its own cell and its own tile, which is what keeps a
/// footprint that straddles two cells honest — averaging inside one tile's
/// sheet coordinates instead would blend a cell's art into its neighbour's,
/// because the sheet is an atlas and adjacent tiles are unrelated pictures.
/// It is also why the sheet cannot simply be given mipmaps.
/// Where a cell's picture is on the sheet, as a tile index.
///
/// The same four operations as `sim::cell::Cell::sprite`, and the two are kept
/// in step by hand because a shader cannot read Rust constants. The sheet's
/// own layout is what it always was: a kind's four states are four columns,
/// its eight ages are eight rows, and the kind's third bit picks which half of
/// the sheet -- kinds 0-3 in the top eight rows, 4-7 in the bottom eight.
fn sprite_index(tile: u32) -> f32 {
    let kind = (tile >> KIND_SHIFT) & KIND_MASK;
    let column = ((kind & 3u) << 2u) | (tile & (ALIVE | ICE));
    let row = ((tile >> AGE_SHIFT) & 7u) | ((kind >> 2u) << 3u);
    return f32((row << 4u) | column);
}

/// Where a tile's texels sit on the sheet, at one level of detail.
///
/// Low nibble across the grid, high nibble down it — the same sum at every
/// level, with the level's own origin and pitch. `within` is always in
/// full-size texels, so it is scaled by how much smaller this level's tile is.
fn sheet_at(tile: f32, within: vec2<f32>, level: f32) -> vec2<f32> {
    let g = level_at(level);
    let tile_xy = vec2<f32>(tile % SHEET_TILES, floor(tile / SHEET_TILES));
    let texel = g.xy + tile_xy * g.z + within * (g.z / f32(TILE_N));
    return texel / vec2<f32>(SHEET_W, SHEET_H);
}

/// One tap. **An explicit level, not an implicit one**: `textureSample` needs
/// derivatives, and derivatives may not be taken in non-uniform control flow —
/// which a branch on the quad's kind is. The sheet has one mip, so level zero
/// is what the implicit path would have chosen; the levels here are the
/// sheet's own, laid out by hand, not the sampler's.
fn tap(tile: f32, within: vec2<f32>, level: f32) -> vec4<f32> {
    return textureSampleLevel(sprites, sprite_sampler, sheet_at(tile, within, level), 0.0);
}

/// A cell's art, at whatever level of detail this zoom wants.
///
/// **Never one level, always the two either side of where the zoom falls.** A
/// level picked by a threshold is a line on the screen where the picture
/// changes; picked as a fraction between two, the change happens over a range
/// of zoom and there is no frame in which anything is disjoint.
///
/// **And the finer of the two is double-sampled while it fades out.** Blending
/// two pictures is not enough on its own: the one being faded *out* is the
/// undersampled one, so a straight mix carries its shimmer into the band at a
/// reducing weight rather than removing it. Two taps half of *its* texel apart
/// average it over the pixel's footprint first, which is what makes the
/// crossing read as one picture changing rather than two swapping. Half a texel
/// at level `L` is `2^L` in `within`, because `within` is in full-size texels
/// however small this level's are.
///
/// The offset tap is clamped inside the tile. The sheet is an atlas and the
/// tile next door is an unrelated picture, so half a texel past the edge must
/// read the edge — the same answer `texel_colour` gives at a chunk boundary and
/// for the same reason.
///
/// Landing exactly on a level costs one tap, which is the top of the zoom range
/// and the bottom of it; everywhere between costs three.
fn art(tile: f32, within: vec2<f32>) -> vec4<f32> {
    let level = level_for(cam.zoom);
    let finer = floor(level);
    let toward = level - finer;
    let here = tap(tile, within, finer);
    if toward <= 0.0 {
        return here;
    }
    let half_texel = pow(2.0, finer) * 0.5;
    let over = clamp(
        within + vec2<f32>(half_texel),
        vec2<f32>(0.0),
        vec2<f32>(f32(TILE_N) - 0.5),
    );
    let softened = 0.5 * (here + tap(tile, over, finer));
    return mix(softened, tap(tile, within, finer + 1.0), toward);
}

/// The colour of one **texel** of the board, from its position in chunk texels.
///
/// Everything is resolved from that position — which cell it falls in, that
/// cell's sprite, its owner, its outline — so a caller may ask about texels on
/// either side of a *cell* boundary and get the right answer for each. That is
/// what lets the filter below treat a cell edge and a texel edge as the same
/// thing, which they are: both are a line where one flat block of colour meets
/// another.
fn texel_colour(at: vec2<f32>, layer: u32, n: f32) -> vec3<f32> {
    // Clamped into the chunk, because a tap for a pixel on the very edge of
    // one reaches half a texel past it. A quad draws its own chunk and nothing
    // else, so the honest answer at the edge is the edge.
    let span = n * f32(TILE_N);
    let here = clamp(at, vec2<f32>(0.0), vec2<f32>(span - 0.5));
    let cell_coord = vec2<i32>(floor(here / f32(TILE_N)));
    let within = here - vec2<f32>(cell_coord) * f32(TILE_N);

    // r is the owner byte, g is what the cell is. Nothing to look up and
    // nothing to branch on: the sheet position is arithmetic on the fields.
    let texel = textureLoad(chunks, cell_coord, i32(layer), 0);
    let player = texel.r >> PLAYER_SHIFT;
    // **Ground nobody holds has a picture of its own**, rather than a player's
    // dead cell drawn grey. `player_chroma` answers nought for player zero,
    // so the two used to differ only in that the colour drained out of
    // one of them — and a field of unclaimed ground read as a grid of
    // somebody's empty squares rather than as open country.
    //
    // The one place appearance depends on the owner as well as the tile byte,
    // which is why it is here and not in `sprite_index`: this is the only
    // function holding both. See `sim::cell::bits::NOBODY`.
    let nobodys = player == 0u && (texel.g & (ALIVE | ICE)) == 0u;
    var tile = sprite_index(texel.g);
    if nobodys {
        tile = NOBODY_TILE;
    }
    let sprite = art(tile, within);

    // **Nothing behind the art but the ground.**
    //
    // A faint ring used to be drawn on every chunk's outer cells, so that
    // chunk boundaries stayed visible and loading was something you could
    // watch. That is a thing to see while building the renderer and a defect
    // once it works: a sprite has a texel of transparency on every side, so
    // whatever is behind it shows through the gap between cells — and the ring
    // made that gap a different colour on a chunk's edge than in its middle.
    // The result was a faint grid over the board at chunk pitch, visible
    // mostly on dead ground where there is least else to look at, and *only*
    // on some of it: unclaimed ground draws an edgeless tile with no gap, and
    // the coarse path has no ring at all, so the same field was ruled in some
    // places and not others.
    var colour = vec3<f32>(0.0);

    colour = mix(
        colour,
        shade(
            player_lightness(sprite.g, player, (texel.g & ALIVE) != 0u),
            sprite.r * player_chroma(player),
            player_hue(player),
        ),
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
    // **Nothing at all on ground nobody holds** — empty ground has no shape to
    // outline, and the backdrop is exactly that, so the mask is ignored there
    // rather than ringing every square of nothing.
    //
    // Said outright rather than left to the alpha. This was `sprite.a > 0.0`,
    // and it worked by accident: the dead tile's outermost texels were
    // transparent, so the outline had nothing to multiply against and vanished.
    // The moment unclaimed ground got an edgeless tile of its own, the accident
    // stopped holding and every unloaded cell grew a full border — all four
    // sides, because the unloaded layer is written as zeros and a zero mask
    // means "no side continues into a neighbour". Loaded ground was fine, since
    // `render::chunks::neighbours` computes a real mask there, which is exactly
    // the shape of the bug: a ruled grid over the ground nobody had reached and
    // none over the ground they had.
    if sprite.a > 0.0 && !nobodys {
        let sides = texel.b;
        let edge = f32(TILE_N) * EDGE;
        let open =
            (f32((sides & 1u) == 0u) * step(within.y, edge))
            + (f32((sides & 2u) == 0u) * step(f32(TILE_N) - edge, within.x))
            + (f32((sides & 4u) == 0u) * step(f32(TILE_N) - edge, within.y))
            + (f32((sides & 8u) == 0u) * step(within.x, edge));
        colour = mix(colour, colour * EDGE_SHADE, min(open, 1.0) * sprite.a);
    }
    // Into the cell without its art, so the coarse path is not a different
    // picture when it arrives — see `flat_fade`. Last, so the outline fades
    // with everything else rather than surviving into a flat field.
    return mix(colour, flat_colour(texel.r, texel.g), flat_fade(cam.zoom));
}

/// The colour of one point, in texels across a chunk.
///
/// **A point, and nothing around it.** Filtering happens once, in screen
/// space, after this pass — see `shaders/resolve.wgsl`. Three attempts at
/// doing it here all foundered on the same rock: the sheet is an atlas, so a
/// filter that stays inside a tile cannot smooth a *cell* boundary, and one
/// that crosses tiles has to resolve two cells per tap. In screen space a
/// pixel's neighbour is its neighbour and none of that exists.
fn point_colour(local: vec2<f32>, layer: u32, n: f32) -> vec3<f32> {
    return texel_colour(local, layer, n);
}

/// Where in the chunk texture a sample `offset` cells away from this fragment
/// lands, in texels.
///
/// Two routes, because the two quads address the texture differently.
///
/// Unloaded ground is ground where every cell is dead, and `chunks` holds a
/// layer of exactly that -- see ChunkStore::init_unloaded_layer. So the
/// backdrop is not a special case with a pattern of its own: it is one quad
/// spanning thousands of chunks, with the world position wrapped onto that one
/// dead chunk. Everything else then runs unchanged, which is what puts a dead
/// cell's sprite on unloaded ground.
///
/// A chunk quad **clamps** rather than wrapping. A sample that fell off the
/// edge would read zeros -- the texture returns them out of bounds -- which is
/// a dead unowned cell, so the outer ring of every chunk would darken towards
/// empty ground at low zoom and make the seam worse rather than better.
/// Clamping repeats the edge cell, which at the zooms this runs at is a
/// fraction of a pixel.
fn sample_local(in: VsOut, offset: vec2<f32>, n: f32) -> vec2<f32> {
    if in.kind == KIND_BACKDROP {
        let world = in.world + offset;
        return (world - floor(world / n) * n) * f32(TILE_N);
    }
    let span = n * f32(TILE_N);
    return clamp(in.local + offset * f32(TILE_N), vec2<f32>(0.0), vec2<f32>(span - 0.5));
}

/// One sample, from whichever texture this quad is drawn out of.
///
/// The offset is in **cells** either way, which is what lets the antialiasing
/// carry across the swap unchanged: its footprint is measured in texels and a
/// coarse texel is a cell, so below one pixel per cell it averages over cells
/// exactly as it averages over sprite texels above.
fn shaded(in: VsOut, offset: vec2<f32>, n: f32) -> vec3<f32> {
    if in.kind == KIND_COARSE {
        return coarse_colour(in.world + offset);
    }
    // **Flat grey once the backdrop's own detail is sub-pixel.** It is one
    // quad standing in for thousands of chunks, drawn by wrapping the world
    // onto a single dead chunk — so what is on it is the dead sprite, one cell
    // in sixteen of which is the transparent gap between sprites. Below a
    // couple of pixels a cell that is not anything a reader can see and it is
    // moire: a field of shimmering grid nobody asked for, which is worse than
    // the nothing it is drawing.
    //
    // The same grey the coarse path gives unheld ground, so the two agree
    // exactly where they meet rather than stepping — see [known-bugs], which
    // is about them disagreeing by a shade at low zoom.
    //
    // [known-bugs]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/known-bugs.md
    // Screen pixels again: whether the backdrop's own detail is too small to
    // see is a question about the screen, not about how many samples this pass
    // is taking of it.
    if in.kind == KIND_BACKDROP && cam.zoom / max(cam.over, 1.0) < BACKDROP_FLAT {
        return ground();
    }
    return point_colour(sample_local(in, offset, n), in.layer, n);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = cam.chunk_n;

    // **One sample, and no filtering of any kind.** This pass decides what is
    // on a pixel; deciding what a pixel should look like *given its
    // neighbours* happens once, at the end, in `shaders/resolve.wgsl`.
    //
    // There was a `k` by `k` supersample here. It is gone, and not because it
    // was wrong — because it was a *second* filter: one box filter over the
    // pixel's footprint here and another over the pixel's neighbours there is
    // a two-pixel kernel, which is a blur. Anything that averages more than
    // one reading of the world belongs in the last pass or nowhere.
    var colour = shaded(in, vec2<f32>(0.0), n);

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
