//! A patch to practise on, small enough to fit in a paragraph.
//!
//! **A region measured in cells, not chunks.** A `World` is built out of chunks
//! and carries residency, subscription, grants, ice seeding and a save format,
//! none of which a nine-by-nine square wants — so this is a flat `Vec<Cell>`
//! that wraps, and the only thing it borrows from the real game is the part
//! that matters: [`crate::sim::next_cell`], the rule itself. What a patch shows
//! happening is what would happen.
//!
//! **Optional by construction.** These sit in the how-to page's scroll view,
//! so somebody who wants to read five rules and leave never touches one, and
//! somebody who wants to find out what a factory does can find out in the place
//! where the question occurred to them rather than by losing a match.
//!
//! Each carries a **target**: the pattern it is asking for, drawn as an outline
//! on the empty squares so there is something to trace rather than a paragraph
//! saying "draw a blinker". Tracing it is the whole interaction.

use super::words;
use crate::client::views::icons::Icons;
use crate::client::views::theme::Theme;
use crate::client::views::words::w;
use crate::net::Placement;
use crate::sim::{next_cell, Cell, Kind, PlayerId};

/// Whose cells these are. Player one, always: a patch is somebody's first look
/// at the game and they have not been given a number yet, and the colour is
/// what the sprite sheet is tinted with.
const ME: PlayerId = PlayerId(1);

/// One practice patch.
pub struct Patch {
    /// Cells a side. **A patch wraps**, so a glider launched into one comes
    /// back rather than walking off the edge and leaving an empty square —
    /// which on a board this size is the difference between a demonstration
    /// and a blank.
    pub side: i32,
    cells: Vec<Cell>,
    /// The pattern this patch is asking for, as offsets from its middle.
    pub target: &'static [(i32, i32)],
    /// What a click puts down.
    pub place: Placement,
    /// What the cells here have earned, on the game's own rule for it.
    pub purse: i32,
    pub generation: u64,
    pub running: bool,
    /// When the last generation was taken, against the menu's clock.
    stepped_at: f64,
}

/// Seconds a generation, slower than the real game's quarter second: a patch is
/// something to watch happen rather than something to keep up with.
const SPAN: f64 = 0.4;

impl Patch {
    pub fn new(side: i32, place: Placement, target: &'static [(i32, i32)]) -> Self {
        Self {
            side,
            cells: vec![Cell::DEAD; (side * side) as usize],
            target,
            place,
            purse: 0,
            generation: 0,
            running: false,
            stepped_at: 0.0,
        }
    }

    fn index(&self, row: i32, col: i32) -> usize {
        let (r, c) = (row.rem_euclid(self.side), col.rem_euclid(self.side));
        (r * self.side + c) as usize
    }

    pub fn at(&self, row: i32, col: i32) -> Cell {
        self.cells[self.index(row, col)]
    }

