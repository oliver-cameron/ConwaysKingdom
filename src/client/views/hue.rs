//! What colour a player's cells are, and what that colour is in sRGB.
//!
//! **One table, read in two places that must not disagree**: the shader,
//! handed every column of it in the camera uniform, and the interface, which
//! reads the same row for a swatch beside a name. Two derivations of one colour
//! are two chances for the lobby and the board to say different things about
//! who is who — and they did, at two different chromas with two different
//! tapers, so the swatch beside a name was duller than the cells it stood for.
//! A team needs nothing extra, because a team *is* a player: everybody on it
//! places cells carrying one number, so they are one colour by construction.
//!
//! ## A fixed table, measured in OKLab
//!
//! It used to be a formula — hue stepped by the golden ratio, saturation and
//! lightness cycling on three and five — and a formula is only as good as its
//! worst pair. As the shader drew it, the closest two of the fifteen were 0.045
//! apart in OKLab. [`PALETTE`] is the fifteen chosen at once instead, by descent
//! on a repulsion energy in OKLab with every colour held inside sRGB and five of
//! them allowed down to a lightness of 0.45. Its closest pair is 0.169 apart,
//! which `the_closest_two_players_are_far_enough_apart` measures rather than
//! asserts. The working is in `docs/rendering.md`.
//!
//! Each row is the colour of a **full-saturation texel at [`L_SWATCH`]**, and
//! the rest of the sheet is placed around it: the texel's saturation scales the
//! row's chroma, and its lightness is remapped so that the reference lands on
//! the row's and white stays white — see [`player_lightness`]. So the art keeps
//! its shading, a swatch is the board's colour exactly, and nothing the sheet
//! can hold leaves the gamut by lightness alone.

use crate::sim::PlayerId;

/// A colour, as the interface wants one: eight bits a channel, sRGB.
pub type Rgb = (u8, u8, u8);

/// The lightness a swatch is drawn at, and the one at which a full-saturation
/// texel lands exactly on its [`PALETTE`] row.
///
/// MUST MATCH `L_SWATCH` in `grid.wgsl`.
pub const L_SWATCH: f32 = 0.62;

/// Every player's colour at [`L_SWATCH`], in OKLCH: lightness, chroma, and
/// hue as a turn in `0..1`. Indexed by [`PlayerId`].
///
/// Player zero is nobody, and nobody's ground is grey: no chroma, and the
/// swatch lightness, which leaves the sheet's own lightness alone.
///
/// The bench's palette C, decided 2026-09-05. Four decimals because three put
/// four of the swatches a byte or two off the sRGB the table was chosen as.
pub const PALETTE: [(f32, f32, f32); PlayerId::COUNT] = [
    (L_SWATCH, 0.0, 0.0),
    (0.4500, 0.1270, 249.83 / 360.0),
    (0.4500, 0.2425, 298.40 / 360.0),
    (0.6220, 0.2256, 33.69 / 360.0),
    (0.4500, 0.1093, 155.96 / 360.0),
    (0.8000, 0.1156, 32.67 / 360.0),
    (0.6224, 0.1062, 204.38 / 360.0),
    (0.6384, 0.1974, 279.39 / 360.0),
    (0.6511, 0.2812, 341.41 / 360.0),
    (0.8000, 0.1107, 242.10 / 360.0),
    (0.4500, 0.1886, 349.02 / 360.0),
    (0.8000, 0.1623, 167.85 / 360.0),
    (0.6165, 0.1279, 99.05 / 360.0),
    (0.4500, 0.1112, 55.33 / 360.0),
    (0.8000, 0.1687, 103.61 / 360.0),
    (0.8000, 0.1584, 318.20 / 360.0),
];

/// Every player's hue, as a turn in `0..1`, indexed by [`PlayerId`].
///
/// The one column of [`PALETTE`] a swatch can be handed from outside — see
/// [`team_colour`]. It used to have to be worked out, because a member's place
/// within its team's family depended on who else was on that team; it is a
/// column of a constant now.
pub fn table() -> &'static [f32; PlayerId::COUNT] {
    // `array::map` is not const, so a loop.
    const HUES: [f32; PlayerId::COUNT] = {
        let mut hues = [0.0; PlayerId::COUNT];
        let mut i = 0;
        while i < PlayerId::COUNT {
            hues[i] = PALETTE[i].2;
            i += 1;
        }
        hues
    };
    &HUES
}

