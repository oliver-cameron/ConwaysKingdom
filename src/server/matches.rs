//! A match: a room with a beginning, an end and a winner.
//!
//! An ordinary room runs forever and nobody wins it. A match is the same world
//! with three things added — everybody starts together, it stops, and when it
//! stops somebody has the most ground.
//!
//! **Nothing here is in `sim`.** The simulation does not know what a match is,
//! the same way it does not know what money is: a match is an arrangement of
//! when a room steps and who may join it, and both of those are the server's
//! business. What that buys is that a match cannot introduce a rule the world
//! has to honour, and so cannot make a match world behave differently from the
//! one people practise in.

use crate::sim::PlayerId;

/// What a room is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not a match. Steps forever, anybody may join, nobody wins.
    Open,
    /// Made and waiting. Players may join and place, and **the world does not
    /// step** — so the opening is drawn rather than raced, and somebody who
    /// joined a minute earlier has not had a minute of generations the others
    /// did not.
    Gathering,
    /// Running, from the tick it started at. Nobody else may join.
    Running { from: u64 },
    /// Decided. The world has stopped and the result stands.
    Over { winner: Option<PlayerId>, held: usize, at: u64 },
}

impl Phase {
    /// Whether the world should advance.
    pub fn stepping(&self) -> bool {
        matches!(self, Self::Open | Self::Running { .. })
    }

    /// Whether somebody who is not already here may join.
    ///
    /// **No late joining.** A match is a race from a shared start, and a
    /// player arriving at generation four hundred is not in the same race:
    /// everybody else has four hundred generations of ground and they have a
    /// block. Refused rather than allowed-and-hopeless, which reads as the
    /// game being broken rather than as a rule.
    pub fn open_to_newcomers(&self) -> bool {
        matches!(self, Self::Open | Self::Gathering)
    }

    /// Whether a player may change the world.
    ///
    /// **Nothing happens before the whistle.** The same set as
    /// [`Self::stepping`] today, and a different question: a match that let
    /// people place while gathering would be fair in *generations* and unfair
    /// in **time**, since somebody who joined ten minutes early has had ten
    /// minutes to think and draw and the last to arrive has had none. Holding
    /// the tick still does not hold a clock still.
    ///
    /// So a match opens with everybody looking at the same thing, and the
    /// first thing anybody does is done against a running clock — which is a
    /// better opening than a leisurely draw, since hesitating costs
    /// generations rather than nothing.
    pub fn accepts_actions(&self) -> bool {
        matches!(self, Self::Open | Self::Running { .. })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Gathering => "gathering",
            Self::Running { .. } => "running",
            Self::Over { .. } => "over",
        }
    }
}

/// How a match is won.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Victory {
    /// Most ground when this many generations have passed.
    ///
    /// The deadline is a **tick**, not a clock. The tick is the generation and
    /// it is already what a client adopts from its `Welcome`, so a match that
    /// ends at generation N needs no clock synchronisation, cannot be
    /// lengthened by a client that pauses, and is the same instant for
    /// everybody by construction.
    Timer { generations: u64 },
    /// First to hold this many squares.
    Territory { squares: usize },
}

impl Victory {
    /// Read `timer 2000` or `territory 500`.
    pub fn parse(kind: &str, value: &str) -> Result<Self, String> {
        let n: u64 = value
            .parse()
            .map_err(|_| format!("\"{value}\" is not a number of {kind}"))?;
        if n == 0 {
            return Err(format!("a {kind} of zero is a match that is over already"));
        }
        match kind {
            "timer" | "time" | "ticks" => Ok(Self::Timer { generations: n }),
            "territory" | "ground" | "land" => Ok(Self::Territory { squares: n as usize }),
            other => Err(format!("no win condition \"{other}\"; try timer or territory")),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Timer { generations } => {
                format!("most ground after {generations} generations")
            }
            Self::Territory { squares } => format!("first to {squares} squares"),
        }
    }
}

/// Who is holding the most, and how much. `None` when nobody holds anything.
///
/// Ties go to the **lower player number**, which is arbitrary and has to be
/// something: two players on exactly the same count is a real possibility on a
/// small world, and a winner picked by iteration order would differ between
/// runs of the same match.
pub fn leader(held: &[usize; PlayerId::COUNT]) -> (Option<PlayerId>, usize) {
    let mut best = (None, 0);
    for (id, &count) in held.iter().enumerate().skip(1) {
        if count > best.1 {
            best = (Some(PlayerId(id as u8)), count);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gathering_world_does_not_step_and_a_running_one_does_not_admit() {
        assert!(!Phase::Gathering.stepping(), "nothing moves before the whistle");
        assert!(!Phase::Gathering.accepts_actions(), "and nobody does anything either");
        assert!(Phase::Gathering.open_to_newcomers());

        let running = Phase::Running { from: 0 };
        assert!(running.stepping() && running.accepts_actions());
        assert!(!running.open_to_newcomers(), "no late joining");

        let over = Phase::Over { winner: None, held: 0, at: 10 };
        assert!(!over.stepping(), "a decided match stops");
        assert!(!over.accepts_actions(), "and cannot be played on afterwards");
        assert!(!over.open_to_newcomers());

        assert!(Phase::Open.stepping() && Phase::Open.accepts_actions());
        assert!(Phase::Open.open_to_newcomers());
    }

    #[test]
    fn win_conditions_read_back_as_they_were_typed() {
        assert_eq!(Victory::parse("timer", "2000"), Ok(Victory::Timer { generations: 2000 }));
        assert_eq!(
            Victory::parse("territory", "500"),
            Ok(Victory::Territory { squares: 500 })
        );
        assert!(Victory::parse("timer", "0").is_err(), "over before it began");
        assert!(Victory::parse("timer", "soon").is_err());
        assert!(Victory::parse("vibes", "3").is_err());
    }

    #[test]
    fn the_leader_is_who_holds_most_and_ties_go_to_the_lower_number() {
        let mut held = [0usize; PlayerId::COUNT];
        assert_eq!(leader(&held), (None, 0), "nobody holds anything");

        held[3] = 10;
        held[7] = 40;
        assert_eq!(leader(&held), (Some(PlayerId(7)), 40));

        held[3] = 40;
        assert_eq!(leader(&held), (Some(PlayerId(3)), 40), "a tie is broken by number");
    }
}
