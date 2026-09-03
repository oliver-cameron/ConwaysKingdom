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
use crate::sim::{Cell, PlayerId, World};

/// The most stamps the hotbar shows before the rest go behind a menu.
///
/// Ten because the number keys run out there, and because a row of small
/// squares stops being something you can pick from at a glance long before a
/// library stops being useful.
pub const ON_THE_BAR: usize = 10;

/// How a stamp is turned before it is laid.
///
/// **So a glider is one stamp and not four.** A pattern and the same pattern
/// rotated are the same pattern; keeping both is the library filling up with
/// its own reflections, and the four gliders are only the beginning — every
/// spaceship, every gun, every corner of a wall has the same four or eight.
///
/// Held rather than stored: this is part of what you are *about to place*, not
/// part of what you saved, so rotating changes nothing in the library and
/// there is nothing to save, migrate or forget.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Turn {
    /// Quarter turns clockwise, nought to three.
    pub quarters: u8,
    /// Mirrored left to right, applied **before** the rotation.
    ///
    /// Both, because a rotation cannot produce a reflection: a glider has four
    /// rotations and four more that are its mirror image, and a pattern that
    /// is not symmetric needs the second set to be reachable at all.
    pub mirrored: bool,
}

impl Turn {
    pub fn right(self) -> Self {
        Self { quarters: (self.quarters + 1) % 4, ..self }
    }

    pub fn left(self) -> Self {
        Self { quarters: (self.quarters + 3) % 4, ..self }
    }

    pub fn mirror(self) -> Self {
        Self { mirrored: !self.mirrored, ..self }
    }

    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}

/// One captured pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub name: String,
    /// `(row, col)` from the pattern's top-left. **A shape and not a
    /// material**: what a stamp is made of is chosen when it is laid, on the
    /// hotbar's second axis, so one captured glider can go down as life, as
    /// mines or as ice. A stamp that carried its own kinds made that choice
    /// twice and let the two disagree.
    pub cells: Vec<(i32, i32)>,
    /// Rows and columns the pattern spans, for the preview and the label.
    pub size: (i32, i32),
    /// Whether this one is on the hotbar.
    ///
    /// **Nothing pinned means the newest ten**, which is what the bar always
    /// showed and is the right default: somebody who has never thought about
    /// it gets the stamp they just took, on the key beside their hand. Pin one
    /// and the bar becomes exactly what is pinned, because half a rule is
    /// worse than either — a bar that was "your pins, then the newest of the
    /// rest" would reshuffle itself under your fingers every time you captured
    /// something.
    pub on_bar: bool,
}

