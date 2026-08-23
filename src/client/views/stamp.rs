//! A pattern captured once and placed again.
//!
//! Nothing on the wire is a stamp: placing one is an [`crate::net::Action`]
//! over the cells it covers, judged against territory and value like anything
//! else. What a stamp is, is a shape somebody bothered to save — so it lives
//! here, in the client, with the interface that saves and places it.
//!
//! **Cells and their kind, not a rectangle of ground.** A pattern is the live
//! cells in it; the dead ones are gaps, and a stamp that carried them would
//! wipe whatever it was placed over. The kind travels because a glider gun
//! built of mines is a different thing from one built of life, and a stamp that
//! forgot which would quietly hand you the cheap one.
//!
//! Coordinates are relative to the pattern's own top-left, so a stamp knows its
//! shape and not where it was found.

use crate::client::views::icons::Icons;
use crate::client::views::theme::Theme;
use crate::client::views::words::stamps as words;
use crate::net::Placement;
use crate::sim::{Cell, Kind, PlayerId, World};

/// The most stamps the hotbar shows before the rest go behind a menu.
///
/// Ten because the number keys run out there, and because a row of small
/// squares stops being something you can pick from at a glance long before a
/// library stops being useful.
pub const ON_THE_BAR: usize = 10;

/// One captured pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub name: String,
    /// `(row, col)` from the pattern's top-left, and what to lay there.
    pub cells: Vec<((i32, i32), Placement)>,
    /// Rows and columns the pattern spans, for the preview and the label.
    pub size: (i32, i32),
}

impl Stamp {
    /// Take the living cells this player owns inside a rectangle.
    ///
    /// `None` if there is nothing of theirs alive in it — an empty stamp is a
    /// button that does nothing, which is worse than a refusal that says why.
    pub fn capture(
        world: &World,
        player: PlayerId,
        from: (i32, i32),
        to: (i32, i32),
    ) -> Option<Self> {
        let (r0, r1) = (from.0.min(to.0), from.0.max(to.0));
        let (c0, c1) = (from.1.min(to.1), from.1.max(to.1));

        let mut found: Vec<((i32, i32), Placement)> = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
                // Only your own life. Somebody else's pattern is theirs, and
                // copying it would be a way to have it without building it.
                if !cell.is_alive() || cell.player() != player {
                    continue;
                }
                let placement = match cell.kind() {
                    Kind::MINE => Placement::Mine,
                    Kind::TURRET => Placement::Turret,
                    _ => Placement::Life,
                };
                found.push(((r, c), placement));
            }
        }
        if found.is_empty() {
            return None;
        }

        // Trimmed to what was actually caught rather than to what was swept, so
        // a sloppy rectangle round a glider still gives you a glider.
        let top = found.iter().map(|&((r, _), _)| r).min()?;
        let left = found.iter().map(|&((_, c), _)| c).min()?;
        let bottom = found.iter().map(|&((r, _), _)| r).max()?;
        let right = found.iter().map(|&((_, c), _)| c).max()?;

        Some(Self {
            name: format!("{}x{}", bottom - top + 1, right - left + 1),
            cells: found
                .into_iter()
                .map(|((r, c), what)| (((r - top), (c - left)), what))
                .collect(),
            size: (bottom - top + 1, right - left + 1),
        })
    }

    /// Where this stamp's cells land if its top-left is put at `at`.
    pub fn at(&self, at: (i32, i32)) -> Vec<((i32, i32), Placement)> {
        self.cells
            .iter()
            .map(|&((r, c), what)| ((at.0 + r, at.1 + c), what))
            .collect()
    }

    /// The cells of one kind only. A `Paint` lays one placement, so a stamp
    /// holding both goes as two actions.
    pub fn of(&self, at: (i32, i32), placement: Placement) -> Vec<(i32, i32)> {
        self.at(at)
            .into_iter()
            .filter(|&(_, what)| what == placement)
            .map(|(cell, _)| cell)
            .collect()
    }

    /// Every placement this stamp uses, in a fixed order so two peers price it
    /// the same way.
    pub fn placements(&self) -> Vec<Placement> {
        let mut out: Vec<Placement> = Vec::new();
        for &(_, what) in &self.cells {
            if !out.contains(&what) {
                out.push(what);
            }
        }
        out.sort_by_key(|p| format!("{p:?}"));
        out
    }

    /// Draw it as the cells it is, fitted to a box.
    ///
    /// `2x2` says nothing about what is about to be placed; its shape does. At
    /// button size a glider is a glider and a block is a block, which is the
    /// whole question a row of ten of them has to answer.
    ///
    /// **And what it is made of, drawn from the sheet the world is drawn
    /// from.** A stamp carries the kind of every cell in it — a gun built of
    /// mines is a different thing from one built of life, and a turret is a
    /// third — so a thumbnail that showed only the shape was hiding the half
    /// of the pattern that decides what it costs and what it does. Every other
    /// square on the bar already shows the cell it lays; this one now shows
    /// the cells it lays.
    ///
    /// The sheet can fail to build, and then the kinds fall back to
    /// lightness: paler for a mine, paler still for a turret. That keeps the
    /// distinction visible without art rather than losing it.
    pub fn draw(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        player: PlayerId,
        sheet: Option<egui::TextureId>,
    ) {
        let (rows, cols) = (self.size.0.max(1) as f32, self.size.1.max(1) as f32);
        // Square cells, sized so the longer axis just fits.
        let step = (rect.width() / cols).min(rect.height() / rows);
        let origin = rect.center() - egui::vec2(cols * step, rows * step) * 0.5;

        for &((r, c), what) in &self.cells {
            let at = egui::Rect::from_min_size(
                origin + egui::vec2(c as f32 * step, r as f32 * step),
                egui::vec2(step, step),
            );
            // Shrunk a hair, so neighbouring cells read as cells rather than as
            // a solid blob -- the difference between a shape and a smear.
            let box_ = at.shrink(step * 0.12);

            match sheet {
                // The tile a stamp's cell would draw as once it is placed:
                // alive, and of this placement's kind. The sheet is already in
                // this player's colour, and the tile byte carries the state,
                // so there is nothing here to look up.
                Some(sheet) => {
                    let tile = what.apply_to(Cell::DEAD, player).tile();
                    painter.image(sheet, box_, Icons::uv(tile), egui::Color32::WHITE);
                }
                None => {
                    let lightness = match what {
                        Placement::Turret => 0.90,
                        Placement::Mine => 0.78,
                        _ => 0.62,
                    };
                    let (red, green, blue) =
                        crate::client::views::hud::shade(lightness, 1.0, player);
                    painter.rect_filled(box_, 0.0, egui::Color32::from_rgb(red, green, blue));
                }
            }
        }
    }

    /// Put its middle under the pointer rather than its corner, because that
    /// is where you are looking when you place it.
    pub fn centred_on(&self, at: (i32, i32)) -> (i32, i32) {
        (at.0 - self.size.0 / 2, at.1 - self.size.1 / 2)
    }
}

