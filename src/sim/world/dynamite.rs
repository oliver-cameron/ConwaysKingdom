//! The dynamite pass, and the ice it breaks.
//!
//! One pass in the docs and one file here: a stick goes off at the top of a
//! generation and panes shatter at the bottom of it, and both are searches
//! over an area that no halo of eight neighbours could answer. The reasoning
//! is on [`World::detonate`] and [`World::break_ice_from`]; the order the two
//! run in, and why, is on [`World::step`].

use std::collections::HashSet;

use super::{Cell, Dir, Kind, PlayerId, Roll, World, CHUNK_N};
use crate::sim::rule;
use crate::sim::{bits, seed};

/// **A blast that went off**, for whoever is drawing rather than for the rule.
///
/// Where and how big, which is everything an effect needs and nothing the
/// simulation reads back. `by` is whose it was, so the fireball can be their
/// colour rather than a colour the interface chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Blast {
    pub at: (i32, i32),
    pub reach: i32,
    pub by: PlayerId,
}

impl World {
    /// Break every pane that life has reached.
    ///
    /// Any live cell in the eight neighbours breaks a pane — placed or born,
    /// whoever owns it — and takes the whole connected run of ice with it,
    /// because a pane is one object and cracking a corner of it does not leave
    /// the rest standing.
    ///
    /// One exception: a cell that is itself under ice. It is frozen, and a
    /// pane must not be broken by what it covers, or none could be laid over
    /// life at all.
    ///
    /// Connectivity is orthogonal. Panes are laid as rectangles, and two that
    /// meet only at a corner are two panes rather than one; joining them
    /// diagonally would let a break travel between panes that merely touch.
    ///
    /// Run after the rules, so it sees the generation that actually did the
    /// touching. Absolute coordinates throughout, so a pane spanning chunks
    /// breaks as one rather than stopping at a boundary.
    pub(super) fn ice_seeds(&self) -> Vec<(i32, i32)> {
        // Life reaches diagonally, so a pane is touched by any of the eight.
        self.ice_cells()
            .into_iter()
            .filter(|&(row, col)| {
                Dir::ALL.iter().any(|dir| {
                    let (dr, dc) = dir.delta();
                    self.cell_at(row + dr, col + dc).is_some_and(|c| c.is_alive() && !c.is_ice())
                })
            })
            .collect()
    }

    /// Set off every dynamite whose fuse has run out, and scramble the ground
    /// around each one.
    ///
    /// **A pass and not a rule**, because "every square within reach" is not a
    /// question a halo of eight neighbours can answer — the same reason
    /// [`Self::fire_turrets`] and [`Self::break_ice_from`] are passes. What
    /// makes this one cheap enough to be one is that it is **one roll per
    /// square**: `sim::seed` is already a stream per cell per generation that
    /// two peers agree on without exchanging anything, so a probability does
    /// directly what a scoring function would have manufactured.
    ///
    /// It runs at the **top** of the generation, which the other two do not.
    /// That is the warning: a fuse reaches full during one generation's rule,
    /// is drawn full for that whole generation, and goes off at the start of
    /// the next.
    pub(super) fn detonate(&mut self) {
        self.detonate_with(rule::DYNAMITE_DENSITY, rule::DYNAMITE_REACH);
    }

    /// Take what went off, leaving nothing behind: a blast is drawn once.
    pub fn take_blasts(&mut self) -> Vec<Blast> {
        std::mem::take(&mut self.blasts)
    }

