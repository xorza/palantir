//! Cluster-precise truncation: cutting a run to a committed width, with
//! or without a trailing ellipsis.
//!
//! The cut is measured against the cached *unbounded* shape rather than
//! by reshaping the whole string per width, which is what keeps a resize
//! drag cheap — [`CosmicMeasure::measure_truncated`] carries the
//! measurement that settled it, including why delegating to cosmic's own
//! `set_ellipsize` was written, benchmarked and reverted.

use crate::text::cosmic::geometry::{ShapedGeometry, shaped_geometry};
use crate::text::cosmic::retention::CacheEntry;
use crate::text::cosmic::{CosmicMeasure, ELLIPSIS_MEMO_SLOTS, attrs_for, recycle_buffer};
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, WrapFloor};
use crate::text::{FontFamily, FontWeight};
use cosmic_text::{Buffer, Metrics, Shaping};
use rustc_hash::FxHashMap;

/// The cached unbounded shape a truncating fit cuts from.
///
/// [`CosmicMeasure::measure_truncated`] calls
/// [`CosmicMeasure::ensure_buffer`] on this key before reaching for it,
/// and re-reads it once per back-off round because the shaping in
/// between needs `&mut self`, so the borrow cannot be held across the
/// loop.
///
/// Hands back the whole entry rather than its buffer: the caller wants
/// the measured [`TextRoot`] as well as the glyphs, and both come out of
/// the one lookup.
///
/// Takes the map rather than `&self` on purpose: the caller holds
/// `&mut self.logical_order` at the same time, and only a borrow of the
/// one field stays disjoint from it.
#[inline]
fn truncation_probe(cache: &FxHashMap<TextShapeKey, CacheEntry>, key: TextShapeKey) -> &CacheEntry {
    cache
        .get(&key)
        .expect("truncation requires the cached unbounded shape")
}

/// Right edge (widest `x + w` across glyphs — an RTL run's last glyph is
/// its leftmost) of a shaped buffer's first layout run, or `0.0` when
/// empty — the rendered width of one line.
///
/// For the one-glyph unbounded probe [`CosmicMeasure::ellipsis_advance`]
/// shapes, whose line starts at 0, the right edge is the width.
/// [`shaped_geometry`] spans `left..right` instead because it also
/// measures width-bounded buffers, which cosmic may anchor away from the
/// origin — and every caller that has a measured [`TextRoot`] to hand
/// reads `size.w` off that rather than walking glyphs again.
fn first_line_right(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .and_then(|r| r.glyphs.iter().map(|g| g.x + g.w).reduce(f32::max))
        .unwrap_or(0.0)
}

/// Memoized trailing advance of "…" for one face.
///
/// `Default` only to satisfy `tinyvec`'s `Array` bound — it fills the
/// unused tail of [`CosmicMeasure::ellipsis`], which `len` keeps out of
/// every read. A zeroed memo could not match a live face anyway:
/// `quantize_metric` floors `size_q` at 1.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EllipsisMemo {
    size_q: u32,
    family_q: u8,
    weight_q: u8,
    advance: f32,
}

impl EllipsisMemo {
    /// Whether both were shaped from the same face at the same size.
    fn same_face(&self, other: &Self) -> bool {
        self.size_q == other.size_q
            && self.family_q == other.family_q
            && self.weight_q == other.weight_q
    }
}

/// One shaped glyph reduced to what the truncation cut reads: the source
/// bytes it covers and the advance it costs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClusterGlyph {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) advance: f32,
}

