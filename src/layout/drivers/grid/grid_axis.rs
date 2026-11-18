//! [`GridAxis`]: one axis of one grid as the track solve reads it, and every predicate over its
//! tracks.

use crate::common::span::Span;
use crate::layout::drivers::grid::grid_track_store::{GridAxisId, GridTrackStore};
use crate::primitives::layout::offer;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::primitives::math::num::F32Px;

/// One grid axis as the solve reads it: which grid and axis, its tracks, and the gap between
/// them.
#[derive(Clone, Copy, Debug)]
pub(super) struct GridAxis<'a> {
    pub(super) id: GridAxisId,
    pub(super) tracks: &'a [Track],
    pub(super) gap: f32,
}

/// What an axis's Fill tracks add to the grid's measured size, and the least offer it holds from.
#[derive(Clone, Copy, Debug)]
pub(super) struct FillShare {
    /// What holds each track's measured extent at a shared fraction ([`GridAxis::fill_extent`]), as
    /// CSS Grid §12.7.1 sizes a flexible track in indefinite space and WPF's Grid sums a star
    /// track's `MinSize` into its desired size. Where the grid fills a finite offer, at most what
    /// the tracks resolved to at it, as arrange gives them no more there: a min-clamped track
    /// would raise the shared fraction past the offer.
    pub(super) extent: f32,
    /// The axis's [`ContentMeasure::stable_from`](crate::layout::measured::ContentMeasure::stable_from), the tracks' own and the cap's.
    pub(super) stable_from: f32,
}

