//! Per-batch instance emission: cosmic `LayoutRun` → `GlyphInstance`s.
//!
//! Two paths:
//!
//! - **Cache hit**: prior frames laid this exact `(TextCacheKey,
//!   scale, subpixel origin bin, area color)` run out into the atlas;
//!   the resulting origin-relative `GlyphInstance` templates are stored
//!   in the [`EncodedCache`]. Emit = a copy with origin-shifted
//!   positions, no cosmic walk, no per-glyph atlas hashmap lookup, no
//!   `CacheKey::new`. This is the ~37% of frame time we're targeting.
//! - **Cache miss**: walks cosmic `LayoutRun`s, touches/inserts atlas
//!   slots, emits to `out`, and populates the cache entry with the
//!   origin-relative templates so the next frame at the same `(key,
//!   scale, bins, color)` lands on the fast path. Runs whose lines
//!   were y-culled against `bounds` are *not* cached — the key omits
//!   bounds, so a truncated template would replay wrong after a
//!   scroll.
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
use crate::renderer::render_buffer::text::TextRun;
use crate::text::TextCacheKey;
use cosmic_text::{Buffer, FontSystem, SubpixelBin, SwashCache, SwashContent};
use rustc_hash::FxHashMap;

use crate::renderer::backend::text::atlas::GlyphAtlas;
use crate::renderer::backend::text::{ContentType, GlyphInstance};

/// One text run resolved to a cosmic buffer + placement.
#[derive(Debug)]
pub(crate) struct ResolvedRun<'a> {
    pub(crate) buffer: &'a Buffer,
    pub(crate) origin: glam::Vec2,
    pub(crate) bounds: URect,
    pub(crate) scale: f32,
    pub(crate) color: ColorU8,
    pub(crate) run_key: EncodedRunKey,
}

/// Cache-hit identity for an encoded run. Subpixel bins capture the
/// fractional component of `origin` that cosmic folds into per-glyph
/// `CacheKey`s (so different fractional origins produce different
/// atlas slots and can't share an entry).
///
/// `area_color` is in the key because the run's colour is baked into
/// every cached [`GlyphInstance::color`] at insert time. **This is only
/// sufficient because aperture shapes every run with one uniform
/// colour** — `attrs_for` (`cosmic.rs`) sets no per-span colour, so
/// cosmic never emits a per-glyph `color_opt`. If per-span colours are
/// ever added, fold a colour-span fingerprint into this key *first*, or
/// the cache will serve a stale run's baked colours. The assertion
/// in `encode_batch`'s glyph loop is the tripwire for that invariant.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    pub(crate) text: TextCacheKey,
    /// `(scale * 65536).round() as u32`. 1/65536 px is below cosmic's
    /// 4-bin subpixel resolution, so distinct quantized scales are the
    /// only ones that produce distinct cosmic cache keys.
    pub(crate) scale_q: u32,
    pub(crate) area_color: u32,
    /// Bits 0-1: `y_bin`; bits 2-3: `x_bin`. Cosmic's `SubpixelBin`
    /// has four variants (2 bits each).
    pub(crate) bins: u8,
}

/// `encode_key_for`'s named result. Carries the cache identity plus
/// the integer-pixel origin (the fractional component is folded into
/// `EncodedKey::bins`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedRunKey {
    pub(crate) key: EncodedKey,
    pub(crate) origin_x: i32,
    pub(crate) origin_y: i32,
}

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedGlyph {
    pub(crate) instance: GlyphInstance,
    pub(crate) atlas_slot: u32,
}

