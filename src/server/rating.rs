//! How good somebody is, as a number that results move.
//!
//! Elo, in the shape everybody knows it: a rating difference predicts a score,
//! a result differs from that prediction, and the difference times a constant
//! is what changes hands. Two players, one game, a dozen lines.
//!
//! A match here is up to fifteen, which is where the dozen lines stop being
//! enough — see [`deltas`] for the reduction and why it is the one taken.
//!
//! **Nothing here reads or writes anything.** It takes numbers and returns
//! numbers, which is deliberate: a rating is only worth having if the result
//! behind it is authoritative, and the server is the only thing that knows a
//! real result. Keeping the arithmetic separate from where it is stored means
//! the storage question — which is the hard one, and is not answered yet — can
//! be settled without touching any of this.
//!
//! ## What this is not, and what it is waiting for
//!
//! It is not a leaderboard and it is not keyed by anybody, because **a rating
//! is a fact about a person and this game has no people.** It has [`PlayerId`],
//! which is a seat in one room and is handed to somebody else when the room
//! fills up again, and a rejoin token, which is a secret filed *per room* — so
//! a rating kept against a token would be earned in a match and thrown away
//! with the room the match was played in. `docs/planned.md` sequences it as
//! identity, then teams, then this, and that order is right: a table keyed by
//! a seat is a table of numbers that belong to whoever sits there next.
//!
//! So this is the half that can be correct now. When a person is a keypair
//! rather than a seat, the table is a map from that key to an `i32` and the
//! call site is [`deltas`] at the moment a match reaches `MatchPhase::Over`.
//!
//! [`PlayerId`]: crate::sim::PlayerId

/// What somebody is rated before they have played anything.
///
/// The traditional figure, and the argument for it is that it is traditional:
/// the number means nothing on its own — only differences do — so the only
/// thing to optimise is recognition, and 1200 is the one people have seen.
pub const START: i32 = 1200;

/// How far apart two ratings have to be for the stronger to be expected to
/// score ten times as often.
///
/// Elo's own constant. Changing it rescales every rating in the table at once,
/// which is why it is named rather than written into [`expected`]: it is a
/// choice of units, not a tuning knob.
pub const SPREAD: f64 = 400.0;

/// The most a single match can move a rating, before the multiplayer
/// reduction in [`deltas`] divides it among opponents.
///
/// Thirty-two is the usual figure for players who are still finding their
/// level, and everybody here is: this game has no history to have settled
/// anybody's number against. A smaller K makes a rating steadier and slower to
/// be right about a newcomer, and there is nothing to be steady about yet.
pub const K: f64 = 32.0;

/// The score `a` is expected to take against `b`, between nought and one.
///
/// Symmetric by construction — `expected(a, b) + expected(b, a) == 1` — which
/// is what makes the whole thing zero-sum and is worth stating because it is
/// the property every test here leans on.
pub fn expected(a: i32, b: i32) -> f64 {
    1.0 / (1.0 + 10f64.powf((b - a) as f64 / SPREAD))
}

/// What holding `mine` against `theirs` scored: a win, a draw, or a loss.
///
/// **Ground held, not a winner's flag.** A match names one winner, and rating
/// everybody against that one name would say the same thing about the player
/// who came second by a square and the one who never got off their block.
/// Every pair is its own result, which is what makes a fifteen-player match
/// fifteen players' worth of information rather than one bit.
fn outcome(mine: usize, theirs: usize) -> f64 {
    match mine.cmp(&theirs) {
        std::cmp::Ordering::Greater => 1.0,
        std::cmp::Ordering::Equal => 0.5,
        std::cmp::Ordering::Less => 0.0,
    }
}

/// One entrant in a finished match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entrant {
    /// What they were rated **going in**. Every delta is computed against
    /// these and none against a rating this match has already moved — see
    /// [`deltas`].
    pub rating: i32,
    /// Which side they were on. In a free-for-all everybody is their own side,
    /// so this is their player number; in a team match it is their team.
    ///
    /// Two entrants sharing a side are never rated against each other. There
    /// is no result between them to rate: they won or lost the same match.
    pub team: u8,
    /// What that **side** scored. Compared between sides and never within one,
    /// so two allies with wildly different ground still take the same result,
    /// which is what being on a side means.
    pub score: usize,
}

