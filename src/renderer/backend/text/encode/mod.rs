//! Per-batch instance emission: extracted glyph placements →
//! `RasterQuad`s.
//!
//! Two paths:
//!
//! - **Cache hit**: prior frames laid this exact `(TextShapeKey,
//!   scale, subpixel origin bin, area color)` run out into the atlas;
//!   the resulting origin-relative `RasterQuad` templates are stored
//!   in the [`EncodedCache`](cache::EncodedCache). Emit = a copy with
//!   positions, no shaper lease, no per-glyph atlas hashmap lookup.
//!   This is the ~37% of frame time we're targeting.
//! - **Cache miss**: extracts the run's glyph placements through the
//!   shaper's glyph lease, touches/inserts atlas slots, emits
//!   to `out`, and populates the cache entry with the origin-relative
//!   templates so the next frame at the same `(key, scale, bins,
//!   color)` lands on the fast path. Runs that came out short — lines
//!   y-culled against their bounds, or a glyph the full atlas had no
//!   room for — are *not* cached: the key records neither bounds nor
//!   atlas occupancy, so a template with a hole would replay it on
//!   every hit and never retry.
//!
//! Atlas eviction reuses slot rectangles for new glyphs; any cached
//! entry holding the old uv would point at the wrong image. Each
//! encoded glyph therefore records its atlas slot's generation and
//! re-checks it while emitting. Atlas growth preserves rects
//! (`etagere::grow`), so no invalidation is needed there.

use crate::primitives::num::F32Ext;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::SubpixelOrigin;

pub(super) mod cache;
pub(super) mod encoder;

/// Cache-hit identity for an encoded run. Subpixel bins capture the
/// fractional component of `origin` that cosmic folds into per-glyph
/// `CacheKey`s (so different fractional origins produce different
/// atlas slots and can't share an entry).
///
/// `area_color` is in the key because the run's colour is baked into
/// every cached [`RasterQuad`](crate::renderer::backend::text::RasterQuad)
/// colour at insert time. **This is only
/// sufficient because palantir shapes every run with one uniform
/// colour** — `attrs_for` (`cosmic.rs`) sets no per-span colour, so
/// cosmic never emits a per-glyph `color_opt`. If per-span colours are
/// ever added, fold a colour-span fingerprint into this key *first*, or
/// the cache will serve a stale run's baked colours. The assertion in
/// `TextGlyphs::extract_glyphs`'s glyph loop is the tripwire for
/// that invariant.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct EncodedKey {
    text: TextShapeKey,
    /// `(scale * 65536).round() as u32`. 1/65536 px is below cosmic's
    /// 4-bin subpixel resolution, so distinct quantized scales are the
    /// only ones that produce distinct cosmic cache keys.
    scale_q: u32,
    area_color: u32,
    /// Packed subpixel bins of the run origin, exactly as produced by
    /// [`crate::text::render::SubpixelOrigin::bins`].
    bins: u8,
}

/// [`Self::for_row`]'s named result. Carries the cache identity plus
/// the integer-pixel origin (the fractional component is folded into
/// `EncodedKey::bins`).
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedRunKey {
    key: EncodedKey,
    origin_x: i32,
    origin_y: i32,
}

impl EncodedRunKey {
    /// The cache key for `row` placed at `frame_scale * row.scale`, plus
    /// the integer-pixel origin — cosmic's subpixel bins absorb the
    /// fractional component into per-glyph `CacheKey`s, so two runs at
    /// different fractional origins live in different cache entries.
    pub(super) fn for_row(row: &TextDrawRow, frame_scale: f32) -> Self {
        let scale = frame_scale * row.scale;
        let area_color: u32 = bytemuck::cast(row.color);
        let sub = SubpixelOrigin::of(row.origin);
        Self {
            key: EncodedKey {
                text: row.text.key,
                scale_q: (scale * 65536.0).fast_round() as u32,
                area_color,
                bins: sub.bins,
            },
            origin_x: sub.x,
            origin_y: sub.y,
        }
    }
}
