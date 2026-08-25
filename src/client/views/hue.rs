//! What colour a player's cells are, and why it depends on their team.
//!
//! A hue per player, worked out **once on the client** and used in two places
//! that must not disagree: the shader, which is handed the whole table in the
//! camera uniform, and the interface, which reads the same table for a swatch
//! beside a name. Two derivations of one number is two chances for the lobby
//! and the board to say different things about who is who.
//!
//! ## In a free-for-all
//!
//! Hues are stepped by the golden ratio, which is the usual answer to "spread
//! N things around a circle without knowing N": every prefix of the sequence is
//! about as evenly spread as it can be, so the first three players are far
//! apart and so are the first twelve. There is nothing to decide.
//!
//! ## In a team match
//!
//! **Allies have to read as allies across a whole screen of cells**, at a zoom
//! where a cell is a few pixels and nobody is comparing two of them side by
//! side. So a team takes a *family* of hue rather than a hue: the team's own
//! golden-ratio step is the middle of the family, and its members are spread
//! over a narrow arc around it.
//!
//! The arc is [`FAMILY`] wide however many are on the team, which is the
//! decision worth stating. Widening it for a larger team would keep members
//! equally distinct from each other and let two teams bleed into one another,
//! and the thing that must survive is **which side**, not which teammate — a
//! player who has to look twice to tell their own two colours apart has lost
//! nothing, and one who mistakes an enemy for an ally has lost the game.
//!
//! Members are placed at the *ends* of the arc before the middle, so a team of
//! two takes the full width and a team of one sits exactly on its family's
//! hue. A team is nearly always two or three.
//!
//! ## What is not here
//!
//! Saturation. A player's tier alternates by number and stays that way, so two
//! neighbours on one team differ a little in strength as well as in hue — see
//! `player_saturation` in `grid.wgsl`, which this deliberately does not touch.

use crate::net::{Sides, TeamId};
use crate::sim::PlayerId;

/// The step between hues, as a turn.
///
/// The golden ratio, so every prefix of the sequence is about as evenly spread
/// around the circle as a prefix can be. Used for players in a free-for-all
/// and for teams in a team match, which are the same problem.
pub const STEP: f32 = 0.618_034;

/// How wide a team's family of hue is, as a turn.
///
/// A twelfth of the circle: wide enough that two on a side are told apart when
/// looked at, narrow enough that at eight teams — `net::MAX_TEAMS`, giving a
/// team every eighth of a circle at best — a family never reaches its
/// neighbour's middle.
pub const FAMILY: f32 = 1.0 / 12.0;

/// Every player's hue, as a turn in `0..1`, indexed by [`PlayerId`].
///
/// The whole table rather than a function of one player, because a member's
/// place within its family depends on **who else is on that team** — so it
/// cannot be answered one player at a time.
pub fn table(sides: &Sides) -> [f32; PlayerId::COUNT] {
    let mut hues = [0.0; PlayerId::COUNT];
    for i in 1..PlayerId::COUNT {
        let player = PlayerId(i as u8);
        let team = sides.team_of(player);
        hues[i] = if team.is_none() {
            // Nobody's side, which is a free-for-all or somebody who has not
            // picked: their own number's hue, as it always was.
            (player.0 as f32 * STEP).fract()
        } else {
            within(sides, team, player)
        };
    }
    hues
}

