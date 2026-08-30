//! What colour a player's cells are.
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
pub fn table() -> [f32; PlayerId::COUNT] {
    let mut hues = [0.0; PlayerId::COUNT];
    for (i, hue) in hues.iter_mut().enumerate().skip(1) {
        *hue = (i as f32 * STEP).fract();
    }
    hues
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