    /// Put one down, or take it back. **Ground is granted everywhere here**:
    /// the rule about building only where your influence reaches is a real one
    /// and it is not what a patch is teaching, so it is not in the way.
    pub fn toggle(&mut self, row: i32, col: i32) {
        let at = self.index(row, col);
        self.cells[at] = if self.cells[at].is_alive() {
            Cell::DEAD
        } else {
            self.place.apply_to(Cell::DEAD, ME)
        };
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::DEAD);
        self.purse = 0;
        self.generation = 0;
        self.running = false;
    }

    /// Draw the pattern it is asking for, so somebody who would rather watch
    /// than trace can.
    pub fn fill_in_the_target(&mut self) {
        self.clear();
        let middle = self.side / 2;
        for (dr, dc) in self.target {
            let at = self.index(middle + dr, middle + dc);
            self.cells[at] = self.place.apply_to(Cell::DEAD, ME);
        }
    }

    /// One generation, by the game's own rule.
    ///
    /// **Toroidal by hand**, which is the one thing here that is not `World`'s
    /// — it works in chunks and this is a square of cells. Everything that
    /// decides what a cell becomes is `next_cell`, so a factory here pays on
    /// exactly the turnover it pays on in a match.
    pub fn step(&mut self) {
        let mut next = self.cells.clone();
        let seed = crate::sim::mix(0x5EED, self.generation);
        for row in 0..self.side {
            for col in 0..self.side {
                let mut around = [Cell::DEAD; 8];
                for (i, dir) in crate::sim::Dir::ALL.iter().enumerate() {
                    let (dr, dc) = dir.delta();
                    around[i] = self.at(row + dr, col + dc);
                }
                let before = self.at(row, col);
                let cell = crate::sim::mix(seed, (row * self.side + col) as u64);
                let after = next_cell(before, &around, cell);
                // The same question `Chunk::step_into` asks, and the same
                // answer: a factory pays when one is *born*, on a chance that
                // falls as its square wears out.
                if after.kind() == Kind::FACTORY && after.is_alive() && !before.is_alive() {
                    let pays = crate::sim::Roll::new(cell)
                        .chance(crate::sim::YIELD_STREAM, crate::sim::factory_chance(after.age()));
                    if pays {
                        self.purse += crate::sim::FACTORY_YIELD;
                    }
                }
                next[self.index(row, col)] = after;
            }
        }
        self.cells = next;
        self.generation += 1;
    }

    /// Advance on the clock while it is running.
    pub fn tick(&mut self, now: f64) {
        if !self.running {
            self.stepped_at = now;
            return;
        }
        if now - self.stepped_at >= SPAN {
            self.stepped_at = now;
            self.step();
        }
    }
}

