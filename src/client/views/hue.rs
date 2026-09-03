//! What colour a player's cells are, and what that colour is in sRGB.
//!
//! Both halves here because they are one decision: the hue table the shader is
//! handed and the swatch beside a name have to agree, and two derivations of
//! one number is two chances for the lobby and the board to disagree about who
//! is who. The conversion used to live in the HUD, which made every other
//! screen that wanted a swatch depend on the HUD to get one.
//!
//! A hue per player, worked out **once on the client** and used in two places
//! that must not disagree: the shader, which is handed the whole table in the
//! camera uniform, and the interface, which reads the same table for a swatch
//! beside a name. Two derivations of one number is two chances for the lobby
//! and the board to say different things about who is who.
//!
//! Hues are stepped by the golden ratio, which is the usual answer to "spread
//! N things around a circle without knowing N": every prefix of the sequence is
//! about as evenly spread as it can be, so the first three players are far
//! apart and so are the first twelve.
//!
//! ## Teams need nothing here
//!
//! **Allies have to read as allies across a whole screen of cells**, at a zoom
//! where a cell is a few pixels and nobody is comparing two of them side by
//! side. That used to be most of this file: a team took a *family* of hue, its
//! members were spread over a narrow arc around the family's middle, and the
//! width of the arc was a judgement about which mistake costs more — mistaking
//! a teammate for a teammate, or an enemy for an ally.
//!
//! None of it is needed now, because a team is a player: everybody on it
//! places cells carrying one number, so they are one colour by construction
//! rather than by arrangement. There is no arc to size, no family to keep clear
//! of its neighbour, and no way for two allies to be drawn differently.
//!
//! ## What is not here
//!
//! Saturation. A player's tier alternates by number and stays that way, so two
//! neighbouring numbers differ a little in strength as well as in hue — see
//! `player_saturation` in `grid.wgsl`, which this deliberately does not touch.

use crate::sim::PlayerId;

/// A colour, as the interface wants one: eight bits a channel, sRGB.
pub type Rgb = (u8, u8, u8);

/// The step between hues, as a turn.
///
/// The golden ratio, so every prefix of the sequence is about as evenly spread
/// around the circle as a prefix can be.
pub const STEP: f32 = 0.618_034;

/// Every player's hue, as a turn in `0..1`, indexed by [`PlayerId`].
///
/// Still a table rather than a function, because the shader is handed the whole
/// thing in one uniform. It used to have to be one: a member's place within its
/// team's family depended on who else was on that team, so it could not be
/// answered a player at a time. It can now, and this is that answer written
/// out.
pub fn table() -> &'static [f32; PlayerId::COUNT] {
    /// **Worked out once.** It is a pure function of nothing, and it was being
    /// recomputed three times a frame — once for the camera uniform and twice
    /// for the lobby, which draws it in two places. `fract` is not const, so a
    /// lock rather than a `const`.
    static HUES: std::sync::LazyLock<[f32; PlayerId::COUNT]> = std::sync::LazyLock::new(|| {
        let mut hues = [0.0; PlayerId::COUNT];
        for (i, hue) in hues.iter_mut().enumerate().skip(1) {
            *hue = (i as f32 * STEP).fract();
        }
        hues
    });
    &HUES
}

/// What the shader draws a sheet texel as, for this player.
///
/// The sheet carries no hue: a texel is saturation and lightness, and the hue
/// comes from the player's number. OKLab with the chroma bisected down until it
/// fits sRGB, which keeps hue and lightness exactly rather than bending them
/// the way clamping would. Mirrors `shade` and `player_hue` in `grid.wgsl`,
/// which is the one that has to be right — this only has to agree with it.
pub fn shade(lightness: f32, saturation: f32, player: PlayerId) -> (u8, u8, u8) {
    shade_at(
        lightness,
        saturation,
        player,
        (player.0 as f32 * crate::client::views::hue::STEP).fract(),
    )
}

/// The same, at a hue somebody else worked out — which is how a team's colour
/// reaches a swatch. See [`crate::client::views::hue`], which is the one place
/// a hue is decided and is handed to the shader as a whole table.
pub fn shade_at(lightness: f32, saturation: f32, player: PlayerId, turn: f32) -> (u8, u8, u8) {
    const TAU: f32 = std::f32::consts::TAU;
    const MAX_CHROMA: f32 = 0.13;

    let hue = turn * TAU;
    // Player zero is nobody, and nobody's ground is grey.
    let tier = if player.0 == 0 {
        0.0
    } else if player.0 % 2 == 1 {
        1.0
    } else {
        0.55
    };
    // Chroma tapers off at the ends, where there is no room for it.
    let taper = 1.0 - (2.0 * lightness - 1.0).abs().powi(2);
    let chroma = MAX_CHROMA * saturation * tier * taper;
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());

    let l_ = lightness + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = lightness - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = lightness - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let linear = [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ];
    let byte = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    (byte(linear[0]), byte(linear[1]), byte(linear[2]))
}

/// The colour of a player's cells, for a swatch beside their name.
pub fn player_colour(player: PlayerId) -> (u8, u8, u8) {
    shade(0.62, 1.0, player)
}

/// The same, for a player whose team decides their hue.
pub fn team_colour(player: PlayerId, hues: &[f32; PlayerId::COUNT]) -> (u8, u8, u8) {
    shade_at(0.62, 1.0, player, hues[player.0 as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every number gets its own hue, and no two are close enough to be
    /// mistaken for each other on a screen of cells.
    #[test]
    fn no_two_players_share_a_colour() {
        let hues = table();
        for a in 1..PlayerId::COUNT {
            for b in a + 1..PlayerId::COUNT {
                let apart = (hues[a] - hues[b]).abs();
                // Round the circle, so 0.99 and 0.01 are close.
                let apart = apart.min(1.0 - apart);
                assert!(apart > 0.02, "{a} and {b} are {apart} apart");
            }
        }
    }

    /// Nought is nobody, and unowned cells are not drawn in anybody's colour.
    #[test]
    fn nobody_has_no_hue() {
        assert_eq!(table()[0], 0.0);
    }

    /// The table is the same every time it is built. Two clients drawing one
    /// world have to agree, and neither asks the other.
    #[test]
    fn the_table_is_the_same_on_every_client() {
        assert_eq!(table(), table());
    }
}
