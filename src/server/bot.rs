//! The player with no socket.
//!
//! A bot is a [`Player`] like anybody else — a number, a purse, a patch of
//! granted ground — with nothing on the other end of it. The server makes up
//! its mind for it in [`Server::step`], wraps the answer in a [`Stamped`] and
//! feeds it through [`Server::act`], so it reaches every client as an
//! ordinary action and can do nothing a client could not. Nothing about the
//! protocol changes for it beyond the lobby saying which seats are bots.
//!
//! **Its play is a book or a search**, and the book comes first.
//! `examples/balance` measured what the economy rewards — a blinker pays, a
//! glider bleeds, sprawl bleeds badly — so a competent bot is a small book of
//! shapes and a rule about where to put them: compact oscillators inside its
//! own ground to earn, life at the frontier where ground is contested, and ice
//! around what it wants to keep. Difficulty is two dials: how often it acts,
//! and what it will do.
//!
//! [`Driver::Search`] is the second version and the dials are the same two.
//! It takes the placements the book would have offered, tries each on a
//! [`World::crop`] of the board, steps that [`SEARCH_HORIZON`] generations and
//! asks a [`Judge`] what it is looking at; the level says how many it tries.
//! The judge is a trait because the hand-written [`Counted`] is not meant to
//! be the last one — a learned evaluator drops in behind it, and the search
//! must not know which one it is holding. See planned.md.
//!
//! Determinism is not a problem, which is worth saying because it looks like
//! one — twice over, since a score is an `f32` and a rollout steps a world. A
//! choice is made once, on the server, and reaches every peer as an action at
//! a stated tick; the dice here are its own — the room's seed, the seat and
//! the tick through [`crate::sim::mix`] — and never touch the streams
//! [`crate::sim::seed`] rolls for a cell. Nothing in [`crate::sim`] sees the
//! `f32`, and the world a rollout moves is a copy nothing else can reach.
//!
//! [`Player`]: crate::sim::Player
//! [`Server::step`]: crate::server::Server::step
//! [`Server::act`]: crate::server::Server::act

use crate::net::{Action, Level, Placement, Rules, Stamped, Tick};
use crate::sim::{
    bits, mix, Kind, PlayerId, Roll, World, CHUNK_N, DYNAMITE_MOST_REACH, TURRET_REACH,
};

/// Generations between one act and the next: the first dial.
const EASY_EVERY: Tick = 16;
const NORMAL_EVERY: Tick = 8;
const HARD_EVERY: Tick = 4;

/// How far from home a bot looks for somewhere to build, in cells each way;
/// half its samples land this far out and half within [`NEAR`] — see
/// [server.md].
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#bots
const REACH: i32 = 5 * crate::net::SPAWN_N;

/// The patch itself.
const NEAR: i32 = crate::net::SPAWN_N / 2;

/// How many squares one act looks at. **Sampled, not scanned**, so an act
/// costs the same on a full torus as on an empty plane.
const SAMPLES: usize = 48;

/// How many a search looks at, which is more because it can use them: a book
/// stops at the first square that fits and a search is choosing between the
/// ones that do. A sample costs a walk of a shape's span against a rollout's
/// several thousand cells, so this is the cheap dial — and it is the one that
/// was measured to matter, see [server.md].
///
/// At most 198, or the row and column streams below overlap.
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#bots
const SEARCH_SAMPLES: usize = 192;

/// How near somebody else's ground makes a square the frontier, in cells.
const FRONTIER: i32 = 6;

/// Placements one act of a search tries: the second dial's other end, and
/// **what a rollout was measured to cost sets the ceiling on it** — see
/// [server.md], which has the figure and the tick it is against.
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#bots
const EASY_TRIES: usize = 2;
const NORMAL_TRIES: usize = 5;
const HARD_TRIES: usize = 10;

/// Generations a rollout runs for.
const SEARCH_HORIZON: usize = 8;

/// How far the edge of a crop reaches into it over one rollout — see
/// [`World::crop`], which is where the arithmetic is argued.
const SEARCH_MARGIN: i32 = SEARCH_HORIZON as i32 + DYNAMITE_MOST_REACH;

/// **The horizon has to fit inside a blast**, or the margin is a lie: past
/// this a turret moves ground further in `SEARCH_HORIZON` generations than
/// [`DYNAMITE_MOST_REACH`] allows for, and the region called exact is not.
const _: () = assert!(SEARCH_HORIZON as i32 * TURRET_REACH <= DYNAMITE_MOST_REACH);

