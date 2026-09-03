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
use crate::client::views::words::w;
use crate::net::Victory;
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
/// Who can reach a world once it exists.
///
/// **Three answers to one question**, where solo used to be a page. A world
/// nobody else can find is not a different kind of thing from one behind a
/// code; it is the same form with the last question answered differently, and
/// putting it here is what stops "play alone" being a control somebody has to
/// already know about to find.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// In the room list, for whoever is on this server.
    Listed,
    /// Not listed. The server gives it a code to share.
    ByCode,
    /// Nobody, and no server: the world is built here and played here.
    Solo,
}

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
    /// Whether it is played in sides. A world may have them as much as a
    /// match: a team is people playing as one player, which is worth having
    /// without a result to win. What a match adds is that the sides have to be
    /// even at the whistle.
    ///
    /// A `bool` and not an enum of two, because it is a yes and a no — and the
    /// two labels it shows live in `words`, where every other one does.
    pub teams: bool,
    /// How many sides, as typed. Read only in a team match.
    pub team_count: String,
    /// Kept out of the listing and reached by a code the server generates.
    ///
    /// The name field is ignored when this is set, and the form says so — a
    /// field that is being quietly discarded is worse than one that is not
    /// there.
    /// **Who can find it — and "nobody" is one of the answers.**
    ///
    /// Playing alone used to be a page of its own, reached from somewhere else
    /// entirely, and it asked the same questions this form asks because it is
    /// the same form. It is not a different errand; it is this one with a
    /// different answer to who else is in it.
    pub access: Access,
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
            teams: false,
            team_count: crate::net::MIN_TEAMS.to_string(),
            note: None,
            asking: false,
            access: Access::Listed,
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
    pub fn parse(&self) -> Result<super::Chose, String> {
        // A private room's name is the code the server generates, so there is
        // nothing here to check and nothing to refuse.
        let name = if self.access == Access::ByCode {
            String::new()
        } else {
            crate::net::room_name(&self.name)?
        };
        let (shape, victory) = self.world()?;
        // Only when asked for; a world may have them as much as a match can.
        let teams = if self.teams { Some(self.sides()?) } else { None };
        // **The choice itself, not a description of one.** This handed back a
        // struct of five whose only caller copied all five into a
        // `Chose::Create` — one shape for one fact, which is the convention
        // every view here already answers a frame with.
        Ok(super::Chose::Create {
            name,
            shape,
            victory,
            teams,
            private: self.access == Access::ByCode,
            laboratory: self.kind == Kind::Experiment,
        })
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
        // **A laboratory picks its own shape**, which it briefly could not:
        // this forced one boundless on the reasoning that a torus is a shape a
        // match wants. A bounded universe is an ordinary thing to want to
        // watch a pattern in — it is what every Life program offers — and
        // taking the choice away was answering a question nobody asked.
        let shape = match self.shape {
            Shape::Boundless => WorldKind::Infinite,
            Shape::Wrapping => WorldKind::Toroidal {
                rows: chunks(&self.rows, w().menu.make.rows)?,
                cols: chunks(&self.cols, w().menu.make.cols)?,
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
            Err(_) => Err(words::make::not_a_number_for(w().menu.make.sides, text)),
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

    use super::super::Chose;

    /// `parse` answers with the choice itself, so the tests take one apart
    /// here rather than at every assertion.
    fn made(draft: &Draft) -> Chose {
        draft.parse().expect("the form was refused")
    }

    /// What a described room came out as, by name, so an assertion says which
    /// answer it is checking.
    macro_rules! room {
        ($draft:expr, $field:ident) => {
            match made(&$draft) {
                Chose::Create { $field, .. } => $field,
                _ => panic!("a form describes a room"),
            }
        };
    }

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
        assert_eq!(room!(draft, name), "arena");
        assert_eq!(shape, WorldKind::Toroidal { rows: 8, cols: 6 });
        assert_eq!((shape, victory), (room!(draft, shape), room!(draft, victory)));
    }

    /// **A way to win is the whole of what makes a room a match**, so a target
    /// typed and then switched away from is somebody changing their mind
    /// rather than a number to refuse.
    ///
    /// Everything else on the form belongs to every kind. A laboratory briefly
    /// had its shape forced boundless, which was answering a question nobody
    /// asked: a bounded universe is an ordinary thing to watch a pattern in.
    #[test]
    fn the_kind_decides_which_answers_are_read() {
        let mut draft = Draft::default();
        draft.name = "bench".into();
        draft.retarget(Ends::Timer);
        draft.shape = Shape::Wrapping;

        assert_eq!(room!(draft, victory), None, "a world has no way to win");
        assert!(!room!(draft, laboratory));

        draft.kind = Kind::Match;
        assert_eq!(room!(draft, victory), Some(Victory::Timer { generations: 2000 }));
        assert!(!room!(draft, laboratory), "a match is a game, so its rules are not yours");

        draft.kind = Kind::Experiment;
        assert!(room!(draft, laboratory));
        assert_eq!(room!(draft, victory), None, "and no way to win, whatever was typed");
        assert_eq!(
            room!(draft, shape),
            WorldKind::Toroidal { rows: 12, cols: 12 },
            "a laboratory picks its own shape like anything else"
        );
    }

    /// A private room is named by the server, so it is the other case where a
    /// typed name is not wanted — and it was already right.
    #[test]
    fn a_private_room_is_named_by_the_server() {
        let mut draft = Draft::default();
        draft.access = Access::ByCode;
        assert_eq!(room!(draft, name), "", "the code the server generates becomes the name");
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
/// **Divided by the same sixteen a chunk grew by.** A form that let somebody
/// ask for a world the server refuses is a form that lies, and the server's
/// answer is `MAX_TORUS_CHUNKS`, which is a count of cells wearing a count of
/// chunks — see [`crate::sim::MAX_TORUS_SIDE`].
pub const MAX_CHUNKS: i32 = 16;

fn chunks(text: &str, which: &str) -> Result<i32, String> {
    let text = text.trim();
    match text.parse::<i32>() {
        Ok(n) if (1..=MAX_CHUNKS).contains(&n) => Ok(n),
        Ok(_) => Err(words::make::out_of_range(which, MAX_CHUNKS)),
        Err(_) => Err(words::make::not_a_number_for(which, text)),
    }
}