/// Everything captured so far, newest first.
///
/// Newest first because the one you just took is the one you are about to
/// place, and because it puts the stale end of the library where it belongs —
/// off the bar and behind the menu.
#[derive(Default)]
pub struct Library {
    stamps: Vec<Stamp>,
}

impl Library {
    pub fn keep(&mut self, stamp: Stamp) {
        self.stamps.insert(0, stamp);
    }

    pub fn get(&self, index: usize) -> Option<&Stamp> {
        self.stamps.get(index)
    }

    pub fn len(&self) -> usize {
        self.stamps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Stamp> {
        self.stamps.iter()
    }

    /// How many fit on the bar. The rest are reachable through the menu.
    pub fn on_the_bar(&self) -> usize {
        self.stamps.len().min(ON_THE_BAR)
    }

    pub fn forget(&mut self, index: usize) {
        if index < self.stamps.len() {
            self.stamps.remove(index);
        }
    }
}

/// What the player did with the picker this frame.
pub enum Picked {
    Nothing,
    Hold(usize),
    Forget(usize),
    Close,
}

/// The whole library, for when there are more stamps than the bar can hold.
///
/// A list rather than a grid of squares: past ten of them you are reading names
/// and sizes, not recognising shapes, and a list is what reading wants.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    library: &Library,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) -> (Picked, Option<egui::Rect>) {
    let p = theme.palette;
    let m = theme.metrics;
    let mut picked = Picked::Nothing;