/// Whether a swept rectangle is small enough to be a stamp.
///
/// **One bound, used by the sweep and by the pad.** A stamp is at most
/// [`SKETCH_N`] cells a side either way, because that is the pad it has to be
/// editable on. The sweep had no bound at all, so a captured pattern could be
/// any size and was then cropped to the pad the moment somebody opened it —
/// two limits that disagree are one limit and a silent loss.
pub fn fits(from: (i32, i32), to: (i32, i32)) -> bool {
    let (rows, cols) = ((from.0 - to.0).abs() + 1, (from.1 - to.1).abs() + 1);
    rows <= SKETCH_N && cols <= SKETCH_N
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
        // **The same bound as the pad**, which it did not have: a sweep took
        // whatever rectangle it was given, so a captured pattern could be any
        // size at all — and then opening it to edit quietly cropped it to the
        // pad. Two limits that disagree are one limit and a silent loss.
        if !fits(from, to) {
            return None;
        }
        let (r0, r1) = (from.0.min(to.0), from.0.max(to.0));
        let (c0, c1) = (from.1.min(to.1), from.1.max(to.1));

        let mut found: Vec<(i32, i32)> = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                let cell = world.cell_at(r, c).unwrap_or(Cell::DEAD);
                // Only your own life. Somebody else's pattern is theirs, and
                // copying it would be a way to have it without building it.
                //
                // The kind is not taken: what a pattern is made of is the
                // hotbar's business now, so what is captured is where the
                // cells are.
                if !cell.is_alive() || cell.player() != player {
                    continue;
                }
                found.push((r, c));
            }
        }
        if found.is_empty() {
            return None;
        }

        // Trimmed to what was actually caught rather than to what was swept, so
        // a sloppy rectangle round a glider still gives you a glider.
        Some(Self::trimmed(found))
    }

    /// A pattern out of the cells it is drawn on, moved to its own corner.
    ///
    /// Trimmed for the reason a capture is: where on the board, or on the pad,
    /// somebody happened to draw is not part of the pattern.
    pub(super) fn trimmed(found: Vec<(i32, i32)>) -> Self {
        let top = found.iter().map(|&(r, _)| r).min().unwrap_or(0);
        let left = found.iter().map(|&(_, c)| c).min().unwrap_or(0);
        let bottom = found.iter().map(|&(r, _)| r).max().unwrap_or(0);
        let right = found.iter().map(|&(_, c)| c).max().unwrap_or(0);
        Self {
            name: format!("{}x{}", bottom - top + 1, right - left + 1),
            cells: found.into_iter().map(|(r, c)| (r - top, c - left)).collect(),
            size: (bottom - top + 1, right - left + 1),
            on_bar: false,
        }
    }

    /// The same pattern, turned.
    ///
    /// A whole `Stamp` rather than a turned view, so everything that already
    /// works on one — placing, pricing, the preview, the thumbnail — goes on
    /// working with no second path to keep in step. It is a handful of cells
    /// and a `Vec`, rebuilt when the pointer moves; the pattern that would
    /// make this worth caching does not exist.
    ///
    /// Trimmed on the way out, which is what keeps a turned stamp's corner in
    /// the same relation to its cells as an unturned one's.
    pub fn turned(&self, turn: Turn) -> Self {
        if turn.is_default() {
            return self.clone();
        }
        let (rows, cols) = self.size;
        let cells = self
            .cells
            .iter()
            .map(|&(r, c)| {
                // Mirrored first, then rotated, because the other order is a
                // different transform and one of the two has to be named.
                let (r, c) = if turn.mirrored { (r, cols - 1 - c) } else { (r, c) };
                match turn.quarters {
                    1 => (c, rows - 1 - r),
                    2 => (rows - 1 - r, cols - 1 - c),
                    3 => (cols - 1 - c, r),
                    _ => (r, c),
                }
            })
            .collect();
        // The name is the size and the size may have swapped, so it is rebuilt
        // rather than carried: a 3x5 turned is a 5x3 and should say so.
        Self::trimmed(cells)
    }

    /// Where this stamp's cells land if its top-left is put at `at`.
    pub fn at(&self, at: (i32, i32)) -> Vec<(i32, i32)> {
        self.cells.iter().map(|&(r, c)| (at.0 + r, at.1 + c)).collect()
    }

    /// Draw it as the cells it is, fitted to a box.
    ///
    /// `2x2` says nothing about what is about to be placed; its shape does. At
    /// button size a glider is a glider and a block is a block, which is the
    /// whole question a row of ten of them has to answer.
    ///
    /// **Drawn in whatever it is about to be made of.** A stamp used to
    /// carry the kind of every cell in it and a thumbnail showed them, which
    /// was right while a pattern was a pattern *and* a material. It is a shape
    /// now, so the square shows the shape in the material the hotbar is
    /// holding — the same pattern reads as life, as mines or as ice depending
    /// on what would come out of it, which is more useful than a fixed
    /// picture of how it was captured.
    ///
    /// The sheet can fail to build, and then the kinds fall back to
    /// lightness: paler for a mine, paler still for a turret. That keeps the
    /// distinction visible without art rather than losing it.
    /// **A stamp is a shape, so it is drawn as one.**
    ///
    /// It used to be drawn in whatever the hotbar was holding, on the
    /// reasoning that a preview should show what it would put down. That is
    /// the wrong question here: the bar already says what is held, and a
    /// pattern redrawn in a mine's art every time somebody changed material
    /// made the same glider look like a different pattern. What the square is
    /// for is telling one saved shape from another.
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

        for &(r, c) in &self.cells {
            let at = egui::Rect::from_min_size(
                origin + egui::vec2(c as f32 * step, r as f32 * step),
                egui::vec2(step, step),
            );
            // **Whole cells, touching.** They were shrunk an eighth so a block
            // of them would not read as one blob, which at this size took the
            // shape apart instead: a five-cell glider came out as five specks
            // with more gap than cell between them. The sprite carries its own
            // outline — a texel of transparency on every side, drawn there for
            // exactly this — so full tiles already read as cells and the shrink
            // was a second answer to a solved question.
            cell(painter, at, player, sheet);
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
    // Which library entry the pad is redrawing, so `keep` can say it replaces
    // rather than adds. See [`Editing`].
    editing: Editing,
    ui: &mut egui::Ui,
    theme: &Theme,
    sketch: &mut Sketch,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) -> Option<Picked> {
    let p = theme.palette;
    let m = theme.metrics;
    let mut asked = None;

    // **No row of materials here any more.** The pad used to carry its own,
    // on the reasoning that what you are drawing and what you are holding out
    // there are different choices. They stopped being two choices when a stamp
    // became a shape: there is nothing to pick here, because what a pattern is
    // made of is decided when it is laid.

    let side = ui.available_width().min(224.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let step = side / SKETCH_N as f32;
    ui.painter().rect_filled(rect, m.rounding, p.surface);

    for row in 0..SKETCH_N {
        for col in 0..SKETCH_N {
            if !sketch.at(row, col) {
                continue;
            }
            let box_ = egui::Rect::from_min_size(
                rect.min + egui::vec2(col as f32 * step, row as f32 * step),
                egui::vec2(step, step),
            );
            // Whole cells here too. The ruling is drawn over them a few lines
            // down, which is what separates one from the next on the pad — so
            // shrinking them as well left a gap *and* a line between every
            // pair, and a drawn shape looked like a sieve.
            cell(ui.painter(), box_, player, sheet);
        }
    }
    // Drawn over the cells, so the grid reads as ruling on paper rather than
    // as gaps between them.
    let faint = egui::Stroke::new(1.0, p.line.gamma_multiply(0.35));
    for i in 0..=SKETCH_N {
        let at = i as f32 * step;
        ui.painter()
            .line_segment([rect.min + egui::vec2(at, 0.0), rect.min + egui::vec2(at, side)], faint);
        ui.painter()
            .line_segment([rect.min + egui::vec2(0.0, at), rect.min + egui::vec2(side, at)], faint);
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
        // Which of the two this is. Keeping while editing replaces the stamp
        // that was opened, and that is worth saying before the press rather
        // than being noticed afterwards.
        ui.small(if editing.is_some() { words::EDITING } else { words::DRAW_HOW });
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
/// One cell of a saved shape, in the player's colour.
///
/// **Always plain life**, never the placement the bar is holding — see
/// [`Stamp::draw`]. A stamp records where the cells are and nothing about what
/// they are made of, so drawing it as a mine claims something the stamp does
/// not say.
fn cell(
    painter: &egui::Painter,
    rect: egui::Rect,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
) {
    match sheet {
        // The sheet is already in the player's colour and the tile byte
        // carries the state, so there is nothing to look up.
        Some(sheet) => {
            let tile = Placement::Life.apply_to(Cell::DEAD, player).sprite();
            painter.image(sheet, rect, Icons::uv(tile), egui::Color32::WHITE);
        }
        // No sheet yet, so a flat square stands in — and this one *is* a solid
        // block when cells touch, so it keeps a corner radius to break it up.
        None => {
            let (red, green, blue) = crate::client::views::hue::shade(0.62, 1.0, player);
            painter.rect_filled(
                rect,
                (rect.width() * 0.18).min(3.0),
                egui::Color32::from_rgb(red, green, blue),
            );
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
/// **Cells on or off**, exactly as a [`Stamp`] is, so what is drawn is what is
/// kept and there is no conversion to get wrong. The pad had its own row of
/// materials and does not need one: a pattern is a shape here as much as it is
/// on the board, and what it is made of is decided when it is laid.
#[derive(Clone, Debug)]
pub struct Sketch {
    cells: Vec<bool>,
}

impl Default for Sketch {
    fn default() -> Self {
        Self { cells: vec![false; (SKETCH_N * SKETCH_N) as usize] }
    }
}

impl Sketch {
    fn index(row: i32, col: i32) -> Option<usize> {
        (0..SKETCH_N).contains(&row).then_some(())?;
        (0..SKETCH_N).contains(&col).then_some(())?;
        Some((row * SKETCH_N + col) as usize)
    }

    pub fn at(&self, row: i32, col: i32) -> bool {
        Self::index(row, col).is_some_and(|i| self.cells[i])
    }

    /// Lay what is held here. A drag always lays and never lifts, which is the
    /// rule a drag follows on the board: a sweep across cells already drawn is
    /// far more likely to be drawing over them than asking for them back.
    pub fn lay(&mut self, row: i32, col: i32) {
        if let Some(i) = Self::index(row, col) {
            self.cells[i] = true;
        }
    }

    /// A click lays, or lifts what it finds — the same question
    /// `net::Placement::is_on` asks of a square on the board, so the pad
    /// behaves like the thing it is drawing for.
    pub fn click(&mut self, row: i32, col: i32) {
        if let Some(i) = Self::index(row, col) {
            self.cells[i] = !self.cells[i];
        }
    }

    /// A stamp on the pad, centred, so editing one starts from what it is.
    ///
    /// Centred rather than at the origin because the pad is a fixed square and
    /// a pattern drawn against its top-left has nowhere to grow up or left.
    pub fn of(stamp: &Stamp) -> Self {
        let mut pad = Self::default();
        let (top, left) =
            ((SKETCH_N - stamp.size.0).max(0) / 2, (SKETCH_N - stamp.size.1).max(0) / 2);
        for &(r, c) in &stamp.cells {
            pad.lay(r + top, c + left);
        }
        pad
    }

    pub fn clear(&mut self) {
        self.cells.fill(false);
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|on| !on)
    }

    /// What is drawn, as a stamp, trimmed to the cells that are there.
    ///
    /// `None` when nothing is drawn: an empty stamp is a button that does
    /// nothing, which is worse than a refusal that says why. Trimmed for the
    /// same reason a capture is — where on the pad you happened to draw is not
    /// part of the pattern.
    pub fn to_stamp(&self) -> Option<Stamp> {
        let found: Vec<(i32, i32)> = (0..SKETCH_N)
            .flat_map(|r| (0..SKETCH_N).map(move |c| (r, c)))
            .filter(|&(r, c)| self.at(r, c))
            .collect();
        if found.is_empty() {
            return None;
        }
        Some(Stamp::trimmed(found))
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

/// The version on a saved library, so a format that changes can say so rather
/// than being misread. Same shape as `client::record`, and for the same reason.
const SAVED: &str = "ck-stamps-1";

impl Library {
    /// What was kept last time, or an empty library.
    pub fn remembered() -> Self {
        Self::read(&crate::net::keep::stamps())
    }

    /// Write it down. Called after every change rather than on the way out: a
    /// browser gives no reliable moment to save at, which is the same reason
    /// `client::record` writes when a game ends.
    pub fn remember(&self) {
        crate::net::keep::remember_stamps(&self.written());
    }

    pub fn keep(&mut self, stamp: Stamp) {
        self.stamps.insert(0, stamp);
    }

    /// Replace one in place, keeping where it sits and whether it is pinned.
    ///
    /// **In place**, because editing a stamp is not capturing a new one: a
    /// stamp that jumped to the top of the library and off the bar every time
    /// its owner corrected a cell would be a stamp nobody corrected twice.
    pub fn replace(&mut self, index: usize, stamp: Stamp) {
        if let Some(slot) = self.stamps.get_mut(index) {
            let (was_pinned, name) = (slot.on_bar, slot.name.clone());
            *slot = Stamp { on_bar: was_pinned, name, ..stamp };
        }
    }

    pub fn rename(&mut self, index: usize, name: &str) {
        if let Some(slot) = self.stamps.get_mut(index) {
            slot.name = tidy(name);
        }
    }

    /// Put one on the hotbar, or take it off.
    ///
    /// Refused past [`ON_THE_BAR`], because the bar has ten squares and the
    /// eleventh pin would be one that silently did nothing.
    pub fn pin(&mut self, index: usize, on: bool) -> bool {
        if on && self.stamps.iter().filter(|s| s.on_bar).count() >= ON_THE_BAR {
            return false;
        }
        if let Some(slot) = self.stamps.get_mut(index) {
            slot.on_bar = on;
        }
        true
    }

    /// Which stamps the bar shows, as indices into the library, in order.
    ///
    /// What is pinned, or the newest ten when nothing is — see
    /// [`Stamp::on_bar`]. A slot on the bar is a *place*, and this is the
    /// mapping from that place to the stamp standing in it, which is why the
    /// keys and the squares agree without either knowing about pinning.
    pub fn bar(&self) -> Vec<usize> {
        let pinned: Vec<usize> =
            self.stamps.iter().enumerate().filter(|(_, s)| s.on_bar).map(|(i, _)| i).collect();
        if pinned.is_empty() {
            (0..self.stamps.len().min(ON_THE_BAR)).collect()
        } else {
            pinned
        }
    }

    /// Everything kept, as text, for [`crate::net::keep`].
    ///
    /// One stamp a line, tab separated, with a version on the front: the same
    /// hand-rolled shape `client::record` uses, and for the same reasons —
    /// it is a handful of lines, it is readable in a file somebody may want to
    /// look at, and it costs no dependency.
    pub fn written(&self) -> String {
        let mut out = String::from(SAVED);
        for stamp in &self.stamps {
            let cells: Vec<String> = stamp.cells.iter().map(|(r, c)| format!("{r},{c}")).collect();
            out.push('\n');
            out.push_str(&format!(
                "{}\t{}\t{}",
                tidy(&stamp.name),
                if stamp.on_bar { "bar" } else { "-" },
                cells.join(" ")
            ));
        }
        out
    }

    /// Read one back. Anything unreadable is skipped rather than fatal: a
    /// library is a convenience, and losing all of it because one line is
    /// malformed is the wrong trade.
    pub fn read(text: &str) -> Self {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(SAVED) {
            return Self::default();
        }
        let stamps = lines
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let name = parts.next()?.to_string();
                let pinned = parts.next()? == "bar";
                let cells: Vec<(i32, i32)> = parts
                    .next()?
                    .split_whitespace()
                    .filter_map(|pair| {
                        let (r, c) = pair.split_once(',')?;
                        Some((r.parse().ok()?, c.parse().ok()?))
                    })
                    .collect();
                if cells.is_empty() {
                    return None;
                }
                // Re-trimmed rather than trusted, so a hand-edited file cannot
                // produce a stamp whose `size` disagrees with its cells and
                // draws a preview that is the wrong shape.
                let mut stamp = Stamp::trimmed(cells);
                stamp.name = name;
                stamp.on_bar = pinned;
                Some(stamp)
            })
            .collect();
        Self { stamps }
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

    /// How many squares the bar shows.
    pub fn on_the_bar(&self) -> usize {
        self.bar().len()
    }

    pub fn forget(&mut self, index: usize) {
        if index < self.stamps.len() {
            self.stamps.remove(index);
        }
    }
}

/// A name with nothing in it that would break the file it is written to, or
/// the row it is drawn in.
fn tidy(raw: &str) -> String {
    let name: String =
        raw.trim().chars().filter(|c| !c.is_control() && *c != '\t').take(24).collect();
    if name.is_empty() {
        "unnamed".into()
    } else {
        name
    }
}

/// What the player did with the picker this frame.
#[derive(Default)]
pub enum Picked {
    #[default]
    Nothing,
    Hold(usize),
    Forget(usize),
    /// A pattern drawn on the pad rather than taken off the board.
    Keep(Stamp),
    /// Put this one on the hotbar, or take it off.
    Pin(usize, bool),
    /// Call it something.
    Rename(usize, String),
    /// Load it into the pad to be redrawn. Kept in place when it comes back.
    Edit(usize),
    Close,
}

/// The pad's contents, and which library entry they came from.
///
/// `Some` means the next `Keep` replaces that entry rather than adding one, so
/// correcting a cell does not leave the old version behind and does not send
/// the corrected one to the top of the library and off the bar.
pub type Editing = Option<usize>;

/// The whole library, for when there are more stamps than the bar can hold.
///
/// A list rather than a grid of squares: past ten of them you are reading names
/// and sizes, not recognising shapes, and a list is what reading wants.
/// `what` is the material the hotbar is holding, and everything here is drawn
/// in it: a stamp is a shape, so a thumbnail shows that shape in what it would
/// come out as rather than a fixed picture of how it was captured.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    library: &Library,
    sketch: &mut Sketch,
    player: PlayerId,
    sheet: Option<egui::TextureId>,
    // What is being typed into a name box. Held by the client because this
    // panel is rebuilt every frame and a half-typed name would vanish between
    // two of them — the same reason a team's name is.
    naming: &mut Option<(usize, String)>,
    editing: Editing,
) -> crate::client::views::Shown<Picked> {
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
                                match naming {
                                    // Being renamed: the field replaces the
                                    // label, so the row keeps its height and
                                    // nothing below it moves while somebody
                                    // types.
                                    Some((at, text)) if *at == i => {
                                        let field = ui.add_sized(
                                            [110.0, m.button_height],
                                            egui::TextEdit::singleline(text),
                                        );
                                        let done = field.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        if done || ui.small_button(words::KEEP_NAME).clicked() {
                                            picked = Picked::Rename(i, text.clone());
                                        }
                                    }
                                    _ => {
                                        if ui
                                            .selectable_label(
                                                false,
                                                format!(
                                                    "{}  ·  {} cells",
                                                    stamp.name,
                                                    stamp.cells.len()
                                                ),
                                            )
                                            .on_hover_text(words::RENAME_HINT)
                                            .clicked()
                                        {
                                            *naming = Some((i, stamp.name.clone()));
                                        }
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button(words::FORGET).clicked() {
                                            picked = Picked::Forget(i);
                                        }
                                        if ui
                                            .small_button(words::EDIT)
                                            .on_hover_text(words::EDIT_HINT)
                                            .clicked()
                                        {
                                            picked = Picked::Edit(i);
                                        }
                                        // The bar has ten squares, so the
                                        // eleventh pin is refused rather than
                                        // silently doing nothing.
                                        let mut on = stamp.on_bar;
                                        if ui
                                            .checkbox(&mut on, words::ON_BAR)
                                            .on_hover_text(words::ON_BAR_HINT)
                                            .changed()
                                        {
                                            picked = Picked::Pin(i, on);
                                        }
                                    },
                                );
                            });
                        }
                    });

                    ui.separator();
                    ui.label(words::DRAW);
                    if let Some(drawn) = pad(editing, ui, theme, sketch, player, sheet) {
                        picked = drawn;
                    }

                    ui.separator();
                    ui.small(words::HOW);
                    if crate::client::views::wide(
                        ui,
                        egui::RichText::new(crate::client::views::words::CLOSE),
                        26.0,
                        theme.palette.surface,
                    )
                    .clicked()
                    {
                        picked = Picked::Close;
                    }
                });
        });

    crate::client::views::Shown::new(area.response.rect, picked)
}

