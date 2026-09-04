//! A face, derived rather than uploaded.
//!
//! **Nobody chooses their picture**, and that is the whole design. A picture
//! somebody picks is a picture somebody has to moderate, and this is a game
//! with no accounts, no email and no way to contact anybody about anything.
//! What it has instead is an answer no other game has to hand: a **pattern**.
//! Seed a small soup from the fingerprint, step it with the game's own rule,
//! and draw what settles. Everybody's face is a still life or an oscillator
//! that is theirs, and it is the same arithmetic the rest of the game is made
//! of. See [planned.md].
//!
//! It costs no storage, no upload path and no moderation, and it is stable:
//! the same key is the same face on every client and every server, forever,
//! because nothing about it is stored anywhere to disagree with.
//!
//! [planned.md]: https://github.com/oliver-cameron/ConwaysKingdom/blob/main/docs/planned.md#a-face

use crate::net::PersonId;

/// Cells a side. Eight is enough for a shape to be a shape and small enough to
/// read at the size a name sits beside.
pub const N: usize = 8;

/// Generations to settle it.
///
/// **Two, and the number is a measurement rather than a taste.** Every step
/// makes a better-looking shape and a less distinctive one, because B3/S23
/// converges: soups that started apart end up at the same still life. Over a
/// thousand keys at this density —
///
/// ```text
///   steps   distinct
///     1       1000
///     2        991
///     3        959
///     4        903
///     5        829
/// ```
///
/// — so four throws away a tenth of everybody's identity to make the picture
/// tidier, which is the wrong way round. Two is one real generation of the
/// rule's shaping at one percent collision.
///
/// **A collision is a repeated picture and never a repeated person.** The
/// fingerprint is what a person *is*, and it is on every row that shows a face
/// — see `net::Seat::label`. This is for recognising somebody you have played,
/// not for telling two strangers apart.
const SETTLE: usize = 2;

/// **Half is drawn and the other half is its reflection**, which is what makes
/// this read as an emblem rather than as noise. Every identicon scheme does it
/// and for the same reason: symmetry is the cheapest signal that a thing was
/// meant.
///
/// The mirror happens **before** the rule runs, so the rule works on a
/// symmetric field and keeps it symmetric — B3/S23 is neighbourhood-symmetric,
/// so a mirrored input stays mirrored. Reflecting afterwards would show a shape
/// the rule never produced.
fn soup(who: &PersonId) -> [[bool; N]; N] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in who.as_str().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    let mut cells = [[false; N]; N];
    for (row, line) in cells.iter_mut().enumerate() {
        for col in 0..N.div_ceil(2) {
            hash = hash.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // A little under half, which leaves room for the rule to do
            // something. At a half it mostly burns down to the same few blocks.
            let live = (hash >> 33) % 100 < 45;
            line[col] = live;
            line[N - 1 - col] = live;
        }
        let _ = row;
    }
    cells
}

/// What settles out of that soup: **the game's own rule**, so a face is made of
/// the same thing the board is.
///
/// Bounded rather than wrapped, and dead outside — a torus of eight would let a
/// glider come back and hit itself, which makes a busier picture but not a more
/// characteristic one.
pub fn face(who: &PersonId) -> [[bool; N]; N] {
    let mut cells = soup(who);
    // **The last generation that had anything in it.** A small bounded soup
    // can die out completely, and a blank face is the one outcome this must
    // not have: everybody gets a picture without choosing one, so there is no
    // "no picture" to fall back on. Walking back to the last living generation
    // costs nothing and removed every empty face in a thousand keys.
    let mut last = cells;
    for _ in 0..SETTLE {
        let mut next = [[false; N]; N];
        for row in 0..N {
            for col in 0..N {
                let mut live = 0usize;
                for dr in -1i32..=1 {
                    for dc in -1i32..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let (r, c) = (row as i32 + dr, col as i32 + dc);
                        if r < 0 || c < 0 || r >= N as i32 || c >= N as i32 {
                            continue;
                        }
                        if cells[r as usize][c as usize] {
                            live += 1;
                        }
                    }
                }
                next[row][col] = if cells[row][col] {
                    crate::sim::SURVIVES_ON.contains(&live)
                } else {
                    crate::sim::BORN_ON.contains(&live)
                };
            }
        }
        cells = next;
        if cells.iter().flatten().any(|c| *c) {
            last = cells;
        }
    }
    last
}