/// Longest logical byte prefix of `count` shaped glyphs whose advances sum
/// within `avail` and stay below `max_end`. `order` is retained scratch,
/// refilled with the glyph indices sorted into logical order; `glyph` reads
/// one glyph by its original (visual-order) index.
///
/// Reading through `glyph` rather than taking `&[LayoutGlyph]` buys two
/// things: the caller keeps its borrow of the cache disjoint from its
/// borrow of `order`, and the cut can be unit-tested against hand-built
/// advances instead of whatever the installed fonts happen to measure.
///
/// The result is always strictly below `max_end`, so passing the previous
/// answer retires at least one more cluster — that is what makes
/// [`CosmicMeasure::measure_truncated`]'s back-off terminate. Pass
/// `usize::MAX` for an unbounded first cut.
///
/// Glyphs arrive in visual order, so a glyph's `x` follows the reading
/// direction rather than the logical prefix — an RTL run's first glyph sits
/// at the *right* edge and its trailing edges descend. Summing advances in
/// logical order instead makes `text[..cut]` the prefix that fits whichever
/// way the run reads.
///
/// One grapheme cluster can shape to several glyphs sharing a byte range
/// (flag and ZWJ emoji, Indic conjuncts), so the prefix only advances past a
/// cluster once every glyph covering it is paid for. Committing mid-cluster
/// would claim bytes whose advance the budget never covered, and the prefix
/// would reshape wider than `avail`.
pub(crate) fn fitting_prefix(
    count: usize,
    glyph: impl Fn(usize) -> ClusterGlyph,
    order: &mut Vec<u32>,
    avail: f32,
    max_end: usize,
) -> usize {
    order.clear();
    order.extend(0..count as u32);
    // Visual order *is* logical order for an LTR run, which is nearly
    // every run this shapes. Checking costs one key call per glyph;
    // sorting costs `n log n` of them, since `sort_unstable_by_key`
    // re-invokes the key rather than caching it. The cut itself only
    // ever reads a short prefix, so on a long single-line run — a file
    // path, a log line — the skipped sort was the dominant term.
    if !order.is_sorted_by_key(|&i| glyph(i as usize).start) {
        order.sort_unstable_by_key(|&i| glyph(i as usize).start);
    }
    let mut cut = 0usize;
    let mut used = 0.0_f32;
    for (pos, &i) in order.iter().enumerate() {
        let g = glyph(i as usize);
        // Ends are non-decreasing in logical order, so once one reaches the
        // bound no later glyph can be committed either.
        if g.end >= max_end {
            break;
        }
        used += g.advance;
        if used > avail {
            break;
        }
        let cluster_paid = order
            .get(pos + 1)
            .is_none_or(|&next| glyph(next as usize).start >= g.end);
        if cluster_paid {
            cut = g.end;
        }
    }
    cut
}