/// What the shader draws a sheet texel as, for this player.
///
/// The sheet carries no hue: a texel is saturation and lightness, and the
/// colour comes from the player's row. Mirrors `shade` in `grid.wgsl`, which is
/// the one that has to be right — this only has to agree with it.
pub fn shade(lightness: f32, saturation: f32, player: PlayerId) -> Rgb {
    shade_at(lightness, saturation, player, PALETTE[player.0 as usize].2)
}

/// The sheet's lightness, placed around the player's.
///
/// **Remapped rather than scaled.** A multiplier lands [`L_SWATCH`] on the
/// row's lightness only by pushing everything above it the same way, and the
/// sheet's brightest live texel is at 0.84: the palest rows would carry it
/// past white, where no chroma survives and the bisection in [`shade_at`] has
/// nothing left to give up. So the map is linear from black to the reference
/// and linear again from the reference to white — a multiplier below, where
/// the art's shading lives and an offset would crush it, and a compression
/// above, where there is no room for one.
///
/// MUST MATCH `player_lightness` in `grid.wgsl`.
pub fn player_lightness(lightness: f32, player: PlayerId) -> f32 {
    let l_ref = PALETTE[player.0 as usize].0;
    if lightness < L_SWATCH {
        lightness * l_ref / L_SWATCH
    } else {
        l_ref + (lightness - L_SWATCH) * (1.0 - l_ref) / (1.0 - L_SWATCH)
    }
}