impl GridAxis<'_> {
    /// The flexible fraction the Fill tracks need to hold their `extents`, each at least its
    /// track's `min`, where no leftover exists to share, as CSS Grid finds the size of an `fr` for
    /// indefinite free space (§12.7.1): the largest extent over its track's weight, so a track of
    /// weight `w` takes `fraction · w` and holds its extent. `0` with no Fill track.
    pub(super) fn fill_fraction(self, extents: &[f32]) -> f32 {
        self.tracks
            .iter()
            .zip(extents)
            .filter_map(|(t, &extent)| Some(extent.max(t.min()) / t.sizing().fill_weight()?))
            .fold(0.0, f32::max)
    }

    /// What the Fill tracks take at the [`Self::fill_fraction`] of `extents`: the fraction times
    /// their summed weight.
    pub(super) fn fill_extent(self, extents: &[f32]) -> f32 {
        let weights: f32 = self
            .tracks
            .iter()
            .filter_map(|t| t.sizing().fill_weight())
            .sum();
        self.fill_fraction(extents) * weights
    }

    /// The sum of `sizes` over the tracks that do not Fill.
    pub(super) fn sum_non_fill(self, sizes: &[f32]) -> f32 {
        self.sum_of(sizes, |t| t.sizing().fill_weight().is_none())
    }

    /// The sum of `sizes` over the tracks that Fill.
    pub(super) fn sum_fill(self, sizes: &[f32]) -> f32 {
        self.sum_of(sizes, |t| t.sizing().fill_weight().is_some())
    }

    fn sum_of(self, sizes: &[f32], keep: impl Fn(&Track) -> bool) -> f32 {
        self.tracks
            .iter()
            .zip(sizes)
            .filter(|(t, _)| keep(t))
            .map(|(_, &size)| size)
            .sum()
    }

    /// What [`Self::sum_non_fill`] sums at each track's least, `content_min` its min-content
    /// floors; never more than the sum.
    pub(super) fn floor_non_fill(self, content_min: &[f32], sizes: &[f32]) -> f32 {
        self.tracks
            .iter()
            .zip(content_min)
            .zip(sizes)
            .map(|((t, &min), &size)| {
                if t.sizing().fill_weight().is_some() {
                    0.0
                } else if t.sizing().is_hug() {
                    t.content_floor(min)
                } else {
                    // A Fixed track at its least: its `min`, below its size when it yields.
                    t.min().min(size)
                }
            })
            .sum()
    }

    /// Whether `span` crosses a Fill track: such a span grows only its Fill tracks
    /// ([`Self::distribute_span`]), and after every span that crosses none, as CSS Grid §12.5
    /// steps 3 and 4 order them.
    pub(super) fn crosses_fill(self, span: Span) -> bool {
        any_fill(&self.tracks[span.range()])
    }

    /// Whether a track of the axis Fills.
    pub(super) fn has_fill(self) -> bool {
        any_fill(self.tracks)
    }

    /// Grow the tracks `span` crosses until the span holds `need` (CSS Grid §12.5 steps 3 and 4):
    /// the shortfall over the tracks' extents and the gaps between them splits over the tracks that
    /// grow. A span that crosses a Fill track grows only its Fill tracks, in proportion to their
    /// weights, and holds every other track at its extent (step 4); one that crosses none grows its
    /// Hug tracks evenly (step 3). A track that reaches its `max` stops there, as the track solve
    /// holds it, and what it could not take splits again over the rest; what no track can take
    /// overflows. A Fixed track's extent is its size; `extents` holds the rest. Fill weights split
    /// as WPF star ratios, so a weight under 1 takes its ratio of the whole shortfall, where CSS
    /// splits only that fraction of it.
    pub(super) fn distribute_span(self, span: Span, need: f32, extents: &mut [f32]) {
        let Self { tracks, gap, .. } = self;
        let range = span.range();
        let crossed = &tracks[range.clone()];
        let extents = &mut extents[range];
        let held: f32 = crossed
            .iter()
            .zip(extents.iter())
            .map(|(t, &extent)| {
                t.sizing()
                    .fixed_value()
                    .map_or(extent, |v| v.clamp(t.min(), t.max()))
            })
            .sum::<f32>()
            + gap.gaps_between(crossed.len());
        let flexible = any_fill(crossed);
        // The share a track grows by: its weight among the Fill tracks of a span that crosses one,
        // else 1 for each Hug track; 0 for a track that does not grow.
        let weight = |t: &Track| {
            if flexible {
                t.sizing().fill_weight().unwrap_or(0.0)
            } else if t.sizing().is_hug() {
                1.0
            } else {
                0.0
            }
        };
        let mut shortfall = need - held;
        // Each round fills every open track or closes at least one, so one round per track and a
        // last one suffice. Bounded, not `while`: a share too small to move a large extent in f32
        // would leave the shortfall where it was.
        for _ in 0..=crossed.len() {
            if shortfall <= 0.0 {
                return;
            }
            let open: f32 = crossed
                .iter()
                .zip(extents.iter())
                .filter(|(t, extent)| **extent < t.max())
                .map(|(t, _)| weight(t))
                .sum();
            if open <= 0.0 {
                return;
            }
            let per_weight = shortfall / open;
            for (t, extent) in crossed.iter().zip(extents.iter_mut()) {
                let w = weight(t);
                if w > 0.0 && *extent < t.max() {
                    let grown = (*extent + per_weight * w).min(t.max());
                    shortfall -= grown - *extent;
                    *extent = grown;
                }
            }
        }
    }

    /// The share of this axis's tracks, resolved in `store`, on a grid sized `sizing` offered
    /// `avail`.
    pub(super) fn fill_share(
        self,
        sizing: Sizing,
        avail: f32,
        store: &GridTrackStore,
    ) -> FillShare {
        let id = self.id;
        let held = self.fill_extent(store.extents(id));
        let stable_from = store.stable_from(id);
        if sizing.is_hug() || !avail.is_finite() || !self.has_fill() {
            return FillShare {
                extent: held,
                stable_from,
            };
        }
        let resolved = self.sum_fill(store.sizes(id));
        if held <= resolved {
            // A larger offer gives the Fill tracks no less, so the hold stands from this offer
            // up; a smaller one may cap it.
            FillShare {
                extent: held,
                stable_from: stable_from.max(avail),
            }
        } else {
            FillShare {
                extent: resolved,
                stable_from: offer::AT_OFFER_ONLY,
            }
        }
    }
}

/// Whether one of `tracks` Fills.
fn any_fill(tracks: &[Track]) -> bool {
    tracks.iter().any(|t| t.sizing().fill_weight().is_some())
}
