//! The icons, by what they do rather than by what they are.
//!
//! **Phosphor**, embedded beside the two IBM Plex faces and used the same way:
//! `include_bytes!`, an egui family, and a `RichText` in that family. No
//! dependency — `egui-phosphor` exists and does exactly this, and it tracks
//! egui 0.35 where this crate is on 0.36, so `cargo add` pulls **a second
//! egui** beside the one already here along with ten transitive crates and a
//! Rust 1.92 floor. Two egui versions cannot share a `FontDefinitions`, which
//! is the one type the integration exists to take.
//!
//! ## Named here, never spelled at a call site
//!
//! For the reason [`super::words`] exists: a codepoint in the middle of a
//! layout is a magic number that nobody can read, check or grep for, and one
//! that is *wrong* draws a blank box rather than failing. A name here is the
//! one place the mapping lives, and `glyphs_are_in_the_font` below is what
//! says every one of them is really in the file.
//!
//! ## What it costs, and how to make it cost less
//!
//! The whole regular face is 477 KB for about twelve hundred icons, of which
//! this uses a dozen. `tools/subset-icons.sh` cuts it to the ones named below
//! — measured at **4.2 KB**, which is a hundredth of the size — and it reads
//! its codepoint list out of this file, so the two cannot drift. The whole
//! font is here rather than the subset because any icon is then a line in this
//! module rather than a line plus a rebuild; swap when the size matters more
//! than that does, and the test below is what makes the swap safe.

/// The family name the fonts are registered under. Its own family rather than
/// a fallback on the text one, so a missing glyph is a blank box in an icon
/// slot and never a letter silently drawn from the wrong face.
pub const FAMILY: &str = "phosphor";

/// Run the world.
pub const PLAY: &str = "\u{e3d0}";
/// Stop it.
pub const PAUSE: &str = "\u{e39e}";
/// One generation, and stay stopped.
pub const STEP: &str = "\u{e5a6}";
/// The rules panel: what the game is doing here.
pub const GEAR: &str = "\u{e270}";
/// Out of this screen. The one control whose job is to be recognised at a
/// glance, and which was a hand-painted arrow because there was no font.
pub const BACK: &str = "\u{e058}";
/// Every key, on one screen.
pub const HELP: &str = "\u{e3e8}";
/// The stamp library.
pub const LIBRARY: &str = "\u{e466}";
/// Take a rectangle of the world as a stamp.
pub const CAPTURE: &str = "\u{e1d4}";
/// What a player has to spend.
pub const PURSE: &str = "\u{e78e}";
/// The generation.
pub const CLOCK: &str = "\u{e19a}";
/// Ground held.
pub const GROUND: &str = "\u{e244}";
/// Rating.
pub const RATING: &str = "\u{e67e}";

/// Every one of them, for the test and for the subsetter.
pub const ALL: &[(&str, &str)] = &[
    ("PLAY", PLAY),
    ("PAUSE", PAUSE),
    ("STEP", STEP),
    ("GEAR", GEAR),
    ("BACK", BACK),
    ("HELP", HELP),
    ("LIBRARY", LIBRARY),
    ("CAPTURE", CAPTURE),
    ("PURSE", PURSE),
    ("CLOCK", CLOCK),
    ("GROUND", GROUND),
    ("RATING", RATING),
];

/// An icon, sized like the text it sits with.
pub fn text(glyph: &str, size: f32) -> egui::RichText {
    egui::RichText::new(glyph).size(size).family(egui::FontFamily::Name(FAMILY.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every name is one character in the private use area.**
    ///
    /// Two glyphs in a constant would draw two icons in a slot sized for one,
    /// and a codepoint outside the private area is a letter somebody typed by
    /// accident — both of which look like a layout bug rather than a wrong
    /// number.
    #[test]
    fn every_glyph_is_one_private_character() {
        for (name, glyph) in ALL {
            let mut chars = glyph.chars();
            let c = chars.next().unwrap_or_else(|| panic!("{name} is empty"));
            assert!(chars.next().is_none(), "{name} is more than one character");
            assert!(
                ('\u{e000}'..='\u{f8ff}').contains(&c),
                "{name} is U+{:04X}, which is not in the private use area — an icon font \
                 puts its glyphs there, so this is a letter rather than an icon",
                c as u32
            );
        }
    }

    /// No two names are the same icon, which is what a copied line looks like.
    #[test]
    fn no_two_names_are_one_glyph() {
        for (i, (name, glyph)) in ALL.iter().enumerate() {
            for (other, same) in &ALL[i + 1..] {
                assert_ne!(glyph, same, "{name} and {other} are the same icon");
            }
        }
    }

    /// **Every named glyph is really in the font that ships.**
    ///
    /// The one that matters, and the one that makes
    /// `tools/subset-icons.sh` safe to run: a name whose glyph was left out of
    /// a subset draws a blank box, silently, on whichever screen happens to
    /// use it. Read out of the embedded bytes rather than trusted, by walking
    /// the `cmap` — which is a few dozen lines and needs no dependency.
    #[test]
    fn glyphs_are_in_the_font() {
        let have = super::super::font_codepoints(super::super::ICON_FONT);
        assert!(have.len() > 10, "only {} codepoints read; the parse is wrong", have.len());
        for (name, glyph) in ALL {
            let c = glyph.chars().next().unwrap() as u32;
            assert!(
                have.contains(&c),
                "{name} is U+{c:04X} and the font does not have it — if this followed a run \
                 of tools/subset-icons.sh, the subset was built from an older list"
            );
        }
    }
}
