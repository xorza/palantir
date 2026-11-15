//! Which driver lays out a node's children, and its per-mode settings.

use crate::common::non_max::NonMaxU16;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use std::marker::PhantomData;

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

/// An index into one of `Tree`'s side tables, a def too large for the 16 bits `LayoutMode` packs;
/// typed by its `Table`, so a grid index can't reach the scrollbar table.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TableIdx<Table>(NonMaxU16, PhantomData<Table>);

/// The table of `Tree::grid_defs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefs;

/// The table of `Tree::scrollbar_defs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollbarsDefs;

/// Index into `Tree::grid_defs`.
pub(crate) type GridDefId = TableIdx<GridDefs>;

/// Index into `Tree::scrollbar_defs`.
pub(crate) type ScrollbarsDefId = TableIdx<ScrollbarsDefs>;

impl<Table: Copy> TableIdx<Table> {
    pub(crate) const fn to_raw(self) -> u16 {
        NonMaxU16::to_raw(Some(self.0))
    }

    /// The inverse of [`Self::to_raw`]; `None` for 0, which no id packs to.
    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match NonMaxU16::from_raw(raw) {
            Some(idx) => Some(Self(idx, PhantomData)),
            None => None,
        }
    }

    /// # Panics
    ///
    /// Panics past [`NonMaxU16::LAST`], at the caller's site.
    #[track_caller]
    pub(crate) fn from_idx(idx: usize) -> Self {
        Self(NonMaxU16::from_idx(idx), PhantomData)
    }
}

impl<Table> From<TableIdx<Table>> for usize {
    fn from(value: TableIdx<Table>) -> Self {
        value.0.idx()
    }
}