/// The same, at a hue somebody else worked out — which is how a team's colour
/// reaches a swatch. Lightness and chroma are still the player's own row.
///
/// OKLab with the chroma bisected down until it fits sRGB, which keeps hue and
/// lightness exactly rather than bending them the way clamping would.
pub fn shade_at(lightness: f32, saturation: f32, player: PlayerId, turn: f32) -> Rgb {
    const TAU: f32 = std::f32::consts::TAU;

    let hue = turn * TAU;
    let (dx, dy) = (hue.cos(), hue.sin());
    let lightness = player_lightness(lightness, player);
    let chroma = PALETTE[player.0 as usize].1 * saturation;

    let mut linear = oklab_to_linear([lightness, chroma * dx, chroma * dy]);
    if !in_gamut(linear) {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..8 {
            let mid = (lo + hi) * 0.5;
            let c = chroma * mid;
            if in_gamut(oklab_to_linear([lightness, c * dx, c * dy])) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let c = chroma * lo;
        linear = oklab_to_linear([lightness, c * dx, c * dy]);
    }
    let byte = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    (byte(linear[0]), byte(linear[1]), byte(linear[2]))
}

fn oklab_to_linear([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

/// The shader's tolerance, so the two agree about what fits.
fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|v| (-0.0005..=1.0005).contains(v))
}

/// The colour of a player's cells, for a swatch beside their name.
pub fn player_colour(player: PlayerId) -> Rgb {
    shade(L_SWATCH, 1.0, player)
}

/// The same, for a player whose team decides their hue.
pub fn team_colour(player: PlayerId, hues: &[f32; PlayerId::COUNT]) -> Rgb {
    shade_at(L_SWATCH, 1.0, player, hues[player.0 as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row as the point in OKLab the table was optimised in.
    fn lab(player: usize) -> [f32; 3] {
        let (l, c, h) = PALETTE[player];
        let h = h * std::f32::consts::TAU;
        [l, c * h.cos(), c * h.sin()]
    }

    /// **How far apart the closest pair is, as a number.**
    ///
    /// Distinctness is the whole job of this module and "they look different"
    /// is not a thing a test can check, so this measures it: the smallest gap
    /// between any two players' cells, in OKLab, which is the space the table
    /// was chosen in and a fair stand-in for what an eye does. A floor rather
    /// than an equality so a row can be retouched without rewriting it; what
    /// must not happen is the floor quietly dropping.
    #[test]
    fn the_closest_two_players_are_far_enough_apart() {
        let mut worst = (f32::MAX, 0, 0);
        for a in 1..PlayerId::COUNT {
            for b in a + 1..PlayerId::COUNT {
                let (p, q) = (lab(a), lab(b));
                let gap =
                    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                if gap < worst.0 {
                    worst = (gap, a, b);
                }
            }
        }
        // The table gives 0.169; the formula it replaced gave 0.045.
        assert!(
            worst.0 > 0.16,
            "players {} and {} are only {:.3} apart in OKLab",
            worst.1,
            worst.2,
            worst.0
        );
    }

    /// The lobby's swatch is the board's colour: `player_colour` lands on the
    /// sRGB each row was chosen as, which is also what `tools/typefaces.html`
    /// carries as `HUES`. Retouch a row and both lists move with it.
    ///
    /// To within a byte a channel, not exactly: three of the rows happen to
    /// fall within a hundredth of a byte of a rounding boundary, so equality
    /// would pin float noise rather than the colour.
    #[test]
    fn a_swatch_is_the_tables_colour() {
        const SRGB: [(u8, u8, u8); PlayerId::COUNT - 1] = [
            (0x00, 0x57, 0x98),
            (0x6c, 0x00, 0xc5),
            (0xf0, 0x37, 0x00),
            (0x00, 0x66, 0x3a),
            (0xff, 0xa2, 0x8e),
            (0x00, 0x99, 0xa5),
            (0x77, 0x77, 0xff),
            (0xf4, 0x00, 0xbf),
            (0x7a, 0xc6, 0xff),
            (0x99, 0x00, 0x65),
            (0x00, 0xde, 0xa9),
            (0x99, 0x85, 0x00),
            (0x82, 0x41, 0x00),
            (0xd1, 0xc1, 0x00),
            (0xe8, 0x9b, 0xff),
        ];
        let within_a_byte = |a: Rgb, b: Rgb| {
            a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1 && a.2.abs_diff(b.2) <= 1
        };
        for (i, want) in SRGB.iter().enumerate() {
            let player = PlayerId(i as u8 + 1);
            let got = player_colour(player);
            assert!(
                within_a_byte(got, *want),
                "player {} draws {got:?} for a row that is {want:?}",
                player.0
            );
            assert_eq!(got, team_colour(player, table()), "the lobby disagrees with the HUD");
        }
    }

    /// Lightness is remapped, not scaled: the swatch lightness lands on the
    /// row's, and black and white stay where they are, so no texel the sheet
    /// can hold leaves the gamut by lightness alone.
    #[test]
    fn the_sheets_range_is_placed_inside_the_gamut() {
        for n in 0..PlayerId::COUNT as u8 {
            let player = PlayerId(n);
            let l_ref = PALETTE[n as usize].0;
            assert!((player_lightness(L_SWATCH, player) - l_ref).abs() < 1e-6);
            assert_eq!(player_lightness(0.0, player), 0.0);
            assert!((player_lightness(1.0, player) - 1.0).abs() < 1e-6);
            // Monotone, so the art's shading keeps its order.
            let mut last = 0.0;
            for step in 1..=64 {
                let now = player_lightness(step as f32 / 64.0, player);
                assert!(now > last, "player {n} folds the sheet at {step}/64");
                last = now;
            }
        }
    }

    /// **A row is a placement of the sheet, not a replacement for it.** A cell
    /// dimmed past about two thirds stops reading as its own picture and
    /// starts reading as a dark patch, which is a worse problem than two
    /// players looking alike — so the darkest row is bounded below.
    #[test]
    fn no_player_is_dimmed_out_of_legibility() {
        for n in 1..PlayerId::COUNT as u8 {
            let dim = player_lightness(0.5, PlayerId(n)) / 0.5;
            assert!(dim > 0.65, "player {n} is drawn at {dim} of the sheet's lightness");
        }
    }

    /// Nought is nobody, and unowned cells are grey at the sheet's own
    /// lightness rather than drawn in anybody's colour.
    #[test]
    fn nobody_is_grey() {
        assert_eq!(table()[0], 0.0);
        assert_eq!(PALETTE[0].1, 0.0);
        for lightness in [0.16, 0.5, 0.84] {
            let (r, g, b) = shade(lightness, 1.0, PlayerId(0));
            assert!(r == g && g == b, "nobody's ground came out {:?}", (r, g, b));
            assert!((player_lightness(lightness, PlayerId(0)) - lightness).abs() < 1e-6);
        }
    }
}