/// **A face before a server has issued you a key.**
///
/// The real one comes off the key, and there is no key until you have joined
/// somewhere — so this is the gap, and an empty box is the wrong thing to put
/// in it. Github's answer is the right one: derive a placeholder from whatever
/// identity there *is*, which here is the name being typed, and let it change
/// when the real thing arrives.
///
/// **Drawn muted, and that is the whole of what makes it honest.** A face that
/// silently became a different face on your first join would read as a bug; one
/// that was visibly provisional and then became yours reads as what it is. So
/// this is the same arithmetic in [`crate::client::views::theme`]'s dim ink
/// rather than in a player's colour.
///
/// Empty name and no key at all still gets a shape, because the point of the
/// scheme is that everybody has one without choosing it.
pub fn show_placeholder(painter: &egui::Painter, rect: egui::Rect, name: &str, dim: egui::Color32) {
    let stand_in = PersonId(if name.trim().is_empty() {
        "nobody".to_string()
    } else {
        name.trim().to_lowercase()
    });
    draw(painter, rect, &face(&stand_in), dim);
}

/// Draw one, in the colour that person's cells are.
///
pub fn show(painter: &egui::Painter, rect: egui::Rect, who: &PersonId) {
    let (r, g, b) = crate::client::views::hue::player_colour(crate::sim::PlayerId(
        crate::client::views::menu::person_hue(who),
    ));
    draw(painter, rect, &face(who), egui::Color32::from_rgb(r, g, b));
}

/// Whole squares that touch, the way a stamp's preview is drawn and for the
/// same reason: a shape this small taken apart by gaps stops being a shape.
fn draw(painter: &egui::Painter, rect: egui::Rect, cells: &[[bool; N]; N], ink: egui::Color32) {
    let side = rect.width().min(rect.height()) / N as f32;
    let span = side * N as f32;
    let origin = rect.center() - egui::vec2(span, span) * 0.5;
    // **Round, and masked by cell rather than clipped.** egui clips to
    // rectangles, so a circular crop has to be drawn rather than asked for —
    // and at eight cells a side, keeping the whole squares whose middles fall
    // inside the circle reads as deliberate where a smooth crop of blocks this
    // large would read as a mistake. It is the same answer the sprite sheet
    // gives: whole texels, no partial coverage.
    let middle = rect.center();
    let radius = span * 0.5;
    for (row, line) in cells.iter().enumerate() {
        for (col, &live) in line.iter().enumerate() {
            if !live {
                continue;
            }
            let at = egui::Rect::from_min_size(
                origin + egui::vec2(col as f32 * side, row as f32 * side),
                egui::vec2(side, side),
            );
            if (at.center() - middle).length() > radius {
                continue;
            }
            painter.rect_filled(at, 0.0, ink);
        }
    }
    // The edge said out loud, dimly, so a face with little in its outer ring
    // still reads as round rather than as a shape that happens to be small.
    painter.circle_stroke(middle, radius, egui::Stroke::new(1.0, ink.gamma_multiply(0.35)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(s: &str) -> PersonId {
        PersonId(s.into())
    }

    /// **The same key is the same face, everywhere and forever.** Nothing about
    /// it is stored, so this is the only thing making it stable.
    #[test]
    fn one_key_is_one_face() {
        assert_eq!(face(&who("aaaa1111")), face(&who("aaaa1111")));
    }

    /// **Nearly every key is its own face**, and nearly is the honest word:
    /// B3/S23 converges, so some soups that started apart settle together. The
    /// step count is chosen against exactly this — see [`SETTLE`] — and what
    /// keeps it from mattering is that a face is for recognising somebody you
    /// have played, while the fingerprint beside it is what says who they are.
    #[test]
    fn nearly_every_key_is_its_own_face() {
        let mut seen = std::collections::HashSet::new();
        let n = 1000;
        for i in 0..n {
            seen.insert(face(&who(&format!("key{i:04}"))));
        }
        assert!(seen.len() * 100 >= n * 98, "only {} of {n} keys have their own face", seen.len());
    }

    /// **Symmetric, which is what makes it read as an emblem.** The soup is
    /// mirrored before the rule runs and B3/S23 is neighbourhood-symmetric, so
    /// what settles is symmetric too — this is the property that would break
    /// silently if the mirror ever moved after the steps.
    #[test]
    fn a_face_is_its_own_reflection() {
        for i in 0..50 {
            let f = face(&who(&format!("key{i:04}")));
            for (row, line) in f.iter().enumerate() {
                for col in 0..N {
                    assert_eq!(line[col], line[N - 1 - col], "key {i} row {row} is not mirrored");
                }
            }
        }
    }

    /// **Something is always there.** A blank face is a person with no picture,
    /// and the whole point is that everybody has one without choosing it.
    #[test]
    fn no_face_settles_to_nothing() {
        for i in 0..500 {
            let f = face(&who(&format!("key{i:04}")));
            let live: usize = f.iter().flatten().filter(|c| **c).count();
            assert!(live > 0, "key {i} settled to an empty face");
        }
    }
}