impl CosmicMeasure {
    /// Shape `text` as a single line truncated to fit `w`. Truncation is
    /// cluster-precise: the cached unbounded shape gives per-glyph advances,
    /// [`fitting_prefix`] cuts after the last fully paid-for cluster, then we
    /// shape the (possibly truncated) prefix on one **natural** line — no
    /// per-line align. The committed width only decides the cut; the encoder
    /// positions/aligns the single line, so the measured extent is the glyph
    /// width, not `w` (binding to `w` + center align would inflate a
    /// fits-anyway label to ~half the box). `LineFit::Ellipsis` reserves room
    /// for and appends a trailing `…`; `LineFit::Clip` cuts flush to `w`
    /// with no marker. The buffer caches under a fit-discriminated key (so it
    /// can't collide with the wrapped buffer — or the other truncation mode —
    /// at the same width). `intrinsic_min` is 0 — a truncated run can shrink
    /// to nothing.
    ///
    /// The shaped prefix is verified against `w` and retires a further
    /// cluster until it fits, so the measured extent never exceeds the
    /// committed width — the cut alone cannot guarantee that, since
    /// reshaping the prefix changes its shaping context.
    ///
    /// # Why not `Buffer::set_ellipsize`
    ///
    /// Cosmic 0.19 can do this itself, and delegating to it deletes
    /// roughly 290 lines: this function, [`fitting_prefix`],
    /// [`ClusterGlyph`], the ellipsis memo, and three retained scratch
    /// fields. That was written, measured, and reverted — **4.9x slower**
    /// on `text_shape/resize_drag_frame` (1.42 µs -> 7.13 µs per run per
    /// frame).
    ///
    /// The reason is not the wrap mode (`Wrap::Glyph`, `Wrap::None` and a
    /// one-line height cap all measure within 10% of each other) but how
    /// much text is reshaped per committed width. Cosmic has to see the
    /// whole string to decide where to cut, so every new width reshapes
    /// all of it; shaping a 108-character label costs ~9x what its
    /// seven-character cut prefix costs.
    ///
    /// **So the dependency on the cached unbounded probe is the point,
    /// not a wart.** It is what a drag reuses: the full-string shape is
    /// paid once, [`fitting_prefix`] finds the cut by scanning glyphs
    /// that are already there, and only the short prefix is reshaped per
    /// frame. Anything that revisits this has to keep the full-string
    /// shape cached across widths — cosmic allows that (`set_size`
    /// re-lays-out without re-shaping), but one buffer holds one layout
    /// and the cache is keyed per width, so it needs a different
    /// buffer/key model rather than a different call.
    pub(super) fn measure_truncated(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        let key = request.key;
        let fit = key.fit();
        let width = key
            .max_width_px()
            .expect("measure_truncated requires a finite width");
        debug_assert!(
            matches!(fit, LineFit::Clip | LineFit::Ellipsis),
            "measure_truncated requires Clip or Ellipsis",
        );
        if let Some(hit) = self.cache_hit(key) {
            return hit;
        }
        let unbounded = request.unbounded_version();
        self.ensure_buffer(unbounded);
        let metrics = Metrics::new(key.font_size_px(), key.line_height_px());
        let family = key.family();
        let weight = key.weight();
        let attrs = attrs_for(family, weight);
        // Reserve the ellipsis width only when we'll append one; a plain
        // clip cuts flush to the full available width. Resolved before
        // borrowing the probe, since shaping "…" needs `&mut self`.
        let mut append_ellipsis = false;
        let avail = if matches!(fit, LineFit::Ellipsis) {
            let ellipsis_w = self.ellipsis_advance(key.size_q, metrics, family, weight);
            append_ellipsis = ellipsis_w <= width;
            (width - ellipsis_w).max(0.0)
        } else {
            width
        };
        let probe_key = unbounded.key;
        // Same question `TextSystem::measure` asks before it ever gets
        // here, against the same root — so it is asked the same way. The
        // probe's own measurement already answers it, which is why this
        // reads the entry rather than re-walking its glyphs.
        let fits_whole =
            fit.resolves_to_unbounded(&truncation_probe(&self.cache, probe_key).root, width);

        // Shape unbounded on one line: the cut already fit it to `w`, and the
        // encoder owns single-line placement. Binding to `Some(w)` + align
        // would measure the aligned glyph position, inflating a fits-anyway
        // label toward the box width.
        let mut buffer = self.acquire_buffer(metrics, None);
        let size = if fits_whole {
            // Re-shaping the identical text reproduces the probe, so this
            // branch cannot overrun `width`.
            buffer.set_text(request.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch)
                .root
                .size
        } else {
            // The cut spends advances measured in the *whole* run's shaping,
            // but the prefix reshapes in its own context: a joining script's
            // last letter is exposed at a new word end and takes a final form
            // wider than the medial one the budget paid for. So verify the
            // shaped result, and while it overruns, retire one more cluster.
            // `max_end` makes every retry strictly shorter, so the sequence
            // bottoms out at the empty prefix.
            let mut max_end = usize::MAX;
            loop {
                let cut = match truncation_probe(&self.cache, probe_key)
                    .buffer
                    .layout_runs()
                    .next()
                {
                    Some(run) => fitting_prefix(
                        run.glyphs.len(),
                        |i| ClusterGlyph {
                            start: run.glyphs[i].start,
                            end: run.glyphs[i].end,
                            advance: run.glyphs[i].w,
                        },
                        &mut self.logical_order,
                        avail,
                        max_end,
                    ),
                    None => 0,
                };
                self.truncate_scratch.clear();
                self.truncate_scratch
                    .push_str(request.text[..cut].trim_end());
                if append_ellipsis {
                    self.truncate_scratch.push('…');
                }
                // `set_text` resets the buffer in place, so a retry reuses
                // the line, shaping, and layout allocations it just filled.
                buffer.set_text(
                    self.truncate_scratch.as_str(),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
                let size = shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch)
                    .root
                    .size;
                if size.w <= width || cut == 0 {
                    break size;
                }
                max_end = cut;
            }
        };

        // Truncated runs are one natural line by construction: the cut
        // prefix comes from the unbounded probe's first layout run, and a
        // truncated run can shrink to nothing, so its floor is zero.
        // The prefix reshapes on an unbounded buffer with no per-line
        // align, so its block already starts at 0.
        let geometry = ShapedGeometry {
            root: TextRoot {
                size,
                // Genuinely zero, not unscanned: a truncated run can
                // shrink to nothing.
                intrinsic_min: Some(0.0),
                single_line: true,
            },
            left: 0.0,
        };
        self.insert(key, buffer, geometry);
        geometry.root
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized for
    /// the last face asked about.
    ///
    /// Only the *opening* budget: [`Self::measure_truncated`] verifies the
    /// shaped result against the committed width either way, so a stale or
    /// imprecise reservation costs retries, never correctness. What it buys
    /// is measured on `text_shape/ellipsis_width_churn`, whose arms hold
    /// the width churning so every frame is a truncation miss and the
    /// reservation is asked for again. See [`CosmicMeasure::ellipsis`]
    /// for what the slot count buys there.
    fn ellipsis_advance(
        &mut self,
        size_q: u32,
        metrics: Metrics,
        family: FontFamily,
        weight: FontWeight,
    ) -> f32 {
        let want = EllipsisMemo {
            size_q,
            family_q: family as u8,
            weight_q: weight as u8,
            advance: 0.0,
        };
        if let Some(memo) = self.ellipsis.iter().find(|memo| memo.same_face(&want)) {
            return memo.advance;
        }
        let mut buffer = self.acquire_buffer(metrics, None);
        buffer.set_text("…", &attrs_for(family, weight), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let advance = first_line_right(&buffer);
        recycle_buffer(&mut self.recycle_pool, buffer);
        self.counters.ellipsis_misses.bump();
        // `insert` panics at capacity, so retire the oldest first.
        if self.ellipsis.len() == ELLIPSIS_MEMO_SLOTS {
            self.ellipsis.pop();
        }
        self.ellipsis.insert(0, EllipsisMemo { advance, ..want });
        advance
    }
}
