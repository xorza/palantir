//! A frame's draws in the order they paint: each a span of one lane's rows under one scissor.

use crate::common::span::Span;
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::paint_tier::PaintTier;

/// The rows a draw lands in: the quads, the text runs, or one [`PaintTier`]'s rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Quad,
    Text,
    Tier(PaintTier),
}

impl Lane {
    /// The lanes: quads, text and each tier.
    pub(crate) const COUNT: usize = 2 + PaintTier::COUNT;

    /// The lane's index below [`Self::COUNT`].
    pub(crate) const fn idx(self) -> usize {
        match self {
            Self::Quad => 0,
            Self::Text => 1,
            Self::Tier(tier) => 2 + tier.idx(),
        }
    }
}

/// A span of one lane's rows the backend draws under one scissor. The composer lists a frame's
/// runs in the order they paint, so the backend replays them as listed, cutting each scissor by
/// the damage; a backdrop's barrier is a run's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DrawRun {
    pub(crate) lane: Lane,
    /// The lane's rows it draws, never empty.
    pub(crate) rows: Span,
    /// `None` paints the whole viewport.
    pub(crate) scissor: Option<URect>,
    /// Its place among the runs of its lane, which the raster backends key the batch they
    /// prepare for it by.
    pub(crate) batch: u32,
}
