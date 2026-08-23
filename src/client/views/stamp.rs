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

            cell(painter, box_, what, player, sheet);
        }
    }

    /// Put its middle under the pointer rather than its corner, because that
    /// is where you are looking when you place it.
    pub fn centred_on(&self, at: (i32, i32)) -> (i32, i32) {
        (at.0 - self.size.0 / 2, at.1 - self.size.1 / 2)
    }
}

/// The pad you draw a stamp on, and the two buttons that do something with it.
///
/// Below the list rather than behind a mode, because drawing one and looking
/// at the ones you have are the same errand — you draw a thing you have not
/// got, and what you have got is right there to tell you whether you have it.
///
/// Returns what the player asked for, if anything.
fn pad(
    ui: &mut egui::Ui,
    theme: &Theme,
    sketch: &mut Sketch,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) -> Option<Picked> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut asked = None;

    // What the pad lays. Its own row rather than the world's hotbar, because
    // what you are drawing here and what you are holding out there are not the
    // same choice -- and a pad that changed what your next click on the board
    // would do would be a trap.
    ui.horizontal(|ui| {
        for what in [Placement::Life, Placement::Mine, Placement::Turret] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(m.slot * 0.6, m.slot * 0.6), egui::Sense::click());
            let held = sketch.holding() == what;
            ui.painter().rect_stroke(
                rect,
                m.rounding,
                egui::Stroke::new(if held { 1.5 } else { 1.0 }, if held { p.accent } else { p.line }),
                egui::StrokeKind::Inside,
            );
            cell(ui.painter(), rect.shrink(4.0), what, player, sheet);
            if response.clicked() {
                sketch.hold(what);
            }
        }
    });

    let side = ui.available_width().min(224.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let step = side / SKETCH_N as f32;
    ui.painter().rect_filled(rect, m.rounding, p.surface);

    for row in 0..SKETCH_N {
        for col in 0..SKETCH_N {
            let Some(what) = sketch.at(row, col) else { continue };
            let box_ = egui::Rect::from_min_size(
                rect.min + egui::vec2(col as f32 * step, row as f32 * step),
                egui::vec2(step, step),
            );
            cell(ui.painter(), box_.shrink(step * 0.08), what, player, sheet);
        }
    }
    // Drawn over the cells, so the grid reads as ruling on paper rather than
    // as gaps between them.
    let faint = egui::Stroke::new(1.0, p.line.gamma_multiply(0.35));
    for i in 0..=SKETCH_N {
        let at = i as f32 * step;
        ui.painter().line_segment(
            [rect.min + egui::vec2(at, 0.0), rect.min + egui::vec2(at, side)],
            faint,
        );
        ui.painter().line_segment(
            [rect.min + egui::vec2(0.0, at), rect.min + egui::vec2(side, at)],
            faint,
        );
    }
    ui.painter().rect_stroke(
        rect,
        m.rounding,
        egui::Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );

    // A drag lays and a click lays or lifts, which is what the same gestures
    // do on the board. Only the cell under the pointer this frame: a pad is
    // small and a hand crossing it is slow, so the interpolation the world's
    // pencil needs would be machinery for nothing.
    if let Some(at) = response.interact_pointer_pos() {
        let local = at - rect.min;
        let (row, col) = ((local.y / step) as i32, (local.x / step) as i32);
        if response.clicked() {
            sketch.click(row, col);
        } else if response.dragged() {
            sketch.lay(row, col);
        }
    }

    ui.horizontal(|ui| {
        // Nothing drawn is nothing to keep, and a button that does nothing
        // says less than one that is plainly not available.
        ui.add_enabled_ui(!sketch.is_empty(), |ui| {
            if ui.small_button(words::KEEP).clicked() {
                if let Some(stamp) = sketch.to_stamp() {
                    sketch.clear();
                    asked = Some(Picked::Keep(stamp));
                }
            }
            if ui.small_button(words::CLEAR).clicked() {
                sketch.clear();
            }
        });
        ui.small(words::DRAW_HOW);
    });

    asked
}

/// Draw one cell of a pattern, as the world would draw it.
///
/// One function because the preview on a button and the pad you draw on must
/// agree: a cell that looks like a mine in one and like life in the other is
/// worse than either.
///
/// The sheet can fail to build, and then the kinds fall back to lightness —
/// paler for a mine, paler still for a turret — so the distinction survives
/// losing the art rather than going with it.
fn cell(
    painter: &egui::Painter,
    rect: egui::Rect,
    what: Placement,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) {
    match sheet {
        // The tile this cell would draw as once it is placed: alive, and of
        // this placement's kind. The sheet is already in the player's colour
        // and the tile byte carries the state, so there is nothing to look up.
        Some(sheet) => {
            let tile = what.apply_to(Cell::DEAD, player).tile();
            painter.image(sheet, rect, Icons::uv(tile), egui::Color32::WHITE);
        }
        None => {
            let lightness = match what {
                Placement::Turret => 0.90,
                Placement::Mine => 0.78,
                _ => 0.62,
            };
            let (red, green, blue) = crate::client::views::hud::shade(lightness, 1.0, player);
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(red, green, blue));
        }
    }
}

