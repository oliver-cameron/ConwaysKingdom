//! The sprites a cell is drawn from.
//!
//! One 16x16 image per sprite, in its own file, loaded into one layer of a
//! texture array. A sprite index is a layer index, so there is no sheet layout
//! to agree on and no risk of a sprite bleeding into its neighbour.
//!
//! Sprites are **strictly 16x16 and not anti-aliased**: sampling is nearest and
//! there is no mip chain, so a cell is pixel art at every zoom rather than a
//! blurred blob. A chunk is 16 cells of 16 texels, so it spans 256x256 — a `u8`
//! on each axis, which is the coordinate space the shader works in.
//!
//! Sprites carry **no hue**. A texel is saturation, lightness and coverage; the
//! hue arrives at draw time from the cell's player number, so one set of art
//! serves every player.

use crate::sim::Kind;

/// Texels along one edge of a sprite.
pub const SPRITE_N: u32 = 16;

/// Layer holding the pane drawn over a cell carrying the glass flag. Glass is
/// a flag rather than a kind — a cell may be alive, glass, both or neither —
/// so it needs a layer of its own rather than a kind's.
pub const LAYER_GLASS: u32 = Kind::COUNT as u32;

/// Every layer: one per kind, then glass.
pub const LAYERS: u32 = LAYER_GLASS + 1;

/// The file behind each layer. A kind's art lives at its own index, so adding a
/// kind without adding a file fails to compile.
const SPRITE_FILES: [&[u8]; LAYERS as usize] = [
    include_bytes!("../../assets/sprites/normal.png"),
    include_bytes!("../../assets/sprites/glass.png"),
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
                width: SPRITE_N,
                height: SPRITE_N,
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
                    bytes_per_row: Some(SPRITE_N * 4),
                    rows_per_image: Some(SPRITE_N),
                },
                wgpu::Extent3d {
                    width: SPRITE_N,
                    height: SPRITE_N,
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
    if info.width != SPRITE_N || info.height != SPRITE_N {
        return Err(format!(
            "sprite is {}x{}, expected {SPRITE_N}x{SPRITE_N}",
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
    buf.truncate((SPRITE_N * SPRITE_N * 4) as usize);
    Ok(buf)
}

/// A hollow square, so a missing sprite is obvious rather than invisible.
fn placeholder() -> Vec<u8> {
    let mut out = vec![0u8; (SPRITE_N * SPRITE_N * 4) as usize];
    for y in 0..SPRITE_N {
        for x in 0..SPRITE_N {
            let edge = x == 0 || y == 0 || x == SPRITE_N - 1 || y == SPRITE_N - 1;
            let at = ((y * SPRITE_N + x) * 4) as usize;
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

    /// Every kind must have art, and it must be exactly 16x16. A kind's sprite
    /// lives at its own index, so adding a kind without a file will not
    /// compile; this catches a file that is present but wrong.
    #[test]
    fn every_kind_has_a_sixteen_by_sixteen_sprite() {
        assert_eq!(SPRITE_FILES.len(), LAYERS as usize);
        for kind in Kind::ALL {
            let texels = decode(SPRITE_FILES[kind.0 as usize])
                .unwrap_or_else(|e| panic!("Kind({}) sprite: {e}", kind.0));
            assert_eq!(texels.len(), (SPRITE_N * SPRITE_N * 4) as usize);
            let covered = texels.chunks(4).filter(|t| t[3] > 8).count();
            assert!(
                covered > (SPRITE_N * SPRITE_N / 8) as usize,
                "Kind({}) is nearly blank: {covered} texels covered",
                kind.0
            );
        }
    }

    #[test]
    fn glass_has_its_own_sprite() {
        let texels = decode(SPRITE_FILES[LAYER_GLASS as usize]).expect("glass sprite");
        assert_eq!(texels.len(), (SPRITE_N * SPRITE_N * 4) as usize);
        // A pane is a frame: its border is opaque and its middle is not.
        let at = |x: usize, y: usize| texels[(y * SPRITE_N as usize + x) * 4 + 3];
        assert_eq!(at(0, 0), 255, "the frame should be solid");
        assert!(at(8, 8) < 128, "the middle should show what is under it");
    }

    /// No anti-aliasing: coverage is on or off, never a soft edge.
    #[test]
    fn sprites_have_hard_edges() {
        for bytes in SPRITE_FILES {
            let texels = decode(bytes).expect("sprite");
            for t in texels.chunks(4) {
                assert!(
                    t[3] == 0 || t[3] == 255 || t[3] == 89,
                    "alpha {} is a soft edge; sprites are pixel art",
                    t[3]
                );
            }
        }
    }
}
