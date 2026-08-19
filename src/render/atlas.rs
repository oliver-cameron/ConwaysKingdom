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

/// Mip levels to build, stopping where one texel covers exactly one sprite.
///
/// Going further would average neighbouring sprites together, so a cell would
/// pick up the colour of whatever sits beside it on the sheet. Five levels
/// takes a sprite from 16x16 down to 1x1, which is as small as a cell is ever
/// drawn: the camera will not zoom below one pixel per cell.
pub const MIP_LEVELS: u32 = 5;

/// A sprite index *is* a [`Kind`], so a cell cannot name art that does not
/// exist. Adding a kind and forgetting to draw it fails the test below.

/// Where a sheet may come from.
pub enum Source<'a> {
    /// A PNG, 256x256 RGBA. R is saturation, G lightness, A coverage; blue is
    /// unused. Hue is *not* in the sheet — it comes from the player.
    Png(&'a [u8]),
    /// Drawn in code, so the project runs with no art checked in.
    Generated,
}

pub struct Atlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Atlas {
    /// `Rgba8Unorm`, not `Srgb`: these are not colours. R and G are saturation
    /// and lightness fed to a colour model, and sRGB decoding would bend them.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, source: Source<'_>) -> Self {
        let texels = match source {
            Source::Png(bytes) => match decode_png(bytes) {
                Ok(t) => t,
                Err(e) => {
                    // Falling back beats refusing to start: a bad sheet should
                    // cost you the art, not the game.
                    log::error!("atlas image unusable ({e}); drawing the built-in sheet");
                    Self::generate()
                }
            },
            Source::Generated => Self::generate(),
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_N,
                height: ATLAS_N,
                depth_or_array_layers: 1,
            },
            mip_level_count: MIP_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut level = texels;
        let mut size = ATLAS_N;
        for mip in 0..MIP_LEVELS {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &level,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size * 4),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            );
            if mip + 1 < MIP_LEVELS {
                level = halve(&level, size);
                size /= 2;
            }
        }

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
            // Blend between levels, or a cell visibly snaps as it crosses one
            // while zooming.
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            lod_max_clamp: (MIP_LEVELS - 1) as f32,
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
        use crate::sim::Kind;
        // Centre-relative, in -1..1, so the shapes are written once and do not
        // depend on SPRITE_N.
        let n = SPRITE_N as f32;
        let u = (x as f32 + 0.5) / n * 2.0 - 1.0;
        let v = (y as f32 + 0.5) / n * 2.0 - 1.0;
        let r = (u * u + v * v).sqrt();

        let (coverage, lightness) = match Kind(index) {
            Kind::NORMAL => {
                // A disc with a soft edge, lit from the top left.
                let a = smoothstep(0.92, 0.72, r);
                let lit = 0.55 + 0.35 * (-(u + v) * 0.5).clamp(-1.0, 1.0);
                (a, lit)
            }
            Kind::GLASS => {
                // A pane: bright frame, faint fill, so what it covers shows
                // through it.
                let inset = u.abs().max(v.abs());
                let frame = smoothstep(0.99, 0.90, inset) * smoothstep(0.74, 0.84, inset);
                let fill = smoothstep(0.90, 0.84, inset) * 0.22;
                ((frame + fill).min(1.0), 0.86)
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

/// Box-filter one mip level down.
///
/// Saturation and lightness are averaged **weighted by coverage**, so a
/// transparent texel does not drag its neighbours' colour towards whatever it
/// happens to hold. Averaging them flat is what gives shrunk sprites dark
/// fringes.
fn halve(src: &[u8], size: u32) -> Vec<u8> {
    let half = size / 2;
    let mut out = vec![0u8; (half * half * 4) as usize];
    for y in 0..half {
        for x in 0..half {
            let mut weighted = [0.0f32; 2];
            let mut alpha = 0.0f32;
            let mut weight = 0.0f32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let at = (((y * 2 + dy) * size + x * 2 + dx) * 4) as usize;
                    let a = src[at + 3] as f32 / 255.0;
                    weighted[0] += src[at] as f32 * a;
                    weighted[1] += src[at + 1] as f32 * a;
                    alpha += a;
                    weight += a;
                }
            }
            let at = ((y * half + x) * 4) as usize;
            if weight > 0.0 {
                out[at] = (weighted[0] / weight).round() as u8;
                out[at + 1] = (weighted[1] / weight).round() as u8;
            }
            out[at + 3] = (alpha / 4.0 * 255.0).round() as u8;
        }
    }
    out
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A 256x256 RGBA PNG, as bytes.
fn decode_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.width != ATLAS_N || info.height != ATLAS_N {
        return Err(format!(
            "sheet is {}x{}, expected {ATLAS_N}x{ATLAS_N}",
            info.width, info.height
        ));
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let bytes_per = match frame.color_type {
        png::ColorType::Rgba => 4,
        other => return Err(format!("{other:?} PNG; needs RGBA")),
    };
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(format!("{:?} PNG; needs 8 bits per channel", frame.bit_depth));
    }
    buf.truncate((ATLAS_N * ATLAS_N * bytes_per) as usize);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Kind;

    #[test]
    fn the_sheet_is_the_size_the_uv_scheme_needs() {
        // A u8 per axis addresses 256 texels, which across 16 cells is 16
        // texels each.
        assert_eq!(ATLAS_N, 256);
        assert_eq!(ATLAS_N / crate::sim::CHUNK_N as u32, SPRITE_N);
        assert_eq!(Atlas::generate().len(), (256 * 256 * 4) as usize);
    }

    /// Every kind must be drawn. A sprite index is a Kind, so adding a kind
    /// without art would silently render nothing; this is what stops that.
    #[test]
    fn every_kind_has_a_sprite() {
        let texels = Atlas::generate();
        for kind in Kind::ALL {
            let (ox, oy) = (
                (kind.0 as u32 % SHEET_N) * SPRITE_N,
                (kind.0 as u32 / SHEET_N) * SPRITE_N,
            );
            let covered = (0..SPRITE_N)
                .flat_map(|y| (0..SPRITE_N).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    texels[((((oy + y) * ATLAS_N + ox + x) * 4) + 3) as usize] > 8
                })
                .count();
            assert!(
                covered > (SPRITE_N * SPRITE_N / 8) as usize,
                "Kind({}) has no sprite: only {covered} texels of coverage",
                kind.0
            );
        }
    }

    /// The chain must stop before a texel spans more than one sprite, or a
    /// cell picks up the colour of its neighbour on the sheet.
    #[test]
    fn the_mip_chain_stops_at_one_texel_per_sprite() {
        let smallest = ATLAS_N >> (MIP_LEVELS - 1);
        assert_eq!(smallest, SHEET_N, "the last level is one texel per sprite");

        let mut level = Atlas::generate();
        let mut size = ATLAS_N;
        for _ in 1..MIP_LEVELS {
            level = halve(&level, size);
            size /= 2;
            assert_eq!(level.len(), (size * size * 4) as usize);
        }
        // A drawn sprite must survive all the way down rather than fading out.
        let at = ((Kind::NORMAL.0 as u32) * 4) as usize;
        assert!(level[at + 3] > 100, "the normal cell vanished when shrunk");
    }

    #[test]
    fn kinds_with_no_meaning_yet_are_blank() {
        let texels = Atlas::generate();
        let index = 200u32;
        let (ox, oy) = ((index % SHEET_N) * SPRITE_N, (index / SHEET_N) * SPRITE_N);
        let (x, y) = (ox + SPRITE_N / 2, oy + SPRITE_N / 2);
        assert_eq!(texels[(((y * ATLAS_N + x) * 4) + 3) as usize], 0);
    }
}