/// What each entrant's rating should change by, in the order they were given.
///
/// ## Every pair, and why not the alternatives
///
/// Elo is a two-player formula and a match here is up to fifteen, so the
/// result has to be reduced to something it can eat. Three readings are usual
/// and this takes the first:
///
/// - **Every pairwise outcome.** A fifteen-player match is a hundred and five
///   little games: you beat everybody below you and lost to everybody above.
/// - Score against the field's average rating, as one game.
/// - Rate the winner and nobody else.
///
/// The second throws away who you actually beat — coming second in a field of
/// experts and second in a field of beginners read the same — and the third
/// says nothing at all about fourteen of the fifteen. The pairwise reading is
/// the usual answer for a free-for-all, and it is also the one that needs no
/// second thought when [teams] arrive, because a team result *is* one pairwise
/// outcome per opposing pair.
///
/// ## Divided by the field
///
/// The surprise is summed over every opponent and then divided by how many
/// there were. Without that, K is paid once per pair: a fifteen-player win
/// would move a rating by up to fourteen times what a duel does, so entering
/// a crowded match would be worth more than being good at it. Dividing makes
/// K what it says it is — the most one *match* can move a rating — and leaves
/// the pairwise detail doing what it is for, which is deciding the direction
/// and the size of the surprise rather than its scale.
///
/// ## All at once
///
/// Every delta is computed from the ratings the entrants came in with, and
/// none from a rating this same match has already moved. Applying them one at
/// a time would make the result depend on the order of this slice, so the same
/// match replayed with the players listed differently would rate them
/// differently.
///
/// An entrant with no opponents — alone, or everybody on one side — gets
/// nought. There was no result.
///
/// [teams]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#teams
pub fn deltas(entrants: &[Entrant]) -> Vec<i32> {
    entrants
        .iter()
        .map(|me| {
            let opponents = entrants.iter().filter(|them| them.team != me.team);
            let (mut expectation, mut actual, mut count) = (0.0, 0.0, 0usize);
            for them in opponents {
                expectation += expected(me.rating, them.rating);
                actual += outcome(me.score, them.score);
                count += 1;
            }
            if count == 0 {
                return 0;
            }
            (K * (actual - expectation) / count as f64).round() as i32
        })
        .collect()
}

