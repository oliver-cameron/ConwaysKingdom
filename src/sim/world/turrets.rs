//! The turret pass: every turret takes the nearest square it acts on.
//!
//! Last of the three passes a generation runs outside the rule. Why a search
//! for "the nearest square that is not mine" cannot be a rule, and why every
//! turret reads the world as the generation left it, is on
//! [`World::fire_turrets`].

use super::{Cell, Kind, PlayerId, Roll, World, CHUNK_N};
use crate::sim::rule;
use crate::sim::seed;

impl World {
    /// Every turret takes the nearest square it acts on, all at once.
    ///
    /// A pass rather than a rule, and for the same reason shattering ice is
    /// one: every rule in [`super::rule`] is a pure function of a cell and its
    /// eight neighbours, which is what lets a generation run out of a `Halo`
    /// with no bounds checks and no knowledge of topology. "The nearest square
    /// that is not mine" is a search no halo can answer.
    ///
    /// **Searched first, applied second**, which is the same discipline as
    /// gathering every halo before writing any of the next generation. Every
    /// turret reads the world as this generation left it, so no turret's
    /// answer depends on which turret went first — two aiming at one square
    /// simply agree or overwrite, and the list is sorted, so which of them
    /// wins is the same on every peer.
    pub(super) fn fire_turrets(&mut self) {
        let turrets = self.turrets();
        if turrets.is_empty() {
            return;
        }

        // The same seed a cell's own rules roll from, at the same position
        // and generation. It does not have to be a different number, because a
        // turret asks on its own **stream** — which is what streams are for,
        // and is stronger than two constants nobody can check are unrelated.
        let generation = seed::generation_seed(self.seed, self.generation);

        let mut shots = Vec::new();
        for (at, owner, live) in turrets {
            let seed = seed::cell_seed(generation, at.0, at.1);
            let (targets, hit) = self.turret_targets(at, owner, live, seed);
            for &target in &targets[..hit] {
                let cell = self.cell_at(target.0, target.1).unwrap_or(Cell::DEAD);
                shots.push((
                    target,
                    if live {
                        // **Planted at full**, not nudged. The rule assigns a
                        // square the strongest claim reaching it rather than
                        // adding to what is there, so a push of three would be
                        // wiped the next time that square worked itself out --
                        // a turret that nudged would achieve nothing at all.
                        //
                        // Planting a flag instead is what a turret always did,
                        // and the level field gives it the brake the old one
                        // needed a constant for: a planted square with nothing
                        // of its owner's near enough to feed it falls back on
                        // its own, so what a turret holds is however much it
                        // can plant against however fast the rule takes it
                        // back.
                        cell.with_player(owner).with_level(rule::TURRET_PUSH)
                    } else {
                        // The mirror, and it takes the square to nothing in one
                        // go for the same reason: half-draining it would be
                        // undone before it mattered. A live cell must have an
                        // owner -- `Cell::alive` asserts it, because unowned
                        // life would have nobody to attribute a birth to -- so
                        // taking a square away from its owner kills whatever
                        // stood on it, which is why a dead turret kills
                        // without a rule about killing.
                        cell.with_alive(false).with_player(PlayerId::UNOWNED).with_level(0)
                    },
                ));
            }
        }

        for ((row, col), cell) in shots {
            self.set_cell_at(row, col, cell);
        }
    }

    /// The squares a turret acts on: the [`rule::TURRET_POWER`] nearest that
    /// answer its question, nearest first, and however many fewer it found.
    ///
    /// One search per square rather than one search for all of them, each
    /// excluding what the last took. Nearest-first falls out of that, and it
    /// costs a second walk of a box already in cache — where collecting the
    /// whole box and sorting it would allocate per turret per generation to
    /// answer a question about its first few entries.
    ///
    /// Each shot mixes its own index into the seed, so a volley does not break
    /// every tie the same way.
    fn turret_targets(
        &self,
        at: (i32, i32),
        owner: PlayerId,
        live: bool,
        seed: u64,
    ) -> ([(i32, i32); rule::TURRET_POWER], usize) {
        let mut chosen = [(0, 0); rule::TURRET_POWER];
        let mut hit = 0;
        // A live turret asks for ground that is not its owner's, and only when
        // there is none within reach does it fall back to reinforcing its own.
        // Falling back once rather than per shot, so a volley that ran out of
        // frontier finishes on the thin ground behind it.
        let mut aim = if live { Aim::Take } else { Aim::Give };
        while hit < rule::TURRET_POWER {
            let shot = seed::mix(seed, hit as u64);
            match self.turret_target(at, owner, aim, shot, &chosen[..hit]) {
                Some(next) => {
                    chosen[hit] = next;
                    hit += 1;
                }
                None if aim == Aim::Take => aim = Aim::Reinforce,
                None => break,
            }
        }
        (chosen, hit)
    }

