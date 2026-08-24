//! What a match *does*. The types it does it to are in [`crate::net`], because
//! a client has to be told what a match is doing and the wire is where the two
//! sides agree on a vocabulary.
//!
//! **Nothing here is in `sim`.** The simulation does not know what a match is,
//! the same way it does not know what money is: a match is an arrangement of
//! when a room steps and who may join it, and both of those are the server's
//! business. What that buys is that a match cannot introduce a rule the world
//! has to honour, and so cannot make a match world behave differently from the
//! one people practise in.

pub use crate::net::Victory;
use crate::sim::PlayerId;

pub use crate::net::MatchPhase as Phase;

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
