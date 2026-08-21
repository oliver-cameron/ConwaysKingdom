//! The sprites a cell is drawn from.
//!
//! One file per cell state, each a 256x256 sheet of 16x16 tiles, loaded into
//! its own layer of a texture array. The layer says what the cell *is*; the
//! cell's own UV says which tile of that sheet it draws, so a structure
//! spanning several cells gives each one a different tile and the parts line
//! up.
//!
//! Tiles are **strictly 16x16 and not anti-aliased**: sampling is nearest and
//! there is no mip chain, so a cell is pixel art at every zoom rather than a
//! blurred blob. A chunk is 16 cells of 16 texels, so it spans 256x256 — a `u8`
//! on each axis, which is the coordinate space the shader works in.
//!
//! Sprites carry **no hue**. A texel is saturation, lightness and coverage; the
//! hue arrives at draw time from the cell's player number, so one set of art
//! serves every player.

use crate::sim::Kind;

/// Texels along one edge of a tile — one cell's worth of picture.
pub const TILE_N: u32 = 16;
/// Tiles along one edge of a sheet, so 256 of them per state.
pub const SHEET_TILES: u32 = 16;
/// A sheet's edge in texels.
pub const SHEET_N: u32 = TILE_N * SHEET_TILES;

/// The states a cell can be drawn in. Alive and iced are independent, so
/// there are four, and **each has its own image** — an iced cell is not the
/// living sprite with a pane composited on top, it is its own picture.
///
/// That is not only an art decision. Compositing means sampling one sprite
/// inside an `if` on whether the cell is alive, and WGSL requires anything
/// using implicit derivatives to sit in uniform control flow. One image per
/// state means one unconditional sample.
pub const STATES: u32 = 4;

/// Layer for a cell, from its kind and state. States are consecutive within a
/// kind, so a kind's four images sit together.
#[inline]
pub const fn layer_for(kind: Kind, alive: bool, ice: bool) -> u32 {
    kind.0 as u32 * STATES + (alive as u32) + (ice as u32) * 2
}

/// Every layer: four states for every kind.
pub const LAYERS: u32 = Kind::COUNT as u32 * STATES;

/// The file behind each layer, in `layer_for` order: for each kind, dead,
/// alive, dead under ice, alive under ice. Adding a kind without adding
/// its four images fails to compile.
const SPRITE_FILES: [&[u8]; LAYERS as usize] = [
    include_bytes!("../../assets/sprites/dead.png"),
    include_bytes!("../../assets/sprites/alive.png"),
    include_bytes!("../../assets/sprites/dead_ice.png"),
    include_bytes!("../../assets/sprites/alive_ice.png"),
];

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
            size: wgpu::Extent3d {
                width: SHEET_N,
                height: SHEET_N,
                depth_or_array_layers: LAYERS,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (layer, bytes) in SPRITE_FILES.iter().enumerate() {
            let texels = decode(bytes).unwrap_or_else(|e| {
                // Falling back beats refusing to start: a bad sprite should
                // cost you that sprite, not the game.
                log::error!("sprite {layer} unusable ({e}); drawing a placeholder");
                placeholder()
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 },
                    aspect: wgpu::TextureAspect::All,
                },
                &texels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SHEET_N * 4),
                    rows_per_image: Some(SHEET_N),
                },
                wgpu::Extent3d {
                    width: SHEET_N,
                    height: SHEET_N,
                    depth_or_array_layers: 1,
                },
            );
        }

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

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

    /// Every kind in every state must have art, sized exactly 16x16. Dead and
    /// ice-free is allowed to be blank; the others are not, since a state you
    /// cannot see is a state you cannot play against.
    #[test]
    fn every_state_of_every_kind_has_a_sprite() {
        assert_eq!(SPRITE_FILES.len(), LAYERS as usize);
        for kind in Kind::ALL {
            for (alive, ice) in [(false, false), (true, false), (false, true), (true, true)] {
                let layer = layer_for(kind, alive, ice) as usize;
                let texels = decode(SPRITE_FILES[layer]).unwrap_or_else(|e| {
                    panic!("Kind({}) alive={alive} ice={ice}: {e}", kind.0)
                });
                assert_eq!(texels.len(), (SHEET_N * SHEET_N * 4) as usize);

                // Tile (0, 0) is the default art and must be drawn.
                let covered = (0..TILE_N)
                    .flat_map(|y| (0..TILE_N).map(move |x| (x, y)))
                    .filter(|&(x, y)| texels[(((y * SHEET_N + x) * 4) + 3) as usize] > 8)
                    .count();
                if alive || ice {
                    assert!(
                        covered > (TILE_N * TILE_N / 8) as usize,
                        "Kind({}) alive={alive} ice={ice}: tile (0,0) is blank",
                        kind.0
                    );
                }
            }
        }
    }

    /// Layers are consecutive and unique, so no two states share art by
    /// accident.
    #[test]
    fn every_state_gets_its_own_layer() {
        let mut seen = Vec::new();
        for kind in Kind::ALL {
            for (alive, ice) in [(false, false), (true, false), (false, true), (true, true)] {
                seen.push(layer_for(kind, alive, ice));
            }
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..LAYERS).collect::<Vec<_>>());
    }

    #[test]
    fn a_pane_is_a_frame_you_can_see_through() {
        let texels = decode(SPRITE_FILES[layer_for(Kind::NORMAL, false, true) as usize])
            .expect("dead under ice");
        let at = |x: usize, y: usize| texels[(y * SHEET_N as usize + x) * 4 + 3];
        assert_eq!(at(0, 0), 255, "the frame should be solid");
        assert!(at(8, 8) < 128, "the middle should show what is under it");
    }

    /// No anti-aliasing: coverage is one of a few fixed inks, never a ramp.
    #[test]
    fn sprites_have_hard_edges() {
        for bytes in SPRITE_FILES {
            let texels = decode(bytes).expect("sprite");
            for t in texels.chunks(4) {
                assert!(
                    matches!(t[3], 0 | 89 | 191 | 255),
                    "alpha {} is a soft edge; tiles are pixel art",
                    t[3]
                );
            }
        }
    }

    /// A cell's UV must be able to reach every tile of its sheet.
    #[test]
    fn a_u8_uv_covers_the_sheet() {
        assert_eq!(SHEET_N / TILE_N, SHEET_TILES);
        assert!(SHEET_TILES <= 256, "a u8 must be able to address every tile");
    }
}