/// Flat-arena cache: one contiguous `Vec<EncodedGlyph>` holds every
/// run's origin-relative glyphs, with each `EncodedEntry` pointing at
/// its span.
/// After warmup this is alloc-free — the arena/map/scratch all retain
/// capacity across frames.
#[derive(Debug, Default)]
pub(crate) struct EncodedCache {
    pub(crate) map: FxHashMap<EncodedKey, EncodedEntry>,
    /// Append-only arena. Replaced runs leave dead spans behind;
    /// `sweep` compacts when dead bytes exceed live ones (see
    /// `COMPACT_RATIO`).
    pub(crate) arena: Vec<EncodedGlyph>,
    /// A cache hit emits each row's instance without walking cosmic,
    /// A cache hit emits `arena` straight out without walking cosmic,
    /// so the atlas slots backing the run would never get their LRU
    /// `last_use` bumped — `evict_one` could then reclaim a slot still
    /// referenced this frame and overwrite it with a different glyph.
    /// On hit we store the current frame through each index — an
    /// indexed write, no map probe per glyph. The entry's `eviction_at`
    /// latch is what keeps these indices honest: only `evict_one` ever
    /// reassigns an allocated slot's index, and it bumps the count.
    /// Retained scratch for the compact pass — kept on the struct so
    /// compaction is a `swap`, not an alloc.
    pub(crate) scratch: Vec<EncodedGlyph>,
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

/// Build the cache key for a `TextRun` placed at `frame_scale * r.scale`,
/// plus the integer-pixel origin (cosmic's subpixel bins absorb the
/// fractional component into per-glyph `CacheKey`s, so two runs at
/// different fractional origins live in different cache entries).
pub(crate) fn encode_key_for(r: &TextRun, frame_scale: f32) -> EncodedRunKey {
    let scale = frame_scale * r.scale;
    let area_color: u32 = bytemuck::cast(r.color);
    let (origin_x, x_bin) = SubpixelBin::new(r.origin.x);
    let (origin_y, y_bin) = SubpixelBin::new(r.origin.y);
    EncodedRunKey {
        key: EncodedKey {
            text: r.key,
            scale_q: (scale * 65536.0).round() as u32,
            area_color,
            bins: ((x_bin as u8) << 2) | (y_bin as u8),
        },
        origin_x,
        origin_y,
    }
}

/// Cache-hit fast path. Returns `true` if `key` resolved to a live
/// entry and the run's glyphs were emitted to `out`. Caller falls
/// through to the slow path on `false`.
pub(crate) fn try_emit_cached(
    cache: &mut EncodedCache,
    atlas: &mut GlyphAtlas,
    run_key: &EncodedRunKey,
    out: &mut Vec<GlyphInstance>,
) -> bool {
    let current_frame = atlas.current_frame;
    let Some(entry) = cache.map.get_mut(&run_key.key) else {
        return false;
    };
    if entry.eviction_at != atlas.eviction_count {
        return false;
    }
    entry.last_use = current_frame;
    let span = entry.span;
    let glyphs = &cache.arena[span.range()];
    out.reserve(glyphs.len());
    // One pass emits the instance and refreshes the backing slot's LRU
    // stamp together, so `evict_one` can't reclaim a slot we're still
    // drawing this frame. The eviction_at check above guarantees no
    // recorded index was evicted since insert, so every slot still
    // holds the same glyph.
    for glyph in glyphs {
        let g = glyph.instance;
        out.push(GlyphInstance {
            pos: [g.pos[0] + run_key.origin_x, g.pos[1] + run_key.origin_y],
            dim: g.dim,
            uv_and_kind: g.uv_and_kind,
            color: g.color,
        });
        atlas.slots[glyph.atlas_slot as usize].last_use = current_frame;
    }
    true
}

/// Stable dependencies for the slow walk in `encode_batch`. Bundles
/// the six refs that would otherwise drag every helper into an
/// 8-arg signature.
#[derive(Debug)]
pub(crate) struct EncodeCtx<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) font_system: &'a mut FontSystem,
    pub(crate) swash_cache: &'a mut SwashCache,
    pub(crate) atlas: &'a mut GlyphAtlas,
    pub(crate) cache: &'a mut EncodedCache,
}