/// How wide the pad you draw a stamp on is, in cells.
///
/// Sixteen because that is a chunk, and because a pattern worth drawing by
/// hand is a small one — anything larger is a thing you build on the board and
/// capture with Grab. What is kept is trimmed to what was drawn, so using a
/// corner of the pad costs nothing.
pub const SKETCH_N: i32 = 16;

/// A stamp being drawn by hand rather than taken off the board.
///
/// Capturing needs something already alive and standing where you can reach
/// it, which makes the first stamp of a session the hardest one to get and
/// makes trying a pattern out mean building it first. Drawing needs nothing:
/// a pad, a kind, and somewhere to put the cells.
///
/// It holds placements rather than cells, exactly as a [`Stamp`] does, so what
/// is drawn is what is kept and there is no conversion to get wrong.
#[derive(Clone, Debug)]
pub struct Sketch {
    cells: Vec<Option<Placement>>,
    /// What the next cell laid will be. The pad's own hotbar, and separate
    /// from the world's, because what you are drawing here is not what you are
    /// holding out there.
    holding: Placement,
}

impl Default for Sketch {
    fn default() -> Self {
        Self {
            cells: vec![None; (SKETCH_N * SKETCH_N) as usize],
            holding: Placement::Life,
        }
    }
}

impl Sketch {
    /// What the pad lays next.
    pub fn holding(&self) -> Placement {
        self.holding
    }

    pub fn hold(&mut self, what: Placement) {
        self.holding = what;
    }

    fn index(row: i32, col: i32) -> Option<usize> {
        (0..SKETCH_N).contains(&row).then_some(())?;
        (0..SKETCH_N).contains(&col).then_some(())?;
        Some((row * SKETCH_N + col) as usize)
    }

    pub fn at(&self, row: i32, col: i32) -> Option<Placement> {
        self.cells[Self::index(row, col)?]
    }

    /// Lay what is held here. A drag always lays and never lifts, which is the
    /// rule a drag follows on the board: a sweep across cells already drawn is
    /// far more likely to be drawing over them than asking for them back.
    pub fn lay(&mut self, row: i32, col: i32) {
        if let Some(i) = Self::index(row, col) {
            self.cells[i] = Some(self.holding);
        }
    }

