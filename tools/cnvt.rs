//! Convert an ordinary RGBA sprite sheet into the one the shader reads, and
//! back again.
//!
//! ```text
//! cargo run --bin cnvt -- art.png assets/sprites/sheet.png
//! cargo run --bin cnvt -- --back assets/sprites/sheet.png art.png
//! cargo run --bin cnvt -- --back --player 3 assets/sprites/sheet.png as-p3.png
//! ```
//!
//! The reverse exists so a sheet can be opened. In the atlas format a sheet is
//! unreadable — three of its channels are numbers fed to a colour model, so a
//! paint program shows something that looks nothing like the art. Converting
//! back gives you a picture you can look at, edit, and convert forward again.
//!
//! `--player N` reverses it the way the *game* will draw it: taking the hue
//! and saturation tier from player N rather than from the sheet, since that is
//! what the shader does. `--player 0` is unowned, which is grey. Without it
//! the sheet's own hue is used, which is the true inverse of the forward pass
//! and so is what a round trip should reproduce.
//!
//! The atlas is not a picture. Its channels are the arguments to `shade()` in
//! `render/shaders/grid.wgsl`, which builds a colour in OKLab so that one sheet
//! serves every player:
//!
//! | channel | meaning |
//! |---|---|
//! | R | saturation, 0..1 |
//! | G | lightness, 0..1 |
//! | B | hue, 0..1 for a full turn |
//! | A | coverage, used to composite over the ground |
//!
//! So art is drawn in any editor, in ordinary colours, and converted here —
//! rather than authored channel by channel in a space nobody can see.
//!
//! **The shader ignores B today.** It takes the hue from the cell's player, so
//! two players' cells are the same shape in different colours, and a sprite
//! with a hue of its own would break that. The channel is written anyway
//! because it is the honest decomposition of the pixel and it costs nothing:
//! a sheet that wants its own hue needs only a shader that reads it.
//!
//! Standalone: `png` and `std`, no dependency on the crate. The sprites are
//! embedded with `include_bytes!`, so the crate cannot build until they exist,
//! and a tool that needed the crate could not make them.

use std::f32::consts::TAU;
use std::path::Path;

// Shared with the other tools rather than copied into each. A `#[path]` module
// because these are binaries, not library code: the crate is the game, and
// nothing the game ships should have to carry the art pipeline.
#[path = "png_io.rs"]
mod png_io;
use png_io::{read_rgba, write_rgba};

/// What `shade()` multiplies saturation by before it becomes chroma. Taper
/// towards black and white, where no hue has any chroma to spare.
fn taper(lightness: f32) -> f32 {
    1.0 - (2.0 * lightness - 1.0).abs()
}

/// The chroma `shade()` asks for at full saturation. Mirrors the constant in
/// the shader; the two have to agree or a converted sheet comes back wrong.
const MAX_CHROMA: f32 = 0.30;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn linear_to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    ]
}

