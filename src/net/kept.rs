//! What a server keeps for a person that **only that person is shown**.
//!
//! Two things: the patterns they have saved, and the games they have played.
//! Both were the client's alone until now, and the reason for moving them is
//! the one the known-bugs list already gave — a library was a fact about a
//! browser rather than about a player, so a phone and a laptop were two people
//! with two sets of everything.
//!
//! ## Why it is here and not in `client`
//!
//! `client` is behind the `render` feature, so a server cannot see a line of
//! it. These are the shapes both ends have to agree on, which is what
//! [`crate::net`] is for — the same argument that puts [`crate::net::auth`]
//! here rather than in `server`.
//!
//! ## Yours, stored by somebody else
//!
//! This is the one thing on a profile that is **not** shown to anybody. The
//! rule the rest of a profile follows is that anything another player sees has
//! to be the server's, because client state is self-asserted; these go the
//! other way. Nobody else is shown them, so nobody can be misled by them, and
//! the server is a locker rather than a witness — it does not read a pattern,
//! it holds one.
//!
//! What it does do is **bound** them, because this is a client writing to a
//! server's disk: [`STAMPS_MOST`], [`GAMES_MOST`], a name clamped like any
//! other, and a pattern that has to fit the pad it is drawn on. A store with no
//! ceiling is a store one client can fill.

use serde::{Deserialize, Serialize};

use crate::sim::WorldKind;

/// How many patterns one person may keep.
///
/// Well past what anybody curates — the bar shows ten — and low enough that
/// the whole library is a few kilobytes, which is what lets it be sent whole
/// rather than as a diff. See [`Kept`].
pub const STAMPS_MOST: usize = 64;

/// How many finished games are remembered, oldest dropped first.
///
/// Fifty, which is what the client kept when it kept this alone: more than a
/// home screen ever shows, and enough that "most ground ever held" means
/// something.
pub const GAMES_MOST: usize = 50;

/// Cells a side a pattern may span.
///
/// The pad it is drawn on, which is the one bound: a pattern captured larger
/// than the pad could not be edited, and two limits that disagree are one limit
/// and a silent loss.
pub const STAMP_N: i32 = 16;

/// The longest a pattern's name may be, for the reason any other name is
/// bounded: it sits under a thumbnail the size of a key.
pub const STAMP_NAME_MAX: usize = 24;

/// How a game ended for the player whose diary this is.
///
/// Three answers and not two: most rooms have no way to end at all, so
/// `Played` is the ordinary outcome and winning is the special case. A world
/// that never ends is not a game anybody lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// A world with no way to win, or a match left before it decided.
    #[default]
    Played,
    Won,
    Lost,
}

/// One finished game, as it looked when this player left it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub room: String,
    pub world: WorldKind,
    /// Generations this player was present for, not the world's age. A world
    /// running for a week before you arrived is not a week you played.
    pub generations: u64,
    /// The most ground held at once, which is a better memory of a game than
    /// the ground held at the end: somebody who built an empire and lost it
    /// played a more interesting game than one who never held anything, and
    /// the closing figure is the same for both.
    pub best: u32,
    pub outcome: Outcome,
}

impl Game {
    /// The same game, with everything a client chose brought inside a bound.
    ///
    /// A room name is what a client says it is, and this one is going into a
    /// server's store — so it is clamped rather than trusted, the same as any
    /// other name off the wire. See [`crate::net::player_name`].
    pub fn clamped(mut self) -> Self {
        self.room = crate::net::player_name(&self.room);
        self
    }

    /// Whether this one was a match, which is the only kind that can be lost.
    pub fn is_match(&self) -> bool {
        self.outcome != Outcome::Played
    }
}