#[cfg(test)]
mod tests {

    fn stamp(name: &str, cells: &[(i32, i32)]) -> Stamp {
        let mut s = Stamp::trimmed(cells.to_vec());
        s.name = name.into();
        s
    }

    /// A library survives a session, so what is written has to read back as
    /// what was written — names, shapes and which are on the bar.
    #[test]
    fn a_library_survives_being_written_down() {
        let mut library = Library::default();
        library.keep(stamp("glider", &[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]));
        library.keep(stamp("block", &[(0, 0), (0, 1), (1, 0), (1, 1)]));
        assert!(library.pin(0, true));

        let back = Library::read(&library.written());
        assert_eq!(back.len(), 2);
        for i in 0..2 {
            let (a, b) = (library.get(i).unwrap(), back.get(i).unwrap());
            assert_eq!(
                (&a.name, &a.cells, a.size, a.on_bar),
                (&b.name, &b.cells, b.size, b.on_bar)
            );
        }
        assert_eq!(back.bar(), vec![0], "the pin did not survive");
    }

    /// Nothing to read is an empty library rather than a panic, and so is
    /// anything this does not recognise — a file written by a later build, or
    /// one somebody edited.
    #[test]
    fn an_unreadable_library_is_an_empty_one() {
        for text in ["", "ck-stamps-99\nglider\t-\t0,0", "nonsense", "ck-stamps-1"] {
            assert!(Library::read(text).is_empty(), "{text:?}");
        }
        // A bad line is skipped and the rest is kept, because losing a whole
        // library to one malformed row is the wrong trade.
        let mixed = "ck-stamps-1\nbad\n-\nfine\tbar\t0,0 1,1";
        let back = Library::read(mixed);
        assert_eq!(back.len(), 1);
        assert_eq!(back.get(0).unwrap().name, "fine");
    }