    /// [`Self::detonate`] with the two numbers `examples/blast` sweeps — what
    /// a disc comes up alive at, out of sixty-four, and how far one stick
    /// reaches — so a density or a reach can be measured without editing
    /// `rule.rs` between runs. The game never calls it: [`Self::step`] goes
    /// through the constants.
    pub fn detonate_with(&mut self, density: u64, reach: i32) {
        let ready = self.dynamite_ready();
        if ready.is_empty() {
            return;
        }
        let generation = seed::generation_seed(self.seed, self.generation);

        // Gathered before anything is written, so a blast does not scramble
        // the ground the next one is deciding where to land on. Two peers
        // stepping the same generation must make the same choices, and a
        // choice that depended on which dynamite was handled first would
        // depend on the iteration order of a map.
        // **A blob of them is one bomb, not many.** Dynamite standing in each
        // other's disc go off as a single, larger one — see
        // [`rule::blast_reach`], where each is worth a constant area — so a
        // hundred of them reach ten times as far as one rather than a hundred
        // small craters in the same place.
        let mut blasts = Vec::new();
        for group in clusters(&ready, reach) {
            let reach = rule::blast_reach_from(reach, group.len());
            let owner = ready[group[0]].1;
            // The middle of the blob, which is where a bomb made of all of
            // them is. Integer division, so it lands on a square.
            let (rows, cols): (i32, i32) =
                group.iter().fold((0, 0), |(r, c), &i| (r + ready[i].0 .0, c + ready[i].0 .1));
            let at = (rows / group.len() as i32, cols / group.len() as i32);
            let seed = seed::cell_seed(generation, at.0, at.1);
            blasts.push((self.blast_centre(at, owner, seed, reach), owner, seed, reach));
        }

        // Every dynamite that went off is consumed, whichever blast it was
        // part of — the first blast's seed decides them all, which is one roll
        // per square either way.
        let seed_for = blasts.first().map(|&(_, _, seed, _)| seed).unwrap_or(generation);
        for ((row, col), owner) in &ready {
            // **Consumed, and it takes the same roll as the ground it threw.**
            // Left alive it is a cell standing in the middle of noise that
            // nothing else in the blast could have produced, which reads as a
            // survivor rather than as a crater; left dead it is a hole in the
            // same way. So it comes up alive or dead on its own square's own
            // roll, exactly like everything else the blast touched.
            let cell = self.cell_at(*row, *col).unwrap_or(Cell::DEAD);
            self.set_cell_at(
                *row,
                *col,
                Self::blasted(cell, *owner, seed_for, density, *row, *col).with_age(0),
            );
        }

        for (centre, owner, seed, reach) in blasts {
            // **Reported, because nothing else says it happened.** A blast is
            // a generation in which a disc of ground quietly becomes
            // different: the cells before and after are both just cells, so
            // the largest thing a player can do reads as the board having
            // glitched. Whoever is drawing takes these; the rule does not care.
            self.blasts.push(Blast { at: centre, reach, by: owner });
            self.scramble(centre, owner, seed, density, reach);
        }
    }