    let area = egui::Area::new("stamps".into())
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.line))
                .corner_radius(m.rounding)
                .inner_margin(m.panel_padding * 1.4)
                .show(ui, |ui| {
                    ui.set_width(280.0);
                    ui.spacing_mut().item_spacing.y = m.item_spacing;
                    ui.heading(words::TITLE);
                    ui.separator();

                    if library.is_empty() {
                        ui.colored_label(p.text_dim, words::NONE_YET);
                    }
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, stamp) in library.iter().enumerate() {
                            ui.horizontal(|ui| {
                                // The shape first, and big enough to recognise:
                                // a name is what you call it once you know
                                // which one it is.
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(m.slot, m.slot),
                                    egui::Sense::click(),
                                );
                                ui.painter().rect_stroke(
                                    rect,
                                    m.rounding,
                                    egui::Stroke::new(1.0, p.line),
                                    egui::StrokeKind::Inside,
                                );
                                stamp.draw(ui.painter(), rect.shrink(4.0), player, sheet);
                                if response.clicked() {
                                    picked = Picked::Hold(i);
                                }
                                ui.colored_label(
                                    p.text_dim,
                                    format!("{}  ·  {} cells", stamp.name, stamp.cells.len()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(words::FORGET).clicked() {
                                            picked = Picked::Forget(i);
                                        }
                                    },
                                );
                            });
                        }
                    });

                    ui.separator();
                    ui.small(words::HOW);
                    if ui
                        .add_sized([ui.available_width(), 26.0], egui::Button::new(words::CLOSE))
                        .clicked()
                    {
                        picked = Picked::Close;
                    }
                });
        });

    (picked, Some(area.response.rect))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with(cells: &[((i32, i32), Kind, PlayerId)]) -> World {
        let mut world = World::infinite_empty();
        for &((r, c), kind, who) in cells {
            world.set_cell_at(r, c, Cell::alive(who).with_kind(kind));
        }
        world
    }

    /// A stamp is the pattern, not the rectangle somebody swept round it.
    #[test]
    fn capture_trims_to_what_it_caught() {
        let me = PlayerId(1);
        // A glider at (10, 10).
        let glider = [(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        let world = world_with(
            &glider.map(|at| (at, Kind::NORMAL, me)),
        );

        // Swept sloppily, far wider than the pattern.
        let stamp = Stamp::capture(&world, me, (5, 5), (20, 20)).unwrap();
        assert_eq!(stamp.size, (3, 3), "trimmed to the glider, not the sweep");
        assert_eq!(stamp.cells.len(), 5);
        assert_eq!(stamp.name, "3x3");

        // And it lands as the same shape wherever it is put.
        let laid: Vec<(i32, i32)> = stamp.of((100, 200), Placement::Life);
        let mut expected: Vec<(i32, i32)> =
            glider.iter().map(|&(r, c)| (r - 10 + 100, c - 10 + 200)).collect();
        expected.sort_unstable();
        let mut laid = laid;
        laid.sort_unstable();
        assert_eq!(laid, expected);
    }

    /// Dead cells are the gaps in a pattern. A stamp that carried them would
    /// wipe whatever it was placed over.
    #[test]
    fn capture_takes_only_your_own_life() {
        let me = PlayerId(1);
        let world = world_with(&[
            ((0, 0), Kind::NORMAL, me),
            ((0, 1), Kind::NORMAL, PlayerId(2)),
            ((0, 2), Kind::MINE, me),
        ]);

        let stamp = Stamp::capture(&world, me, (0, 0), (0, 2)).unwrap();
        assert_eq!(stamp.cells.len(), 2, "somebody else's cell is not yours to copy");
        assert_eq!(stamp.size, (1, 3), "and the gap it leaves is part of the shape");

        // The kind travels: a gun of mines is a different thing from one of life.
        assert_eq!(stamp.of((0, 0), Placement::Life), vec![(0, 0)]);
        assert_eq!(stamp.of((0, 0), Placement::Mine), vec![(0, 2)]);
        assert_eq!(stamp.placements().len(), 2);
    }

    /// Nothing to capture is a refusal, not an empty button.
    #[test]
    fn capturing_nothing_gives_nothing() {
        let world = world_with(&[((0, 0), Kind::NORMAL, PlayerId(2))]);
        assert!(Stamp::capture(&world, PlayerId(1), (-5, -5), (5, 5)).is_none());
        assert!(Stamp::capture(&World::infinite_empty(), PlayerId(1), (0, 0), (9, 9)).is_none());
    }

    /// Newest first, because the one you just took is the one you are about to
    /// place — and it is what puts the stale end of the library behind the menu.
    #[test]
    fn the_library_keeps_the_newest_where_the_hand_is() {
        let me = PlayerId(1);
        let world = world_with(&[((0, 0), Kind::NORMAL, me)]);
        let mut library = Library::default();
        for _ in 0..ON_THE_BAR + 3 {
            library.keep(Stamp::capture(&world, me, (0, 0), (0, 0)).unwrap());
        }
        assert_eq!(library.len(), ON_THE_BAR + 3);
        assert_eq!(library.on_the_bar(), ON_THE_BAR, "the rest go behind the menu");

        library.forget(0);
        assert_eq!(library.len(), ON_THE_BAR + 2);
    }
}
