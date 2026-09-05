//! The overclock pass: the rule run again over every overclocked disc.
//!
//! Third of the three passes a generation runs outside the rule, and the only
//! one inside [`World::step`] rather than at either end of it. What it is and
//! why a disc is not something eight neighbours can answer is on
//! [`World::overclock_pass`].

use std::collections::BTreeMap;

use super::{ChunkMask, Coord, Kind, Takings, World, CHUNK_N};
use crate::sim::rule;
use crate::sim::seed;

impl World {
    /// Run the rule again over every overclocked disc.
    ///
    /// **A pass and not a rule**, for the reason the turret's is one: a disc
    /// is not a question eight neighbours can answer. It runs after the whole
    /// world has stepped and before the generation is called done, so the
    /// generation stays the unit on the wire, in the save and in the digest —
    /// every peer runs the same passes and there is nothing new to agree
    /// about.
    ///
    /// The discs are found from the world **as the pass before left it**, so
    /// a machine that died this generation does not run again; and every halo
    /// is gathered before any cell is written, which is the discipline the
    /// first pass keeps and for the same reason. At the edge of a disc a
    /// masked cell reads neighbours the pass before left and this pass will
    /// not move, and an unmasked cell sees the disc's second state next
    /// generation: the inside runs twice as fast and the outside sees every
    /// other step of it. That is the whole of the border, and it is a hazard
    /// the way a pane's edge is rather than a bug — see [docs/simulation.md].
    ///
    /// The dice are [`seed::pass_seed`]'s. Handed the generation's own
    /// seed, a pass would roll every cell the identical dice twice.
    ///
    /// [docs/simulation.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/simulation.md#overclockers
    pub(super) fn overclock_pass(&mut self, generation: u64, pass: u64, earned: &mut Takings) {
        let masks = self.overclock_masks(&self.overclockers());
        if masks.is_empty() {
            return;
        }
        for &coord in masks.keys() {
            self.ensure(coord);
        }
        // The whole-world pass is done with its halos by now, so the scratch
        // is free and this allocates nothing either.
        self.scratch.clear();
        for &coord in masks.keys() {
            let halo = self.gather_halo(coord);
            self.scratch.push(halo);
        }
        let seed = seed::pass_seed(generation, pass);
        for (i, (&coord, mask)) in masks.iter().enumerate() {
            let halo = self.scratch[i];
            let at = (coord.0 * CHUNK_N as i32, coord.1 * CHUNK_N as i32);
            if let Some(chunk) = self.chunk_at_mut(coord) {
                halo.step_into_where(chunk, seed, at, earned, mask);
            }
        }
    }

    /// The cells every overclocker's disc covers, as a mask per chunk it
    /// touches.
    ///
    /// A `BTreeMap`, so the chunks come out sorted without a second pass, and
    /// a set of bits, so a cell two discs cover — or one a disc wraps onto on
    /// a small torus — is stepped once. Folded onto the chunks the world has
    /// as it goes, the way every absolute coordinate is.
    pub(super) fn overclock_masks(&self, at: &[(i32, i32)]) -> BTreeMap<Coord, ChunkMask> {
        let n = CHUNK_N as i32;
        let reach = rule::OVERCLOCK_REACH;
        let mut masks = BTreeMap::new();
        for &(row, col) in at {
            for dr in -reach..=reach {
                for dc in -reach..=reach {
                    // **A square, not a disc.** The region is the cell and the
                    // cells a birth from it can land on, and Conway reads a
                    // square of eight neighbours — a disc would leave the four
                    // corners of that out, which are birth sites like any
                    // other, and a shape would run twice on its sides and once
                    // at its corners.
                    if dr.abs().max(dc.abs()) > reach {
                        continue;
                    }
                    let (r, c) = (row + dr, col + dc);
                    let coord = self.canonical((r.div_euclid(n), c.div_euclid(n)));
                    masks
                        .entry(coord)
                        .or_insert(ChunkMask::NONE)
                        .set(r.rem_euclid(n) as usize, c.rem_euclid(n) as usize);
                }
            }
        }
        masks
    }

    /// Every live, ice-free overclocker, in absolute coordinates. Unsorted,
    /// unlike [`Self::turrets`]: a disc is a set of bits, so nothing about
    /// the pass depends on which was found first.
    pub(super) fn overclockers(&self) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for ((crow, ccol), chunk) in self.stored() {
            for row in 0..CHUNK_N {
                for col in 0..CHUNK_N {
                    let cell = chunk[(row, col)];
                    // A frozen one runs nothing: a pane stops time over
                    // whatever it covers, and that is every rule.
                    if cell.kind() != Kind::OVERCLOCK || cell.is_ice() || !cell.is_alive() {
                        continue;
                    }
                    if !cell.player().is_owned() {
                        continue;
                    }
                    out.push((
                        crow * CHUNK_N as i32 + row as i32,
                        ccol * CHUNK_N as i32 + col as i32,
                    ));
                }
            }
        }
        out
    }
}