    /// Turn a disc of ground into noise, and light every dynamite it reaches.
    ///
    /// **The blast decides whose noise it is**, which is the whole of what a
    /// dynamite buys. Every square it reaches is re-rolled: the roughly one in
    /// three that comes up alive is *yours*, and the rest is reset to
    /// no-man's-land. So a bomb does not merely animate what was already
    /// there — it breaks a country apart and leaves you a third of the pieces.
    ///
    /// It used to leave the owner alone and set only alive or dead, which
    /// meant a blast into somebody's empty ground **manufactured life for
    /// them**: a disc of theirs at [`rule::DYNAMITE_DENSITY`] where there had
    /// been nothing, on ground they still held. Aimed at an empty frontier a
    /// dynamite was a gift.
    ///
    /// Two squares are left alone. **Ice**, because a pane stops time over
    /// whatever it covers and that is every rule. And **granted ground** —
    /// see [`Cell::is_home`] — which no rule moves: [`rule::territory`] returns
    /// before it, so a home square only ever changes hands by being written,
    /// and `net::already_granted` reads exactly that to keep a returning
    /// player's seat. A blast that took one would evict somebody from their
    /// spawn permanently and hand them a second patch on their next join. Life
    /// standing on it is still scrambled; the owner is not.
    ///
    /// Ground nobody has loaded is not a third. An infinite world holds only
    /// the chunks something has touched, and a disc that ran past them used to
    /// be scrambled on one side and left alone on the other — the same stick
    /// did half as much at a chunk corner as in the middle of one. An absent
    /// chunk reads as dead and nobody's, the way [`Self::turret_wants`] reads
    /// it, and writing there is what loads it.
    pub(super) fn scramble(
        &mut self,
        centre: (i32, i32),
        owner: PlayerId,
        seed: u64,
        density: u64,
        reach: i32,
    ) {
        let mut chained = Vec::new();
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                if dr * dr + dc * dc > reach * reach {
                    continue;
                }
                let (row, col) = (centre.0 + dr, centre.1 + dc);
                let cell = self.cell_at(row, col).unwrap_or(Cell::DEAD);
                if cell.is_ice() {
                    continue;
                }
                // **The chain, and it cannot recurse.** A dynamite in the blast
                // has its fuse set to full, so it goes off at the top of the
                // *next* generation — a line of them is a fuse and a cluster
                // is one ring a generation, rather than one pass re-entering
                // itself.
                if cell.kind() == Kind::DYNAMITE && cell.is_alive() {
                    chained.push(((row, col), cell.with_age(bits::MAX_AGE)));
                    continue;
                }
                self.set_cell_at(row, col, Self::blasted(cell, owner, seed, density, row, col));
            }
        }
        for ((row, col), cell) in chained {
            self.set_cell_at(row, col, cell);
        }
    }

    /// What a blast leaves on one square, which is the whole of what a dynamite
    /// does to the board.
    ///
    /// One roll, and it decides ownership as well as life: alive is *yours*,
    /// dead is nobody's. That is what makes a bomb take ground rather than
    /// only stir it, and it is deliberately the same roll for both — a square
    /// that came up alive for you and stayed somebody else's would be a live
    /// cell of theirs standing in your crater, which is the state this whole
    /// change is about.
    ///
    /// **Full strength when it lives**, because [`Cell::alive`] is: level and
    /// influence have to agree on a source, and a corpse owned at level nought
    /// is a state the rule says cannot exist.
    ///
    /// **Level nought and nobody when it does not.** Ground with an owner and
    /// no strength is the same impossible state from the other side, so the
    /// two move together.
    ///
    /// Granted ground keeps its owner whatever the roll says — see
    /// [`Self::scramble`] for why nothing may move one.
    pub(super) fn blasted(
        cell: Cell,
        owner: PlayerId,
        seed: u64,
        density: u64,
        row: i32,
        col: i32,
    ) -> Cell {
        // Its own square's own roll, on the blast's own stream, so two
        // overlapping blasts do not decide the same square twice the same way
        // — and so a peer that never saw the dynamite placed still lands on the
        // same board.
        let square = seed::cell_seed(seed, row, col);
        let alive = Roll::new(square).chance(rule::BLAST_STREAM, density);
        // **The age goes with the kind.** A factory three quarters of the way
        // through its rot, turned into ordinary ground, kept that three — and
        // `Cell::sprite` reads the age as a sheet row, so it drew from a row
        // that only ageing kinds have art in and came out as nothing at all.
        // `Kind::NORMAL` is `Ages::Never`, so nought is the only age it has.
        let cell = cell.with_kind(Kind::NORMAL).with_age(0);
        if cell.is_home() {
            // **Cleared, not scrambled.** Its owner cannot move, so a square
            // that came up alive here would be alive *for them* — which is
            // the gift this whole rule exists to stop, and a spawn is exactly
            // where somebody would aim to exploit it. So the blast may only
            // take life off a granted patch, never put it there.
            return cell.with_alive(false);
        }
        if alive {
            cell.with_player(owner).with_alive(true).with_level(bits::MAX_LEVEL)
        } else {
            cell.with_alive(false).with_player(PlayerId::UNOWNED).with_level(0)
        }
    }

    /// Break every pane reached from these cells, and everything each pane is
    /// joined to.
    pub(super) fn break_ice_from(&mut self, seeds: Vec<(i32, i32)>) {
        if seeds.is_empty() {
            return;
        }

        let mut broken: HashSet<(i32, i32)> = HashSet::new();
        let mut queue = seeds;
        while let Some(at) = queue.pop() {
            if !broken.insert(at) {
                continue;
            }
            for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let next = (at.0 + dr, at.1 + dc);
                if !broken.contains(&next)
                    && self.cell_at(next.0, next.1).is_some_and(|c| c.is_ice())
                {
                    queue.push(next);
                }
            }
        }

        for (row, col) in broken {
            if let Some(cell) = self.cell_at(row, col) {
                self.set_cell_at(row, col, cell.with_ice(false));
            }
        }
    }

    /// **Where a blast is worth setting off**, walking outward from the
    /// dynamite until it finds somewhere.
    ///
    /// A blast wasted on its owner's own ground is a blast wasted: a
    /// detonation inside your own country turns your own patterns into your
    /// own noise. So this searches rings at increasing distance for a centre
    /// whose disc is at least [`rule::DYNAMITE_FOREIGN`] not its owner's, takes
    /// the nearest, and breaks a tie with a seeded roll — which is
    /// [`Self::turret_target`] again, in shape: the nearest square answering a
    /// question, with the tie broken so a volley does not always favour one
    /// direction.
    ///
    /// What that buys is that **a dynamite does not have to be placed
    /// exactly.** Placing is confined to your own influence, so without it the
    /// only useful dynamite is one laid on the exact square of your border
    /// nearest something worth hitting — a precision the interface does not
    /// support, against a frontier that moves every generation.
    ///
    /// Bounded by [`rule::DYNAMITE_THROW`], and it goes off where it stands if
    /// nothing within that is better. Unbounded it would be a homing weapon
    /// with a range of the whole world.
    pub(super) fn blast_centre(
        &self,
        at: (i32, i32),
        owner: PlayerId,
        seed: u64,
        reach: i32,
    ) -> (i32, i32) {
        let throw = rule::DYNAMITE_THROW;
        // Ring by ring, so the first distance that has any answer is the one
        // taken and a dynamite on its own frontier stops at once. The worst
        // case — one in the middle of a large country — is what the bound is
        // for.
        for ring in 0..=throw {
            let mut ties = 0usize;
            let walk = |count: &mut usize, want: usize| -> Option<(i32, i32)> {
                for dr in -ring..=ring {
                    for dc in -ring..=ring {
                        // The ring and not the disc: the box's inside was
                        // covered by an earlier, nearer ring.
                        if dr.abs().max(dc.abs()) != ring {
                            continue;
                        }
                        let centre = (at.0 + dr, at.1 + dc);
                        if !self.worth_hitting(centre, owner, reach) {
                            continue;
                        }
                        if *count == want {
                            return Some(centre);
                        }
                        *count += 1;
                    }
                }
                None
            };
            walk(&mut ties, usize::MAX);
            if ties == 0 {
                continue;
            }
            let pick = Roll::new(seed).pick(rule::THROW_STREAM, ties);
            let mut n = 0;
            if let Some(found) = walk(&mut n, pick) {
                return found;
            }
        }
        at
    }

    /// Whether a blast centred here would be worth setting off.
    ///
    /// **Ground that is not already yours**, which now includes no-man's-land.
    ///
    /// This counted somebody *else's* ground and skipped the empty kind, on
    /// the reasoning that a blast over no-man's-land does nothing to anybody.
    /// That was true while a blast only disturbed what it reached. It claims
    /// what it reaches now — see [`Self::scramble`] — so open country is worth
    /// hitting, and refusing to go off over it would leave a dynamite unable to
    /// do the thing it was just given.
    ///
    /// What that re-admits is the crater loop the old rule was written to
    /// stop: the debris of a blast is mostly unowned, so one can be aimed at
    /// the last one's hole. It costs [`rule::DYNAMITE_COST`] a time and pays a
    /// third of a disc, which is a worse rate than any of the ordinary ways to
    /// hold ground, so it is priced out rather than ruled out.
    ///
    /// A count and not a cost, and it stops the moment it has seen enough — so
    /// a dynamite on a frontier answers in a handful of reads. Ground nobody
    /// has loaded is nobody's and counts: it used to count for nothing, so a
    /// stick at the edge of what was stored would not walk toward the open
    /// country the rule says is worth hitting.
    pub(super) fn worth_hitting(&self, centre: (i32, i32), owner: PlayerId, reach: i32) -> bool {
        let mut theirs = 0u64;
        // How many squares of a disc this radius holds, so the threshold is a
        // fraction of the disc rather than of the box around it.
        let total: u64 = (-reach..=reach)
            .flat_map(|dr| (-reach..=reach).map(move |dc| (dr, dc)))
            .filter(|(dr, dc)| dr * dr + dc * dc <= reach * reach)
            .count() as u64;
        for dr in -reach..=reach {
            for dc in -reach..=reach {
                if dr * dr + dc * dc > reach * reach {
                    continue;
                }
                let there = self.cell_at(centre.0 + dr, centre.1 + dc).unwrap_or(Cell::DEAD);
                if there.player() != owner {
                    theirs += 1;
                    if theirs * 64 >= total * rule::DYNAMITE_FOREIGN {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Every dynamite whose fuse has run out, sorted, so two peers set them off
    /// in the same order.
    fn dynamite_ready(&self) -> Vec<((i32, i32), PlayerId)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen dynamite does not go off: a pane stops time over
                    // whatever it covers, and that is every rule.
                    if cell.kind() != Kind::DYNAMITE || cell.is_ice() {
                        continue;
                    }
                    if !cell.is_alive() || cell.age() < bits::MAX_AGE {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        (crow * CHUNK_N as i32 + row as i32, ccol * CHUNK_N as i32 + col as i32),
                        cell.player(),
                    ));
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Every iced cell, in absolute coordinates.
    fn ice_cells(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    if chunk[(row, col)].is_ice() {
                        out.push((
                            crow * CHUNK_N as i32 + row as i32,
                            ccol * CHUNK_N as i32 + col as i32,
                        ));
                    }
                }
            }
        }
        out
    }
}

