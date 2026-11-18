//! How a split shares its extent between its two panes.

use serde::{Deserialize, Serialize};

use crate::primitives::layout::track::Track;
use crate::primitives::math::domain;
use crate::widgets::splitter::split_half::SplitHalf;

/// How a split shares its extent along its axis between its two panes: the value a
/// [`Splitter`](crate::Splitter) binds, and each split of a [`DockState`](crate::DockState).
///
/// A ratio grows both panes with the split; a fixed size keeps one pane at its pixels and gives
/// the other every pixel the split gains or loses, as WPF sizes a sidebar as a pixel column
/// beside a star column
/// (<https://learn.microsoft.com/en-us/dotnet/desktop/wpf/controls/how-to-resize-columns-with-a-gridsplitter>)
/// and GTK's `resize-start-child` and Qt's stretch factor of 0 keep one pane as the window
/// resizes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SplitSize {
    /// The first pane's share of the space past the rule, `0..=1`; an invalid share draws at
    /// half.
    Ratio(f32),
    /// The pane on `half` at `px` logical pixels, the other pane taking the rest. Where the split
    /// cannot hold `px` and the other pane at the splitter's `min_pane`, the pane gives way toward
    /// that floor in the same frame and keeps `px` for when the split grows back.
    Fixed {
        /// The pane that keeps its size.
        half: SplitHalf,
        /// Its size along the split's axis, a *length*; an invalid one draws at zero.
        px: f32,
    },
}

impl SplitSize {
    /// Both panes at half, the start of a split a person has not sized.
    pub const HALF: Self = Self::Ratio(0.5);

    /// This size with its number made safe to lay out: a ratio as a share, centered when invalid,
    /// and pixels as a length, zero when invalid.
    pub(super) const fn sanitized(self) -> Self {
        match self {
            Self::Ratio(ratio) => Self::Ratio(domain::fraction_or(ratio, 0.5)),
            Self::Fixed { half, px } => Self::Fixed {
                half,
                px: if domain::is_length(px) { px } else { 0.0 },
            },
        }
    }

    /// This size's shape, with the first pane at `first` of `span`, the space past the rule: a
    /// ratio as that share, a fixed size as its pane's extent.
    pub(super) fn with_first(self, first: f32, span: f32) -> Self {
        match self {
            Self::Ratio(_) => Self::Ratio(domain::fraction_or(first / span, 0.5)),
            Self::Fixed { half, .. } => Self::Fixed {
                half,
                px: match half {
                    SplitHalf::First => first,
                    SplitHalf::Second => span - first,
                },
            },
        }
    }

    /// The first pane's extent of `span` where [`Self::tracks`] draws it for a splitter whose
    /// panes keep `min_pane`, before content floors: a ratio's share, or the fixed pane at its
    /// pixels, given way toward the floor the other pane keeps.
    pub(super) fn first_of(self, span: f32, min_pane: f32) -> f32 {
        match self {
            Self::Ratio(ratio) => ratio * span,
            Self::Fixed { half, px } => {
                let fixed = px.min(span - min_pane).max(min_pane.min(px)).min(span);
                match half {
                    SplitHalf::First => fixed,
                    SplitHalf::Second => span - fixed,
                }
            }
        }
    }

    /// The two panes' tracks, first then second, for a splitter whose panes keep `min_pane`: two
    /// `Fill` shares for a ratio, as before a floor; for a fixed size its pane's pixels, giving way
    /// toward the floor, beside a `Fill` pane held at it.
    pub(super) fn tracks(self, min_pane: f32) -> [Track; 2] {
        match self {
            Self::Ratio(ratio) => [Track::fill(ratio), Track::fill(1.0 - ratio)],
            Self::Fixed { half, px } => {
                let fixed = Track::fixed(px).with_min(min_pane.min(px));
                let rest = Track::FILL.with_min(min_pane);
                match half {
                    SplitHalf::First => [fixed, rest],
                    SplitHalf::Second => [rest, fixed],
                }
            }
        }
    }

    /// Whether `self` and `other` draw differently: another shape or another number, past
    /// rounding.
    pub(super) fn differs(self, other: Self) -> bool {
        match (self, other) {
            (Self::Ratio(a), Self::Ratio(b)) => !domain::is_approx_zero(a - b),
            (Self::Fixed { half: a, px: pa }, Self::Fixed { half: b, px: pb }) => {
                a != b || !domain::is_approx_zero(pa - pb)
            }
            _ => true,
        }
    }
}
