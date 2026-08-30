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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    Never,
    Timer,
    Territory,
}

/// Whether a match is played in sides. Only a match can be: a team is a way of
/// deciding a result, and a world has none.
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

/// A world being described, before it exists.
///
/// Everything here is what was **typed**, including the numbers: a size and a
/// target are held as text so that a field half-way through being corrected is
/// a field with something wrong in it rather than one that snaps back to a
/// number every keystroke. [`Self::parse`] is where typed becomes chosen.
pub struct Draft {
    pub name: String,
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
            shape: Shape::Boundless,
            rows: rows.to_string(),
            cols: cols.to_string(),
            ends: Ends::Never,
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
    pub fn parse(&self) -> Result<(RoomName, WorldKind, Option<Victory>, Option<u8>), String> {
        // A private room's name is the code the server generates, so there is
        // nothing here to check and nothing to refuse.
        let name = if self.private { String::new() } else { crate::net::room_name(&self.name)? };
        let shape = match self.shape {
            Shape::Boundless => WorldKind::Infinite,
            Shape::Wrapping => WorldKind::Toroidal {
                rows: chunks(&self.rows, words::make::ROWS)?,
                cols: chunks(&self.cols, words::make::COLS)?,
            },
        };
        let victory = match self.ends {
            Ends::Never => None,
            Ends::Timer => Some(Victory::Timer { generations: self.number()? }),
            Ends::Territory => Some(Victory::Territory { squares: self.number()? as usize }),
        };
        // Teams only on a match, and only when asked for. A world with teams
        // is a world with a field nobody could ever read.
        let teams = match (victory, self.together) {
            (Some(_), Together::Teams) => Some(self.sides()?),
            _ => None,
        };
        Ok((name, shape, victory, teams))
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
            Ends::Never => return,
            Ends::Timer => crate::net::DEFAULT_TIMER.to_string(),
            Ends::Territory => crate::net::DEFAULT_TERRITORY.to_string(),
        };
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
