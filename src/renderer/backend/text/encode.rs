//! Per-batch instance emission: extracted glyph placements →
//! `GlyphInstance`s.
//!
//! Two paths:
//!
//! - **Cache hit**: prior frames laid this exact `(TextShapeKey,
//!   scale, subpixel origin bin, area color)` run out into the atlas;
//!   the resulting origin-relative `GlyphInstance` templates are stored
//!   in the [`EncodedCache`]. Emit = a copy with origin-shifted
//!   positions, no shaper session, no per-glyph atlas hashmap lookup.
//!   This is the ~37% of frame time we're targeting.
//! - **Cache miss**: extracts the run's glyph placements through the
//!   shaper's [`TextRenderSession`], touches/inserts atlas slots, emits
//!   to `out`, and populates the cache entry with the origin-relative
//!   templates so the next frame at the same `(key, scale, bins,
//!   color)` lands on the fast path. Runs whose lines were y-culled
//!   against their bounds are *not* cached — the key omits bounds, so
//!   a truncated template would replay wrong after a scroll.
//!
//! Atlas eviction reuses slot rectangles for new glyphs; any cached
//! entry holding the old uv would point at the wrong image. Each
//! encoded glyph therefore records its atlas slot's generation and
//! re-checks it while emitting. Atlas growth preserves rects
//! (`etagere::grow`), so no invalidation is needed there.

use crate::primitives::num::F32Ext;
use crate::primitives::span::Span;
use crate::renderer::render_buffer::text::TextRun;
use crate::text::TextShapeRequest;
use crate::text::cosmic::{self, GlyphRasterKey};
use crate::text::key::TextShapeKey;
use crate::text::render::{GlyphImageKind, PlacedGlyph, RunPlacement, TextRenderSession};
use rustc_hash::FxHashMap;

use crate::renderer::backend::text::atlas::{GlyphAtlas, GlyphSlot, PackedGlyphMetadata};
use crate::renderer::backend::text::{ContentType, GlyphInstance};

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
/// the cache will serve a stale run's baked colours. The assertion in
/// `CosmicMeasure::extract_glyphs`'s glyph loop is the tripwire for
/// that invariant.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    pub(crate) text: TextShapeKey,
    /// `(scale * 65536).round() as u32`. 1/65536 px is below cosmic's
    /// 4-bin subpixel resolution, so distinct quantized scales are the
    /// only ones that produce distinct cosmic cache keys.
    pub(crate) scale_q: u32,
    pub(crate) area_color: u32,
    /// Packed subpixel bins of the run origin, exactly as produced by
    /// [`cosmic::SubpixelOrigin::bins`].
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
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedGlyph {
    pub(crate) instance: GlyphInstance,
    pub(crate) atlas_slot: u32,
    pub(crate) generation: u32,
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
    /// A cache hit emits `arena` straight out without walking cosmic,
    /// so the atlas slots backing the run would never get their LRU
    /// `last_use` bumped — `evict_one` could then reclaim a slot still
    /// referenced this frame and overwrite it with a different glyph.
    /// On hit we store the current frame through each index — an
    /// indexed write, no map probe per glyph. Each encoded glyph's
    /// generation keeps the index honest when `evict_one` makes a slot
    /// reusable.
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
    let sub = cosmic::subpixel_origin(r.origin);
    EncodedRunKey {
        key: EncodedKey {
            text: r.key,
            scale_q: (scale * 65536.0).fast_round() as u32,
            area_color,
            bins: sub.bins,
        },
        origin_x: sub.x,
        origin_y: sub.y,
    }
}

/// Cache-hit fast path. Returns `true` if `key` resolved to a live
/// entry and the run's glyphs were emitted to `out`. Caller falls
/// through to the slow path on `false`.
pub(crate) fn try_emit_cached(
    cache: &mut EncodedCache,
    slots: &mut [GlyphSlot],
    current_frame: u64,
    run_key: &EncodedRunKey,
    out: &mut Vec<GlyphInstance>,
) -> bool {
    let Some(entry) = cache.map.get_mut(&run_key.key) else {
        return false;
    };
    let span = entry.span;
    let glyphs = &cache.arena[span.range()];
    let out_start = out.len();
    out.reserve(glyphs.len());
    // One pass emits the instance and refreshes the backing slot's LRU
    // stamp together, so `evict_one` can't reclaim a slot we're still
    // drawing this frame.
    for glyph in glyphs {
        let slot = &mut slots[glyph.atlas_slot as usize];
        if slot.generation != glyph.generation {
            out.truncate(out_start);
            return false;
        }
        let g = glyph.instance;
        out.push(GlyphInstance {
            pos: [g.pos[0] + run_key.origin_x, g.pos[1] + run_key.origin_y],
            dim: g.dim,
            uv_and_kind: g.uv_and_kind,
            color: g.color,
        });
        slot.last_use = current_frame;
    }
    entry.last_use = current_frame;
    true
}

