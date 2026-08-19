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
use crate::primitives::span::Span;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::{self};

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
pub(super) struct EncodedKey {
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

/// `encode_key_for`'s named result. Carries the cache identity plus
/// the integer-pixel origin (the fractional component is folded into
/// `EncodedKey::bins`).
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedRunKey {
    key: EncodedKey,
    origin_x: i32,
    origin_y: i32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedEntry {
    /// Slice into `EncodedCache.arena` holding this run's glyph
    /// templates.
    pub(super) span: Span,
    last_use: u64,
}

/// Build the cache key for a `TextDrawRow` placed at `frame_scale * r.scale`,
/// plus the integer-pixel origin (cosmic's subpixel bins absorb the
/// fractional component into per-glyph `CacheKey`s, so two runs at
/// different fractional origins live in different cache entries).
pub(super) fn encode_key_for(r: &TextDrawRow, frame_scale: f32) -> EncodedRunKey {
    let scale = frame_scale * r.scale;
    let area_color: u32 = bytemuck::cast(r.color);
    let sub = render::subpixel_origin(r.origin);
    EncodedRunKey {
        key: EncodedKey {
            text: r.text.key,
            scale_q: (scale * 65536.0).fast_round() as u32,
            area_color,
            bins: sub.bins,
        },
        origin_x: sub.x,
        origin_y: sub.y,
    }
}

// Gated with its two readers exactly — the `text_atlas` benchmark and
// the retention tests below — rather than on `internals`, which the two
// integration suites enable without ever building a churn fixture.
// `pub(super)` reaches both: the benchmark's caller lives in this
// module's sibling `bench.rs`, not outside the text backend.
#[cfg(any(test, feature = "bench"))]
pub(super) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::common::block_arena::BlockArenaCounts;
    use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
    use crate::renderer::backend::text::encode::cache::{EncodedCache, EncodedGlyph};
    #[cfg(test)]
    use crate::renderer::backend::text::encoded_counters::EncodedCounts;

    /// Churn harness: `runs` rows **re-keyed every frame**, which is what
    /// a zoom (a fresh `scale_q` per `TEXT_SCALE_STEP` rung) or a resize
    /// drag (a fresh `max_w_q` per committed width) produces.
    ///
    /// Not modelled on `bins`: that component takes only 16 values and a
    /// pan cycles back through them, so sub-pixel motion re-hits its
    /// entries instead of minting new ones.
    #[derive(Debug, Default)]
    pub(crate) struct ChurnBench {
        cache: EncodedCache,
        frame: u64,
        runs: u32,
        glyphs_per_row: u32,
    }

    impl ChurnBench {
        pub(crate) fn new(runs: u32, glyphs_per_row: u32) -> Self {
            Self {
                cache: EncodedCache::default(),
                frame: 0,
                runs,
                glyphs_per_row,
            }
        }

        /// One gesture frame: every run mints a key it will never be
        /// asked for again, encodes its glyphs into the arena, and the
        /// sweep runs. Returns the resident row count.
        pub(crate) fn churn_frame(&mut self) -> usize {
            self.frame += 1;
            for run in 0..self.runs {
                for glyph in 0..self.glyphs_per_row {
                    self.cache.pending.push(EncodedGlyph {
                        instance: RasterQuad {
                            pos: [glyph as i32, run as i32],
                            dim: 0,
                            uv_and_kind: 0,
                            color: 0,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                let key = EncodedKey {
                    text: TextShapeKey::INVALID,
                    // The churn axis: one fresh rung per frame.
                    scale_q: self.frame as u32,
                    // Run identity, stable across the gesture.
                    area_color: run,
                    bins: 0,
                };
                self.cache.settle(key, self.frame, true);
            }
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }

        #[cfg(test)]
        pub(crate) fn rows(&self) -> usize {
            self.cache.map.len()
        }

        pub(crate) fn arena_len(&self) -> usize {
            self.cache.arena.slots.len()
        }

        #[cfg(test)]
        pub(crate) fn counts(&self) -> EncodedCounts {
            self.cache.counters.counts()
        }

        /// The *arena*'s tallies rather than the cache's — whether a
        /// saturated gesture still extends the block storage.
        #[cfg(test)]
        pub(crate) fn block_counts(&self) -> BlockArenaCounts {
            self.cache.arena.counters.counts()
        }
    }

    /// Sweep harness for the `encoded_cache_sweep` benchmark. Populates
    /// a cache with `rows` live rows of `glyphs_per_row` each — the
    /// steady-state shape a text-heavy frame leaves behind — so a
    /// benchmark iteration measures [`EncodedCache::sweep`] alone,
    /// isolated from the encode work that surrounds it in `end_frame`.
    /// Gated with that benchmark alone — unlike [`ChurnBench`], nothing
    /// here answers a question a test asks.
    #[cfg(feature = "bench")]
    #[derive(Debug, Default)]
    pub(crate) struct SweepBench {
        cache: EncodedCache,
        frame: u64,
    }

    #[cfg(feature = "bench")]
    impl SweepBench {
        /// Build `rows` rows **one per frame**, so their expiry tickets
        /// land on distinct buckets exactly as a real scene's inserts
        /// do.
        ///
        /// Populating them all on one frame would be easier and wrong:
        /// every ticket would share a bucket, and the measurement would
        /// alternate between frames that drain nothing and one frame in
        /// a ring that drains everything — a burst the fixture invented,
        /// not one the cache produces.
        pub(crate) fn new(rows: u32, glyphs_per_row: u32) -> Self {
            let mut cache = EncodedCache::default();
            let mut frame = 0;
            for row in 0..rows {
                frame += 1;
                for glyph in 0..glyphs_per_row {
                    cache.pending.push(EncodedGlyph {
                        instance: RasterQuad {
                            pos: [glyph as i32, row as i32],
                            dim: 0,
                            uv_and_kind: 0,
                            color: 0,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                // Through `settle`, not by poking the map: it is what
                // files the expiry ticket and reserves the block, and a
                // fixture missing either would measure a sweep with
                // nothing to do.
                let key = EncodedKey {
                    text: TextShapeKey::INVALID,
                    scale_q: row,
                    area_color: 0,
                    bins: 0,
                };
                cache.settle(key, frame, true);
                // Park `last_use` beyond any frame the bench reaches, so
                // rows never expire and every fired ticket is re-filed.
                // That is the steady-state load: a drawn run refreshes
                // `last_use` on the encoded-cache hit path and files
                // nothing, so the sweep's whole job is re-filing.
                cache
                    .map
                    .get_mut(&key)
                    .expect("settle just inserted this row")
                    .last_use = u64::MAX / 2;
                // Keep the wheel's clock in step with the inserts, or
                // tickets more than a ring out get clamped together and
                // the stagger is lost before the bench starts.
                cache.sweep(frame);
            }
            Self { cache, frame }
        }

        /// One steady-state `end_frame` sweep: the clock advances, the
        /// handful of tickets that came due are re-filed, and nothing
        /// expires — exactly the pass a frame pays when the cache is
        /// warm and every row is still on screen. Returns the surviving
        /// row count so the caller can assert the fixture stayed intact.
        ///
        /// The frame *must* advance per call. Sweeping the same frame
        /// twice is a no-op under a deadline wheel, so a fixed-frame
        /// harness would measure an early return and guard nothing.
        pub(crate) fn sweep_steady(&mut self) -> usize {
            self.frame += 1;
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }
    }
}

#[cfg(test)]
mod tests;
