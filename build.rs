//! Cut the icon font down to the glyphs the client actually names.
//!
//! **The whole face is 477 KB for about twelve hundred icons, of which this
//! uses a dozen.** Subsetting it by hand is a script somebody has to remember
//! to run, and a subset built from a stale list draws a blank box on whichever
//! screen happens to use the icon that was missed — silently, because a font
//! with no glyph for a codepoint is not an error.
//!
//! So it happens here. The list is read out of `client::views::glyph`, which
//! is the one place icons are named, so the font and the names cannot drift:
//! adding an icon is a line in that module and nothing else.
//!
//! `allsorts` rather than `subsetter`, which is otherwise the obvious choice
//! and is what Typst uses — it drops the `cmap`, because it subsets for PDF
//! embedding where glyphs are addressed by index. egui looks a glyph up by
//! *character*, so a font with no `cmap` is twelve blank boxes.
//!
//! A build dependency, so none of it ships. It runs on the host even for a
//! wasm build, which is what makes an embedded artifact the right shape for
//! this rather than something loaded at runtime.

use std::path::{Path, PathBuf};

/// Where the icons are named, and the only place they are.
const NAMED: &str = "src/client/views/glyph.rs";
/// The whole face, as it comes from upstream.
const FACE: &str = "assets/fonts/Phosphor-Regular.ttf";

fn main() {
    println!("cargo:rerun-if-changed={NAMED}");
    println!("cargo:rerun-if-changed={FACE}");
    println!("cargo:rerun-if-changed=build.rs");

    let named = std::fs::read_to_string(NAMED).unwrap_or_else(|e| panic!("reading {NAMED}: {e}"));
    let wanted = codepoints(&named);
    assert!(!wanted.is_empty(), "no `\\u{{...}}` constants found in {NAMED}");

    let face = std::fs::read(FACE).unwrap_or_else(|e| panic!("reading {FACE}: {e}"));
    let subset = subset(&face, &wanted);

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("icons.ttf");
    write_if_changed(&out, &subset);
    // To the build log rather than `cargo:warning`, which cargo replays on
    // every build from cache — an informational line that cannot be silenced
    // is noise by the third time you read it. `cargo build -vv` shows this.
    eprintln!(
        "icons: {} glyphs, {} bytes from {} ({}%)",
        wanted.len(),
        subset.len(),
        face.len(),
        subset.len() * 100 / face.len().max(1)
    );
}

/// Every codepoint named in the glyph module, in the order it appears.
///
/// Parsed rather than shared through a data file, so the module stays the
/// readable thing with the documentation on it and there is no second list to
/// keep in step. The shape it looks for is what `rustfmt` produces and what
/// the module's own test asserts: one character, in an escape.
fn codepoints(source: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with("pub const ") || !line.contains("&str") {
            continue;
        }
        let Some(rest) = line.split_once("\"\\u{") else { continue };
        let Some((hex, _)) = rest.1.split_once('}') else { continue };
        if let Ok(c) = u32::from_str_radix(hex, 16) {
            out.push(c);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn subset(face: &[u8], wanted: &[u32]) -> Vec<u8> {
    use allsorts::binary::read::ReadScope;
    use allsorts::font::MatchingPresentation;
    use allsorts::font_data::FontData;
    use allsorts::subset::{CmapTarget, SubsetProfile};

    let file = ReadScope::new(face).read::<FontData>().expect("the icon face is not a font");
    let mut font =
        allsorts::Font::new(file.table_provider(0).expect("no font in the file")).expect("font");

    // Nought is the notdef glyph, which every subset keeps: it is what draws
    // when a codepoint is not there, and a font without it is a font that
    // cannot say so.
    let mut glyphs = vec![0u16];
    for &code in wanted {
        let ch = char::from_u32(code).unwrap_or_else(|| panic!("U+{code:04X} is not a character"));
        match font.lookup_glyph_index(ch, MatchingPresentation::NotRequired, None) {
            (0, _) => panic!(
                "{FACE} has no glyph for U+{code:04X}, which {NAMED} names. \
                 Either the codepoint is wrong or the face is not the one it came from."
            ),
            (glyph, _) => glyphs.push(glyph),
        }
    }
    glyphs.sort_unstable();
    glyphs.dedup();

    let provider = file.table_provider(0).expect("no font in the file");
    // **A Unicode cmap, explicitly.** The default picks the smallest that
    // fits, which for twelve glyphs can be Mac Roman — and a browser rejects a
    // font whose only cmap is that one, so the icons would work natively and
    // be blank on the web.
    allsorts::subset::subset(&provider, &glyphs, &SubsetProfile::Minimal, CmapTarget::Unicode)
        .expect("subsetting the icon face")
}

/// Only touch the file when the bytes differ, so an unchanged font does not
/// make everything downstream of it rebuild.
fn write_if_changed(path: &Path, bytes: &[u8]) {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return;
    }
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}
