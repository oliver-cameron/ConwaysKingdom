//! How badly this client and the server disagree, as a rate rather than an event.
//!
//! A desync is already detected: `Checkpoint` sends per-chunk digests every
//! [`CHECKPOINT_EVERY`] generations, and the server answers with a `Resync`
//! naming the chunks that do not match. What that gives is a **bang** — a log
//! line, a refetch, and silence again — and a bang tells you nothing about
//! whether the last one was an isolated hiccup or the fourth this minute.
//!
//! So this is a **geiger counter**: every disagreeing chunk is one click, the
//! clicks decay, and what is shown is the rate. A single chunk after a lagged
//! frame reads as background; a stream of them reads as something wrong, and it
//! reads that way *while it is happening* rather than in the log afterwards.
//! The distinction matters because prediction makes one generation of
//! disagreement normal by design — see [docs/networking.md] — so the useful
//! question was never "did we disagree" but "how often, and is it settling".
//!
//! Pure arithmetic, no egui and no socket: the client feeds it and the HUD
//! reads it, which is what lets it be tested without either.
//!
//! [`CHECKPOINT_EVERY`]: crate::client::views::game
//! [docs/networking.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/networking.md

/// How long a click takes to fall to half its weight, in seconds.
///
/// Measured against real time rather than generations because it is watched by
/// eyes: a rate that decayed on the tick would freeze whenever the world did,
/// and a stalled world is exactly when somebody is staring at this.
///
/// Twelve seconds is about three checkpoint intervals at four generations a
/// second, so a burst that stops is visibly falling by the next checkpoint and
/// gone by the third — fast enough to say "it has settled" and slow enough
/// that two separate hiccups a few seconds apart still add up.
pub const HALF_LIFE: f64 = 12.0;

/// Rate at or above which the disagreement is worth a colour.
///
/// One click is a chunk, and one chunk out of step is the ordinary cost of
/// predicting: it happens whenever an action misses a server step. Two at once
/// is a pattern.
pub const NOTICEABLE: f64 = 2.0;

/// Rate at or above which something is actually wrong.
///
/// Sustained: with a half-life of twelve seconds, holding eight means roughly
/// a chunk every two seconds and not stopping, which is no longer prediction
/// error settling out — it is a world being rebuilt faster than it is played.
pub const ALARMING: f64 = 8.0;

/// How loud the disagreement is right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Nothing has clicked recently.
    Quiet,
    /// Ticking over. Prediction costs this much and always has.
    Background,
    /// More than the ordinary cost, and worth an eye.
    Noticeable,
    /// Something is wrong.
    Alarming,
}

/// A decaying count of chunks the server has had to correct.
///
/// Exponential rather than a window of timestamps: a window needs a list, a
/// capacity and a decision about what to do when it fills, and gives a number
/// that steps down as entries fall off the end. This is two floats and falls
/// smoothly, which is what makes it readable as a level rather than a count.
#[derive(Clone, Copy, Debug, Default)]
pub struct Geiger {
    /// Clicks, decayed. Not an integer: half a click is what one click looks
    /// like a half-life later.
    rate: f64,
    /// Chunks corrected since the client started. The rate says how it is
    /// going; this says whether it has ever happened at all, which is the
    /// question after the fact.
    total: u64,
    /// Elapsed seconds when the rate was last decayed, so `decay` can be
    /// called at any cadence and mean the same thing.
    at: f64,
}

impl Geiger {
    /// Bring the rate up to `now`, where `now` is the client's elapsed
    /// seconds.
    ///
    /// Idempotent for a given `now`, so calling it once a frame and again
    /// before a read costs nothing and cannot decay twice.
    pub fn decay(&mut self, now: f64) {
        let dt = now - self.at;
        if dt <= 0.0 {
            return;
        }
        self.at = now;
        // 2^(-dt/half-life): one half-life halves it, whatever dt was.
        self.rate *= 0.5f64.powf(dt / HALF_LIFE);
        // Below a hundredth of a click nothing is left to show, and letting it
        // run down to a denormal is arithmetic nobody is reading.
        if self.rate < 0.01 {
            self.rate = 0.0;
        }
    }

