//! Convert an ordinary RGBA sprite sheet into the one the shader reads.
//!
//! ```text
//! cargo run --bin cnvt -- sheet.png assets/sprites/alive.png
//! ```
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

/// What the shader would draw from a converted pixel, back in sRGB bytes.
/// Used only to report how faithful the conversion was.
fn back(sheet: [u8; 4]) -> [u8; 3] {
    let rgb = shade(
        sheet[1] as f32 / 255.0,
        sheet[0] as f32 / 255.0,
        sheet[2] as f32 / 255.0 * TAU,
    );
    rgb.map(|v| (linear_to_srgb(v).clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn read_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{}: expected 8-bit RGBA, found {:?} at {:?}",
            path.display(),
            frame.color_type,
            frame.bit_depth
        ));
    }
    buf.truncate(frame.buffer_size());
    Ok((frame.width, frame.height, buf))
}

fn write_rgba(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .write_image_data(pixels)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: cnvt <in.png> <out.png>");
        eprintln!("  Converts an RGBA sheet into the atlas format: R saturation,");
        eprintln!("  G lightness, B hue, A coverage. See the top of tools/cnvt.rs.");
        std::process::exit(2);
    };

    let (width, height, pixels) = match read_rgba(Path::new(input)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cnvt: {e}");
            std::process::exit(1);
        }
    };

    let mut out = Vec::with_capacity(pixels.len());
    // The largest error any visible pixel takes on the round trip, so a sheet
    // that will not survive it says so here rather than on screen.
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

    if let Err(e) = write_rgba(Path::new(output), width, height, &out) {
        eprintln!("cnvt: {e}");
        std::process::exit(1);
    }

    println!("{input} -> {output}  {width}x{height}");
    println!(
        "worst round trip: {worst}/255 at ({}, {})",
        worst_at.0, worst_at.1
    );
    if worst > 8 {
        println!(
            "  Colours past what OKLab can show at that lightness clamp to full\n  \
             saturation and come back duller. Lighten or desaturate to fix."
        );
    }
}