/// Slow-walk state for one batch's encoded-cache misses: the GPU-side
/// dependencies plus the extraction scratch and instance sink borrowed
/// from `TextBackend`. [`Self::encode_run`] encodes one miss.
#[derive(Debug)]
pub(crate) struct EncodeCtx<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) atlas: &'a mut GlyphAtlas,
    pub(crate) cache: &'a mut EncodedCache,
    /// Retained per-run extraction scratch.
    pub(crate) placed: &'a mut Vec<PlacedGlyph>,
    /// Drawable-instance sink shared with the cache-hit fast path.
    pub(crate) out: &'a mut Vec<GlyphInstance>,
}

impl EncodeCtx<'_> {
    /// Encode one run that missed the encoded cache: extract its glyph
    /// placements through the shaper `session` (which restores evicted
    /// buffers and applies the y-cull), touch/insert atlas slots, emit
    /// `GlyphInstance`s and populate the encoded cache as a side
    /// effect. Callers are expected to have already filtered out
    /// invalid keys and cache hits.
    pub(crate) fn encode_run(
        &mut self,
        session: &mut TextRenderSession<'_>,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        run_key: EncodedRunKey,
    ) {
        let current_frame = self.atlas.current_frame;
        // The straight-linear cast of the run's colour — already baked
        // into the cache identity, reused as the emit colour.
        let color = run_key.key.area_color;

        // `culled` records whether the extraction dropped any line: a
        // truncated encode must not become a cache template
        // (`EncodedKey` carries no bounds, so integer-pixel scrolling
        // replays the same key with lines newly in view — they'd stay
        // blank forever).
        let culled = session.extract_glyphs(request, placement, self.placed);

        // Build a fresh cache entry as a side effect of the slow walk.
        // Slots used earlier this frame cannot be eviction candidates,
        // so an atlas eviction during the walk cannot invalidate a
        // template already appended here.
        let pending_start = self.cache.arena.len() as u32;

        for g in self.placed.iter() {
            let idx = match self.atlas.touch(&g.raster_key) {
                Some(i) => i,
                None => {
                    match rasterize_and_insert(self.device, session, self.atlas, g.raster_key) {
                        Some(i) => i,
                        None => continue,
                    }
                }
            };
            let slot = self.atlas.slots[idx as usize];

            if slot.alloc.is_none() {
                continue;
            }

            let abs_x = g.x + slot.left as i32;
            let abs_y = g.y - slot.top as i32;
            let dim = (slot.width as u32) | ((slot.height as u32) << 16);
            let uv_and_kind = pack_uv(slot.x, slot.y, slot.content);

            self.out.push(GlyphInstance {
                pos: [abs_x, abs_y],
                dim,
                uv_and_kind,
                color,
            });
            self.cache.arena.push(EncodedGlyph {
                instance: GlyphInstance {
                    pos: [abs_x - run_key.origin_x, abs_y - run_key.origin_y],
                    dim,
                    uv_and_kind,
                    color,
                },
                atlas_slot: idx,
                generation: slot.generation,
            });
        }

        // Only cache full encodes. The caller already filtered invalid
        // keys; valid-key here is a precondition. Partially visible
        // runs re-encode each frame; the reverse (a cached full
        // template replayed under narrower bounds) is safe — the batch
        // scissor is the real clip.
        if !culled {
            let span = Span::new(pending_start, self.cache.arena.len() as u32 - pending_start);
            self.cache.map.insert(
                run_key.key,
                EncodedEntry {
                    span,
                    last_use: current_frame,
                },
            );
        } else {
            // Roll back the partial entry truncated by the cull.
            self.cache.arena.truncate(pending_start as usize);
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

/// Cache miss path: ask the shaper session for the bitmap, push into
/// the atlas. Returns the new slot's slab index. A free fn, not an
/// `EncodeCtx` method: it's called while `self.placed` is being
/// iterated, so it may borrow only the disjoint device/atlas fields.
fn rasterize_and_insert(
    device: &wgpu::Device,
    session: &mut TextRenderSession<'_>,
    atlas: &mut GlyphAtlas,
    key: GlyphRasterKey,
) -> Option<u32> {
    let image = session.rasterize(key)?;
    let content = match image.kind {
        GlyphImageKind::Color => ContentType::Color,
        GlyphImageKind::Mask => ContentType::Mask,
    };
    let Ok(metadata): Result<PackedGlyphMetadata, _> = (&image.placement).try_into() else {
        tracing::warn!(
            ?key,
            width = image.placement.width,
            height = image.placement.height,
            left = image.placement.left,
            top = image.placement.top,
            "skipping glyph raster outside packed atlas metadata range",
        );
        return Some(atlas.insert_unallocated(key, content, PackedGlyphMetadata::EMPTY));
    };

    if metadata.is_empty() {
        return Some(atlas.insert_unallocated(key, content, metadata));
    }
    atlas.insert(device, key, content, metadata, &image.data)
}

#[cfg(test)]
mod tests {
    use etagere::AllocId;

    use crate::primitives::span::Span;
    use crate::renderer::backend::text::GlyphInstance;
    use crate::renderer::backend::text::atlas::GlyphSlot;
    use crate::renderer::backend::text::encode::{
        ContentType, EncodedCache, EncodedEntry, EncodedGlyph, EncodedKey, EncodedRunKey, pack_uv,
        try_emit_cached,
    };
    use crate::text::key::TextShapeKey;

    #[test]
    fn pack_uv_round_trip() {
        let p = pack_uv(12345, 54321, ContentType::Color);
        assert_eq!(p & 0x7FFF, 12345);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);

        let p = pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
    }

    fn run_key(text_hash: u64, origin_x: i32) -> EncodedRunKey {
        EncodedRunKey {
            key: EncodedKey {
                text: TextShapeKey {
                    text_hash,
                    ..TextShapeKey::INVALID
                },
                scale_q: 65_536,
                area_color: 0,
                bins: 0,
            },
            origin_x,
            origin_y: 0,
        }
    }

    fn slot(generation: u32) -> GlyphSlot {
        GlyphSlot {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            left: 0,
            top: 0,
            content: ContentType::Mask,
            alloc: Some(AllocId::deserialize(0)),
            generation,
            last_use: 1,
        }
    }

    fn encoded_glyph(atlas_slot: u32, generation: u32, x: i32) -> EncodedGlyph {
        EncodedGlyph {
            instance: GlyphInstance {
                pos: [x, 0],
                dim: 1 | (1 << 16),
                uv_and_kind: 0,
                color: 0,
            },
            atlas_slot,
            generation,
        }
    }

    #[test]
    fn slot_generation_invalidates_only_referencing_entry() {
        let invalidated_key = run_key(1, 10);
        let stable_key = run_key(2, 20);
        let mut cache = EncodedCache {
            arena: vec![
                encoded_glyph(1, 4, 1),
                encoded_glyph(0, 2, 2),
                encoded_glyph(2, 7, 3),
            ],
            ..EncodedCache::default()
        };
        cache.map.insert(
            invalidated_key.key,
            EncodedEntry {
                span: Span::new(0, 2),
                last_use: 1,
            },
        );
        cache.map.insert(
            stable_key.key,
            EncodedEntry {
                span: Span::new(2, 1),
                last_use: 1,
            },
        );
        let mut slots = vec![slot(3), slot(4), slot(7)];
        let mut out = vec![GlyphInstance {
            pos: [-1, -1],
            dim: 0,
            uv_and_kind: 0,
            color: 0,
        }];

        assert!(try_emit_cached(
            &mut cache,
            &mut slots,
            9,
            &stable_key,
            &mut out,
        ));
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].pos, [23, 0]);
        assert_eq!(slots[2].last_use, 9);
        assert_eq!(cache.map[&stable_key.key].last_use, 9);

        assert!(!try_emit_cached(
            &mut cache,
            &mut slots,
            9,
            &invalidated_key,
            &mut out,
        ));
        assert_eq!(
            out.len(),
            2,
            "a late generation mismatch must roll back partial output",
        );
        assert_eq!(cache.map[&invalidated_key.key].last_use, 1);
        assert_eq!(
            slots[1].last_use, 9,
            "validated slots stay live for the slow-path rebuild",
        );
    }
}