/// Where in its team's family this player sits.
fn within(sides: &Sides, team: TeamId, player: PlayerId) -> f32 {
    let middle = (team.0 as f32 * STEP).fract();
    let members = sides.members(team);
    let n = members.len();
    let at = members.iter().position(|&p| p == player).unwrap_or(0);
    if n <= 1 {
        return middle;
    }
    // Ends first: two on a side take the full width, and a lone member sits on
    // the family's own hue rather than at one edge of it.
    let offset = at as f32 / (n - 1) as f32 - 0.5;
    (middle + offset * FAMILY).rem_euclid(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sides(assignments: &[(u8, u8)]) -> Sides {
        let mut s = Sides::SOLO;
        for &(player, team) in assignments {
            s.put(PlayerId(player), TeamId(team));
        }
        s
    }

    /// How far apart two hues are on the circle, as a turn, the short way
    /// round — the wheel wraps, so 0.99 and 0.01 are close.
    fn apart(a: f32, b: f32) -> f32 {
        let d = (a - b).abs() % 1.0;
        d.min(1.0 - d)
    }

    /// With nobody on a side, nothing changes: this is what the game did
    /// before teams existed and what it still does in a free-for-all.
    #[test]
    fn a_free_for_all_is_the_hue_it_always_was() {
        let hues = table(&Sides::SOLO);
        for i in 1..PlayerId::COUNT {
            assert_eq!(hues[i], (i as f32 * STEP).fract(), "player {i} moved");
        }
    }

    /// **The load-bearing property.** Two players on one side must be closer
    /// to each other than either is to anybody on another side — at a zoom
    /// where a cell is a few pixels, that difference is the whole of what
    /// tells a friend from an enemy.
    #[test]
    fn teammates_are_closer_to_each_other_than_to_anybody_else() {
        // Three teams of two, which is the shape most likely to crowd.
        let s = sides(&[(1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (6, 3)]);
        let hues = table(&s);
        let teams = [[1usize, 2], [3, 4], [5, 6]];

        for team in teams {
            let inside = apart(hues[team[0]], hues[team[1]]);
            for other in teams {
                if other == team {
                    continue;
                }
                for &them in &other {
                    for &us in &team {
                        assert!(
                            inside < apart(hues[us], hues[them]),
                            "player {us} is no closer to their ally {} than to {them}",
                            team.iter().find(|&&p| p != us).unwrap()
                        );
                    }
                }
            }
        }
    }

    /// A family never reaches its neighbour's middle, which is what keeps the
    /// property above true at the cap rather than only at three teams.
    #[test]
    fn families_do_not_reach_each_other_at_the_most_teams_allowed() {
        let middles: Vec<f32> =
            (1..=crate::net::MAX_TEAMS).map(|t| (t as f32 * STEP).fract()).collect();
        let mut closest = 1.0f32;
        for (i, a) in middles.iter().enumerate() {
            for b in &middles[i + 1..] {
                closest = closest.min(apart(*a, *b));
            }
        }
        assert!(closest > FAMILY, "two families overlap: {closest} apart with a width of {FAMILY}");
    }

    /// A team of one sits on its family's own hue rather than at one edge of
    /// it, so a side does not change colour when its second member arrives and
    /// then again when they leave.
    #[test]
    fn a_lone_member_sits_on_their_teams_own_hue() {
        let s = sides(&[(4, 2)]);
        assert_eq!(table(&s)[4], (2.0 * STEP).fract());
    }

    /// Somebody who has not picked keeps their own number's hue, which is what
    /// makes a lobby before anybody has chosen look like a free-for-all rather
    /// than like everybody being on one side.
    #[test]
    fn a_player_on_nobodys_side_keeps_their_own_hue() {
        let s = sides(&[(1, 1), (2, 1)]);
        let hues = table(&s);
        assert_eq!(hues[5], (5.0 * STEP).fract(), "an unplaced player was recoloured");
        assert_ne!(hues[1], (1.0 * STEP).fract(), "and a placed one was not");
    }

    /// The table is handed to the shader as four `vec4`s, because a uniform
    /// array of scalars has a 16-byte stride in WGSL. The packing and the
    /// shader's indexing have to agree, and there is nothing at compile time
    /// that says they do.
    #[test]
    fn the_table_packs_four_to_a_row_the_way_the_shader_reads_it() {
        let hues = table(&Sides::SOLO);
        let packed: [[f32; 4]; 4] =
            std::array::from_fn(|row| std::array::from_fn(|col| hues[row * 4 + col]));
        // `cam.hues[player / 4u][player % 4u]`, which is the shader's line.
        for player in 0..PlayerId::COUNT {
            assert_eq!(packed[player / 4][player % 4], hues[player], "player {player}");
        }
        assert_eq!(PlayerId::COUNT, 16, "the packing is four rows of four");
    }

    /// Every hue is a turn, which is what the shader and the swatch both
    /// expect. A negative one would come out of `rem_euclid` being forgotten.
    #[test]
    fn every_hue_is_on_the_circle() {
        for s in [Sides::SOLO, sides(&[(1, 1), (2, 1), (3, 8), (4, 8)])] {
            for (i, h) in table(&s).iter().enumerate() {
                assert!((0.0..1.0).contains(h), "player {i} has hue {h}");
            }
        }
    }
}
