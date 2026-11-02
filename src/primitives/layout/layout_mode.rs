//! Which driver lays out a node's children, and its per-mode settings.

use crate::common::idx16::Idx16;
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
pub(crate) struct GridDefId(Idx16);

impl GridDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        self.0.to_raw()
    }

    /// The inverse of [`Self::to_raw`]; `None` for 0, which no id packs to.
    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match Idx16::from_raw(raw) {
            Some(idx) => Some(Self(idx)),
            None => None,
        }
    }

    pub(crate) fn from_idx(idx: usize) -> Self {
        Self(Idx16::new(idx, "grid_defs"))
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
pub(crate) struct ScrollbarsDefId(Idx16);

impl ScrollbarsDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        self.0.to_raw()
    }

    /// The inverse of [`Self::to_raw`]; `None` for 0, which no id packs to.
    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match Idx16::from_raw(raw) {
            Some(idx) => Some(Self(idx)),
            None => None,
        }
    }

    pub(crate) fn from_idx(idx: usize) -> Self {
        Self(Idx16::new(idx, "scrollbar_defs"))
    }
}

impl From<ScrollbarsDefId> for usize {
    fn from(value: ScrollbarsDefId) -> Self {
        value.0.idx()
    }
}
