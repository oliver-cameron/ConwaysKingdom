//! What somebody is describing when they make a world.
//!
//! Split out of the menu because it is the one thing on that screen that is
//! *state a person is building* rather than a picture of what a server said.
//! Everything else there is read: a room list arrives, a record is loaded, a
//! name is remembered. This is typed, and it survives being drawn.
//!
//! [`Draft::parse`] is where it stops being text and becomes a choice, which
//! is the menu's whole job — the client above it is handed a `WorldKind` and a
//! `Victory` and never sees a string somebody typed.

use super::words;
use crate::net::{RoomName, Victory};
use crate::sim::WorldKind;

/// **What kind of room this describes**, which is the first question because
/// it decides which of the others are worth asking.
///
/// It used to be implied: `Ends::Never` meant a world and anything else meant
/// a match, and an experiment was not a room at all but a mode the client went
/// into with no server. Three things a person picks between, asked as one
/// question — and the two that used to be `Never` are now told apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Steps forever, anybody may join, nobody wins.
    World,
    /// Won somehow, and gathers before it starts.
    Match,
    /// A laboratory: the clock is a control and the game's two placing rules
    /// can be taken off. Multiplayer like any other room — a shared bench,
    /// not a solitary one.
    Experiment,
}

/// How a match is won. Only asked when [`Kind::Match`] is; there is no
/// `Never` any more, because "does not end" is what the other two kinds are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    Timer,
    Territory,
}

/// Whether it is played in teams. A world may be as much as a match: a team is
/// people playing as one player, which is worth having without a result to
/// win. What a match adds is that the teams have to be even at the whistle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Together {
    Solo,
    Teams,
}

/// Whether the ground stops. Two answers, so a row of buttons rather than a
/// list to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Boundless,
    Wrapping,
}

/// A room as it was described, once what was typed has been checked.
///
/// Named fields rather than the tuple of five this used to be: four of those
/// are the same two shapes in a row, and two that differ only in order are the
/// ones that get swapped without anything noticing — the same argument as
/// [`super::super::Shown`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Described {
    pub name: RoomName,
    pub shape: WorldKind,
    pub victory: Option<Victory>,
    /// `None` is a free-for-all; `Some(n)` is n sides.
    pub teams: Option<u8>,
    /// Make it a laboratory. Never true beside a `victory`: a match with the
    /// rules off is not a match.
    pub laboratory: bool,
}

/// A world being described, before it exists.
///
/// Everything here is what was **typed**, including the numbers: a size and a
/// target are held as text so that a field half-way through being corrected is
/// a field with something wrong in it rather than one that snaps back to a
/// number every keystroke. [`Self::parse`] is where typed becomes chosen.
pub struct Draft {
    pub name: String,
    /// A world, a match or an experiment. Everything below is read only where
    /// it applies to the one chosen.
    pub kind: Kind,
    pub shape: Shape,
    /// How many chunks tall and wide, read only when the shape is wrapping.
    ///
    /// Two fields rather than one `ROWSxCOLS` string, because a size is two
    /// numbers and typing it as one was asking the player to learn a format
    /// in order to answer a question they already understood. It also puts the
    /// error where it belongs: a rows field that will not parse is a wrong
    /// number in a labelled box, not a whole size that "is not a size".
    pub rows: String,
    pub cols: String,
    /// How a match is won. Read only when [`Self::kind`] is a match.
    pub ends: Ends,
    /// Generations or squares, read only when it ends.
    pub target: String,
    /// Why the last attempt was refused — by this form, or by the server.
    pub note: Option<String>,
    /// Sent, and waiting for an answer. The form stays on screen while it is
    /// true, because a refusal has to arrive back into something.
    pub asking: bool,
    /// Free-for-all, or sides. Read only when the match ends somehow, because
    /// a world has no result for a side to win.
    pub together: Together,
    /// How many sides, as typed. Read only in a team match.
    pub team_count: String,
    /// Kept out of the listing and reached by a code the server generates.
    ///
    /// The name field is ignored when this is set, and the form says so — a
    /// field that is being quietly discarded is worse than one that is not
    /// there.
    pub private: bool,
}

