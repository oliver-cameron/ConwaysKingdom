//! The player with no socket.
//!
//! A bot is a [`Player`] like anybody else — a number, a purse, a patch of
//! granted ground — with nothing on the other end of it. The server makes up
//! its mind for it in [`Server::step`], wraps the answer in a [`Stamped`] and
//! feeds it through [`Server::act`], so it reaches every client as an
//! ordinary action and can do nothing a client could not. Nothing about the
//! protocol changes for it beyond the lobby saying which seats are bots.
//!
//! **Its play is a book, not a search.** `examples/balance` measured what the
//! economy rewards — a blinker pays, a glider bleeds, sprawl bleeds badly — so
//! a competent bot is a small book of shapes and a rule about where to put
//! them: compact oscillators inside its own ground to earn, life at the
//! frontier where ground is contested, and ice around what it wants to keep.
//! Difficulty is two dials rather than an algorithm: how often it acts, and
//! what it will do. A bot that *chooses* — tries a placement on a copy of the
//! world and scores what happened — is the second version, and `World: Clone`
//! is there for it; see planned.md.
//!
//! Determinism is not a problem, which is worth saying because it looks like
//! one: a choice is made once, on the server, and reaches every peer as an
//! action at a stated tick. The dice here are its own — the room's seed, the
//! seat and the tick through [`crate::sim::mix`] — and never touch the
//! streams [`crate::sim::seed`] rolls for a cell.
//!
//! [`Player`]: crate::sim::Player
//! [`Server::step`]: crate::server::Server::step
//! [`Server::act`]: crate::server::Server::act

use crate::net::{Action, Level, Placement, Rules, Stamped, Tick};
use crate::sim::{mix, Kind, PlayerId, Roll, World};

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

/// How near somebody else's ground makes a square the frontier, in cells.
const FRONTIER: i32 = 6;

/// Who is driving the seat.
pub enum Driver {
    /// The server, from [`BOOK`].
    Book,
    /// Something outside, through `server::api`. Its actions are priced the
    /// moment they arrive, so there is nothing to do here at a step.
    External,
}

/// One seat the server plays.
pub struct Bot {
    pub level: Level,
    pub driver: Driver,
    /// The generation it next acts on.
    pub next_at: Tick,
    /// The room's seed and the seat, mixed once; the tick goes in per act.
    dice: u64,
    /// Factories it has laid, so `Keep` has something to come back to. At
    /// most [`REMEMBERED`] of them.
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

/// How many factories a bot remembers laying.
///
/// **A bound, because only Hard walks the list.** `keep` prunes as it goes,
/// and it is the one intent the other two levels never pick — so an easy bot
/// laying a factory every few hundred generations grew this by one entry
/// apiece for as long as its room was open. The oldest goes: a factory old
/// enough to fall off has died or been walled long since, and `keep` starts
/// from the oldest anyway.
const REMEMBERED: usize = 32;

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
    /// The intents its level allows, from one the dice pick, and the first
    /// that finds an affordable placement wins. Every placement is tested cell
    /// by cell with [`crate::net::may_place_under`] and priced with
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
        let roll = Roll::new(mix(self.dice, tick));
        let intents = self.level.intents();
        let first = roll.pick(0, intents.len());
        let ground = Ground::around(world, rules, plays_as);
        for k in 0..intents.len() {
            let intent = intents[(first + k) % intents.len()];
            let found = match intent {
                Intent::Earn => ground.place(&roll, &EARNERS, Placement::Factory, false),
                Intent::Contest => ground.place(&roll, &HOLDERS, Placement::Life, true),
                Intent::Keep => self.keep(&ground),
            };
            let Some((action, note)) = found else { continue };
            let stamped = Stamped { tick, player: plays_as, seat: plays_as, action };
            if purse + crate::net::price_under(world, &stamped, rules) < 0 {
                continue;
            }
            // Remembered only once it is affordable, or a thin purse would
            // write a factory off as walled without a pane ever going down.
            match note {
                Note::Laid(laid) => {
                    self.laid.push(laid);
                    if self.laid.len() > REMEMBERED {
                        self.laid.remove(0);
                    }
                }
                Note::Iced(i) => self.laid[i].iced = true,
                Note::Nothing => {}
            }
            return Some(stamped.action);
        }
        None
    }

    /// Ice round the oldest factory that is still standing and not yet iced.
    /// One that has died is forgotten rather than walled; one with nowhere
    /// left to put a pane counts as walled.
    fn keep(&mut self, ground: &Ground<'_>) -> Option<(Action, Note)> {
        while let Some(i) = self.laid.iter().position(|l| !l.iced) {
            let Laid { at, shape, .. } = self.laid[i];
            if !ground.standing(at, &BOOK[shape]) {
                self.laid.remove(i);
                continue;
            }
            let ring = ground.ring(at, &BOOK[shape]);
            if ring.is_empty() {
                self.laid[i].iced = true;
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
        let shape = shapes[roll.pick(1, shapes.len())];
        let pattern = &BOOK[shape];
        for k in 0..SAMPLES as u64 {
            let reach = if k < SAMPLES as u64 / 2 { NEAR } else { REACH };
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
            return Some((Action::Paint { cells, placement }, note));
        }
        None
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

    fn granted(me: PlayerId) -> World {
        let mut world = World::infinite_empty();
        crate::net::grant(&mut world, me);
        world
    }

    /// Every shape in the book is what it says: the cells sit inside the span
    /// it declares, so the margin kept clear round one is round all of it.
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

    /// **What a bot remembers is bounded.** `keep` is the only thing that
    /// prunes the list and it is the one intent an easy bot never picks, so
    /// this used to grow by an entry a factory for as long as the room was
    /// open — a room that runs for a week is a list nobody ever reads to the
    /// end of.
    #[test]
    fn what_a_bot_remembers_is_bounded_at_every_level() {
        let me = PlayerId(1);
        let mut world = granted(me);
        let rules = Rules::default();
        let mut bot = Bot::new(Level::Easy, Driver::Book, 0, me, 0);
        for tick in 0..REMEMBERED as Tick * 4 {
            // A purse deep enough that nothing is refused for want of money,
            // and the world left as it was so every act finds somewhere.
            if let Some(action) = bot.choose(&world, &rules, me, 100_000, tick) {
                crate::net::apply(&mut world, &Stamped { tick, player: me, seat: me, action });
            }
        }
        assert!(bot.laid.len() > 1, "the bot laid nothing, so this proves nothing");
        assert!(bot.laid.len() <= REMEMBERED, "{} factories remembered", bot.laid.len());
    }

    /// With nothing in hand there is nothing to lay, and the bot says so
    /// rather than sending an action the server would refuse.
    #[test]
    fn a_bot_with_no_money_chooses_nothing() {
        let me = PlayerId(1);
        let world = granted(me);
        let mut bot = Bot::new(Level::Hard, Driver::Book, 0, me, 0);
        assert!(bot.choose(&world, &Rules::default(), me, 0, 0).is_none());
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
