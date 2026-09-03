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
//! ## Three axes, because one wraps and two were not enough
//!
//! Hue alone crowds. The golden step spreads a prefix about as well as a prefix
//! can be spread and still comes back around, so over fifteen players some pair
//! ends up nearly the same colour — and which pair is not arbitrary: the ones
//! it brings closest are a **Fibonacci** number apart.
//!
//! Saturation was the second axis and alternated, which is a period of two —
//! and eight is even, so it gave players 1 and 9 the same strength on top of
//! nearly the same hue. The second axis did nothing for exactly the pairs the
//! first one crowded.
//!
//! So: saturation cycles on **three** and lightness on **five**, which is the
//! smallest pair of periods for which every one of the fifteen live players
//! gets its own combination — their least common multiple is fifteen. No two
//! players are told apart by hue alone, and the worst pair is about thirty
//! apart in sRGB, which `the_closest_two_players_are_far_enough_apart`
//! measures rather than asserts.
//!
//! Both are **multipliers on the sheet**, not replacements for it: a texel
//! carries its own saturation and lightness, the player scales each, and the
//! art keeps its shading. Below about two thirds a cell stops reading as its
//! own picture and starts reading as a dark patch, which is why the range is
//! narrow.

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

/// A player's saturation, as a multiplier on the sheet's own.
///
/// **Three tiers, cycling on three** — and the three is the point.
///
/// It alternated, which is a period of two, and that is the one period that
/// could not help. Hues are stepped by the golden ratio, so the pairs it brings
/// closest together are the ones a **Fibonacci** number apart — and eight is
/// even, so a tier repeating every two gave players 1 and 9 the same strength
/// on top of nearly the same hue. The second axis did nothing for exactly the
/// pairs the first one crowded.
///
/// Three against lightness's five, so their combination repeats every fifteen —
/// which is every live player. Each of the fifteen gets its own pair, and no
/// two of them differ by hue alone. See [`lightness_tier`].
///
/// Player zero is nobody and nobody's ground is grey.
///
/// MUST MATCH `player_saturation` in `grid.wgsl`.
pub fn saturation_tier(player: PlayerId) -> f32 {
    if player.0 == 0 {
        return 0.0;
    }
    match player.0 % 3 {
        0 => 1.0,
        1 => 0.78,
        _ => 0.58,
    }
}

/// A player's lightness, as a multiplier on the sheet's own — **the third
/// axis**, and the one that does the most work.
///
/// Hue is stepped by the golden ratio, which spreads a prefix about as well as
/// a prefix can be spread and still wraps: by fifteen players two of them are
/// nearer each other than the eye separates at the size a cell is drawn.
/// Saturation was the second axis and repeats every two, so it does not help a
/// pair four apart.
///
/// **Five tiers, cycling on five**, against saturation's three.
///
/// Three and five are the smallest pair of periods for which *every* pair of
/// live players differs in at least one — checked by hand over all fifteen,
/// which is what makes it a choice rather than a guess. Their combination
/// repeats every fifteen, and fifteen is exactly the number of players there
/// can be, so each one gets its own pair of tiers and no two are told apart by
/// hue alone.
///
/// Multiplied into the sheet's lightness exactly the way saturation is
/// multiplied into its saturation, so the art keeps its shading and the whole
/// cell moves together rather than only its colour.
///
/// The range is narrow on purpose. Below about two thirds a cell stops reading
/// as its own art and starts reading as a dark patch, which is a worse problem
/// than two players looking alike.
///
/// MUST MATCH `player_lightness` in `grid.wgsl`.
pub fn lightness_tier(player: PlayerId) -> f32 {
    // Nobody's ground keeps the sheet's own lightness: it is grey either way,
    // and dimming it would make unclaimed ground a shade nothing else is.
    if player.0 == 0 {
        return 1.0;
    }
    match player.0 % 5 {
        0 => 1.0,
        1 => 0.91,
        2 => 0.83,
        3 => 0.75,
        _ => 0.68,
    }
}

/// The same, at a hue somebody else worked out — which is how a team's colour
/// reaches a swatch. See [`crate::client::views::hue`], which is the one place
/// a hue is decided and is handed to the shader as a whole table.
pub fn shade_at(lightness: f32, saturation: f32, player: PlayerId, turn: f32) -> (u8, u8, u8) {
    const TAU: f32 = std::f32::consts::TAU;
    const MAX_CHROMA: f32 = 0.13;

    let hue = turn * TAU;
    let tier = saturation_tier(player);
    // **And a third axis, because two were not enough.** Hue alone crowds: the
    // golden step spreads a prefix well and still comes back around, and by
    // fifteen players the closest pair is closer than the eye separates at the
    // size a cell is drawn. Saturation was the second axis and alternates, so
    // it repeats every two.
    //
    // Lightness is the third and cycles every three, so the pair repeats every
    // six rather than every two — and it is the axis the eye is *best* at, so
    // it does the most work per step of the three. See [`lightness_tier`].
    let lightness = lightness * lightness_tier(player);
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
    /// **How far apart the closest pair is, as a number.**
    ///
    /// Distinctness is the whole job of this module and "they look different"
    /// is not a thing a test can check, so this measures it: the smallest gap
    /// between any two players' cells, in sRGB, which is a coarse stand-in for
    /// a perceptual distance and is the one the screen actually shows.
    ///
    /// Hue alone crowds — the golden step spreads a prefix well and still wraps
    /// — and saturation repeating every two does nothing for a pair four apart.
    /// Lightness cycling on three is what pulls the worst pair apart, and this
    /// is what says by how much. It is a floor rather than an equality so the
    /// tiers can be tuned without rewriting it; what must not happen is the
    /// floor quietly dropping.
    #[test]
    fn the_closest_two_players_are_far_enough_apart() {
        let live: Vec<(u8, (u8, u8, u8))> =
            (1..PlayerId::COUNT as u8).map(|n| (n, shade(0.62, 1.0, PlayerId(n)))).collect();
        let apart = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
            ((d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)) as f64).sqrt()
        };
        let mut worst = (f64::MAX, 0u8, 0u8);
        for (i, (n, a)) in live.iter().enumerate() {
            for (m, b) in &live[i + 1..] {
                let gap = apart(*a, *b);
                if gap < worst.0 {
                    worst = (gap, *n, *m);
                }
            }
        }
        // Thirty is what three axes achieve over fifteen players; the floor is
        // a little under it, so the tiers can be tuned without rewriting this
        // and a change that quietly makes two players harder to tell apart
        // still fails.
        assert!(
            worst.0 > 28.0,
            "players {} and {} are only {:.1} apart in sRGB",
            worst.1,
            worst.2,
            worst.0
        );
    }

    /// **A tier is a multiplier on the sheet, not a replacement for it.** Both
    /// axes have to leave the art recognisable: a cell dimmed past about two
    /// thirds stops reading as its own picture and starts reading as a dark
    /// patch, which is a worse problem than two players looking alike.
    #[test]
    fn no_player_is_dimmed_out_of_legibility() {
        for n in 0..PlayerId::COUNT as u8 {
            let tier = lightness_tier(PlayerId(n));
            assert!(tier > 0.65, "player {n} is drawn at {tier} of the sheet's lightness");
            assert!(tier <= 1.0, "player {n} is drawn brighter than the sheet");
        }
    }

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
