//! Which driver lays out a node's children, and its per-mode settings.

use crate::common::index16::Index16;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::packed_layout_meta::PackedLayoutMeta;
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

impl From<PackedLayoutMeta> for LayoutMode {
    #[inline(always)]
    fn from(packed: PackedLayoutMeta) -> Self {
        let tag = packed.tag();
        let payload = packed.payload();
        match tag {
            0 => Self::Leaf,
            1 => Self::Stack(Axis::from_bit(payload)),
            2 => Self::WrapStack(Axis::from_bit(payload)),
            3 => Self::ZStack,
            4 => Self::Canvas,
            5 => Self::Grid(GridDefId(
                Index16::from_raw(payload).expect("packed grid mode has no definition id"),
            )),
            6 => Self::Scroll(ScrollAxes::from_bits(payload)),
            7 => Self::Scrollbars(ScrollbarsDefId(
                Index16::from_raw(payload).expect("packed scrollbars mode has no definition id"),
            )),
            _ => unreachable!("packed layout mode tag {tag} is invalid"),
        }
    }
}

// One index type per side table, so a grid index can't reach the scrollbar table.

/// Index into `Tree::grid_defs`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefId(Index16);

impl GridDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        self.0.to_raw()
    }

    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index, "grid_defs"))
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
pub(crate) struct ScrollbarsDefId(Index16);

impl ScrollbarsDefId {
    pub(crate) const fn to_raw(self) -> u16 {
        self.0.to_raw()
    }

    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index, "scrollbar_defs"))
    }
}

impl From<ScrollbarsDefId> for usize {
    fn from(value: ScrollbarsDefId) -> Self {
        value.0.idx()
    }
}
