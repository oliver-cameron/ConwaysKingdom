//! Reading and writing the only kind of PNG the tools deal in: 8-bit RGBA.
//!
//! Shared by the tools in this directory through `#[path]`, so the plumbing is
//! written once without putting it in the crate — the crate is the game, and
//! the art pipeline is not part of what it ships.
//!
//! Deliberately strict about the format. A 16-bit or palette PNG could be
//! converted, but silently accepting one means silently changing the art's
//! depth, and a sprite sheet that quietly lost precision on the way through is
//! worse than one that would not open.

use std::path::Path;

pub fn read_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let frame = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
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

pub fn write_rgba(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
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
