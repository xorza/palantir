//! The vocabulary a node declares layout in (sizing, alignment, justification, clipping, grid and scroll modes, overlay placement) and the bounds screens the pass math trusts.
//!
//! The drivers in [`crate::layout`] read it; the little behaviour here is overlay placement beside an anchor and what counts as a usable bound.

pub(crate) mod align;
pub(crate) mod anchor;
pub(crate) mod axis;
pub(crate) mod clip_mode;
pub(crate) mod grid_cell;
pub(crate) mod justify;
pub(crate) mod layout_mode;
pub(crate) mod packed_layout_meta;
pub(crate) mod placement;
pub(crate) mod scroll_axes;
pub(crate) mod sizing;
pub(crate) mod track;
pub(crate) mod visibility;