    /// **Nothing pinned is the newest ten**, which is what the bar always
    /// showed; pin one and the bar is exactly what is pinned. Half a rule —
    /// pins first, then the newest of the rest — would reshuffle the bar under
    /// somebody's fingers every time they captured something.
    #[test]
    fn the_bar_is_what_is_pinned_or_the_newest_ten() {
        let mut library = Library::default();
        for i in 0..12 {
            library.keep(stamp(&format!("s{i}"), &[(0, 0)]));
        }
        assert_eq!(library.bar(), (0..ON_THE_BAR).collect::<Vec<_>>());

        assert!(library.pin(11, true));
        assert!(library.pin(4, true));
        assert_eq!(library.bar(), vec![4, 11], "a pinned bar is only what is pinned");

        assert!(library.pin(4, false));
        assert_eq!(library.bar(), vec![11]);
        assert!(library.pin(11, false));
        assert_eq!(library.bar(), (0..ON_THE_BAR).collect::<Vec<_>>(), "back to the newest ten");
    }

    /// The bar has ten squares, so the eleventh pin is refused rather than
    /// silently doing nothing.
    #[test]
    fn the_bar_holds_ten_and_says_so() {
        let mut library = Library::default();
        for i in 0..12 {
            library.keep(stamp(&format!("s{i}"), &[(0, 0)]));
        }
        for i in 0..ON_THE_BAR {
            assert!(library.pin(i, true), "pin {i} was refused");
        }
        assert!(!library.pin(ON_THE_BAR, true), "an eleventh pin was accepted");
        assert_eq!(library.bar().len(), ON_THE_BAR);
    }

