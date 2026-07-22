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
