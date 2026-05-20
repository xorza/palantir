//! Per-batch instance emission: cosmic `LayoutRun` → `GlyphInstance`s.
//!
//! No CPU per-glyph clipping. Composer scissor + interpolated UVs
//! handle partial glyphs at the GPU. The cheap y-range pre-cull stays
//! so off-screen lines don't touch the atlas cache.

use crate::primitives::color::ColorU8;
use crate::primitives::urect::URect;
use cosmic_text::{Buffer, FontSystem, SwashCache, SwashContent};

use super::atlas::GlyphAtlas;
use super::{ContentType, GlyphInstance};

/// One text run resolved to a cosmic buffer + placement.
pub(crate) struct ResolvedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    pub(crate) origin: glam::Vec2,
    pub(crate) bounds: URect,
    pub(crate) scale: f32,
    pub(crate) color: ColorU8,
}

/// Walk one batch's runs, append a `GlyphInstance` per visible glyph
/// to `out`.
pub(crate) fn encode_batch<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    atlas: &mut GlyphAtlas,
    runs: impl IntoIterator<Item = ResolvedRun<'a>>,
    out: &mut Vec<GlyphInstance>,
) {
    for area in runs {
        let area_color: u32 = bytemuck::cast(area.color);
        let scale = area.scale;
        let origin = area.origin;
        let bounds_top = area.bounds.y as f32;
        let bounds_bot = (area.bounds.y + area.bounds.h) as f32;

        // Cheap y-range pre-cull (runs are y-sorted).
        let runs_iter = area
            .buffer
            .layout_runs()
            .skip_while(move |run| (run.line_top + run.line_height) * scale + origin.y < bounds_top)
            .take_while(move |run| run.line_top * scale + origin.y <= bounds_bot);

        for run in runs_iter {
            let line_y_px = (run.line_y * scale).round() as i32;
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((origin.x, origin.y), scale);

                let color = match glyph.color_opt {
                    Some(c) => cosmic_color_to_linear_rgba_u32(c),
                    None => area_color,
                };

                let slot = match atlas.touch(&physical.cache_key) {
                    Some(s) => s,
                    None => match rasterize_and_insert(
                        device,
                        queue,
                        font_system,
                        swash_cache,
                        atlas,
                        physical.cache_key,
                    ) {
                        Some(s) => s,
                        None => continue, // genuine atlas-full at GPU max
                    },
                };

                if slot.alloc.is_none() {
                    continue; // zero-area glyph
                }

                out.push(GlyphInstance {
                    pos: [
                        physical.x + slot.left as i32,
                        line_y_px + physical.y - slot.top as i32,
                    ],
                    dim: (slot.width as u32) | ((slot.height as u32) << 16),
                    uv_and_kind: pack_uv(slot.x, slot.y, slot.content),
                    color,
                });
            }
        }
    }
}

/// Pack `(u, v, kind)` into the 32-bit `uv_and_kind` field. `u`'s
/// high bit carries `content_type` (atlases cap at 16384 = 14 bits).
#[inline]
pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
    debug_assert!(u <= 0x7FFF, "uv high bit reserved for content_type");
    (u as u32) | ((kind as u32) << 15) | ((v as u32) << 16)
}

/// Cache miss path: ask swash for the bitmap, push into the atlas.
fn rasterize_and_insert(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    atlas: &mut GlyphAtlas,
    key: cosmic_text::CacheKey,
) -> Option<super::atlas::GlyphSlot> {
    let image = swash_cache.get_image_uncached(font_system, key)?;
    let content = match image.content {
        SwashContent::Color => ContentType::Color,
        SwashContent::Mask | SwashContent::SubpixelMask => ContentType::Mask,
    };
    let w = image.placement.width as u16;
    let h = image.placement.height as u16;
    let left = image.placement.left as i16;
    let top = image.placement.top as i16;

    if w == 0 || h == 0 {
        return Some(atlas.insert_empty(key, content, left, top));
    }
    atlas.insert(device, queue, key, content, w, h, left, top, &image.data)
}

/// Cosmic stores `0xAARRGGBB` (low→high: B,G,R,A). Our shader reads R
/// from the low byte. Byte-swap so channels match Palantir's linear
/// straight-RGBA convention.
fn cosmic_color_to_linear_rgba_u32(c: cosmic_text::Color) -> u32 {
    let [b, g, r, a] = c.0.to_le_bytes();
    u32::from_le_bytes([r, g, b, a])
}

#[cfg(test)]
mod tests {
    use super::{ContentType, cosmic_color_to_linear_rgba_u32, pack_uv};

    #[test]
    fn cosmic_to_rgba_byteswap() {
        let c = cosmic_text::Color::rgba(0x11, 0x22, 0x33, 0x44);
        assert_eq!(cosmic_color_to_linear_rgba_u32(c), 0x44332211);
    }

    #[test]
    fn pack_uv_round_trip() {
        let p = pack_uv(12345, 54321, ContentType::Color);
        assert_eq!(p & 0x7FFF, 12345);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);

        let p = pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
    }
}