    /// A click lays, or lifts what it finds if that is already what is held —
    /// the same question `net::Placement::is_on` asks of a square on the
    /// board, so the pad behaves like the thing it is drawing for.
    pub fn click(&mut self, row: i32, col: i32) {
        let Some(i) = Self::index(row, col) else { return };
        self.cells[i] = if self.cells[i] == Some(self.holding) { None } else { Some(self.holding) };
    }

    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| *c = None);
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(Option::is_none)
    }

    /// What is drawn, as a stamp, trimmed to the cells that are there.
    ///
    /// `None` when nothing is drawn: an empty stamp is a button that does
    /// nothing, which is worse than a refusal that says why. Trimmed for the
    /// same reason a capture is — where on the pad you happened to draw is not
    /// part of the pattern.
    pub fn to_stamp(&self) -> Option<Stamp> {
        let found: Vec<((i32, i32), Placement)> = (0..SKETCH_N)
            .flat_map(|r| (0..SKETCH_N).map(move |c| (r, c)))
            .filter_map(|(r, c)| self.at(r, c).map(|what| ((r, c), what)))
            .collect();
        if found.is_empty() {
            return None;
        }

        let top = found.iter().map(|&((r, _), _)| r).min()?;
        let left = found.iter().map(|&((_, c), _)| c).min()?;
        let bottom = found.iter().map(|&((r, _), _)| r).max()?;
        let right = found.iter().map(|&((_, c), _)| c).max()?;
        Some(Stamp {
            name: format!("{}x{}", bottom - top + 1, right - left + 1),
            cells: found
                .into_iter()
                .map(|((r, c), what)| ((r - top, c - left), what))
                .collect(),
            size: (bottom - top + 1, right - left + 1),
        })
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
    /// A pattern drawn on the pad rather than taken off the board.
    Keep(Stamp),
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
    sketch: &mut Sketch,
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
                    ui.label(words::DRAW);
                    if let Some(drawn) = pad(ui, theme, sketch, player, sheet) {
                        picked = drawn;
                    }

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

    /// Drawn rather than captured: the first stamp of a session no longer
    /// needs something already alive and standing where you can reach it.
    #[test]
    fn a_drawn_stamp_is_the_pattern_not_where_it_was_drawn() {
        let mut pad = Sketch::default();
        // A glider, drawn away from the pad's corner.
        for (r, c) in [(5, 6), (6, 7), (7, 5), (7, 6), (7, 7)] {
            pad.lay(r, c);
        }
        let stamp = pad.to_stamp().expect("five cells is a pattern");
        assert_eq!(stamp.size, (3, 3), "trimmed to what was drawn");
        assert_eq!(stamp.name, "3x3");
        let mut cells: Vec<(i32, i32)> = stamp.cells.iter().map(|&(at, _)| at).collect();
        cells.sort_unstable();
        assert_eq!(cells, vec![(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
    }

    /// The pad asks the same question the board does: what you are holding is
    /// already there, so a click takes it back, and anything else it lays.
    #[test]
    fn a_click_lays_or_lifts_and_a_drag_only_lays() {
        let mut pad = Sketch::default();
        assert_eq!(pad.holding(), Placement::Life);

        pad.click(0, 0);
        assert_eq!(pad.at(0, 0), Some(Placement::Life));
        pad.click(0, 0);
        assert_eq!(pad.at(0, 0), None, "clicking what is held lifts it");

        // Holding something else replaces rather than lifts, as on the board.
        pad.click(1, 1);
        pad.hold(Placement::Turret);
        pad.click(1, 1);
        assert_eq!(pad.at(1, 1), Some(Placement::Turret));

        // A drag lays and never lifts: a sweep over cells already drawn is far
        // more likely to be drawing over them than asking for them back.
        pad.lay(1, 1);
        assert_eq!(pad.at(1, 1), Some(Placement::Turret));
    }

    /// Every cell keeps the kind it was drawn with, which is the whole of what
    /// a stamp carries beyond its shape.
    #[test]
    fn a_drawn_stamp_keeps_the_kind_of_every_cell() {
        let mut pad = Sketch::default();
        pad.lay(0, 0);
        pad.hold(Placement::Mine);
        pad.lay(0, 1);
        pad.hold(Placement::Turret);
        pad.lay(0, 2);

        let stamp = pad.to_stamp().unwrap();
        let mut cells = stamp.cells.clone();
        cells.sort_by_key(|&(at, _)| at);
        assert_eq!(
            cells,
            vec![
                ((0, 0), Placement::Life),
                ((0, 1), Placement::Mine),
                ((0, 2), Placement::Turret),
            ]
        );
        assert_eq!(stamp.placements().len(), 3, "and each is laid as its own action");
    }

    /// An empty stamp is a button that does nothing, which is worse than a
    /// refusal that says why.
    #[test]
    fn an_empty_pad_is_not_a_stamp() {
        let mut pad = Sketch::default();
        assert!(pad.is_empty());
        assert!(pad.to_stamp().is_none());

        pad.lay(3, 3);
        assert!(!pad.is_empty());
        pad.clear();
        assert!(pad.to_stamp().is_none(), "clearing leaves nothing to keep");
    }

    /// Off the pad is nothing at all rather than a panic or a wrapped cell —
    /// a pointer leaving the grid mid-drag is the ordinary case.
    #[test]
    fn drawing_off_the_pad_does_nothing() {
        let mut pad = Sketch::default();
        for (r, c) in [(-1, 0), (0, -1), (SKETCH_N, 0), (0, SKETCH_N), (99, 99)] {
            pad.lay(r, c);
            pad.click(r, c);
            assert_eq!(pad.at(r, c), None);
        }
        assert!(pad.is_empty());
    }

    /// A stamp is a stamp however it was made: what is drawn goes on the wire
    /// as the same `Paint` a captured one does.
    #[test]
    fn a_drawn_stamp_places_like_a_captured_one() {
        let mut pad = Sketch::default();
        for (r, c) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            pad.lay(r, c);
        }
        let drawn = pad.to_stamp().unwrap();
        let captured = Stamp::capture(
            &world_with(&[
                ((0, 0), Kind::NORMAL, PlayerId(1)),
                ((0, 1), Kind::NORMAL, PlayerId(1)),
                ((1, 0), Kind::NORMAL, PlayerId(1)),
                ((1, 1), Kind::NORMAL, PlayerId(1)),
            ]),
            PlayerId(1),
            (0, 0),
            (1, 1),
        )
        .unwrap();
        assert_eq!(drawn.size, captured.size);
        assert_eq!(drawn.at((5, 5)), captured.at((5, 5)));
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