/// Draw one, and take what is done to it.
pub fn show(ui: &mut egui::Ui, theme: &Theme, patch: &mut Patch, sheet: Option<egui::TextureId>) {
    let (m, p) = (theme.metrics, theme.palette);
    let side = ui.available_width().min(240.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let step = side / patch.side as f32;
    let painter = ui.painter();
    painter.rect_filled(rect, m.rounding, p.ground);

    let middle = patch.side / 2;
    for row in 0..patch.side {
        for col in 0..patch.side {
            let box_ = egui::Rect::from_min_size(
                rect.min + egui::vec2(col as f32 * step, row as f32 * step),
                egui::vec2(step, step),
            );
            let cell = patch.at(row, col);
            if cell.is_alive() {
                match sheet {
                    Some(sheet) => {
                        painter.image(sheet, box_, Icons::uv(cell.sprite()), egui::Color32::WHITE);
                    }
                    None => {
                        let (r, g, b) = crate::client::views::hue::player_colour(ME);
                        painter.rect_filled(box_, 0.0, egui::Color32::from_rgb(r, g, b));
                    }
                }
                continue;
            }
            // **The target, on the empty squares only.** An outline under a
            // cell that is already there is noise; the ones still missing are
            // the whole of what it is asking for.
            let wanted =
                patch.target.iter().any(|(dr, dc)| (middle + dr, middle + dc) == (row, col));
            if wanted {
                painter.rect_stroke(
                    box_.shrink(1.0),
                    2.0,
                    egui::Stroke::new(1.0, p.accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
    // Ruling over the cells, so the grid reads as paper rather than as gaps.
    let faint = egui::Stroke::new(1.0, p.line.gamma_multiply(0.3));
    for i in 0..=patch.side {
        let at = i as f32 * step;
        painter
            .line_segment([rect.min + egui::vec2(at, 0.0), rect.min + egui::vec2(at, side)], faint);
        painter
            .line_segment([rect.min + egui::vec2(0.0, at), rect.min + egui::vec2(side, at)], faint);
    }

    // Dragged as well as clicked, because tracing an outline is a stroke.
    if let Some(pos) = response.interact_pointer_pos() {
        if response.clicked() || response.dragged() {
            let local = pos - rect.min;
            let (col, row) = ((local.x / step) as i32, (local.y / step) as i32);
            if (0..patch.side).contains(&row)
                && (0..patch.side).contains(&col)
                && response.clicked()
            {
                patch.toggle(row, col);
            }
        }
    }

    ui.add_space(m.item_spacing * 0.5);
    ui.horizontal(|ui| {
        let label = if patch.running { w().menu.tutorial.stop } else { w().menu.tutorial.run };
        if ui.small_button(label).clicked() {
            patch.running = !patch.running;
        }
        if ui.small_button(w().menu.tutorial.step).clicked() {
            patch.running = false;
            patch.step();
        }
        if ui.small_button(w().menu.tutorial.show_me).clicked() {
            patch.fill_in_the_target();
        }
        if ui.small_button(w().menu.tutorial.clear).clicked() {
            patch.clear();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Monospaced: it is a figure in a place that changes, and a
            // proportional face shuffles it sideways every time it does.
            ui.label(
                egui::RichText::new(words::tutorial::purse(patch.purse))
                    .size(m.text_small)
                    .color(if patch.purse > 0 { p.good } else { p.text_dim })
                    .monospace(),
            );
            ui.label(
                egui::RichText::new(words::tutorial::generation(patch.generation))
                    .size(m.text_small)
                    .color(p.text_dim)
                    .monospace(),
            );
        });
    });
}

/// The patches the how-to page shows, in order, one per entry in
/// [`w().menu.tutorial.lessons`].
///
/// **Nine cells a side.** Five is too small for a glider to be a glider — it
/// meets itself before it has travelled — and anything much larger stops being
/// something you can trace in a paragraph. Nine holds a blinker, a block, a
/// glider and a couple of generations of room around them.
pub fn lessons() -> Vec<Patch> {
    vec![
        // A blinker of factories: three in a row, which turn over every
        // generation and so pay every generation.
        Patch::new(9, Placement::Factory, &[(0, -1), (0, 0), (0, 1)]),
        // And the same four cells that never move: a block, which is a still
        // life and earns nothing at all.
        Patch::new(9, Placement::Factory, &[(0, 0), (0, 1), (1, 0), (1, 1)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blinker is the smallest thing that proves the rule is the real one:
    /// three in a row become three in a column and back, forever.
    #[test]
    fn a_patch_runs_the_games_own_rule() {
        let mut patch = Patch::new(9, Placement::Life, &[(0, -1), (0, 0), (0, 1)]);
        patch.fill_in_the_target();
        let before: Vec<bool> = (0..9)
            .flat_map(|r| (0..9).map(move |c| (r, c)))
            .map(|(r, c)| patch.at(r, c).is_alive())
            .collect();
        patch.step();
        let turned: Vec<bool> = (0..9)
            .flat_map(|r| (0..9).map(move |c| (r, c)))
            .map(|(r, c)| patch.at(r, c).is_alive())
            .collect();
        assert_ne!(before, turned, "a blinker did not turn");
        patch.step();
        let back: Vec<bool> = (0..9)
            .flat_map(|r| (0..9).map(move |c| (r, c)))
            .map(|(r, c)| patch.at(r, c).is_alive())
            .collect();
        assert_eq!(before, back, "a blinker did not come back");
    }

    /// **It wraps**, which is what keeps a glider on a board this small from
    /// walking off it and leaving nothing to look at.
    #[test]
    fn a_patch_wraps() {
        let mut patch = Patch::new(5, Placement::Life, &[]);
        patch.toggle(0, 0);
        assert!(patch.at(5, 5).is_alive(), "the far corner is not the near one");
        assert!(patch.at(-5, -5).is_alive());
    }

    /// A factory pays on turnover and a still life never turns over, which is
    /// the single most counter-intuitive rule in the game and the one a patch
    /// exists to show rather than assert.
    #[test]
    fn a_block_of_factories_earns_nothing_and_a_blinker_earns() {
        let block = &[(0, 0), (0, 1), (1, 0), (1, 1)][..];
        let mut still = Patch::new(9, Placement::Factory, block);
        still.fill_in_the_target();
        for _ in 0..20 {
            still.step();
        }
        assert_eq!(still.purse, 0, "a block of factories paid something");

        let mut turning = Patch::new(9, Placement::Factory, &[(0, -1), (0, 0), (0, 1)]);
        turning.fill_in_the_target();
        for _ in 0..20 {
            turning.step();
        }
        assert!(turning.purse > 0, "a blinker of factories paid nothing");
    }
}