    /// **Editing keeps its place and its pin.** A stamp that jumped to the top
    /// of the library and off the bar every time its owner corrected a cell is
    /// a stamp nobody corrects twice.
    #[test]
    fn editing_a_stamp_leaves_it_where_it_was() {
        let mut library = Library::default();
        library.keep(stamp("old", &[(0, 0)]));
        library.keep(stamp("first", &[(0, 0)]));
        assert!(library.pin(1, true));

        library.replace(1, stamp("ignored", &[(0, 0), (0, 1), (1, 0)]));
        let edited = library.get(1).unwrap();
        assert_eq!(edited.cells.len(), 3, "the new shape was not taken");
        assert_eq!(edited.name, "old", "editing renamed it");
        assert!(edited.on_bar, "editing knocked it off the bar");
        assert_eq!(library.len(), 2, "editing added a second copy");
    }

    /// A name reaches a file and a row of a list, so what cannot go in either
    /// is taken out rather than written and read back differently.
    #[test]
    fn a_name_is_tidied_before_it_is_kept() {
        let mut library = Library::default();
        library.keep(stamp("x", &[(0, 0)]));
        library.rename(0, "  a\tname\nwith\tjunk  ");
        assert!(!library.get(0).unwrap().name.contains('\t'));
        library.rename(0, "   ");
        assert_eq!(library.get(0).unwrap().name, "unnamed", "a blank name is not a name");
        // And it survives the round trip it was tidied for.
        library.rename(0, "glider gun");
        assert_eq!(Library::read(&library.written()).get(0).unwrap().name, "glider gun");
    }

