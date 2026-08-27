//! The vocabulary a node declares its layout in — sizing, alignment,
//! justification, clipping, grid and scroll modes, overlay placement — and
//! the bounds screens the pass math trusts them through.
//!
//! Authoring code writes these through `Configure` and the drivers in
//! `layout` read them. What little behaviour lives here is the resolution
//! that belongs to the vocabulary rather than to a pass — where an
//! overlay lands beside its anchor, and what counts as a usable bound.

pub(crate) mod align;
pub(crate) mod clip_mode;
pub(crate) mod grid_cell;
pub(crate) mod justify;
pub(crate) mod layout_mode;
pub(crate) mod limits;
pub(crate) mod overlay;
pub(crate) mod placement;
pub(crate) mod sizing;
pub(crate) mod track;