/// The biggest span in [`BOOK`], pinned by `the_book_keeps_to_its_own_spans`.
const MOST_SPAN: i32 = 4;

/// **Everywhere a placement can reach**, and all a [`Judge`] is shown: as far
/// out as the book offers one, the shape itself, and what a generation of it
/// moves over the horizon. A window any tighter cuts the frontier in half —
/// a shape laid at [`REACH`] would have the ground it claims fall outside the
/// score, and the search would prefer home for a reason that is arithmetic
/// rather than play.
///
/// Public because `examples/duel` records this exact window: what a corpus
/// holds and what a judge is handed have to be the same picture.
pub const SEARCH_SEEN: i32 = REACH + MOST_SPAN + SEARCH_HORIZON as i32;

/// Half the board a rollout runs on: what is scored, and the margin round it.
const SEARCH_CROP: i32 = SEARCH_SEEN + SEARCH_MARGIN;

/// A square of this player's at full influence: the unit a match is won in.
const SCORE_GROUND: f32 = 1.0;
/// One of theirs held below it.
const SCORE_EDGE: f32 = 0.5;
/// One of somebody else's.
const SCORE_THEIRS: f32 = -1.0;
/// A living cell, over the square it stands on.
const SCORE_LIFE: f32 = 0.5;
/// A living factory, over the life.
const SCORE_FACTORY: f32 = 2.0;
/// A factory's corpse: a bill that has not fallen due.
const SCORE_CORPSE: f32 = -1.0;

/// **What a position is worth to a player**, in the units the game counts.
///
/// Higher is better and the scale is arbitrary: a search compares scores on
/// one board and never across boards.
///
/// A trait because [`Counted`] is not meant to be the last word. A learned
/// evaluator is the next piece of work here and arrives behind this signature
/// — see planned.md — so a search must be able to hold one without knowing
/// which it is.
///
/// The world handed over is a crop and **only its exact part**: see
/// [`World::crop`] for what a crop stops being able to say, and
/// [`SEARCH_SEEN`] for how much of one is trimmed off before a judge sees it.
pub trait Judge: Send + Sync {
    fn score(&self, world: &World, me: PlayerId) -> f32;
}

/// The hand-written judge: ground, life and manufacture, counted and weighted.
/// The argument for the weights is in [server.md].
///
/// [server.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/server.md#bots
pub struct Counted;

impl Judge for Counted {
    fn score(&self, world: &World, me: PlayerId) -> f32 {
        let mut total = 0.0;
        for (_, chunk) in world.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    if !cell.player().is_owned() {
                        continue;
                    }
                    if cell.player() != me {
                        total += SCORE_THEIRS;
                        continue;
                    }
                    total +=
                        if cell.influence() == bits::MAX_LEVEL { SCORE_GROUND } else { SCORE_EDGE };
                    if cell.is_alive() {
                        total += SCORE_LIFE;
                    }
                    if cell.kind() == Kind::FACTORY {
                        total += if cell.is_alive() { SCORE_FACTORY } else { SCORE_CORPSE };
                    }
                }
            }
        }
        total
    }
}

/// Who is driving the seat.
pub enum Driver {
    /// The server, from [`BOOK`].
    Book,
    /// The server, from the book and a rollout — see [`Ground::best`]. It
    /// carries its own judge, so what a bot is scoring by is a fact about the
    /// seat rather than about the build.
    Search(Box<dyn Judge>),
    /// Something outside, through `server::api`. Its actions are priced the
    /// moment they arrive, so there is nothing to do here at a step.
    External,
}

impl Driver {
    /// The word a console and the API name one by. Not [`Self::External`]:
    /// that one is not asked for, it is what sitting down through the API
    /// makes you.
    pub fn parse(word: &str) -> Result<Self, String> {
        match word {
            "book" => Ok(Self::Book),
            "search" => Ok(Self::Search(Box::new(Counted))),
            _ => Err(format!("no driver \"{word}\"; try book or search")),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Search(_) => "search",
            Self::External => "api",
        }
    }
}

/// One seat the server plays.
pub struct Bot {
    pub level: Level,
    pub driver: Driver,
    /// The generation it next acts on.
    pub next_at: Tick,
    /// The room's seed and the seat, mixed once; the tick goes in per act.
    dice: u64,
    /// Factories it has laid, so `Keep` has something to come back to.
    laid: Vec<Laid>,
}

struct Laid {
    at: (i32, i32),
    shape: usize,
    iced: bool,
}