/// Walk one batch's runs that didn't hit the encoded cache: shape via
/// cosmic, touch/insert atlas slots, emit `GlyphInstance`s and
/// populate the encoded cache as a side effect. Callers are expected
/// to have already filtered out invalid keys and cache hits.
pub(crate) fn encode_batch<'a>(
    ctx: &mut EncodeCtx<'_>,
    runs: impl IntoIterator<Item = ResolvedRun<'a>>,
    out: &mut Vec<GlyphInstance>,
) {
    let current_frame = ctx.atlas.current_frame;
    for area in runs {
        let run_key = area.run_key;
        let area_color: u32 = bytemuck::cast(area.color);
        let scale = area.scale;
        let origin = area.origin;

        let bounds_top = area.bounds.y as f32;
        let bounds_bot = (area.bounds.y + area.bounds.h) as f32;

        // Cheap y-range pre-cull (runs are y-sorted). `culled` records
        // whether any line was dropped: a truncated encode must not
        // become a cache template (`EncodedKey` carries no bounds, so
        // integer-pixel scrolling replays the same key with lines
        // newly in view — they'd stay blank forever).
        let mut culled = false;

        // Build a fresh cache entry as a side effect of the slow
        // walk. We push templates straight onto `cache.arena`; if an
        // atlas eviction happens mid-walk we truncate back to
        // `pending_start` so the partial run never becomes an entry
        // (eviction could have invalidated earlier glyphs' uv coords).
        let eviction_at_start = ctx.atlas.eviction_count;
        let pending_start = ctx.cache.arena.len() as u32;

        for run in area.buffer.layout_runs() {
            if (run.line_top + run.line_height) * scale + origin.y < bounds_top {
                culled = true;
                continue;
            }
            if run.line_top * scale + origin.y > bounds_bot {
                culled = true;
                break;
            }
            let line_y_px = (run.line_y * scale).round() as i32;
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((origin.x, origin.y), scale);

                // `EncodedKey` caches on the run's `area_color`, not
                // per-glyph colour — correct only while cosmic never
                // produces a per-glyph override (aperture's `attrs_for`
                // sets no per-span colour). If this fires, per-span
                // colour was added without growing `EncodedKey`, and the
                // encoded cache would alias runs differing only in glyph
                // colour.
                assert!(
                    glyph.color_opt.is_none(),
                    "per-glyph colour override requires folding colour into EncodedKey",
                );
                let color = area_color;

                let idx = match ctx.atlas.touch(&physical.cache_key) {
                    Some(i) => i,
                    None => match rasterize_and_insert(
                        ctx.device,
                        ctx.font_system,
                        ctx.swash_cache,
                        ctx.atlas,
                        physical.cache_key,
                    ) {
                        Some(i) => i,
                        None => continue, // genuine atlas-full at GPU max
                    },
                };
                let slot = ctx.atlas.slots[idx as usize];

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
                ctx.cache.arena.push(EncodedGlyph {
                    instance: GlyphInstance {
                        pos: [abs_x - run_key.origin_x, abs_y - run_key.origin_y],
                        dim,
                        uv_and_kind,
                        color,
                    },
                    atlas_slot: idx,
                });
            }
        }

        // Only cache full encodes with no mid-walk eviction. Pass 1
        // already filtered invalid keys; valid-key here is a
        // precondition. Partially visible runs re-encode each frame;
        // the reverse (a cached full template replayed under narrower
        // bounds) is safe — the batch scissor is the real clip.
        if ctx.atlas.eviction_count == eviction_at_start && !culled {
            let span = Span::new(pending_start, ctx.cache.arena.len() as u32 - pending_start);
            ctx.cache.map.insert(
                run_key.key,
                EncodedEntry {
                    span,
                    last_use: current_frame,
                    eviction_at: ctx.atlas.eviction_count,
                },
            );
        } else {
            // Roll back the partial entry — truncated by the cull, or
            // its uv coords are stale after an eviction.
            ctx.cache.arena.truncate(pending_start as usize);
        }
    }
}

/// Pack `(u, v, kind)` into the 32-bit `uv_and_kind` field. `u`'s
/// high bit carries `content_type` (atlases cap at 16384 = 14 bits).
#[inline]
pub(crate) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
    assert!(u <= 0x7FFF, "uv high bit reserved for content_type");
    (u as u32) | ((kind as u32) << 15) | ((v as u32) << 16)
}

/// Cache miss path: ask swash for the bitmap, push into the atlas.
/// Returns the new slot's slab index.
fn rasterize_and_insert(
    device: &wgpu::Device,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    atlas: &mut GlyphAtlas,
    key: cosmic_text::CacheKey,
) -> Option<u32> {
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
    atlas.insert(device, key, content, w, h, left, top, &image.data)
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::text::encode::{ContentType, pack_uv};

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