    /// `chunks` disagreed, as of `now`.
    ///
    /// Counted per chunk rather than per message, because a `Resync` naming
    /// forty chunks and one naming a single chunk are not the same event —
    /// the first is a world being rebuilt and the second is one prediction
    /// that missed.
    pub fn clicks(&mut self, chunks: usize, now: f64) {
        self.decay(now);
        self.rate += chunks as f64;
        self.total += chunks as u64;
    }

    /// Clicks per half-life, decayed to the last [`Self::decay`].
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Every chunk ever corrected on this connection.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Whether anything has ever disagreed. A rate back at nought and a
    /// connection that has never slipped look identical otherwise, and they
    /// are not the same thing to somebody deciding whether to trust it.
    pub fn ever(&self) -> bool {
        self.total > 0
    }

    /// How loud it is, for something that has to pick a colour or a word.
    pub fn level(&self) -> Level {
        if self.rate >= ALARMING {
            Level::Alarming
        } else if self.rate >= NOTICEABLE {
            Level::Noticeable
        } else if self.rate > 0.0 {
            Level::Background
        } else {
            Level::Quiet
        }
    }

    /// A new connection is a new world and a new argument about it. Cleared on
    /// `Welcome` rather than carried, or a client that changed rooms would
    /// show the last room's trouble against this room's world.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: one click is background, a burst is not, and a burst
    /// that stops falls back on its own.
    #[test]
    fn a_burst_is_loud_and_then_it_is_not() {
        let mut g = Geiger::default();
        assert_eq!(g.level(), Level::Quiet);
        assert!(!g.ever());

        g.clicks(1, 0.0);
        assert_eq!(g.level(), Level::Background, "one chunk is what predicting costs");

        g.clicks(9, 0.0);
        assert_eq!(g.level(), Level::Alarming, "ten at once is a world being rebuilt");

        // One half-life halves it: ten becomes five, which is still more than
        // ordinary but no longer alarming.
        g.decay(HALF_LIFE);
        assert!((g.rate() - 5.0).abs() < 1e-9, "{}", g.rate());
        assert_eq!(g.level(), Level::Noticeable);

        // And left alone it goes quiet, without ever pretending it did not
        // happen.
        g.decay(HALF_LIFE * 12.0);
        assert_eq!(g.level(), Level::Quiet);
        assert!(g.ever(), "quiet is not the same as never");
        assert_eq!(g.total(), 10);
    }

    /// Decay is a function of elapsed time, not of how often it is called, or
    /// the rate would depend on the frame rate — which is the one thing a
    /// desync reading must not do, since a client in trouble is usually a
    /// client dropping frames.
    #[test]
    fn the_rate_does_not_depend_on_how_often_it_is_looked_at() {
        let mut often = Geiger::default();
        let mut seldom = Geiger::default();
        often.clicks(8, 0.0);
        seldom.clicks(8, 0.0);

        for i in 1..=600 {
            often.decay(i as f64 * 0.01);
        }
        seldom.decay(6.0);

        assert!(
            (often.rate() - seldom.rate()).abs() < 1e-9,
            "{} vs {}",
            often.rate(),
            seldom.rate()
        );
    }

    /// Time not moving is not time moving backwards. A clock that went
    /// backwards would multiply the rate up rather than down.
    #[test]
    fn a_clock_that_does_not_move_does_not_change_anything() {
        let mut g = Geiger::default();
        g.clicks(4, 10.0);
        let was = g.rate();
        g.decay(10.0);
        g.decay(9.0);
        assert_eq!(g.rate(), was);
    }

    /// A different room is a different argument.
    #[test]
    fn joining_somewhere_else_starts_over() {
        let mut g = Geiger::default();
        g.clicks(20, 0.0);
        g.reset();
        assert_eq!(g.level(), Level::Quiet);
        assert!(!g.ever(), "the new room has not disagreed about anything yet");
    }
}
