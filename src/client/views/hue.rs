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
//! can hold leaves the gamut by lightness alone. Held ground is floored at
//! [`HELD_FLOOR`], so the dark rows' territory stays visible against the
//! backdrop and only their live cells carry the whole of the darkness.

use crate::sim::PlayerId;

/// A colour, as the interface wants one: eight bits a channel, sRGB.
pub type Rgb = (u8, u8, u8);

/// The lightness a swatch is drawn at, and the one at which a full-saturation
/// texel lands exactly on its [`PALETTE`] row.
///
/// MUST MATCH `L_SWATCH` in `grid.wgsl`.
pub const L_SWATCH: f32 = 0.62;

/// The least of the sheet's lightness held ground is drawn at.
///
/// MUST MATCH `HELD_FLOOR` in `grid.wgsl`.
pub const HELD_FLOOR: f32 = 0.85;

/// Every player's colour at [`L_SWATCH`], in OKLCH: lightness, chroma, and
/// hue as a turn in `0..1`. Indexed by [`PlayerId`].
///
/// Player zero is nobody, and nobody's ground is grey: no chroma, and the
/// swatch lightness, which leaves the sheet's own lightness alone.
///
/// **Fifteen hues, evenly spaced, and three lightnesses under them.**
///
/// Hue is what says *which* colour something is, so no two players are allowed
/// to share one: the hues sit twenty-four degrees apart, which is the circle
/// divided by fifteen and the most any fifteen can have. Two earlier tables
/// were chosen by pushing colours apart under a distance instead, and both
/// bought their score by putting near-hue pairs at different lightnesses —
/// two olives four degrees apart, called distinct because one was darker. On
/// a board that is one player's ground in two lights.
///
/// Lightness cycles on three against fifteen hues, so it divides exactly and
/// no neighbour on the circle shares a tier. It is there to separate colours
/// that are already a full hue apart, never to rescue two that are not.
///
/// **The order is not the hue order, and that is the point.** A room fills
/// from player one up and most games are small, so what matters is not only
/// how far apart fifteen are but how far apart the first few are. Laid out
/// around the circle, players one and two took adjacent hues — the two
/// closest of the fifteen, handed to the two people most likely to be the
/// only ones playing. So it opens on blue and yellow, a hundred and
/// sixty-eight degrees apart, and every row after is the one furthest from
/// everything already seated. Six can sit down before any two are inside
/// forty-eight degrees, against a floor of twenty-four for all fifteen.
///
/// Chroma is as much as the gamut allows up to a cap, which is why the pale
/// rows carry less of it: at this lightness there is less to be had. Four
/// decimals because three put some swatches a byte or two off.
pub const PALETTE: [(f32, f32, f32); PlayerId::COUNT] = [
    (L_SWATCH, 0.0, 0.0),
    (0.6800, 0.1500, 263.50 / 360.0),
    (0.8200, 0.1500, 95.50 / 360.0),
    (0.5500, 0.1500, 143.50 / 360.0),
    (0.5500, 0.1500, 359.50 / 360.0),
    (0.8200, 0.1244, 311.50 / 360.0),
    (0.6800, 0.1500, 47.50 / 360.0),
    (0.8200, 0.1500, 167.50 / 360.0),
    (0.6800, 0.1169, 191.50 / 360.0),
    (0.5500, 0.1182, 71.50 / 360.0),
    (0.5500, 0.1500, 287.50 / 360.0),
    (0.6800, 0.1500, 335.50 / 360.0),
    (0.6800, 0.1500, 119.50 / 360.0),
    (0.5500, 0.0975, 215.50 / 360.0),
    (0.8200, 0.1012, 239.50 / 360.0),
    (0.8200, 0.1008, 23.50 / 360.0),
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
/// colour comes from the player's row. `alive` is the tile's own bit, which
/// [`player_lightness`] reads. Mirrors `shade` in `grid.wgsl`, which is the
/// one that has to be right — this only has to agree with it.
pub fn shade(lightness: f32, saturation: f32, player: PlayerId, alive: bool) -> Rgb {
    shade_at(lightness, saturation, player, alive, PALETTE[player.0 as usize].2)
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
/// **Held ground is floored and live cells are not.** Five rows sit at 0.45,
/// which below the reference is 0.73 of the sheet: legible, and wanted on a
/// live cell, where the separation the table was chosen for is needed. A dead
/// tile is dark already and is told from the backdrop by hue and chroma rather
/// than by shading, so at 0.73 a dark player's territory sinks into it. Its
/// reference is floored at [`HELD_FLOOR`] of the swatch lightness instead,
/// which floors the multiplier and leaves hue and chroma alone. The working is
/// in `docs/rendering.md`.
///
/// MUST MATCH `player_lightness` in `grid.wgsl`.
pub fn player_lightness(lightness: f32, player: PlayerId, alive: bool) -> f32 {
    let row = PALETTE[player.0 as usize].0;
    let l_ref = if alive { row } else { row.max(L_SWATCH * HELD_FLOOR) };
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
pub fn shade_at(lightness: f32, saturation: f32, player: PlayerId, alive: bool, turn: f32) -> Rgb {
    const TAU: f32 = std::f32::consts::TAU;

    let hue = turn * TAU;
    let (dx, dy) = (hue.cos(), hue.sin());
    let lightness = player_lightness(lightness, player, alive);
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

/// The colour of a player's cells, for a swatch beside their name. A live
/// cell's, which is the row itself; held ground is the same row floored.
pub fn player_colour(player: PlayerId) -> Rgb {
    shade(L_SWATCH, 1.0, player, true)
}

/// The same, for a player whose team decides their hue.
pub fn team_colour(player: PlayerId, hues: &[f32; PlayerId::COUNT]) -> Rgb {
    shade_at(L_SWATCH, 1.0, player, true, hues[player.0 as usize])
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

    /// **No two players are near each other in hue**, which is the one thing
    /// the table guarantees by construction rather than by arithmetic.
    ///
    /// Hue says which colour something is; lightness and chroma say which of
    /// it. Two tables before this were chosen by maximising a distance, and
    /// both spent their score the same way — a near-hue pair held apart by a
    /// lightness, which on a board is one player's ground in two lights. The
    /// hues are a circle divided by fifteen now, so the floor is what fifteen
    /// can have and nothing can quietly trade it away.
    #[test]
    fn no_two_players_are_near_each_other_in_hue() {
        let mut worst = (f32::MAX, 0, 0);
        for (a, one) in PALETTE.iter().enumerate().skip(1) {
            for (b, other) in PALETTE.iter().enumerate().skip(a + 1) {
                let turn = (one.2 - other.2).abs();
                let degrees = turn.min(1.0 - turn) * 360.0;
                if degrees < worst.0 {
                    worst = (degrees, a, b);
                }
            }
        }
        assert!(
            worst.0 > 23.0,
            "players {} and {} are {:.1} degrees apart, and fifteen hues have 24",
            worst.1,
            worst.2,
            worst.0
        );
    }

    /// **A small game is better off than a full one.** A room fills from
    /// player one up, so the order the table is written in decides what two
    /// or three people get. Laid out around the circle they got adjacent
    /// hues; this pins that they do not — and that the floor rises as the
    /// room empties rather than staying at what fifteen can manage.
    #[test]
    fn the_first_few_players_are_further_apart_than_the_last() {
        let hue_gap = |a: usize, b: usize| {
            let turn = (PALETTE[a].2 - PALETTE[b].2).abs();
            turn.min(1.0 - turn) * 360.0
        };
        let worst_among = |seated: usize| {
            (1..=seated)
                .flat_map(|a| (a + 1..=seated).map(move |b| (a, b)))
                .map(|(a, b)| hue_gap(a, b))
                .fold(f32::MAX, f32::min)
        };
        assert!(worst_among(2) > 160.0, "the first two should be most of the circle apart");
        assert!(worst_among(4) > 45.0, "four seated should still be well clear");
        assert!(worst_among(6) > 45.0, "and six, which is a full lobby of friends");
        // And it never rises again as more sit down, which is what makes the
        // order meaningful rather than arbitrary.
        let mut last = f32::MAX;
        for seated in 2..PlayerId::COUNT {
            let now = worst_among(seated);
            assert!(now <= last + 1e-3, "seating {seated} spread the colours out");
            last = now;
        }
    }

    /// **And how far apart the closest pair is once everything counts.**
    ///
    /// Distinctness is the whole job of this module and "they look different"
    /// is not a thing a test can check, so this measures it in OKLab, which is
    /// the space the table is built in. A floor rather than an equality so a
    /// row can be retouched; what must not happen is the floor quietly
    /// dropping. It is lower than a table chosen to maximise it, and
    /// deliberately: what was bought with the difference is the hue floor
    /// above.
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
        // The table gives 0.134; the formula this all replaced gave 0.045.
        assert!(
            worst.0 > 0.13,
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
            (0x67, 0x95, 0xf4),
            (0xe2, 0xc3, 0x3c),
            (0x2e, 0x87, 0x2f),
            (0xb3, 0x44, 0x6f),
            (0xdf, 0xad, 0xff),
            (0xe1, 0x77, 0x3c),
            (0x41, 0xe3, 0xb0),
            (0x00, 0xaf, 0xaa),
            (0x9c, 0x64, 0x00),
            (0x6f, 0x5f, 0xc3),
            (0xd0, 0x71, 0xbc),
            (0x8e, 0xa5, 0x21),
            (0x00, 0x7f, 0x95),
            (0x85, 0xcd, 0xff),
            (0xff, 0xaa, 0xa5),
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
            assert!((player_lightness(L_SWATCH, player, true) - l_ref).abs() < 1e-6);
            for alive in [true, false] {
                assert_eq!(player_lightness(0.0, player, alive), 0.0);
                assert!((player_lightness(1.0, player, alive) - 1.0).abs() < 1e-6);
                // Monotone, so the art's shading keeps its order.
                let mut last = 0.0;
                for step in 1..=64 {
                    let now = player_lightness(step as f32 / 64.0, player, alive);
                    assert!(now > last, "player {n} folds the sheet at {step}/64");
                    last = now;
                }
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
            let dim = player_lightness(0.5, PlayerId(n), true) / 0.5;
            assert!(dim > 0.65, "player {n} is drawn at {dim} of the sheet's lightness");
        }
    }

    /// **Held ground keeps a floor that live cells do not.** A dead tile is
    /// dark already and is told from the backdrop by hue and chroma, so the
    /// dark rows' own multiplier would sink it; their live cells are where the
    /// darkness is wanted. Measured at the sheet's dead body, 60/255, which is
    /// what a held square is drawn from.
    #[test]
    fn held_ground_is_floored_and_live_cells_are_not() {
        let dead = 60.0 / 255.0;
        let darkest = (1..PlayerId::COUNT)
            .min_by(|&a, &b| PALETTE[a].0.total_cmp(&PALETTE[b].0))
            .map(|n| PlayerId(n as u8))
            .unwrap();
        let held = player_lightness(dead, darkest, false);
        assert!(
            held >= dead * HELD_FLOOR - 1e-6,
            "player {}'s held ground is at {} of the sheet's lightness",
            darkest.0,
            held / dead
        );
        // **And nobody needs it.** The darkest row sits at a multiplier above
        // the floor, so held ground clears it on its own and the floor is a
        // guard rather than a correction — which is where it should be. It
        // earned its keep against a table whose darkest rows were much lower;
        // it is kept because the table may move again.
        assert!(
            PALETTE[darkest.0 as usize].0 / L_SWATCH > HELD_FLOOR,
            "the darkest row now needs the floor, which means it is too dark"
        );
        // Everywhere below the swatch, for everybody.
        for n in 1..PlayerId::COUNT as u8 {
            for l in (1..64).map(|step| step as f32 / 64.0).filter(|l| *l < L_SWATCH) {
                let held = player_lightness(l, PlayerId(n), false);
                assert!(held >= l * HELD_FLOOR - 1e-6, "player {n} sinks held ground at {l}");
            }
        }
    }

    /// Nought is nobody, and unowned cells are grey at the sheet's own
    /// lightness rather than drawn in anybody's colour.
    #[test]
    fn nobody_is_grey() {
        assert_eq!(table()[0], 0.0);
        assert_eq!(PALETTE[0].1, 0.0);
        for lightness in [0.16, 0.5, 0.84] {
            for alive in [true, false] {
                let (r, g, b) = shade(lightness, 1.0, PlayerId(0), alive);
                assert!(r == g && g == b, "nobody's ground came out {:?}", (r, g, b));
                assert!((player_lightness(lightness, PlayerId(0), alive) - lightness).abs() < 1e-6);
            }
        }
    }
}