    /// The square a turret acts on: the nearest one that answers its question
    /// and is not already `taken` by this volley, and one of them at random
    /// where several tie.
    ///
    /// The tie-break is the whole reason there is a roll here. A ring holds
    /// many squares at the same distance, and letting the scan order choose
    /// between them would have every turret in the world prefer the same
    /// direction — territory would grow in a lopsided plume that reads as a
    /// bug rather than as a rule.
    ///
    /// Two passes over the box rather than a list of candidates: the first
    /// finds the nearest distance and counts how many share it, the second
    /// walks to the one the roll picked. That costs a second read of a box
    /// that is already in cache and saves allocating per turret per
    /// generation.
    ///
    /// A disc, not a square. The box is what is walked, `d > reach²` is what
    /// makes the reach the same in every direction.
    pub(super) fn turret_target(
        &self,
        at: (i32, i32),
        owner: PlayerId,
        aim: Aim,
        seed: u64,
        taken: &[(i32, i32)],
    ) -> Option<(i32, i32)> {
        let reach = rule::TURRET_REACH;
        let mut best = i32::MAX;
        let mut ties = 0usize;
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                let d = dr * dr + dc * dc;
                if d == 0 || d > reach * reach || d > best {
                    continue;
                }
                if taken.contains(&(at.0 + dr, at.1 + dc)) {
                    continue;
                }
                if !self.turret_wants((at.0 + dr, at.1 + dc), owner, aim) {
                    continue;
                }
                if d < best {
                    best = d;
                    ties = 1;
                } else {
                    ties += 1;
                }
            }
        }
        if ties == 0 {
            return None;
        }

        let mut nth = Roll::new(seed).pick(rule::TURRET_STREAM, ties);
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                let d = dr * dr + dc * dc;
                if d != best {
                    continue;
                }
                let target = (at.0 + dr, at.1 + dc);
                if taken.contains(&target) || !self.turret_wants(target, owner, aim) {
                    continue;
                }
                if nth == 0 {
                    return Some(target);
                }
                nth -= 1;
            }
        }
        unreachable!("the second pass walks the same squares the first counted")
    }

    /// Whether a turret will act on this square, for the [`Aim`] it is asking
    /// with.
    ///
    /// **Dead squares only**, for both of a live turret's aims: claiming a
    /// living cell would hand its owner the cell itself rather than the square
    /// under it, there being one owner field, and territory has never worked
    /// that way. Unheld ground counts as dead and unowned, which is exactly
    /// what an absent chunk reads as and exactly what a turret is for
    /// reaching.
    ///
    /// A **dead** turret is the mirror and takes its owner's own squares,
    /// alive or not. `HOME` is exempt for the same reason it never decays: it
    /// is the ground its owner can still build on at the base rate, and a
    /// machine of theirs that failed should not be what takes that away.
    ///
    /// `HOME` is exempt from reinforcing too, and needs no arm saying so:
    /// granted ground is a source, so [`Cell::influence`] already reads it as
    /// full and there is nothing to top up.
    ///
    /// Ice is exempt from all three. A pane stops time over what it covers,
    /// and a pane's cover is not claimed out from under it.
    pub(super) fn turret_wants(&self, at: (i32, i32), owner: PlayerId, aim: Aim) -> bool {
        let cell = self.cell_at(at.0, at.1).unwrap_or(Cell::DEAD);
        if cell.is_ice() {
            return false;
        }
        match aim {
            Aim::Take => !cell.is_alive() && cell.player() != owner,
            Aim::Reinforce => {
                !cell.is_alive() && cell.player() == owner && cell.influence() < rule::TURRET_PUSH
            }
            Aim::Give => cell.player() == owner && !cell.is_home(),
        }
    }

    /// Every turret, in absolute coordinates, with its owner and whether it is
    /// alive.
    ///
    /// **Sorted**, because `stored` walks a `HashMap` on an infinite world and
    /// a `HashMap` iterates differently in different processes. Two turrets
    /// aiming at one square is decided by which fires last, so an unsorted
    /// list would let a client and a server disagree about who owns it.
    ///
    /// A scan rather than an index, the way `ice_cells` is. The world has no
    /// list of anything, and a turret is found by looking, which costs one
    /// pass over what is held per generation.
    pub(super) fn turrets(&self) -> Vec<((i32, i32), PlayerId, bool)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen turret does not fire: a pane stops time over
                    // whatever it covers, and that is every rule, not just the
                    // ones inside `rule`.
                    if cell.kind() != Kind::TURRET || cell.is_ice() {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        (crow * CHUNK_N as i32 + row as i32, ccol * CHUNK_N as i32 + col as i32),
                        cell.player(),
                        cell.is_alive(),
                    ));
                }
            }
        }
        out.sort_unstable();
        out
    }
}

/// What a turret is looking for on one shot.
///
/// A live turret has **two** aims and takes them in order, which is what
/// makes it work from the middle of a country as well as from its edge.
/// The rule here has never changed; the world around it did. Before
/// territory was a level, a player's ground was a tight halo, so ground
/// that was not theirs was within six cells of anywhere they would put a
/// turret. Now granted ground is a source and a country reaches much
/// further, so a turret standing inside one finds its whole disc already
/// owned and had nothing to do.
///
/// Reinforcing is strictly the fallback, and that is the whole of why it
/// is safe. Making it the only rule was tried when levels arrived and
/// quietly ruined the piece: influence falls off, so from the middle of a
/// country the nearest thin square is a step or two away and a turret
/// spent its life topping up ground it already held instead of pushing on
/// anybody. Asked second, it only ever fires when there was nobody to push
/// on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Aim {
    /// A live turret's first choice: ground that is not its owner's.
    Take,
    /// Its fallback: its owner's own thinnest ground, planted back up to
    /// full, which feeds the frontier through the sum rather than at it.
    Reinforce,
    /// A dead turret, running the first backwards.
    Give,
}