/// Which dynamite go off together: the groups standing in each other's disc.
///
/// **Two within one reach of each other are one bomb** — nearer than discs
/// merely overlapping, which would join a pair two reaches apart. Otherwise a
/// blob of them is a hundred craters in the same place, each doing again what
/// the last already did — where what a player built is one charge made of a
/// hundred, and [`rule::blast_reach`] is what that is worth.
///
/// Connected by distance and transitively, so a line of dynamite is one long
/// bomb rather than a chain of pairs. `O(n²)` over the ones that are *ready*,
/// which is a handful in the generations it is not nought.
///
/// The order is the order they came in, which is sorted — so two peers group
/// them identically without exchanging anything.
pub(super) fn clusters(ready: &[((i32, i32), PlayerId)], reach: i32) -> Vec<Vec<usize>> {
    let mut group: Vec<Option<usize>> = vec![None; ready.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..ready.len() {
        if group[i].is_some() {
            continue;
        }
        let g = out.len();
        out.push(vec![i]);
        group[i] = Some(g);
        // Grown rather than scanned once: reaching a dynamite can bring in
        // others only that one is near, which is what makes a line one bomb.
        let mut k = 0;
        while k < out[g].len() {
            let (at, _) = ready[out[g][k]];
            for j in 0..ready.len() {
                if group[j].is_some() {
                    continue;
                }
                let (dr, dc) = (ready[j].0 .0 - at.0, ready[j].0 .1 - at.1);
                if dr * dr + dc * dc <= reach * reach {
                    group[j] = Some(g);
                    out[g].push(j);
                }
            }
            k += 1;
        }
    }
    out
}
