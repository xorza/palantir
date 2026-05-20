//! Per-batch instance emission: cosmic `LayoutRun` → `GlyphInstance`s.
//!
//! Two paths:
//!
//! - **Cache hit**: prior frames laid this exact `(TextCacheKey,
//!   scale, subpixel origin bin, area color)` run out into the atlas;
//!   the resulting `GlyphInstance` templates are stored in the
//!   [`EncodedCache`]. Emit = `Vec::extend` with origin-shifted
//!   positions, no cosmic walk, no per-glyph atlas hashmap lookup, no
//!   `CacheKey::new`. This is the ~37% of frame time we're targeting.
//! - **Cache miss**: walks cosmic `LayoutRun`s, touches/inserts atlas
//!   slots, emits to `out`, and populates the cache entry with the
//!   origin-relative templates so the next frame at the same `(key,
//!   scale, bins, color)` lands on the fast path.
//!
//! Atlas eviction reuses slot rectangles for new glyphs; any cached
//! entry holding the old uv would point at the wrong image. The
//! atlas's `eviction_count` is latched on insert and re-checked on
//! lookup — any eviction since the entry was built invalidates it.
//! Atlas growth preserves rects (`etagere::grow`), so no
//! invalidation is needed there.

use crate::primitives::color::ColorU8;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::text::TextCacheKey;
use cosmic_text::{Buffer, FontSystem, SubpixelBin, SwashCache, SwashContent};
use rustc_hash::FxHashMap;

use super::atlas::GlyphAtlas;
use super::{ContentType, GlyphInstance};

/// One text run resolved to a cosmic buffer + placement.
pub(crate) struct ResolvedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    pub(crate) key: TextCacheKey,
    pub(crate) origin: glam::Vec2,
    pub(crate) bounds: URect,
    pub(crate) scale: f32,
    pub(crate) color: ColorU8,
}

/// Cache-hit identity for an encoded run. Subpixel bins capture the
/// fractional component of `origin` that cosmic folds into per-glyph
/// `CacheKey`s (so different fractional origins produce different
/// atlas slots and can't share an entry). Area color is part of the
/// key because per-glyph color overrides are baked into the cached
/// `EncodedGlyph.color` field.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    pub(crate) text: TextCacheKey,
    /// `(scale * 65536).round() as u32`. 1/65536 px is below cosmic's
    /// 4-bin subpixel resolution, so distinct quantized scales are the
    /// only ones that produce distinct cosmic cache keys.
    pub(crate) scale_q: u32,
    pub(crate) area_color: u32,
    /// Low nibble: `y_bin`. Next nibble up: `x_bin`. Cosmic's
    /// `SubpixelBin` has four variants (2 bits each).
    pub(crate) bins: u8,
}

