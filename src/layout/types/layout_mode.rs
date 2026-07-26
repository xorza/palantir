use crate::common::index16::Index16;
use crate::layout::types::align::Align;
use crate::scene::visibility::Visibility;
use glam::BVec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LayoutMode {
    Leaf,
    HStack,
    VStack,
    WrapHStack,
    WrapVStack,
    ZStack,
    Canvas,
    Grid(GridDefId),
    Scroll(ScrollSpec),
    ScrollBars(ScrollBarsDefId),
}

impl LayoutMode {
    /// Whether this driver's `arrange` is a pure function of the slot it
    /// is handed — reading nothing outside its own subtree and that rect.
    ///
    /// [`LayoutEngine::replay_arranged`] rests on exactly this: a measure
    /// hit proves the subtree's authoring is unchanged, so given an
    /// identical slot its rects can be copied forward instead of
    /// re-derived. A driver that reads *outside* its subtree breaks the
    /// implication — its inputs can move while its own hash and slot sit
    /// still — and the damage is silent: stale rects, no panic, nothing
    /// that fails to compile. It surfaces only as a visual bug, which is
    /// how a scrollbar once survived the content that justified it.
    ///
    /// So this is exhaustive on purpose. A new variant must say which
    /// side it is on, and a driver that answers `false` opts out of
    /// replay for its whole subtree.
    ///
    /// [`LayoutEngine::replay_arranged`]: crate::layout::engine::LayoutEngine
    pub(crate) const fn arrange_depends_only_on_slot(self) -> bool {
        match self {
            Self::Leaf
            | Self::HStack
            | Self::VStack
            | Self::WrapHStack
            | Self::WrapVStack
            | Self::ZStack
            | Self::Canvas
            | Self::Grid(_)
            | Self::Scroll(_) => true,
            // Sizes its thumbs from the *sibling* viewport's measured
            // `scroll_content`, so content that stops overflowing leaves
            // this subtree's own hash and slot untouched while the bars
            // it should retire stay exactly where they were.
            Self::ScrollBars(_) => false,
        }
    }
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

    #[inline(always)]
    pub(crate) fn visibility(self) -> Visibility {
        let raw = (self.metadata() & Self::VIS_MASK) >> Self::VIS_SHIFT;
        unsafe { std::mem::transmute::<u8, Visibility>(raw) }
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
            LayoutMode::HStack => (1, 0),
            LayoutMode::VStack => (2, 0),
            LayoutMode::WrapHStack => (3, 0),
            LayoutMode::WrapVStack => (4, 0),
            LayoutMode::ZStack => (5, 0),
            LayoutMode::Canvas => (6, 0),
            LayoutMode::Grid(id) => (7, u16::from(id.0)),
            LayoutMode::Scroll(spec) => (8, spec.0),
            LayoutMode::ScrollBars(id) => (9, u16::from(id.0)),
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
            1 => Self::HStack,
            2 => Self::VStack,
            3 => Self::WrapHStack,
            4 => Self::WrapVStack,
            5 => Self::ZStack,
            6 => Self::Canvas,
            7 => Self::Grid(GridDefId(
                Index16::from_raw(payload).expect("packed grid mode has no definition id"),
            )),
            8 => Self::Scroll(ScrollSpec(payload)),
            9 => Self::ScrollBars(ScrollBarsDefId(
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

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefId(Index16);

impl GridDefId {
    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index))
    }
}

impl From<GridDefId> for usize {
    fn from(value: GridDefId) -> Self {
        value.0.idx()
    }
}

/// Index into `Tree::scrollbar_defs`. Separate table rather than an
/// inline payload because the def carries nine fields and `LayoutMode`
/// packs into 16 bits — same arrangement as [`GridDefId`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollBarsDefId(Index16);

impl ScrollBarsDefId {
    pub(crate) fn from_index(index: usize) -> Self {
        Self(Index16::new(index))
    }
}

impl From<ScrollBarsDefId> for usize {
    fn from(value: ScrollBarsDefId) -> Self {
        value.0.idx()
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollSpec(u16);

impl ScrollSpec {
    const PAN_X: u16 = 0b0001;
    const PAN_Y: u16 = 0b0010;
    const FIT_X: u16 = 0b0100;
    const FIT_Y: u16 = 0b1000;

    pub(crate) const HORIZONTAL: Self = Self(Self::PAN_X);
    pub(crate) const VERTICAL: Self = Self(Self::PAN_Y);
    pub(crate) const BOTH: Self = Self(Self::PAN_X | Self::PAN_Y);

    #[inline]
    pub(crate) fn pan_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::PAN_X != 0, self.0 & Self::PAN_Y != 0)
    }

    #[inline]
    pub(crate) fn fit_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::FIT_X != 0, self.0 & Self::FIT_Y != 0)
    }

    pub(crate) fn with_fit(mut self, fit: BVec2) -> Self {
        let pan = self.pan_mask();
        debug_assert!(
            (!fit.x || pan.x) && (!fit.y || pan.y),
            "Scroll fit axes must be a subset of its pan axes",
        );
        self.0 &= !(Self::FIT_X | Self::FIT_Y);
        self.0 |= u16::from(fit.x) * Self::FIT_X;
        self.0 |= u16::from(fit.y) * Self::FIT_Y;
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ScrollBarsDefId, ScrollSpec};

    /// `ScrollBars` is the sole driver that reads outside its own subtree,
    /// and the only thing standing between that and silently stale rects
    /// is this predicate. Pinning both sides keeps a future `true` from
    /// being added by reflex — the failure mode is invisible at runtime.
    #[test]
    fn only_scrollbars_opts_out_of_arrange_replay() {
        let slot_pure = [
            LayoutMode::Leaf,
            LayoutMode::HStack,
            LayoutMode::VStack,
            LayoutMode::WrapHStack,
            LayoutMode::WrapVStack,
            LayoutMode::ZStack,
            LayoutMode::Canvas,
            LayoutMode::Grid(GridDefId::from_index(0)),
            LayoutMode::Scroll(ScrollSpec::BOTH),
        ];
        for mode in slot_pure {
            assert!(
                mode.arrange_depends_only_on_slot(),
                "{mode:?} arranges from its own subtree, so it may replay",
            );
        }
        assert!(
            !LayoutMode::ScrollBars(ScrollBarsDefId::from_index(0)).arrange_depends_only_on_slot(),
            "ScrollBars reads a sibling's scroll_content and must never replay",
        );
    }
}