impl Default for Draft {
    fn default() -> Self {
        let (rows, cols) = crate::sim::DEFAULT_TORUS;
        Self {
            name: String::new(),
            kind: Kind::World,
            shape: Shape::Boundless,
            rows: rows.to_string(),
            cols: cols.to_string(),
            ends: Ends::Timer,
            target: crate::net::DEFAULT_TIMER.to_string(),
            together: Together::Solo,
            team_count: crate::net::MIN_TEAMS.to_string(),
            note: None,
            asking: false,
            private: false,
        }
    }
}

impl Draft {
    /// What was typed, as what was chosen — or the first thing wrong with it.
    ///
    /// Checked here as well as on the server, and that is not duplication for
    /// its own sake: `net::room_name` exists to be callable from both sides so
    /// that a bad name is a message beside the field rather than a round trip
    /// that comes back refused. The server checks anyway, because nothing a
    /// client says about a filename is trusted.
    ///
    /// A field that does not apply is not read. A size typed while boundless
    /// is selected, or a target typed and then switched to "never", is
    /// somebody changing their mind — refusing on it would be refusing a
    /// number nobody is asking to use.
    pub fn parse(&self) -> Result<Described, String> {
        // A private room's name is the code the server generates, so there is
        // nothing here to check and nothing to refuse.
        let name = if self.private { String::new() } else { crate::net::room_name(&self.name)? };
        let (shape, victory) = self.world()?;
        // Only when asked for; a world may have them as much as a match can.
        let teams = match self.together {
            Together::Teams => Some(self.sides()?),
            Together::Solo => None,
        };
        Ok(Described { name, shape, victory, teams, laboratory: self.kind == Kind::Experiment })
    }

    /// The **world** this describes, without the room around it.
    ///
    /// A name, a listing and sides are what a *server* adds — they are how
    /// other people find the room and who they are in it — and a world nobody
    /// else can reach has none of them. The form already hides those fields
    /// when there is no server to ask; this is the half of that which was
    /// missing, and its absence was a refusal about a field that was not on
    /// screen: `room_name("")` says "a room needs a name", so pressing Play
    /// alone answered a question nobody had been asked.
    pub fn world(&self) -> Result<(WorldKind, Option<Victory>), String> {
        // **A laboratory is boundless**, which is a game answer to a game
        // question taken off: a torus is a shape a match wants so its ground
        // is finite and contested, and that means nothing to somebody watching
        // a pattern. See [planned.md](../../../../docs/planned.md#experiments).
        let shape = match (self.kind, self.shape) {
            (Kind::Experiment, _) | (_, Shape::Boundless) => WorldKind::Infinite,
            (_, Shape::Wrapping) => WorldKind::Toroidal {
                rows: chunks(&self.rows, words::make::ROWS)?,
                cols: chunks(&self.cols, words::make::COLS)?,
            },
        };
        // A way to win is the whole of what makes a room a match, so nothing
        // else has one — and a target typed and then switched away from is
        // somebody changing their mind rather than a number to refuse.
        let victory = match (self.kind, self.ends) {
            (Kind::World | Kind::Experiment, _) => None,
            (Kind::Match, Ends::Timer) => Some(Victory::Timer { generations: self.number()? }),
            (Kind::Match, Ends::Territory) => {
                Some(Victory::Territory { squares: self.number()? as usize })
            }
        };
        Ok((shape, victory))
    }

    /// How many sides, or what is wrong with the number.
    fn sides(&self) -> Result<u8, String> {
        let text = self.team_count.trim();
        match text.parse::<u8>() {
            Ok(n) if (crate::net::MIN_TEAMS..=crate::net::MAX_TEAMS).contains(&n) => Ok(n),
            Ok(_) => Err(words::make::sides_range(crate::net::MIN_TEAMS, crate::net::MAX_TEAMS)),
            Err(_) => Err(words::make::not_a_number_for(words::make::SIDES, text)),
        }
    }

    fn number(&self) -> Result<u64, String> {
        let text = self.target.trim();
        match text.parse::<u64>() {
            Ok(0) | Err(_) => Err(words::make::not_a_number(text)),
            Ok(n) => Ok(n),
        }
    }

