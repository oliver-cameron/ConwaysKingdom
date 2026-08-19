//! Write the sprite files. Run once; the PNGs are checked in and editable.
//!
//!     cargo run --example mksprites
//!
//! Each sprite is strictly 16x16 with hard edges — no anti-aliasing, so a cell
//! is pixel art rather than a smooth blob. RGBA, where R is saturation, G is
//! lightness and A is coverage. There is deliberately no hue: it comes from the
//! player at draw time, so one sprite serves every player.
//!
//! The art is ASCII here so a change is legible in a diff. Editing the PNG in
//! an image editor works just as well; this only regenerates the defaults.

const N: usize = 16;

/// ' ' empty, '.' faint, '-' mid, '#' solid, '=' bright edge.
fn encode(rows: [&str; N]) -> Vec<u8> {
    let mut out = vec![0u8; N * N * 4];
    for (y, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        assert_eq!(chars.len(), N, "row {y} is {} wide, not {N}", chars.len());
        for (x, &c) in chars.iter().enumerate() {
            let (sat, light, alpha) = match c {
                ' ' => (0.0, 0.0, 0.0),
                '.' => (0.85, 0.42, 0.35),
                '-' => (0.85, 0.55, 0.75),
                '#' => (0.85, 0.62, 1.00),
                '=' => (0.70, 0.88, 1.00),
                other => panic!("unknown sprite character {other:?}"),
            };
            let at = (y * N + x) * 4;
            out[at] = (sat * 255.0) as u8;
            out[at + 1] = (light * 255.0) as u8;
            out[at + 2] = 0;
            out[at + 3] = (alpha * 255.0) as u8;
        }
    }
    out
}

fn write(name: &str, rows: [&str; N]) {
    let path = format!("assets/sprites/{name}.png");
    let file = std::fs::File::create(&path).expect("create sprite");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), N as u32, N as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(&encode(rows))
        .expect("png data");
    println!("wrote {path}");
}

fn main() {
    // A living cell: a solid block with its corners cut, so a lone cell reads
    // as a rounded tile and a run of them reads as a wall.
    write("normal", [
        "                ",
        "   ##########   ",
        "  ############  ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        " ############## ",
        "  ############  ",
        "   ##########   ",
        "                ",
    ]);

    // A pane: a bright frame with a faint fill, so whatever it covers still
    // reads through it.
    write("glass", [
        "================",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "=..............=",
        "================",
    ]);
}