/// A pattern from the book: its cells, and the box it moves inside over its
/// whole period, so a margin can be kept clear round all of it.
pub struct Shape {
    pub name: &'static str,
    pub cells: &'static [(i32, i32)],
    pub span: (i32, i32),
}

/// The book. Oscillators earn; still lifes hold ground and cost nothing to
/// keep. Nothing that travels: a glider bleeds corpses behind it.
pub const BOOK: [Shape; 5] = [
    Shape { name: "blinker", cells: &[(1, 0), (1, 1), (1, 2)], span: (3, 3) },
    Shape { name: "toad", cells: &[(1, 1), (1, 2), (1, 3), (2, 0), (2, 1), (2, 2)], span: (4, 4) },
    Shape {
        name: "beacon",
        cells: &[(0, 0), (0, 1), (1, 0), (2, 3), (3, 2), (3, 3)],
        span: (4, 4),
    },
    Shape { name: "block", cells: &[(0, 0), (0, 1), (1, 0), (1, 1)], span: (2, 2) },
    Shape {
        name: "beehive",
        cells: &[(0, 1), (0, 2), (1, 0), (1, 3), (2, 1), (2, 2)],
        span: (3, 4),
    },
];

/// Which of the book pays as a factory: the ones with births in them.
const EARNERS: [usize; 3] = [0, 1, 2];
/// Which holds a frontier: cheap, and stays where it is put.
const HOLDERS: [usize; 3] = [3, 4, 0];

/// What a bot is trying to do with one act: the second dial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Intent {
    /// An oscillator of factories inside its own ground, away from anybody.
    Earn,
    /// Life on the frontier, where ground is contested.
    Contest,
    /// Ice round a factory it laid.
    Keep,
}

impl Level {
    fn every(self) -> Tick {
        match self {
            Self::Easy => EASY_EVERY,
            Self::Normal => NORMAL_EVERY,
            Self::Hard => HARD_EVERY,
        }
    }

    fn intents(self) -> &'static [Intent] {
        match self {
            Self::Easy => &[Intent::Earn],
            Self::Normal => &[Intent::Earn, Intent::Contest],
            Self::Hard => &[Intent::Earn, Intent::Contest, Intent::Keep],
        }
    }

    /// How deep a searching bot looks: the level scales this the way it scales
    /// [`Self::every`], so a level is one word for both dials.
    fn tries(self) -> usize {
        match self {
            Self::Easy => EASY_TRIES,
            Self::Normal => NORMAL_TRIES,
            Self::Hard => HARD_TRIES,
        }
    }
}

impl Bot {
    pub fn new(level: Level, driver: Driver, room_seed: u64, seat: PlayerId, now: Tick) -> Self {
        Self { level, driver, next_at: now, dice: mix(room_seed, seat.0 as u64), laid: Vec::new() }
    }

    /// Generations between acts.
    pub fn cadence(&self) -> Tick {
        self.level.every()
    }

    /// One act, or nothing if nowhere fits or nothing is affordable.
    ///
    /// The intents its level allows, from one the dice pick. A book takes the
    /// first that finds an affordable placement; a search takes what every one
    /// of them offers, up to [`Level::tries`], and picks between them by
    /// looking. Either way a placement is tested cell by cell with
    /// [`crate::net::may_place_under`] and priced with
    /// [`crate::net::price_under`], the way the server will judge it, so what
    /// comes back is an action that will be taken.
    pub fn choose(
        &mut self,
        world: &World,
        rules: &Rules,
        plays_as: PlayerId,
        purse: i32,
        tick: Tick,
    ) -> Option<Action> {
        // Taken apart rather than used through `self`, because one act needs
        // the judge on the driver and the factories beside it at once, and
        // that is two borrows of one struct.
        let Self { level, driver, laid, dice, .. } = self;
        let roll = Roll::new(mix(*dice, tick));
        let ground = Ground::around(world, rules, plays_as);
        let intents = level.intents();
        let first = roll.pick(0, intents.len());
        let afford = |action: &Action| {
            let stamped =
                Stamped { tick, player: plays_as, seat: plays_as, action: action.clone() };
            purse + crate::net::price_under(world, &stamped, rules) >= 0
        };
        let found = match driver {
            // Priced as it arrived; nothing waits for a step.
            Driver::External => None,
            Driver::Book => (0..intents.len()).find_map(|k| {
                let found = match intents[(first + k) % intents.len()] {
                    Intent::Earn => ground.place(&roll, &EARNERS, Placement::Factory, false),
                    Intent::Contest => ground.place(&roll, &HOLDERS, Placement::Life, true),
                    Intent::Keep => Self::keep(laid, &ground),
                };
                found.filter(|(action, _)| afford(action))
            }),
            Driver::Search(judge) => {
                let tries = level.tries();
                let mut offers = Vec::new();
                // Shared out between the intents rather than spent on the
                // first, or a hard bot would never see a frontier while there
                // was anywhere left at home to earn on.
                let each = tries.div_ceil(intents.len());
                for k in 0..intents.len() {
                    let mut got = match intents[(first + k) % intents.len()] {
                        Intent::Earn => {
                            ground.offers(&roll, &EARNERS, Placement::Factory, false, each)
                        }
                        Intent::Contest => {
                            ground.offers(&roll, &HOLDERS, Placement::Life, true, each)
                        }
                        // One ring or none: there is nothing to choose between.
                        Intent::Keep => Self::keep(laid, &ground).into_iter().collect(),
                    };
                    got.retain(|(action, _)| afford(action));
                    offers.append(&mut got);
                }
                offers.truncate(tries);
                ground.best(judge.as_ref(), tick, offers)
            }
        };
        let (action, note) = found?;
        // Remembered only once it is affordable, or a thin purse would
        // write a factory off as walled without a pane ever going down.
        match note {
            Note::Laid(one) => laid.push(one),
            Note::Iced(i) => laid[i].iced = true,
            Note::Nothing => {}
        }
        Some(action)
    }

