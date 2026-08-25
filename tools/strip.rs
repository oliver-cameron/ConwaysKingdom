//! Zero a channel across a sprite sheet.
//!
//! ```text
//! cargo run --bin strip -- b assets/sprites/sheet.png assets/sprites/sheet.png
//! ```
//!
//! Written for the blue channel, which is hue. `cnvt` writes it because it is
//! the honest decomposition of a pixel, but the shader does not read it: hue
//! comes from the cell's player, so one sheet serves every player and two
//! players' cells are the same shape in different colours. A sheet that
//! carries hue is carrying a number nothing will ever look at, and worse, a
//! number that *looks* like it means something — open the sheet later and the
//! art appears to specify a colour it has no say over.
//!
//! It says how much it found before it removes it, because "any lingering
//! blue" is a question as much as an instruction: a sheet that never carried
//! any is worth knowing about too.
//!
//! Any channel, not just blue. Zeroing saturation makes a sheet greyscale,
//! which is a real thing to want; zeroing lightness makes it black, which is
//! not, but a tool that argues about which is which would be wrong more often
//! than it was right.

use std::path::Path;

#[path = "png_io.rs"]
mod png_io;
use png_io::{read_rgba, write_rgba};

/// The channels, in the order they sit in a pixel, and what each means once a
/// sheet has been through `cnvt`.
const CHANNELS: [(&str, &str); 4] = [
    ("r", "saturation"),
    ("g", "lightness"),
    ("b", "hue, which the shader ignores"),
    ("a", "coverage"),
];

fn index_of(name: &str) -> Option<usize> {
    let name = name.trim_start_matches("--").to_ascii_lowercase();
    CHANNELS.iter().position(|(short, _)| *short == name).or_else(|| match name.as_str() {
        "red" => Some(0),
        "green" => Some(1),
        "blue" => Some(2),
        "alpha" => Some(3),
        _ => None,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [channel, input, output] = args.as_slice() else {
        eprintln!("usage: strip <channel> <in.png> <out.png>");
        eprintln!("  Zeroes one channel. In and out may be the same file.");
        for (short, meaning) in CHANNELS {
            eprintln!("    {short}  {meaning}");
        }
        std::process::exit(2);
    };

    let Some(channel) = index_of(channel) else {
        eprintln!("strip: no channel called {channel:?}; try r, g, b or a");
        std::process::exit(2);
    };

    let (width, height, mut pixels) = match read_rgba(Path::new(input)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("strip: {e}");
            std::process::exit(1);
        }
    };

    // Counted before it is cleared, since afterwards there is nothing to
    // count and the answer is the interesting part.
    let mut carried = 0usize;
    let mut largest = 0u8;
    for px in pixels.chunks_exact_mut(4) {
        if px[channel] != 0 {
            carried += 1;
            largest = largest.max(px[channel]);
            px[channel] = 0;
        }
    }

    if channel == 3 && carried > 0 {
        eprintln!("strip: warning -- that was coverage. Every pixel is now invisible.");
    }

    if let Err(e) = write_rgba(Path::new(output), width, height, &pixels) {
        eprintln!("strip: {e}");
        std::process::exit(1);
    }

    let (short, meaning) = CHANNELS[channel];
    println!("{input} -> {output}  {width}x{height}");
    let total = (width as usize) * (height as usize);
    if carried == 0 {
        println!("{short} ({meaning}) was already empty; nothing to remove");
    } else {
        println!(
            "cleared {short} ({meaning}) from {carried} of {total} pixels, largest was {largest}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_is_named_by_letter_or_by_word() {
        assert_eq!(index_of("b"), Some(2));
        assert_eq!(index_of("B"), Some(2));
        assert_eq!(index_of("blue"), Some(2));
        // Tolerated because a habit of writing flags is hard to put down.
        assert_eq!(index_of("--blue"), Some(2));
        assert_eq!(index_of("r"), Some(0));
        assert_eq!(index_of("alpha"), Some(3));
        assert_eq!(index_of("x"), None);
        assert_eq!(index_of(""), None);
    }

    /// Clearing one channel must leave the other three exactly as they were:
    /// the point is to remove a number nothing reads, not to touch the art.
    #[test]
    fn only_the_named_channel_moves() {
        for channel in 0..4 {
            let mut px = vec![10u8, 20, 30, 40, 200, 210, 220, 230];
            let before = px.clone();
            for p in px.chunks_exact_mut(4) {
                p[channel] = 0;
            }
            for (i, (a, b)) in px.iter().zip(&before).enumerate() {
                if i % 4 == channel {
                    assert_eq!(*a, 0, "channel {channel} should be cleared");
                } else {
                    assert_eq!(a, b, "channel {} should not have moved", i % 4);
                }
            }
        }
    }
}