    /// The pad opens on what a stamp is, so editing starts from the pattern
    /// rather than from nothing.
    #[test]
    fn the_pad_opens_on_the_stamp_being_edited() {
        let glider = stamp("glider", &[(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
        let pad = Sketch::of(&glider);
        let back = pad.to_stamp().expect("the pad was empty");
        assert_eq!(back.cells, glider.cells, "the shape changed on the way to the pad");
    }
    use super::*;
    use crate::sim::Kind;

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
        let mut cells: Vec<(i32, i32)> = stamp.cells.clone();
        cells.sort_unstable();
        assert_eq!(cells, vec![(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
    }

    /// The pad asks the same question the board does: what is already there
    /// comes back on a click, and anything else goes down.
    ///
    /// **No material here any more.** The pad carried its own row of them
    /// while a stamp was a pattern *and* a material; a stamp is a shape now,
    /// so this is a shape editor and what it is made of is chosen when it is
    /// laid.
    #[test]
    fn a_click_lays_or_lifts_and_a_drag_only_lays() {
        let mut pad = Sketch::default();
        assert!(!pad.at(0, 0));

        pad.click(0, 0);
        assert!(pad.at(0, 0));
        pad.click(0, 0);
        assert!(!pad.at(0, 0), "clicking what is there lifts it");

        // A drag lays and never lifts: a sweep over cells already drawn is far
        // more likely to be drawing over them than asking for them back.
        pad.lay(1, 1);
        pad.lay(1, 1);
        assert!(pad.at(1, 1));
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
            assert!(!pad.at(r, c));
        }
        assert!(pad.is_empty());
    }

    /// **A glider is one stamp and not four.** Four quarter turns come back
    /// to where they started, and each one is the pattern rather than a
    /// pattern beside it.
    #[test]
    fn four_quarter_turns_come_back() {
        let mut pad = Sketch::default();
        // A glider, which is the whole reason this exists: asymmetric under
        // both rotation and reflection, so nothing here can pass by accident.
        for (r, c) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
            pad.lay(r, c);
        }
        let glider = pad.to_stamp().unwrap();

        let mut turn = Turn::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            turn = turn.right();
            let turned = glider.turned(turn);
            assert_eq!(turned.cells.len(), glider.cells.len(), "a turn lost a cell");
            let mut cells = turned.cells.clone();
            cells.sort_unstable();
            seen.push(cells);
        }
        let mut original = glider.cells.clone();
        original.sort_unstable();
        assert_eq!(seen[3], original, "four quarters did not come home");
        assert_ne!(seen[0], original, "a quarter turn changed nothing");
        assert_ne!(seen[0], seen[1]);
        assert_ne!(seen[0], seen[2]);
    }

    /// Turning the other way three times is turning this way once, so the
    /// second binding is a convenience rather than a second transform.
    #[test]
    fn left_is_three_rights() {
        let mut pad = Sketch::default();
        for (r, c) in [(0, 0), (0, 1), (0, 2), (1, 0)] {
            pad.lay(r, c);
        }
        let stamp = pad.to_stamp().unwrap();
        let left = stamp.turned(Turn::default().left());
        let thrice = stamp.turned(Turn::default().right().right().right());
        assert_eq!(left.cells, thrice.cells);
    }

    /// **A rotation cannot produce a reflection**, which is why there are two
    /// keys: a glider has four turns and four more that are its mirror image,
    /// and without the second set half of them are unreachable.
    #[test]
    fn mirroring_is_not_any_rotation() {
        let mut pad = Sketch::default();
        for (r, c) in [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)] {
            pad.lay(r, c);
        }
        let glider = pad.to_stamp().unwrap();
        let sorted = |s: &Stamp| {
            let mut c = s.cells.clone();
            c.sort_unstable();
            c
        };

        let mirrored = sorted(&glider.turned(Turn::default().mirror()));
        for quarters in 0..4 {
            let mut turn = Turn::default();
            for _ in 0..quarters {
                turn = turn.right();
            }
            assert_ne!(mirrored, sorted(&glider.turned(turn)), "{quarters} quarters");
        }
    }

    /// A tall pattern turned is a wide one, and it says so: the name is the
    /// size and the size has swapped.
    #[test]
    fn a_turn_swaps_the_size_and_the_name_follows() {
        let mut pad = Sketch::default();
        for r in 0..4 {
            pad.lay(r, 0);
        }
        let tall = pad.to_stamp().unwrap();
        assert_eq!((tall.size, tall.name.as_str()), ((4, 1), "4x1"));
        let wide = tall.turned(Turn::default().right());
        assert_eq!((wide.size, wide.name.as_str()), ((1, 4), "1x4"));
    }

    /// Doing nothing is doing nothing, and it is the common case -- a stamp is
    /// turned once and placed many times.
    #[test]
    fn an_untouched_turn_changes_nothing() {
        let mut pad = Sketch::default();
        pad.lay(0, 0);
        pad.lay(2, 3);
        let stamp = pad.to_stamp().unwrap();
        assert!(Turn::default().is_default());
        assert_eq!(stamp.turned(Turn::default()), stamp);
        assert!(!Turn::default().right().is_default());
        assert!(!Turn::default().mirror().is_default());
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
    /// **One bound, and the sweep is held to it as well as the pad.**
    ///
    /// A capture took whatever rectangle it was given, so a pattern could be
    /// any size at all — and then opening it to edit cropped it to the pad
    /// without saying so. Two limits that disagree are one limit and a silent
    /// loss of whatever fell outside it.
    #[test]
    fn a_sweep_larger_than_the_pad_is_refused_rather_than_cropped() {
        let me = PlayerId(1);
        let wide: Vec<((i32, i32), Kind, PlayerId)> =
            (0..SKETCH_N + 1).map(|c| (((0, c)), Kind::NORMAL, me)).collect();
        let world = world_with(&wide);

        assert!(fits((0, 0), (0, SKETCH_N - 1)), "a stamp exactly the pad's width does not fit");
        assert!(!fits((0, 0), (0, SKETCH_N)), "one wider than the pad fits");
        assert!(!fits((0, 0), (SKETCH_N, 0)), "one taller than the pad fits");

        assert!(
            Stamp::capture(&world, me, (0, 0), (0, SKETCH_N)).is_none(),
            "a sweep wider than the pad was captured anyway"
        );
        let kept = Stamp::capture(&world, me, (0, 0), (0, SKETCH_N - 1))
            .expect("a sweep the pad's own width was refused");
        assert_eq!(kept.size.1, SKETCH_N, "the widest stamp is not the pad's width");
    }

    #[test]
    fn capture_trims_to_what_it_caught() {
        let me = PlayerId(1);
        // A glider at (10, 10).
        let glider = [(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        let world = world_with(&glider.map(|at| (at, Kind::NORMAL, me)));

        // Swept sloppily, far wider than the pattern — but inside what a
        // stamp may be, which is the other half of what `capture` checks.
        let stamp = Stamp::capture(&world, me, (5, 5), (19, 19)).unwrap();
        assert_eq!(stamp.size, (3, 3), "trimmed to the glider, not the sweep");
        assert_eq!(stamp.cells.len(), 5);
        assert_eq!(stamp.name, "3x3");

        // And it lands as the same shape wherever it is put.
        let laid: Vec<(i32, i32)> = stamp.at((100, 200));
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

        // **And the kind does not travel.** A stamp is a shape; a gun of
        // mines and a gun of life are one pattern laid in two materials, and
        // which one is a decision made when it is placed rather than when it
        // was captured.
        assert_eq!(stamp.at((0, 0)), vec![(0, 0), (0, 2)]);
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