    /// Ice round the oldest factory that is still standing and not yet iced.
    /// One that has died is forgotten rather than walled; one with nowhere
    /// left to put a pane counts as walled.
    fn keep(laid: &mut Vec<Laid>, ground: &Ground<'_>) -> Option<(Action, Note)> {
        while let Some(i) = laid.iter().position(|l| !l.iced) {
            let Laid { at, shape, .. } = laid[i];
            if !ground.standing(at, &BOOK[shape]) {
                laid.remove(i);
                continue;
            }
            let ring = ground.ring(at, &BOOK[shape]);
            if ring.is_empty() {
                laid[i].iced = true;
                continue;
            }
            return Some((Action::Paint { cells: ring, placement: Placement::Ice }, Note::Iced(i)));
        }
        None
    }
}

/// What to remember about an act once it is taken.
enum Note {
    Laid(Laid),
    Iced(usize),
    Nothing,
}

/// The ground one act looks at: a window round the seat's home, and the rules
/// it is judged under.
struct Ground<'a> {
    world: &'a World,
    rules: &'a Rules,
    me: PlayerId,
    home: (i32, i32),
}

impl Ground<'_> {
    fn around<'a>(world: &'a World, rules: &'a Rules, me: PlayerId) -> Ground<'a> {
        let (row, col) = crate::net::spawn_for(me, world);
        let half = crate::net::SPAWN_N / 2;
        Ground { world, rules, me, home: (row + half, col + half) }
    }

    /// Somewhere one of these shapes fits, as a paint: every cell placeable
    /// and the margin round it quiet, on the frontier or off it as asked.
    fn place(
        &self,
        roll: &Roll,
        shapes: &[usize],
        placement: Placement,
        frontier: bool,
    ) -> Option<(Action, Note)> {
        let mut out = Vec::new();
        let shape = shapes[roll.pick(1, shapes.len())];
        self.sample(roll, shape, placement, frontier, SAMPLES, 1, &mut out);
        out.pop()
    }

    /// **Everywhere one of these shapes fits, up to `most`.** What a search
    /// chooses between, and it is the book's own generator rather than a
    /// second one: [`Self::sample`] with the loop left running, so a candidate
    /// is a square [`Self::fits`] passed exactly as it would have to for the
    /// book to lay there. It looks at [`SEARCH_SAMPLES`] of them against the
    /// book's [`SAMPLES`], so these are **not the book's own squares** — what
    /// is the book's is which of them may be built on at all.
    ///
    /// The budget is **taken a shape at a time round the ring** rather than
    /// spent on the first, so a search picks what to lay as well as where.
    /// Every shape samples the same squares, so one pooled cap is a cap the
    /// first shape fills every time — and a search that can only choose a
    /// square is half a search. Going round means a shape that fits nowhere
    /// costs the others nothing.
    fn offers(
        &self,
        roll: &Roll,
        shapes: &[usize],
        placement: Placement,
        frontier: bool,
        most: usize,
    ) -> Vec<(Action, Note)> {
        let first = roll.pick(1, shapes.len());
        let mut each: Vec<Vec<(Action, Note)>> = (0..shapes.len())
            .map(|k| {
                let mut got = Vec::new();
                let shape = shapes[(first + k) % shapes.len()];
                self.sample(roll, shape, placement, frontier, SEARCH_SAMPLES, most, &mut got);
                // Reversed, so popping hands them back in the order they were
                // sampled in.
                got.reverse();
                got
            })
            .collect();
        let mut out = Vec::with_capacity(most);
        while out.len() < most && each.iter().any(|got| !got.is_empty()) {
            for got in &mut each {
                if out.len() >= most {
                    break;
                }
                out.extend(got.pop());
            }
        }
        out
    }

    /// Squares round home this shape fits on, **sampled and not scanned**, so
    /// an act costs the same on a full torus as on an empty plane.
    fn sample(
        &self,
        roll: &Roll,
        shape: usize,
        placement: Placement,
        frontier: bool,
        samples: usize,
        most: usize,
        out: &mut Vec<(Action, Note)>,
    ) {
        let pattern = &BOOK[shape];
        for k in 0..samples as u64 {
            if out.len() >= most {
                return;
            }
            let reach = if k < samples as u64 / 2 { NEAR } else { REACH };
            let window = (2 * reach) as usize;
            let at = (
                self.home.0 - reach + roll.pick(2 + k, window) as i32,
                self.home.1 - reach + roll.pick(200 + k, window) as i32,
            );
            if !self.fits(at, pattern) || self.frontier(at, pattern) != frontier {
                continue;
            }
            let cells = pattern.cells.iter().map(|&(r, c)| (at.0 + r, at.1 + c)).collect();
            let note = match placement {
                Placement::Factory => Note::Laid(Laid { at, shape, iced: false }),
                _ => Note::Nothing,
            };
            out.push((Action::Paint { cells, placement }, note));
        }
    }

    /// **Try each of them, step a copy, and take the best.**
    ///
    /// One crop for the whole act rather than one per candidate, which is what
    /// makes this honest rather than a fudge: the board's missing edge is
    /// wrong in the same way for everything scored on it, so what does not
    /// cancel is small and the ordering — the only thing read here — survives
    /// it. Only [`SEARCH_SEEN`] of the rollout reaches the judge, because that
    /// is as much of it as is the real world's future; see [`World::crop`].
    ///
    /// The first of equal scores wins, so one board and one set of offers give
    /// one answer.
    fn best(
        &self,
        judge: &dyn Judge,
        tick: Tick,
        mut offers: Vec<(Action, Note)>,
    ) -> Option<(Action, Note)> {
        if offers.len() < 2 {
            return offers.pop();
        }
        let board = self.world.crop(self.home, SEARCH_CROP);
        let mut best: Option<(f32, usize)> = None;
        for (i, (action, _)) in offers.iter().enumerate() {
            let mut trial = board.clone();
            let stamped = Stamped { tick, player: self.me, seat: self.me, action: action.clone() };
            crate::net::apply(&mut trial, &stamped);
            for _ in 0..SEARCH_HORIZON {
                trial.step();
            }
            let score = judge.score(&trial.crop(self.home, SEARCH_SEEN), self.me);
            if best.is_none_or(|(top, _)| score > top) {
                best = Some((score, i));
            }
        }
        Some(offers.swap_remove(best?.1))
    }

    /// Every cell of the shape is this player's to place on, and nothing
    /// lives or is frozen anywhere in the span or the ring round it. All or
    /// nothing, as the server prices it.
    fn fits(&self, at: (i32, i32), shape: &Shape) -> bool {
        let quiet = |r: i32, c: i32| {
            self.world.cell_at(r, c).is_none_or(|cell| !cell.is_alive() && !cell.is_ice())
        };
        let placeable =
            |r: i32, c: i32| crate::net::may_place_under(self.world, self.me, r, c, self.rules);
        (at.0 - 1..=at.0 + shape.span.0)
            .all(|r| (at.1 - 1..=at.1 + shape.span.1).all(|c| quiet(r, c)))
            && shape.cells.iter().all(|&(r, c)| placeable(at.0 + r, at.1 + c))
    }

    /// Whether somebody else's ground is within [`FRONTIER`] of the span.
    fn frontier(&self, at: (i32, i32), shape: &Shape) -> bool {
        (at.0 - FRONTIER..=at.0 + shape.span.0 + FRONTIER).any(|r| {
            (at.1 - FRONTIER..=at.1 + shape.span.1 + FRONTIER).any(|c| {
                self.world
                    .cell_at(r, c)
                    .is_some_and(|cell| cell.player().is_owned() && cell.player() != self.me)
            })
        })
    }

    /// Whether any of a laid factory is still alive and ours.
    fn standing(&self, at: (i32, i32), shape: &Shape) -> bool {
        (at.0..at.0 + shape.span.0).any(|r| {
            (at.1..at.1 + shape.span.1).any(|c| {
                self.world.cell_at(r, c).is_some_and(|cell| {
                    cell.is_alive() && cell.kind() == Kind::FACTORY && cell.player() == self.me
                })
            })
        })
    }

    /// The border one cell outside a shape's span, where a pane can go: dead,
    /// unfrozen, and reachable.
    fn ring(&self, at: (i32, i32), shape: &Shape) -> Vec<(i32, i32)> {
        let (rows, cols) = shape.span;
        let mut out = Vec::new();
        for r in at.0 - 1..=at.0 + rows {
            for c in at.1 - 1..=at.1 + cols {
                let inside = (at.0..at.0 + rows).contains(&r) && (at.1..at.1 + cols).contains(&c);
                if inside {
                    continue;
                }
                let free =
                    self.world.cell_at(r, c).is_none_or(|cell| !cell.is_alive() && !cell.is_ice());
                if free && crate::net::may_place_under(self.world, self.me, r, c, self.rules) {
                    out.push((r, c));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn granted(me: PlayerId) -> World {
        let mut world = World::infinite_empty();
        crate::net::grant(&mut world, me);
        world
    }

    fn searching() -> Driver {
        Driver::Search(Box::new(Counted))
    }

    /// A judge that says nothing and counts what it was asked, which is how a
    /// test sees how many rollouts one act ran.
    struct Counting(std::sync::Arc<AtomicUsize>);

    impl Judge for Counting {
        fn score(&self, _: &World, _: PlayerId) -> f32 {
            self.0.fetch_add(1, Ordering::Relaxed);
            0.0
        }
    }

    /// Every shape in the book is what it says: the cells sit inside the span
    /// it declares, so the margin kept clear round one is round all of it —
    /// and no span is wider than [`MOST_SPAN`], which is what a crop is sized
    /// against.
    #[test]
    fn the_book_keeps_to_its_own_spans() {
        for shape in &BOOK {
            for &(r, c) in shape.cells {
                assert!(
                    (0..shape.span.0).contains(&r) && (0..shape.span.1).contains(&c),
                    "{}: ({r}, {c}) is outside its {:?} span",
                    shape.name,
                    shape.span
                );
            }
            assert!(
                shape.span.0 <= MOST_SPAN && shape.span.1 <= MOST_SPAN,
                "{} is {:?}, over the {MOST_SPAN} a crop leaves room for",
                shape.name,
                shape.span
            );
        }
    }

    /// An earner is an oscillator: stepped through a period on its own it is
    /// back where it started and it had births on the way, which is what a
    /// factory is paid for. A holder is a still life and has none.
    #[test]
    fn earners_oscillate_and_holders_stand_still() {
        let me = PlayerId(1);
        for (i, shape) in BOOK.iter().enumerate() {
            let mut world = World::infinite_empty();
            for &(r, c) in shape.cells {
                world.set_cell_at(10 + r, 10 + c, Cell::alive(me).with_kind(Kind::FACTORY));
            }
            let before = world.live_cells();
            let mut born = 0;
            for _ in 0..2 {
                born += world.step().born[1];
            }
            assert_eq!(world.live_cells(), before, "{} did not come back", shape.name);
            if EARNERS.contains(&i) {
                assert!(born > 0, "{} earns nothing", shape.name);
            } else {
                assert_eq!(born, 0, "{} is not still", shape.name);
            }
        }
    }

    /// The first thing a bot does on fresh ground is lay factories inside it:
    /// every cell reachable, none on top of the block it was granted.
    #[test]
    fn a_fresh_bot_lays_a_factory_it_may_afford_inside_its_own_ground() {
        let me = PlayerId(1);
        let world = granted(me);
        let rules = Rules::default();
        let mut bot = Bot::new(Level::Easy, Driver::Book, 0, me, 0);
        let action = bot.choose(&world, &rules, me, 100, 0).expect("nowhere to build on a grant");
        let Action::Paint { cells, placement } = &action else { panic!("not a paint") };
        assert_eq!(*placement, Placement::Factory, "easy earns and does nothing else");
        for &(r, c) in cells {
            assert!(
                crate::net::may_place_under(&world, me, r, c, &rules),
                "({r}, {c}) is not ours"
            );
            assert!(!world.cell_at(r, c).unwrap().is_alive(), "({r}, {c}) is the block");
        }
        assert!(bot.laid.len() == 1, "a factory laid is a factory remembered");
    }

    /// With nothing in hand there is nothing to lay, and the bot says so
    /// rather than sending an action the server would refuse. True of a
    /// search as well, which must not spend a rollout on a placement it
    /// cannot pay for.
    #[test]
    fn a_bot_with_no_money_chooses_nothing() {
        let me = PlayerId(1);
        let world = granted(me);
        for driver in [Driver::Book, searching()] {
            let mut bot = Bot::new(Level::Hard, driver, 0, me, 0);
            assert!(bot.choose(&world, &Rules::default(), me, 0, 0).is_none());
        }
        let counted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut bot =
            Bot::new(Level::Hard, Driver::Search(Box::new(Counting(counted.clone()))), 0, me, 0);
        assert!(bot.choose(&world, &Rules::default(), me, 0, 0).is_none());
        assert_eq!(counted.load(Ordering::Relaxed), 0, "a broke bot rolled a world forward");
    }

    /// **A search chooses between the book's offers and never outside them**,
    /// which is what keeps everything the book refuses out of reach of the
    /// thing that does the choosing.
    #[test]
    fn a_search_lays_only_what_the_book_would_have_offered() {
        let me = PlayerId(1);
        let world = granted(me);
        let rules = Rules::default();
        let mut bot = Bot::new(Level::Normal, searching(), 11, me, 0);
        let laid = bot.choose(&world, &rules, me, 1000, 0).expect("nowhere to build on a grant");

        // The same generator, at the same tick, off the same dice.
        let roll = Roll::new(mix(mix(11, me.0 as u64), 0));
        let ground = Ground::around(&world, &rules, me);
        let mut book: Vec<Action> = Vec::new();
        for (shapes, placement, frontier) in
            [(&EARNERS[..], Placement::Factory, false), (&HOLDERS[..], Placement::Life, true)]
        {
            let offers = ground.offers(&roll, shapes, placement, frontier, NORMAL_TRIES);
            book.extend(offers.into_iter().map(|(action, _)| action));
        }
        assert!(book.contains(&laid), "{laid:?} is nothing the book offered");
    }

    /// **A search chooses what to lay as well as where.** Every shape samples
    /// the same squares, so a budget pooled across them is a budget the first
    /// one spends — and a search that can only pick a square is half a search.
    #[test]
    fn what_a_search_is_offered_is_more_than_one_shape() {
        let me = PlayerId(1);
        let world = granted(me);
        let rules = Rules::default();
        let ground = Ground::around(&world, &rules, me);
        let offers =
            ground.offers(&Roll::new(mix(3, 0)), &EARNERS, Placement::Factory, false, HARD_TRIES);
        let shapes: std::collections::BTreeSet<usize> = offers
            .iter()
            .filter_map(|(_, note)| match note {
                Note::Laid(laid) => Some(laid.shape),
                _ => None,
            })
            .collect();
        assert!(shapes.len() > 1, "every one of {} offers was the same shape", offers.len());
    }

    /// A level is how deep it looks as well as how often it acts, and the
    /// budget is a cap rather than a target: however many squares the book
    /// offers, an act rolls the world forward at most [`Level::tries`] times.
    #[test]
    fn a_search_rolls_the_world_forward_no_more_often_than_its_level_allows() {
        let me = PlayerId(1);
        let world = granted(me);
        for level in Level::ALL {
            let counted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let judge = Driver::Search(Box::new(Counting(counted.clone())));
            let mut bot = Bot::new(level, judge, 5, me, 0);
            bot.choose(&world, &Rules::default(), me, 10_000, 0);
            let ran = counted.load(Ordering::Relaxed);
            assert!(ran <= level.tries(), "{} ran {ran} rollouts", level.name());
        }
    }

    /// **A search with money in hand actually chooses.** A budget is a cap
    /// and a cap is met by offering nothing, so the test above passes on a
    /// search that never chooses at all; this is the other side of it — an
    /// act with a purse rolls the world forward more than once, which is the
    /// only condition under which a rollout is worth what it costs.
    #[test]
    fn an_act_with_money_in_hand_tries_more_than_one_placement() {
        let me = PlayerId(1);
        let world = granted(me);
        let counted = std::sync::Arc::new(AtomicUsize::new(0));
        let judge = Driver::Search(Box::new(Counting(counted.clone())));
        let mut bot = Bot::new(Level::Normal, judge, 5, me, 0);
        bot.choose(&world, &Rules::default(), me, 1000, 0);
        let ran = counted.load(Ordering::Relaxed);
        assert!(ran >= 2, "an act with a thousand in hand rolled the world forward {ran} times");
    }

    /// **The cheapest end-to-end assertion there is**: a seat the server
    /// searches for, against a seat nobody plays, on one board through the
    /// same `act` a client's placement goes through. If the loop works at
    /// all, the ground says so.
    #[test]
    fn a_searching_bot_beats_a_seat_that_never_plays() {
        let mut server = crate::server::Server::new(World::infinite_empty());
        let bot = server.add_bot("searcher", Level::Normal, searching(), None).unwrap();
        let idle = server.join_with("idle", None).unwrap();
        for _ in 0..120 {
            server.step();
        }
        let held = server.territory();
        assert!(
            held[bot.0 as usize] > held[idle.0 as usize],
            "the search held {} against {} doing nothing",
            held[bot.0 as usize],
            held[idle.0 as usize]
        );
    }

    /// A hard bot walls what it built: once a factory is standing, some act
    /// soon after is ice on the ring round it and nowhere else.
    #[test]
    fn a_hard_bot_ices_the_ring_round_a_factory_it_laid() {
        let me = PlayerId(1);
        let mut world = granted(me);
        let rules = Rules::default();
        let mut bot = Bot::new(Level::Hard, Driver::Book, 7, me, 0);
        let first = bot.choose(&world, &rules, me, 1000, 0).expect("nowhere to build");
        let stamped = Stamped { tick: 0, player: me, seat: me, action: first.clone() };
        crate::net::apply(&mut world, &stamped);
        let Action::Paint { placement: Placement::Factory, cells: laid } = &first else {
            panic!("the first act was not a factory: {first:?}")
        };

        let mut iced = None;
        for tick in 1..64 {
            if let Some(Action::Paint { cells, placement: Placement::Ice }) =
                bot.choose(&world, &rules, me, 1000, tick)
            {
                iced = Some(cells);
                break;
            }
        }
        let ring = iced.expect("a hard bot never iced anything");
        for &(r, c) in &ring {
            assert!(!laid.contains(&(r, c)), "ice on its own factory at ({r}, {c})");
            let near = laid.iter().any(|&(lr, lc)| (lr - r).abs() <= 2 && (lc - c).abs() <= 2);
            assert!(near, "({r}, {c}) is nowhere near the factory");
        }
    }

    /// The frontier is where somebody else's ground is, and a normal bot
    /// puts plain life there rather than factories.
    #[test]
    fn contested_ground_gets_life_and_quiet_ground_gets_factories() {
        let (me, them) = (PlayerId(1), PlayerId(2));
        let mut world = granted(me);
        let (row, col) = crate::net::spawn_for(me, &world);
        // Their country, hard up against the top of our patch.
        for r in row - 8..row - 2 {
            for c in col - 8..col + crate::net::SPAWN_N + 8 {
                world.set_cell_at(r, c, Cell::DEAD.with_player(them).with_home(true));
            }
        }
        let rules = Rules::default();
        let ground = Ground::around(&world, &rules, me);
        let blinker = &BOOK[0];
        assert!(ground.frontier((row, col + 4), blinker), "the top edge is not the frontier");
        assert!(!ground.frontier((row + 8, col + 4), blinker), "the middle is");

        // Their band ends two rows above the patch, so a shape is on the
        // frontier when its top is within `FRONTIER` of that and off it
        // otherwise -- which, for anything placeable at all, is a row.
        let mut bot = Bot::new(Level::Normal, Driver::Book, 3, me, 0);
        let mut seen = std::collections::BTreeSet::new();
        for tick in 0..96 {
            if let Some(Action::Paint { cells, placement }) =
                bot.choose(&world, &rules, me, 1000, tick)
            {
                let top = cells.iter().map(|&(r, _)| r).min().unwrap();
                match placement {
                    Placement::Factory => {
                        assert!(top >= row + 4, "a factory on the frontier, at row {top}")
                    }
                    Placement::Life => {
                        assert!(top <= row + 5, "life laid nowhere near anybody, at row {top}")
                    }
                    other => panic!("normal does not lay {other:?}"),
                }
                seen.insert(placement.cost());
            }
        }
        assert_eq!(seen.len(), 2, "a normal bot both earns and contests");
    }
}
