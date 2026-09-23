//! Which driver lays a node's children out, and the per-mode settings that
//! driver reads — grid tracks, scroll axes, wrap direction.

use crate::common::index16::Index16;
use crate::layout::axis::Axis;
use crate::layout::types::align::Align;
use crate::layout::types::scroll_axes::ScrollAxes;
use crate::scene::visibility::Visibility;

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

#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackedLayoutMeta(u32);

impl PackedLayoutMeta {
    const ALIGN_MASK: u8 = 0b11_1111;
    const VIS_SHIFT: u8 = 6;
    const VIS_MASK: u8 = 0b11 << Self::VIS_SHIFT;
    const PAYLOAD_MASK: u32 = u16::MAX as u32;
    const METADATA_SHIFT: u32 = 16;
    const METADATA_MASK: u32 = (u8::MAX as u32) << Self::METADATA_SHIFT;
    const TAG_SHIFT: u32 = 24;

    #[inline(always)]
    pub(crate) fn new(mode: LayoutMode, align: Align, visibility: Visibility) -> Self {
        let metadata = (align.raw() & Self::ALIGN_MASK)
            | (((visibility as u8) << Self::VIS_SHIFT) & Self::VIS_MASK);
        Self::from(mode).with_metadata(metadata)
    }

    #[inline(always)]
    pub(crate) fn align(self) -> Align {
        Align::from_raw(self.metadata() & Self::ALIGN_MASK)
    }

    /// Matched rather than transmuted: the two-bit field admits a `3`
    /// that is not a `Visibility` discriminant, and the `const _` below
    /// only pins that the widest *valid* variant fits — it says nothing
    /// about the unused pattern. `NodeFlags::clip_mode` unpacks its own
    /// two-bit enum the same way, and both compile to the same load.
    #[inline(always)]
    pub(crate) fn visibility(self) -> Visibility {
        match (self.metadata() & Self::VIS_MASK) >> Self::VIS_SHIFT {
            0 => Visibility::Visible,
            1 => Visibility::Hidden,
            2 => Visibility::Collapsed,
            _ => unreachable!("packed visibility bits are invalid"),
        }
    }

    #[inline(always)]
    fn with_metadata(mut self, metadata: u8) -> Self {
        self.0 = (self.0 & !Self::METADATA_MASK) | (u32::from(metadata) << Self::METADATA_SHIFT);
        self
    }

    #[inline(always)]
    pub(crate) fn metadata(self) -> u8 {
        (self.0 >> Self::METADATA_SHIFT) as u8
    }

    #[inline(always)]
    pub(crate) fn tag(self) -> u8 {
        (self.0 >> Self::TAG_SHIFT) as u8
    }
}

impl From<LayoutMode> for PackedLayoutMeta {
    #[inline(always)]
    fn from(mode: LayoutMode) -> Self {
        let (tag, payload): (u8, u16) = match mode {
            LayoutMode::Leaf => (0, 0),
            LayoutMode::Stack(axis) => (1, axis.bit()),
            LayoutMode::WrapStack(axis) => (2, axis.bit()),
            LayoutMode::ZStack => (3, 0),
            LayoutMode::Canvas => (4, 0),
            LayoutMode::Grid(id) => (5, u16::from(id.0)),
            LayoutMode::Scroll(axes) => (6, axes.to_bits()),
            LayoutMode::Scrollbars(id) => (7, u16::from(id.0)),
        };
        Self(u32::from(payload) | (u32::from(tag) << Self::TAG_SHIFT))
    }
}

impl From<PackedLayoutMeta> for LayoutMode {
    #[inline(always)]
    fn from(packed: PackedLayoutMeta) -> Self {
        let tag = packed.tag();
        let payload = (packed.0 & PackedLayoutMeta::PAYLOAD_MASK) as u16;
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

const _: () = assert!(
    (Visibility::Collapsed as u8) <= (PackedLayoutMeta::VIS_MASK >> PackedLayoutMeta::VIS_SHIFT),
    "Visibility discriminant exceeds 2 bits",
);

// One index type per side table a `LayoutMode` variant carries a
// definition through, so a grid index cannot reach the scrollbar
// table, over the one `Index16` that bounds-checks it and names the
// table it overflowed.

/// Index into `Tree::grid_defs`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefId(Index16);

impl GridDefId {
    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index, "grid_defs"))
    }
}

impl From<GridDefId> for usize {
    fn from(value: GridDefId) -> Self {
        value.0.idx()
    }
}

/// Index into `Tree::scrollbar_defs`. A side table rather than an
/// inline payload because the def is far wider than the 16 bits
/// `LayoutMode` packs into.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollbarsDefId(Index16);

impl ScrollbarsDefId {
    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index, "scrollbar_defs"))
    }
}

impl From<ScrollbarsDefId> for usize {
    fn from(value: ScrollbarsDefId) -> Self {
        value.0.idx()
    }
}
