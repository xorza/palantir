//! A layout mode, alignment and visibility packed into one `u32`.

use crate::layout::types::align::Align;
use crate::layout::types::layout_mode::LayoutMode;
use crate::scene::visibility::Visibility;

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
    pub(crate) const fn align(self) -> Align {
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
    pub(crate) const fn metadata(self) -> u8 {
        (self.0 >> Self::METADATA_SHIFT) as u8
    }

    #[inline(always)]
    pub(crate) const fn tag(self) -> u8 {
        (self.0 >> Self::TAG_SHIFT) as u8
    }

    /// The mode's 16-bit payload: an axis bit, scroll axes, or a
    /// definition id.
    #[inline(always)]
    pub(crate) const fn payload(self) -> u16 {
        (self.0 & Self::PAYLOAD_MASK) as u16
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
            LayoutMode::Grid(id) => (5, id.to_raw()),
            LayoutMode::Scroll(axes) => (6, axes.to_bits()),
            LayoutMode::Scrollbars(id) => (7, id.to_raw()),
        };
        Self(u32::from(payload) | (u32::from(tag) << Self::TAG_SHIFT))
    }
}

const _: () = assert!(
    (Visibility::Collapsed as u8) <= (PackedLayoutMeta::VIS_MASK >> PackedLayoutMeta::VIS_SHIFT),
    "Visibility discriminant exceeds 2 bits",
);