/// One cached glyph: origin-relative position + slot's atlas uv +
/// final post-blend color (per-glyph cosmic override resolved against
/// the run's area color at cache-insertion time).
#[derive(Clone, Copy)]
pub(crate) struct EncodedGlyph {
    pub(crate) rel_x: i32,
    pub(crate) rel_y: i32,
    pub(crate) dim: u32,
    pub(crate) uv_and_kind: u32,
    pub(crate) color: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct EncodedEntry {
    /// Slice into `EncodedCache.arena` holding this run's glyph
    /// templates.
    pub(crate) span: Span,
    pub(crate) last_use: u64,
    /// `GlyphAtlas::eviction_count` at insert. If the atlas has
    /// evicted any slot since (count differs), this entry's uv coords
    /// may point at a re-used rectangle holding a different glyph —
    /// drop and rebuild.
    pub(crate) eviction_at: u64,
}

/// Flat-arena cache: one contiguous `Vec<EncodedGlyph>` holds every
/// run's glyphs, with each `EncodedEntry` pointing at its span.
/// After warmup this is alloc-free — the arena/map/scratch all retain
/// capacity across frames.
#[derive(Default)]
pub(crate) struct EncodedCache {
    pub(crate) map: FxHashMap<EncodedKey, EncodedEntry>,
    /// Append-only arena. Replaced runs leave dead spans behind;
    /// `sweep` compacts when dead bytes exceed live ones (see
    /// `COMPACT_RATIO`).
    pub(crate) arena: Vec<EncodedGlyph>,
    /// Retained scratch for the compact pass — kept on the struct so
    /// compaction is a `swap`, not an alloc.
    scratch: Vec<EncodedGlyph>,
}

/// Compact when `arena.len() > live_glyphs * (1 + COMPACT_RATIO)`,
/// i.e. dead glyphs exceed 50% of live ones. Tuned to amortize the
/// compact cost over many frames while bounding wasted memory.
const COMPACT_RATIO: usize = 1;

impl EncodedCache {
    /// Drop entries not touched in the last `keep_frames` frames and,
    /// when the arena holds more dead-glyph slack than live, compact
    /// it into the retained scratch. Compaction rewrites every
    /// surviving entry's `span`.
    pub(crate) fn sweep(&mut self, current_frame: u64, keep_frames: u64) {
        let cutoff = current_frame.saturating_sub(keep_frames);
        self.map.retain(|_, e| e.last_use >= cutoff);

        let live: usize = self.map.values().map(|e| e.span.len as usize).sum();
        if self.arena.len() <= live * (1 + COMPACT_RATIO) {
            return;
        }
        self.scratch.clear();
        for entry in self.map.values_mut() {
            let new_start = self.scratch.len() as u32;
            let r = entry.span.range();
            self.scratch.extend_from_slice(&self.arena[r]);
            entry.span = Span::new(new_start, entry.span.len);
        }
        std::mem::swap(&mut self.arena, &mut self.scratch);
    }
}

/// Walk one batch's runs, append a `GlyphInstance` per visible glyph
/// to `out`. See module docs for the cache-hit vs. miss split.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_batch<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    atlas: &mut GlyphAtlas,
    cache: &mut EncodedCache,
    runs: impl IntoIterator<Item = ResolvedRun<'a>>,
    out: &mut Vec<GlyphInstance>,
) {
    let current_frame = atlas.current_frame;
    for area in runs {
        let area_color: u32 = bytemuck::cast(area.color);
        let scale = area.scale;
        let origin = area.origin;

        // Subpixel-aware origin: cosmic folds the fractional component
        // of (x, y) into each glyph's `CacheKey` via 4-bin subpixel
        // bins, so two runs at different fractional origins land in
        // distinct atlas slots even at the same scale. Splitting the
        // integer-pixel origin out lets the cache emit positions as
        // `i32 + i32` adds.
        let (origin_x_i, x_bin) = SubpixelBin::new(origin.x);
        let (origin_y_i, y_bin) = SubpixelBin::new(origin.y);
        let key = EncodedKey {
            text: area.key,
            scale_q: (scale * 65536.0).round() as u32,
            area_color,
            bins: ((x_bin as u8) << 2) | (y_bin as u8),
        };

        if !area.key.is_invalid()
            && let Some(entry) = cache.map.get_mut(&key)
            && entry.eviction_at == atlas.eviction_count
        {
            entry.last_use = current_frame;
            let span = entry.span;
            let glyphs = &cache.arena[span.range()];
            out.reserve(glyphs.len());
            for g in glyphs {
                out.push(GlyphInstance {
                    pos: [g.rel_x + origin_x_i, g.rel_y + origin_y_i],
                    dim: g.dim,
                    uv_and_kind: g.uv_and_kind,
                    color: g.color,
                });
            }
            continue;
        }

        let bounds_top = area.bounds.y as f32;
        let bounds_bot = (area.bounds.y + area.bounds.h) as f32;

        // Cheap y-range pre-cull (runs are y-sorted).
        let runs_iter = area
            .buffer
            .layout_runs()
            .skip_while(move |run| (run.line_top + run.line_height) * scale + origin.y < bounds_top)
            .take_while(move |run| run.line_top * scale + origin.y <= bounds_bot);

        // Build a fresh cache entry as a side effect of the slow
        // walk. We push templates straight onto `cache.arena`; if an
        // atlas eviction happens mid-walk we truncate back to
        // `pending_start` so the partial run never becomes an entry
        // (eviction could have invalidated earlier glyphs' uv coords).
        let eviction_at_start = atlas.eviction_count;
        let pending_start = cache.arena.len() as u32;

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

                let abs_x = physical.x + slot.left as i32;
                let abs_y = line_y_px + physical.y - slot.top as i32;
                let dim = (slot.width as u32) | ((slot.height as u32) << 16);
                let uv_and_kind = pack_uv(slot.x, slot.y, slot.content);

                out.push(GlyphInstance {
                    pos: [abs_x, abs_y],
                    dim,
                    uv_and_kind,
                    color,
                });
                cache.arena.push(EncodedGlyph {
                    rel_x: abs_x - origin_x_i,
                    rel_y: abs_y - origin_y_i,
                    dim,
                    uv_and_kind,
                    color,
                });
            }
        }

        // Only cache if no eviction happened mid-walk and the run had
        // a valid shaping key (the mono fallback emits no glyphs).
        if !area.key.is_invalid() && atlas.eviction_count == eviction_at_start {
            let span = Span::new(pending_start, cache.arena.len() as u32 - pending_start);
            cache.map.insert(
                key,
                EncodedEntry {
                    span,
                    last_use: current_frame,
                    eviction_at: atlas.eviction_count,
                },
            );
        } else {
            // Roll back the partial entry — its uv coords are stale.
            cache.arena.truncate(pending_start as usize);
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