    /// The number that belongs beside the condition now selected. Swapped when
    /// the condition is, because two thousand generations and two thousand
    /// squares are not the same order of thing and carrying one across reads
    /// as the form having kept the wrong number.
    pub(super) fn retarget(&mut self, ends: Ends) {
        if self.ends == ends {
            return;
        }
        self.ends = ends;
        self.target = match ends {
            Ends::Timer => crate::net::DEFAULT_TIMER.to_string(),
            Ends::Territory => crate::net::DEFAULT_TERRITORY.to_string(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A world with nobody in it is not asked for a room's name.**
    ///
    /// The form hides the field when there is no server, and for a while the
    /// *validation* did not follow — so pressing Play alone came back with "a
    /// room needs a name", about a box that was not on screen. `world` is the
    /// description without the room around it, and `parse` is that plus the
    /// things only a server needs.
    #[test]
    fn a_solitary_world_needs_no_name_and_a_room_does() {
        let draft = Draft::default();
        assert_eq!(draft.name, "", "the form leaves it empty when nobody can be asked");

        let (shape, victory) = draft.world().expect("a world nobody else is in was refused");
        assert_eq!(shape, WorldKind::Infinite);
        assert_eq!(victory, None);

        assert!(draft.parse().is_err(), "a room with no name should still be refused");
    }

    /// And the rest of the description is the same either way, so the two
    /// cannot come to mean different things about one form.
    #[test]
    fn both_read_the_same_description() {
        let mut draft = Draft::default();
        draft.name = "arena".into();
        draft.kind = Kind::Match;
        draft.shape = Shape::Wrapping;
        draft.rows = "8".into();
        draft.cols = "6".into();
        draft.retarget(Ends::Territory);

        let (shape, victory) = draft.world().unwrap();
        let described = draft.parse().unwrap();
        assert_eq!(described.name, "arena");
        assert_eq!(shape, WorldKind::Toroidal { rows: 8, cols: 6 });
        assert_eq!((shape, victory), (described.shape, described.victory));
    }

    /// **The three kinds, and what each one takes off the form.**
    ///
    /// A way to win is the whole of what makes a room a match, and a
    /// laboratory is boundless — so a target typed under one kind and a size
    /// typed under another are somebody changing their mind, not numbers to
    /// refuse.
    #[test]
    fn the_kind_decides_which_answers_are_read() {
        let mut draft = Draft::default();
        draft.name = "bench".into();
        draft.retarget(Ends::Timer);
        draft.shape = Shape::Wrapping;

        assert_eq!(draft.parse().unwrap().victory, None, "a world has no way to win");
        assert!(!draft.parse().unwrap().laboratory);

        draft.kind = Kind::Match;
        let described = draft.parse().unwrap();
        assert_eq!(described.victory, Some(Victory::Timer { generations: 2000 }));
        assert!(!described.laboratory, "a match is a game, so its rules are not yours");

        draft.kind = Kind::Experiment;
        let described = draft.parse().unwrap();
        assert!(described.laboratory);
        assert_eq!(described.victory, None, "and no way to win, whatever was typed");
        assert_eq!(described.shape, WorldKind::Infinite, "boundless, whatever was typed");
    }

    /// A private room is named by the server, so it is the other case where a
    /// typed name is not wanted — and it was already right.
    #[test]
    fn a_private_room_is_named_by_the_server() {
        let mut draft = Draft::default();
        draft.private = true;
        let described = draft.parse().expect("a private room was refused for its name");
        assert_eq!(described.name, "", "the code the server generates becomes the name");
    }
}

/// One side of a wrapping world, in chunks, or what is wrong with it.
///
/// Named in the error, because with two fields "that is not a number" would
/// not say which one. Bounded above as well as below: a torus is allocated
/// whole, so a thousand by a thousand is not a slow world, it is a client that
/// asks its own machine for sixteen gigabytes and stops.
/// The largest a wrapping world may be asked for, per side, in chunks.
///
/// A torus is allocated whole rather than growing into what is used, so this
/// is a real memory figure and not a preference: at sixty-four, a side is a
/// thousand cells and the world is about a megabyte of cells, which is
/// nothing. It is here to stop a typo asking for a world that will not fit,
/// not to say what makes a good arena.
pub const MAX_CHUNKS: i32 = 64;

fn chunks(text: &str, which: &str) -> Result<i32, String> {
    let text = text.trim();
    match text.parse::<i32>() {
        Ok(n) if (1..=MAX_CHUNKS).contains(&n) => Ok(n),
        Ok(_) => Err(words::make::out_of_range(which, MAX_CHUNKS)),
        Err(_) => Err(words::make::not_a_number_for(which, text)),
    }
}
