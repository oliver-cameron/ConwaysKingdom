//! The sprite sheet a cell is drawn from.
//!
//! A chunk is 16x16 cells and each cell is drawn as a 16x16 texel image, so a
//! chunk covers a 256x256 texel grid — a `u8` on each axis, which is where the
//! resolution comes from.
//!
//! Sprites carry **no hue**. Each texel is saturation, lightness and coverage;
//! the hue arrives at draw time from the cell's player number. One sheet
//! therefore serves every player, and two players' cells are the same shape in
//! different colours rather than two sets of art.
//!
//! Tiles are generated rather than loaded, so there is no asset pipeline to
//! keep in step with the code. Swapping in a real image later means replacing
//! [`Atlas::generate`] and nothing else.

/// Texels along one edge of a cell's sprite.
pub const SPRITE_N: u32 = 16;
/// Sprites along one edge of the sheet, so 256 of them.
pub const SHEET_N: u32 = 16;
/// The sheet's edge in texels.
pub const ATLAS_N: u32 = SPRITE_N * SHEET_N;

/// Sprite indices with a meaning. A cell's index comes from its metadata, so
/// these are the defaults until metadata means something.
pub const SPRITE_BLOB: u8 = 0;
pub const SPRITE_SOLID: u8 = 1;
pub const SPRITE_RING: u8 = 2;
pub const SPRITE_DIAMOND: u8 = 3;

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
        let texels = Self::generate();

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_N,
                height: ATLAS_N,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
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
                bytes_per_row: Some(ATLAS_N * 4),
                rows_per_image: Some(ATLAS_N),
            },
            wgpu::Extent3d {
                width: ATLAS_N,
                height: ATLAS_N,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Linear, so a cell keeps its shape when zoomed past one pixel per
        // texel. Clamped, so a sprite cannot bleed into its neighbour on the
        // sheet — the reason to clamp rather than repeat.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self { texture, view, sampler }
    }

    /// R = saturation, G = lightness, B = unused, A = coverage.
    fn generate() -> Vec<u8> {
        let mut texels = vec![0u8; (ATLAS_N * ATLAS_N * 4) as usize];
        for index in 0..(SHEET_N * SHEET_N) {
            let (ox, oy) = (
                (index % SHEET_N) * SPRITE_N,
                (index / SHEET_N) * SPRITE_N,
            );
            for y in 0..SPRITE_N {
                for x in 0..SPRITE_N {
                    let px = Self::sprite_texel(index as u8, x, y);
                    let at = (((oy + y) * ATLAS_N + ox + x) * 4) as usize;
                    texels[at..at + 4].copy_from_slice(&px);
                }
            }
        }
        texels
    }

    fn sprite_texel(index: u8, x: u32, y: u32) -> [u8; 4] {
        // Centre-relative, in -1..1, so the shapes are written once and do not
        // depend on SPRITE_N.
        let n = SPRITE_N as f32;
        let u = (x as f32 + 0.5) / n * 2.0 - 1.0;
        let v = (y as f32 + 0.5) / n * 2.0 - 1.0;
        let r = (u * u + v * v).sqrt();

        let (coverage, lightness) = match index {
            SPRITE_BLOB => {
                // A disc with a soft edge, lit from the top left.
                let a = smoothstep(0.92, 0.72, r);
                let lit = 0.55 + 0.35 * (-(u + v) * 0.5).clamp(-1.0, 1.0);
                (a, lit)
            }
            SPRITE_SOLID => {
                let inset = u.abs().max(v.abs());
                (smoothstep(0.96, 0.88, inset), 0.62)
            }
            SPRITE_RING => {
                let a = smoothstep(0.92, 0.80, r) * smoothstep(0.42, 0.56, r);
                (a, 0.70)
            }
            SPRITE_DIAMOND => {
                let d = u.abs() + v.abs();
                (smoothstep(0.95, 0.78, d), 0.66)
            }
            // Everything else is blank until it means something.
            _ => (0.0, 0.0),
        };

        [
            to_u8(0.85),      // saturation
            to_u8(lightness),
            0,
            to_u8(coverage),
        ]
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_is_the_size_the_uv_scheme_needs() {
        // A u8 per axis addresses 256 texels, which across 16 cells is 16
        // texels each.
        assert_eq!(ATLAS_N, 256);
        assert_eq!(ATLAS_N / crate::sim::CHUNK_N as u32, SPRITE_N);
        assert_eq!(Atlas::generate().len(), (256 * 256 * 4) as usize);
    }

    #[test]
    fn drawn_sprites_have_coverage_and_blank_ones_do_not() {
        let texels = Atlas::generate();
        let alpha_of = |index: u32| {
            let (ox, oy) = ((index % SHEET_N) * SPRITE_N, (index / SHEET_N) * SPRITE_N);
            // Centre texel of the sprite.
            let (x, y) = (ox + SPRITE_N / 2, oy + SPRITE_N / 2);
            texels[(((y * ATLAS_N + x) * 4) + 3) as usize]
        };
        for drawn in [SPRITE_BLOB, SPRITE_SOLID, SPRITE_DIAMOND] {
            assert!(alpha_of(drawn as u32) > 200, "sprite {drawn} should be solid at its centre");
        }
        assert_eq!(alpha_of(SPRITE_RING as u32), 0, "a ring is hollow");
        assert_eq!(alpha_of(200), 0, "unused sprites are blank");
    }
}