/// A pattern somebody saved.
///
/// **Cells and their positions, not a rectangle of ground.** A pattern is the
/// live cells in it; the dead ones are gaps, and a stamp that carried them
/// would wipe whatever it was placed over. What it is *made of* is chosen when
/// it is laid rather than when it is captured, so one saved glider can go down
/// as life, as factories or as ice.
///
/// Coordinates are relative to the pattern's own top-left, so a stamp knows its
/// shape and not where it was found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
// **`size` is never on the wire.** It is `cells` said a second way, and a
// second saying is a second chance to disagree — a client that sent a size its
// cells did not support would draw a preview the wrong shape. It is re-derived
// on the way in instead, which is what `trimmed` is.
#[serde(from = "BareStamp", into = "BareStamp")]
pub struct Stamp {
    pub name: String,
    /// `(row, col)` from the pattern's top-left.
    pub cells: Vec<(i32, i32)>,
    /// Rows and columns the pattern spans, for the preview and the label.
    /// Derived from `cells` and kept beside them because it is read every
    /// frame; [`Stamp::trimmed`] is the only thing that sets it.
    pub size: (i32, i32),
    /// Whether this one is on the hotbar.
    ///
    /// **Nothing pinned means the newest ten**, which is the right default:
    /// somebody who has never thought about it gets the pattern they just took,
    /// on the key beside their hand. Pin one and the bar becomes exactly what
    /// is pinned, because half a rule is worse than either.
    pub on_bar: bool,
}

/// A [`Stamp`] as it travels and as it is stored: no `size`, because that is
/// derived.
#[derive(Serialize, Deserialize)]
struct BareStamp {
    name: String,
    cells: Vec<(i32, i32)>,
    #[serde(default)]
    on_bar: bool,
}

impl From<BareStamp> for Stamp {
    fn from(bare: BareStamp) -> Self {
        let mut stamp = Stamp::trimmed(bare.cells);
        stamp.name = bare.name;
        stamp.on_bar = bare.on_bar;
        stamp
    }
}

impl From<Stamp> for BareStamp {
    fn from(stamp: Stamp) -> Self {
        Self { name: stamp.name, cells: stamp.cells, on_bar: stamp.on_bar }
    }
}

impl Stamp {
    /// A pattern from the cells it is made of, moved to its own top-left and
    /// named for its shape.
    pub fn trimmed(found: Vec<(i32, i32)>) -> Self {
        let top = found.iter().map(|&(r, _)| r).min().unwrap_or(0);
        let left = found.iter().map(|&(_, c)| c).min().unwrap_or(0);
        let bottom = found.iter().map(|&(r, _)| r).max().unwrap_or(0);
        let right = found.iter().map(|&(_, c)| c).max().unwrap_or(0);
        Self {
            name: format!("{}x{}", bottom - top + 1, right - left + 1),
            cells: found.into_iter().map(|(r, c)| (r - top, c - left)).collect(),
            size: (bottom - top + 1, right - left + 1),
            on_bar: false,
        }
    }

    /// Whether this is a pattern at all, which is the check a server makes
    /// before storing one.
    ///
    /// Empty is not a pattern, and neither is one larger than the pad it has to
    /// be editable on. Both are refusals rather than repairs: a stamp cropped
    /// to fit is a shape somebody did not draw.
    pub fn is_drawable(&self) -> bool {
        !self.cells.is_empty() && self.size.0 <= STAMP_N && self.size.1 <= STAMP_N
    }

    /// The same pattern with its name brought inside a bound, for storing.
    pub fn clamped(mut self) -> Self {
        let name: String =
            self.name.trim().chars().filter(|c| !c.is_control()).take(STAMP_NAME_MAX).collect();
        self.name = if name.is_empty() { format!("{}x{}", self.size.0, self.size.1) } else { name };
        self
    }
}

/// Everything a server holds for one person that only that person sees.
///
/// **Sent whole rather than as a change.** A library is a few kilobytes and a
/// diary is fifty short rows, so replacing the lot is one message, one meaning
/// and no merge — which is what makes "the server's copy wins" a rule with
/// nothing behind it rather than a policy with edge cases. See
/// [`crate::net::ClientMessage::Keep`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub stamps: Vec<Stamp>,
    /// Newest first, which is the order a home screen reads them in.
    pub games: Vec<Game>,
}