fn oklab_to_linear([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

fn in_gamut(c: [f32; 3]) -> bool {
    c.iter().all(|v| (-0.0005..=1.0005).contains(v))
}

/// `shade()` from the shader, in Rust, so a conversion can be checked against
/// the thing that will actually draw it. Any drift between the two shows up
/// here as a round-trip error rather than as art that looks wrong on screen.
fn shade(lightness: f32, saturation: f32, hue: f32) -> [f32; 3] {
    let (dx, dy) = (hue.cos(), hue.sin());
    let chroma = MAX_CHROMA * saturation * taper(lightness);

    let full = oklab_to_linear([lightness, chroma * dx, chroma * dy]);
    if in_gamut(full) {
        return full.map(|v| v.clamp(0.0, 1.0));
    }
    // Bisect the chroma down until it fits, which keeps hue and lightness
    // exactly and gives up only the saturation that could not be shown.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..8 {
        let mid = (lo + hi) * 0.5;
        let c = chroma * mid;
        if in_gamut(oklab_to_linear([lightness, c * dx, c * dy])) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let c = chroma * lo;
    oklab_to_linear([lightness, c * dx, c * dy]).map(|v| v.clamp(0.0, 1.0))
}

/// Golden ratio: consecutive player numbers land far apart on the hue circle.
/// Mirrors `player_hue` in the shader.
const HUE_STEP: f32 = 0.618_033_99;

/// What the shader will draw a sheet's pixel as, for a given player.
///
/// Mirrors `player_hue` and `player_saturation` in `grid.wgsl`. Player zero is
/// nobody: unclaimed ground has no colour of its own, so it is grey however
/// much saturation the art asks for.
fn player_shade(sheet: [u8; 4], player: u8) -> [f32; 3] {
    let hue = (player as f32 * HUE_STEP).fract() * TAU;
    let tier = if player == 0 {
        0.0
    } else if player % 2 == 1 {
        1.0
    } else {
        0.55
    };
    shade(sheet[1] as f32 / 255.0, sheet[0] as f32 / 255.0 * tier, hue)
}

/// One pixel, from sRGB bytes to the sheet's four channels.
///
/// Saturation is the pixel's chroma as a fraction of what `shade()` would ask
/// for at full saturation — the exact inverse, so the shader reproduces the
/// pixel. A colour more saturated than `MAX_CHROMA` allows clamps to 1 and
/// comes back a little duller; there is no room above 1 to say otherwise.
fn convert(px: [u8; 4]) -> [u8; 4] {
    let linear = [
        srgb_to_linear(px[0] as f32 / 255.0),
        srgb_to_linear(px[1] as f32 / 255.0),
        srgb_to_linear(px[2] as f32 / 255.0),
    ];
    let [lightness, a, b] = linear_to_oklab(linear);
    let chroma = (a * a + b * b).sqrt();
    let room = MAX_CHROMA * taper(lightness);
    let saturation = if room > 1e-6 { (chroma / room).min(1.0) } else { 0.0 };
    let hue = b.atan2(a).rem_euclid(TAU);

    [
        (saturation * 255.0).round() as u8,
        (lightness * 255.0).round() as u8,
        (hue / TAU * 255.0).round() as u8,
        px[3],
    ]
}

/// A sheet pixel back to sRGB bytes, using the sheet's own hue.
///
/// The true inverse of [`convert`], which is what makes it worth reporting a
/// round trip against: any drift between this and `shade()` in the shader
/// shows up as an error here rather than as art that looks wrong on screen.
fn back(sheet: [u8; 4]) -> [u8; 3] {
    let rgb =
        shade(sheet[1] as f32 / 255.0, sheet[0] as f32 / 255.0, sheet[2] as f32 / 255.0 * TAU);
    rgb.map(|v| (linear_to_srgb(v).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A whole sheet back to something you can look at. `player` picks whose hue
/// to draw it in; `None` uses the hue the sheet carries.
fn reverse(pixels: &[u8], player: Option<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len());
    for px in pixels.chunks_exact(4) {
        let sheet = [px[0], px[1], px[2], px[3]];
        let rgb = match player {
            Some(p) => player_shade(sheet, p)
                .map(|v| (linear_to_srgb(v).clamp(0.0, 1.0) * 255.0).round() as u8),
            None => back(sheet),
        };
        out.extend_from_slice(&rgb);
        // Coverage is kept rather than composited, so the result can be edited
        // and converted forward again without a background baked into it.
        out.push(sheet[3]);
    }
    out
}

fn main() {
    let mut back_wards = false;
    let mut player: Option<u8> = None;
    let mut paths: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--back" => back_wards = true,
            "--player" => match args.next().and_then(|v| v.parse::<u8>().ok()) {
                Some(p) if p <= 31 => player = Some(p),
                _ => {
                    eprintln!("cnvt: --player takes a number from 0 to 31");
                    std::process::exit(2);
                }
            },
            other => paths.push(other.to_string()),
        }
    }

    let [input, output] = paths.as_slice() else {
        eprintln!("usage: cnvt [--back [--player N]] <in.png> <out.png>");
        eprintln!("  Forward: an RGBA sheet into the atlas format -- R saturation,");
        eprintln!("  G lightness, B hue, A coverage.");
        eprintln!("  --back:  the atlas format into something you can look at.");
        eprintln!("  --player N: draw it the way the game will, in player N's");
        eprintln!("  colour. 0 is unowned, which is grey.");
        std::process::exit(2);
    };
    if player.is_some() && !back_wards {
        eprintln!("cnvt: --player only means anything with --back");
        std::process::exit(2);
    }

    let (width, height, pixels) = match read_rgba(Path::new(input)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cnvt: {e}");
            std::process::exit(1);
        }
    };

    let (out, report) = if back_wards {
        let what = match player {
            Some(p) => format!("as player {p} sees it"),
            None => "in the sheet's own hue".to_string(),
        };
        (reverse(&pixels, player), format!("reversed {what}"))
    } else {
        forward(&pixels, width)
    };

    if let Err(e) = write_rgba(Path::new(output), width, height, &out) {
        eprintln!("cnvt: {e}");
        std::process::exit(1);
    }

    println!("{input} -> {output}  {width}x{height}");
    println!("{report}");
}

/// The forward pass, and how faithful it was.
///
/// The worst error any visible pixel takes on a round trip, so a sheet that
/// will not survive one says so here rather than on screen.
fn forward(pixels: &[u8], width: u32) -> (Vec<u8>, String) {
    let mut out = Vec::with_capacity(pixels.len());
    let mut worst = 0i32;
    let mut worst_at = (0u32, 0u32);

    for (i, px) in pixels.chunks_exact(4).enumerate() {
        let px = [px[0], px[1], px[2], px[3]];
        let sheet = convert(px);
        out.extend_from_slice(&sheet);

        if px[3] == 0 {
            continue; // nothing to see, so nothing to be faithful to
        }
        let error = back(sheet)
            .iter()
            .zip(&px[..3])
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap_or(0);
        if error > worst {
            worst = error;
            worst_at = (i as u32 % width, i as u32 / width);
        }
    }

    let mut report = format!("worst round trip: {worst}/255 at ({}, {})", worst_at.0, worst_at.1);
    if worst > 8 {
        report.push_str(
            "\n  Colours past what OKLab can show at that lightness clamp to full\n  \
             saturation and come back duller. Lighten or desaturate to fix.",
        );
    }
    (out, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two directions are inverses. This is the property the pair exists
    /// for, and the one that quietly breaks if `shade` here and `shade` in the
    /// shader ever drift apart.
    #[test]
    fn a_colour_survives_the_round_trip() {
        // Greys and moderate colours, which is what the art is. Vivid colours
        // at extreme lightness are outside what the format can say and are
        // covered by the test below.
        for &px in &[
            [0u8, 0, 0, 255],
            [255, 255, 255, 255],
            [128, 128, 128, 255],
            [140, 90, 70, 255],
            [70, 110, 140, 255],
            [90, 140, 90, 255],
        ] {
            let there = convert(px);
            let and_back = back(there);
            for (a, b) in and_back.iter().zip(&px[..3]) {
                assert!((*a as i32 - *b as i32).abs() <= 2, "{px:?} came back {and_back:?}");
            }
        }
    }

    /// A grey has no hue to lose, so it must come back exactly — any drift
    /// here is arithmetic error rather than the format running out of room.
    #[test]
    fn greys_are_exact() {
        for level in [0u8, 17, 64, 128, 200, 255] {
            let px = [level, level, level, 255];
            let and_back = back(convert(px));
            assert_eq!(and_back, [level, level, level], "grey {level} drifted");
            assert_eq!(convert(px)[0], 0, "a grey has no saturation");
        }
    }

    /// Colour the format cannot hold says so, rather than pretending.
    #[test]
    fn a_colour_too_vivid_to_hold_comes_back_duller() {
        // Full green: far more chroma than `MAX_CHROMA` allows at that
        // lightness, so saturation clamps and the trip loses some of it.
        let px = [0u8, 255, 0, 255];
        assert_eq!(convert(px)[0], 255, "clamped to full saturation");
        let and_back = back(convert(px));
        let error = and_back
            .iter()
            .zip(&px[..3])
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .max()
            .unwrap();
        assert!(error > 8, "this one is meant to be lossy, got {error}");
    }

    /// Player zero is nobody, and nobody's ground is grey. Mirrors the rule in
    /// the shader, so `--back --player 0` shows what unclaimed ground looks
    /// like rather than a hue nobody has.
    #[test]
    fn player_zero_reverses_to_grey() {
        let sheet = convert([150, 90, 60, 255]);
        let rgb = player_shade(sheet, 0);
        assert!(
            (rgb[0] - rgb[1]).abs() < 1e-4 && (rgb[1] - rgb[2]).abs() < 1e-4,
            "unowned should have no colour, got {rgb:?}"
        );
        // And a real player does have one.
        let owned = player_shade(sheet, 1);
        assert!((owned[0] - owned[2]).abs() > 1e-3, "player one should be coloured");
    }

    /// Coverage is carried through untouched in both directions, so a sheet
    /// can be reversed, edited and converted forward without a background
    /// being baked into it.
    #[test]
    fn coverage_is_carried_not_composited() {
        for alpha in [0u8, 1, 128, 255] {
            assert_eq!(convert([10, 20, 30, alpha])[3], alpha);
            let reversed = reverse(&[10, 20, 30, alpha], None);
            assert_eq!(reversed[3], alpha);
        }
    }
}
