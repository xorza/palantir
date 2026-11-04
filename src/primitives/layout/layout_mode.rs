//! Which driver lays out a node's children, and its per-mode settings.

use crate::common::non_max::NonMaxU16;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LayoutMode {
    Leaf,
    /// Children laid along `Axis` on one line.
    Stack(Axis),
    /// Children laid along `Axis`, wrapping onto further lines.
    WrapStack(Axis),
    ZStack,
    Canvas,
    Grid(GridDefId),
    Scroll(ScrollAxes),
    Scrollbars(ScrollbarsDefId),
}

/// Index into `Tree::grid_defs`; a type per side table, so a grid index can't reach the scrollbar table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefId(NonMaxU16);

impl GridDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        NonMaxU16::to_raw(Some(self.0))
    }

    /// The inverse of [`Self::to_raw`]; `None` for 0, which no id packs to.
    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match NonMaxU16::from_raw(raw) {
            Some(idx) => Some(Self(idx)),
            None => None,
        }
    }

    /// # Panics
    ///
    /// Panics past [`NonMaxU16::LAST`], at the caller's site.
    #[track_caller]
    pub(crate) fn from_idx(idx: usize) -> Self {
        Self(NonMaxU16::index(idx))
    }
}

impl From<GridDefId> for usize {
    fn from(value: GridDefId) -> Self {
        value.0.idx()
    }
}

/// Index into `Tree::scrollbar_defs`, a side table as the def exceeds the 16 bits `LayoutMode` packs.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollbarsDefId(NonMaxU16);

impl ScrollbarsDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        NonMaxU16::to_raw(Some(self.0))
    }

    /// The inverse of [`Self::to_raw`]; `None` for 0, which no id packs to.
    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match NonMaxU16::from_raw(raw) {
            Some(idx) => Some(Self(idx)),
            None => None,
        }
    }

    /// # Panics
    ///
    /// Panics past [`NonMaxU16::LAST`], at the caller's site.
    #[track_caller]
    pub(crate) fn from_idx(idx: usize) -> Self {
        Self(NonMaxU16::index(idx))
    }
}

impl From<ScrollbarsDefId> for usize {
    fn from(value: ScrollbarsDefId) -> Self {
        value.0.idx()
    }
}