impl Kept {
    /// What a server will actually store, out of what a client offered.
    ///
    /// **Bounded rather than believed.** This is the one message that writes a
    /// client's own words to a server's disk, so every part of it is capped
    /// here: too many patterns, too many games, a pattern that is not one, a
    /// name of any length. What is over the cap is dropped from the end, which
    /// for games is the oldest and for patterns is the least recently kept.
    pub fn clamped(self) -> Self {
        Self {
            stamps: self
                .stamps
                .into_iter()
                .filter(Stamp::is_drawable)
                .take(STAMPS_MOST)
                .map(Stamp::clamped)
                .collect(),
            games: self.games.into_iter().take(GAMES_MOST).map(Game::clamped).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty() && self.games.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(cells: &[(i32, i32)]) -> Stamp {
        Stamp::trimmed(cells.to_vec())
    }

    fn game(room: &str) -> Game {
        Game {
            room: room.into(),
            world: WorldKind::Infinite,
            generations: 10,
            best: 5,
            outcome: Outcome::Played,
        }
    }

    /// A pattern is moved to its own corner, so two drawings of one shape in
    /// two places are one stamp.
    #[test]
    fn a_pattern_is_its_shape_and_not_where_it_was_found() {
        let here = stamp(&[(0, 0), (0, 1), (1, 0)]);
        let far = stamp(&[(40, 70), (40, 71), (41, 70)]);
        assert_eq!(here.cells, far.cells, "where it was drawn came with it");
        assert_eq!(far.size, (2, 2));
    }

    /// **`size` is derived, so a client cannot disagree with itself about it.**
    /// It is not on the wire at all: one that sent a size its cells did not
    /// support would draw a preview the wrong shape everywhere it appeared.
    #[test]
    fn a_size_off_the_wire_is_recomputed_rather_than_taken() {
        let json = r#"{"name":"liar","cells":[[0,0],[0,1]],"size":[99,99],"on_bar":true}"#;
        let back: Stamp = serde_json::from_str(json).expect("would not read");
        assert_eq!(back.size, (1, 2), "a claimed size was believed");
        assert_eq!(back.name, "liar", "and the name it chose is its own");
        assert!(back.on_bar);
        assert!(!serde_json::to_string(&back).unwrap().contains("size"), "size went on the wire");
    }

    /// Round trip, because this is what a store writes and reads.
    #[test]
    fn a_locker_survives_being_written_down() {
        let kept = Kept {
            stamps: vec![stamp(&[(0, 0), (1, 1)]), stamp(&[(0, 0)])],
            games: vec![game("main"), game("arena")],
        };
        let text = serde_json::to_string(&kept).unwrap();
        assert_eq!(serde_json::from_str::<Kept>(&text).unwrap(), kept);
    }

    /// **A client writing to a server's disk is bounded**, or one client is a
    /// way to fill it.
    #[test]
    fn what_a_client_offers_is_capped_before_it_is_kept() {
        let kept = Kept {
            stamps: (0..STAMPS_MOST * 2).map(|n| stamp(&[(0, 0), (0, n as i32 % 8)])).collect(),
            games: (0..GAMES_MOST * 2).map(|_| game("main")).collect(),
        }
        .clamped();
        assert_eq!(kept.stamps.len(), STAMPS_MOST);
        assert_eq!(kept.games.len(), GAMES_MOST);
    }

    /// A pattern that is not one is refused rather than repaired: a stamp
    /// cropped to fit is a shape nobody drew.
    #[test]
    fn a_pattern_that_will_not_fit_the_pad_is_not_kept() {
        let too_wide = stamp(&[(0, 0), (0, STAMP_N)]);
        let empty = Stamp::trimmed(Vec::new());
        let fine = stamp(&[(0, 0), (STAMP_N - 1, STAMP_N - 1)]);

        let kept =
            Kept { stamps: vec![too_wide, empty, fine.clone()], games: Vec::new() }.clamped();
        assert_eq!(kept.stamps.len(), 1, "something unusable was stored");
        assert_eq!(kept.stamps[0].cells, fine.cells);
    }

    /// Every name a client chooses is clamped, here as everywhere: a pattern's
    /// own, and the room a game was played in.
    #[test]
    fn the_names_a_client_chose_are_clamped() {
        let mut named = stamp(&[(0, 0)]);
        named.name = format!("  a\tb\n{}  ", "x".repeat(200));
        let kept = Kept { stamps: vec![named], games: vec![game("a\troom\nname")] }.clamped();

        let name = &kept.stamps[0].name;
        assert!(!name.contains(['\t', '\n']), "a name kept a control character: {name:?}");
        assert!(name.chars().count() <= STAMP_NAME_MAX);
        assert_eq!(kept.games[0].room, "aroomname");
    }

    /// An unnamed pattern is named for its shape rather than left blank, so a
    /// thumbnail always has a label under it.
    #[test]
    fn a_pattern_with_no_name_is_named_for_its_shape() {
        let mut blank = stamp(&[(0, 0), (2, 2)]);
        blank.name = "   ".into();
        assert_eq!(blank.clamped().name, "3x3");
    }
}