/// The ratings after a match, saturating rather than wrapping.
///
/// A convenience over [`deltas`] for the caller that has no use for the change
/// on its own. The floor is nought: a rating is a number people read, and a
/// negative one reads as a bug in the game rather than as a bad run.
pub fn after(entrants: &[Entrant]) -> Vec<i32> {
    deltas(entrants)
        .into_iter()
        .zip(entrants)
        .map(|(delta, entrant)| entrant.rating.saturating_add(delta).max(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffa(ratings_and_scores: &[(i32, usize)]) -> Vec<Entrant> {
        ratings_and_scores
            .iter()
            .enumerate()
            .map(|(i, &(rating, score))| Entrant { rating, team: i as u8, score })
            .collect()
    }

    /// The property the whole thing rests on: two ratings predict two scores
    /// that sum to one, so what one player is expected to take is exactly what
    /// the other is expected to lose.
    #[test]
    fn expectations_between_two_players_sum_to_one() {
        for (a, b) in [(1200, 1200), (1200, 1600), (900, 1201), (0, 3000)] {
            let total = expected(a, b) + expected(b, a);
            assert!((total - 1.0).abs() < 1e-9, "{a} vs {b} expected {total}");
        }
        assert!((expected(1200, 1200) - 0.5).abs() < 1e-9, "equals are an even match");
        assert!(expected(1600, 1200) > 0.9, "four hundred points is a heavy favourite");
    }

    /// A duel is the ordinary two-player case, and it is zero-sum: the winner
    /// takes exactly what the loser gives up. This is what stops a rating pool
    /// inflating by being played in.
    #[test]
    fn a_duel_moves_the_same_number_both_ways() {
        for pair in [[(1200, 9), (1200, 4)], [(1000, 9), (1800, 4)], [(1800, 9), (1000, 4)]] {
            let d = deltas(&ffa(&pair));
            assert_eq!(d[0], -d[1], "{pair:?} was not zero-sum: {d:?}");
            assert!(d[0] >= 0, "{pair:?}: the winner lost rating");
        }
        assert!(deltas(&ffa(&[(1200, 9), (1200, 4)]))[0] > 0, "an even match paid nothing");
        // And a favourite who was always going to win gains **nothing**, once
        // the surprise is rounded to a whole number. That is Elo doing its job
        // rather than failing at it: there was no information in the result.
        assert_eq!(deltas(&ffa(&[(1800, 9), (1000, 4)]))[0], 0);
    }

    /// Beating somebody better than you is worth more than beating somebody
    /// worse, which is the entire point of a rating rather than a win count.
    #[test]
    fn the_surprise_is_what_is_paid_for() {
        let over_a_favourite = deltas(&ffa(&[(1200, 9), (1600, 4)]))[0];
        let over_an_equal = deltas(&ffa(&[(1200, 9), (1200, 4)]))[0];
        let over_an_underdog = deltas(&ffa(&[(1200, 9), (800, 4)]))[0];
        assert!(
            over_a_favourite > over_an_equal && over_an_equal > over_an_underdog,
            "{over_a_favourite} then {over_an_equal} then {over_an_underdog}"
        );
        // And nobody is paid nothing for winning, however lopsided the match.
        assert!(over_an_underdog > 0, "beating an underdog was worth nothing");
    }

    /// Equal ratings and equal ground is the one result that says nothing, so
    /// it moves nothing.
    #[test]
    fn a_draw_between_equals_changes_nothing() {
        assert_eq!(deltas(&ffa(&[(1200, 5), (1200, 5)])), vec![0, 0]);
        assert_eq!(deltas(&ffa(&[(1200, 5), (1200, 5), (1200, 5)])), vec![0, 0, 0]);
    }

    /// Ground held, not a winner's flag: coming second is not the same result
    /// as coming last, and the numbers have to say so.
    #[test]
    fn every_place_gets_its_own_result() {
        let d = deltas(&ffa(&[(1200, 30), (1200, 20), (1200, 10)]));
        assert!(d[0] > d[1] && d[1] > d[2], "three places came back as {d:?}");
        assert_eq!(d[1], 0, "the middle of an even field beat one and lost to one");
    }

    /// K is the most a *match* moves a rating, not the most a pairing does.
    /// Without the division, entering a crowded game would be worth more than
    /// being good at one.
    #[test]
    fn a_crowd_does_not_pay_more_than_a_duel() {
        let duel = deltas(&ffa(&[(1200, 9), (1200, 4)]))[0];
        let field: Vec<(i32, usize)> =
            std::iter::once((1200, 99)).chain((0..14).map(|i| (1200, i))).collect();
        let crowd = deltas(&ffa(&field))[0];
        assert_eq!(duel, crowd, "winning a duel and sweeping a field of fifteen differ");
        assert!(crowd <= K as i32, "one match moved a rating by more than K");
    }

    /// Every delta comes from the ratings everybody walked in with, so listing
    /// the same match in a different order rates it the same way.
    #[test]
    fn the_order_of_the_entrants_does_not_matter() {
        let forward = ffa(&[(1000, 30), (1400, 20), (1200, 10)]);
        let mut backward = forward.clone();
        backward.reverse();
        let mut theirs = deltas(&backward);
        theirs.reverse();
        assert_eq!(deltas(&forward), theirs);
    }

    /// Allies are never rated against each other: there is no result between
    /// two people who won the same match. They take the side's result, and
    /// they take it against the other side's members one for one.
    #[test]
    fn a_side_shares_one_result_and_never_rates_itself() {
        // Two on a side. The winning side scored more ground; both of its
        // members beat both of the other side's.
        let match_ = [
            Entrant { rating: 1200, team: 1, score: 40 },
            Entrant { rating: 1200, team: 1, score: 40 },
            Entrant { rating: 1200, team: 2, score: 10 },
            Entrant { rating: 1200, team: 2, score: 10 },
        ];
        let d = deltas(&match_);
        assert_eq!(d[0], d[1], "allies on equal ratings took different results");
        assert_eq!(d[2], d[3]);
        assert_eq!(d[0], -d[2], "a two-a-side match was not zero-sum");
        assert!(d[0] > 0);

        // And a weaker ally does not drag a stronger one's *result* about: the
        // side's score is what is compared, so the two differ only by what
        // their own ratings predicted.
        let mixed = [
            Entrant { rating: 1000, team: 1, score: 40 },
            Entrant { rating: 1400, team: 1, score: 40 },
            Entrant { rating: 1200, team: 2, score: 10 },
            Entrant { rating: 1200, team: 2, score: 10 },
        ];
        let d = deltas(&mixed);
        assert!(d[0] > d[1], "the underrated ally should gain more for the same win: {d:?}");
    }

    /// Nobody to play means nothing to rate. A solo room and a match where
    /// everybody ended up on one side are the same case.
    #[test]
    fn a_match_with_no_opponents_rates_nobody() {
        assert_eq!(deltas(&ffa(&[(1200, 9)])), vec![0]);
        assert_eq!(deltas(&[]), Vec::<i32>::new());
        let one_side = [
            Entrant { rating: 1200, team: 1, score: 9 },
            Entrant { rating: 1400, team: 1, score: 4 },
        ];
        assert_eq!(deltas(&one_side), vec![0, 0]);
    }

    /// A rating is a number people read, and a negative one reads as the game
    /// being broken rather than as a bad run.
    #[test]
    fn a_rating_never_goes_below_nought() {
        // Against an equal, so there is a full loss to take: nobody loses much
        // to somebody they were never expected to beat.
        let hopeless =
            [Entrant { rating: 3, team: 1, score: 0 }, Entrant { rating: 3, team: 2, score: 500 }];
        assert_eq!(after(&hopeless)[0], 0, "a rating went below nought");
        assert_eq!(after(&hopeless)[1], 19);
    }

    /// Played out over many matches, a rating finds the level of the player
    /// rather than wandering: somebody who wins two thirds of their games
    /// against a fixed opponent settles above them and stays there.
    #[test]
    fn a_rating_converges_on_who_keeps_winning() {
        let (mut me, mut them) = (START, START);
        for game in 0..300 {
            // Two wins in three, for ever.
            let (mine, theirs) = if game % 3 == 2 { (4, 9) } else { (9, 4) };
            let after = after(&[
                Entrant { rating: me, team: 1, score: mine },
                Entrant { rating: them, team: 2, score: theirs },
            ]);
            (me, them) = (after[0], after[1]);
        }
        // Two wins in three is a gap of about a hundred and twenty in Elo's
        // own units — `expected` of 2/3 is `SPREAD * log10(2)` — and K-sized
        // jitter around it is the most that can be asked of a rating kept in
        // whole numbers.
        let gap = me - them;
        assert!((80..=160).contains(&gap), "settled {gap} apart, on {me} against {them}");
    }
}
