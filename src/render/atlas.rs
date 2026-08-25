//! The sprites a cell is drawn from.
//!
//! One 256x256 sheet of 16x16 tiles. A cell's tile byte is the index into it,
//! low nibble across and high nibble down, and that byte already carries alive
//! and ice — so what a cell looks like is one number, and finding its picture
//! is arithmetic rather than a lookup.
//!
//! Tiles are **strictly 16x16 and not anti-aliased**: sampling is nearest and
//! there is no mip chain, so a cell is pixel art at every zoom rather than a
//! blurred blob. A chunk is 16 cells of 16 texels, so it spans 256x256 — a `u8`
//! on each axis, which is the coordinate space the shader works in.
//!
//! Sprites carry **no hue**. A texel is saturation, lightness and coverage; the
//! hue arrives at draw time from the cell's player number, so one set of art
//! serves every player.

/// Texels along one edge of a tile — one cell's worth of picture.
pub const TILE_N: u32 = 16;
/// Tiles along one edge of a sheet, so 256 of them per state.
pub const SHEET_TILES: u32 = 16;
/// A sheet's edge in texels.
pub const SHEET_N: u32 = TILE_N * SHEET_TILES;

/// The one sheet. A cell's tile byte is the index into it: low nibble across,
/// high nibble down, so 256 tiles in a 16x16 grid of 16x16 texels.
///
/// One sheet rather than a layer per state. The tile byte already carries
/// alive and ice in its bottom two bits, so a kind's four pictures are four
/// consecutive tiles and there is nothing to look up — no layer to choose, no
/// UV to carry, and one unconditional sample, which is what WGSL needs since
/// it forbids implicit derivatives in non-uniform control flow.
///
/// **Provisional art.** This sheet is a stand-in: four flat tiles so the four
/// states are told apart and the game is playable. Redraw it and drop it in;
/// nothing in the code needs to change, because the mapping is the tile byte
/// and nothing else.
const SHEET: &[u8] = include_bytes!("../../assets/sprites/sheet.png");

pub struct Atlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Atlas {
    /// `Rgba8Unorm`, not `Srgb`: these are not colours. R and G are saturation
    /// and lightness fed to a colour model, and sRGB decoding would bend them.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprites"),
            size: wgpu::Extent3d { width: SHEET_N, height: SHEET_N, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texels = decode(SHEET).unwrap_or_else(|e| {
            // Falling back beats refusing to start: bad art should cost you
            // the art, not the game.
            log::error!("the sprite sheet is unusable ({e}); drawing a placeholder");
            placeholder()
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SHEET_N * 4),
                rows_per_image: Some(SHEET_N),
            },
            wgpu::Extent3d { width: SHEET_N, height: SHEET_N, depth_or_array_layers: 1 },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Nearest, and no mip chain: the sprites are pixel art and are meant to
        // stay crisp. The cost is that far-out zoom point-samples a 16x16
        // sprite down to a pixel or two and will shimmer -- which is why the
        // camera does not zoom below one pixel per cell.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self { texture, view, sampler }
    }
}

/// The sheet's texels, or `None` if the art is unusable.
///
/// The interface draws the same sprites the world does — a hotbar button
/// showing the cell says more than one spelling its name — and it draws them
/// through egui rather than through this pipeline, so it needs the pixels
/// rather than the GPU texture.
pub fn decoded() -> Option<Vec<u8>> {
    decode(SHEET).ok()
}

/// A 16x16 RGBA PNG, as raw texels.
fn decode(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.width != SHEET_N || info.height != SHEET_N {
        return Err(format!(
            "sheet is {}x{}, expected {SHEET_N}x{SHEET_N}",
            info.width, info.height
        ));
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if frame.color_type != png::ColorType::Rgba {
        return Err(format!("{:?} PNG; needs RGBA", frame.color_type));
    }
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(format!("{:?} PNG; needs 8 bits per channel", frame.bit_depth));
    }
    buf.truncate((SHEET_N * SHEET_N * 4) as usize);
    Ok(buf)
}

/// A hollow square, so a missing sprite is obvious rather than invisible.
fn placeholder() -> Vec<u8> {
    let mut out = vec![0u8; (SHEET_N * SHEET_N * 4) as usize];
    for y in 0..SHEET_N {
        for x in 0..SHEET_N {
            let edge = x % TILE_N == 0 || y % TILE_N == 0;
            let at = ((y * SHEET_N + x) * 4) as usize;
            out[at] = 255;
            out[at + 1] = 128;
            out[at + 3] = if edge { 255 } else { 0 };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Cell, Kind, PlayerId};

    /// The sheet is the size the mapping assumes. A tile byte indexes a 16x16
    /// grid of 16x16 tiles and nothing checks that at runtime, so it is
    /// checked here.
    #[test]
    fn the_sheet_is_the_shape_the_tile_byte_assumes() {
        let texels = decode(SHEET).expect("the sheet must decode");
        assert_eq!(texels.len(), (SHEET_N * SHEET_N * 4) as usize);
        assert_eq!(SHEET_N, TILE_N * SHEET_TILES);
        assert_eq!(SHEET_TILES * SHEET_TILES, 256, "a tile byte must reach every tile");
    }

    /// Every state a cell can be in must have art drawn at the tile its byte
    /// points to. Dead and ice-free is allowed to be blank -- it is empty
    /// ground -- but a state you cannot see is a state you cannot play
    /// against.
    #[test]
    fn every_state_has_art_at_the_tile_its_byte_names() {
        let texels = decode(SHEET).expect("the sheet must decode");
        for kind in Kind::ALL {
            for (alive, ice) in [(false, false), (true, false), (false, true), (true, true)] {
                let cell = Cell::DEAD
                    .with_kind(kind)
                    .with_alive(alive)
                    .with_ice(ice)
                    .with_player(PlayerId(1));
                let tile = cell.tile();
                let (tx, ty) = ((tile % 16) as u32, (tile / 16) as u32);

                let covered = (0..TILE_N)
                    .flat_map(|y| (0..TILE_N).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let (px, py) = (tx * TILE_N + x, ty * TILE_N + y);
                        texels[(((py * SHEET_N + px) * 4) + 3) as usize] > 8
                    })
                    .count();
                if alive || ice {
                    assert!(
                        covered > (TILE_N * TILE_N / 8) as usize,
                        "Kind({}) alive={alive} ice={ice}: tile {tile} is blank",
                        kind.0
                    );
                }
            }
        }
    }

    /// A kind's four states are four consecutive tiles, which is the whole
    /// reason alive and ice live in the low bits of the tile byte.
    #[test]
    fn a_kinds_states_are_consecutive_tiles() {
        for kind in Kind::ALL {
            let base = Cell::DEAD.with_kind(kind);
            let tiles: Vec<u8> = [(false, false), (true, false), (false, true), (true, true)]
                .iter()
                .map(|&(a, i)| base.with_alive(a).with_ice(i).tile())
                .collect();
            let first = tiles[0];
            assert_eq!(tiles, vec![first, first + 1, first + 2, first + 3]);
        }
    }

    /// No anti-aliasing: coverage is one of a few fixed inks, never a ramp.
    /// Sampling is nearest with no mip chain, so a soft edge is a soft edge at
    /// every zoom rather than art that resolves when you look closer.
    #[test]
    fn sprites_have_hard_edges() {
        let texels = decode(SHEET).expect("the sheet must decode");
        for t in texels.chunks(4) {
            assert!(t[3] == 0 || t[3] == 255, "coverage {} is neither on nor off", t[3]);
        }
    }
}
