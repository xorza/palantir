//! A layout mode, alignment and visibility packed into one `u32`.

use crate::primitives::layout::align::Align;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::visibility::Visibility;

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
    const TAG_LEAF: u8 = 0;
    const TAG_STACK: u8 = 1;
    const TAG_WRAP_STACK: u8 = 2;
    const TAG_ZSTACK: u8 = 3;
    const TAG_CANVAS: u8 = 4;
    const TAG_GRID: u8 = 5;
    const TAG_SCROLL: u8 = 6;
    const TAG_SCROLLBARS: u8 = 7;

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

    /// Matched, not transmuted: the two-bit field admits a `3` that is no `Visibility` discriminant, and the `const _` below only pins that the widest valid variant fits. `NodeFlags::clip_mode` unpacks its own two-bit enum the same way.
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
    const fn payload(self) -> u16 {
        (self.0 & Self::PAYLOAD_MASK) as u16
    }
}

impl From<LayoutMode> for PackedLayoutMeta {
    #[inline(always)]
    fn from(mode: LayoutMode) -> Self {
        let (tag, payload): (u8, u16) = match mode {
            LayoutMode::Leaf => (Self::TAG_LEAF, 0),
            LayoutMode::Stack(axis) => (Self::TAG_STACK, axis.bit()),
            LayoutMode::WrapStack(axis) => (Self::TAG_WRAP_STACK, axis.bit()),
            LayoutMode::ZStack => (Self::TAG_ZSTACK, 0),
            LayoutMode::Canvas => (Self::TAG_CANVAS, 0),
            LayoutMode::Grid(id) => (Self::TAG_GRID, id.to_raw()),
            LayoutMode::Scroll(axes) => (Self::TAG_SCROLL, axes.to_bits()),
            LayoutMode::Scrollbars(id) => (Self::TAG_SCROLLBARS, id.to_raw()),
        };
        Self(u32::from(payload) | (u32::from(tag) << Self::TAG_SHIFT))
    }
}

impl From<PackedLayoutMeta> for LayoutMode {
    #[inline(always)]
    fn from(packed: PackedLayoutMeta) -> Self {
        let payload = packed.payload();
        match packed.tag() {
            PackedLayoutMeta::TAG_LEAF => Self::Leaf,
            PackedLayoutMeta::TAG_STACK => Self::Stack(Axis::from_bit(payload)),
            PackedLayoutMeta::TAG_WRAP_STACK => Self::WrapStack(Axis::from_bit(payload)),
            PackedLayoutMeta::TAG_ZSTACK => Self::ZStack,
            PackedLayoutMeta::TAG_CANVAS => Self::Canvas,
            PackedLayoutMeta::TAG_GRID => Self::Grid(
                GridDefId::from_raw(payload).expect("packed grid mode has no definition id"),
            ),
            PackedLayoutMeta::TAG_SCROLL => Self::Scroll(ScrollAxes::from_bits(payload)),
            PackedLayoutMeta::TAG_SCROLLBARS => Self::Scrollbars(
                ScrollbarsDefId::from_raw(payload)
                    .expect("packed scrollbars mode has no definition id"),
            ),
            tag => unreachable!("packed layout mode tag {tag} is invalid"),
        }
    }
}

const _: () = assert!(
    (Visibility::Collapsed as u8) <= (PackedLayoutMeta::VIS_MASK >> PackedLayoutMeta::VIS_SHIFT),
    "Visibility discriminant exceeds 2 bits",
);

#[cfg(test)]
mod tests {
    use crate::primitives::layout::align::{Align, HAlign, VAlign};
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
    use crate::primitives::layout::packed_layout_meta::PackedLayoutMeta;
    use crate::primitives::layout::scroll_axes::ScrollAxes;
    use crate::primitives::layout::sizing::{SizeSpec, Sizing};
    use crate::primitives::layout::visibility::Visibility;

    /// Every mode, its payload included, packs and unpacks beside each alignment and visibility
    /// it shares the word with; a scroll keeps its fit flags.
    #[test]
    fn every_mode_round_trips_beside_its_align_and_visibility() {
        let fitted = ScrollAxes::BOTH.fit_content(SizeSpec::new(Sizing::HUG, Sizing::FILL));
        let modes = [
            LayoutMode::Leaf,
            LayoutMode::Stack(Axis::X),
            LayoutMode::Stack(Axis::Y),
            LayoutMode::WrapStack(Axis::X),
            LayoutMode::WrapStack(Axis::Y),
            LayoutMode::ZStack,
            LayoutMode::Canvas,
            LayoutMode::Grid(GridDefId::from_idx(42)),
            LayoutMode::Scroll(ScrollAxes::VERTICAL),
            LayoutMode::Scroll(ScrollAxes::HORIZONTAL),
            LayoutMode::Scroll(fitted),
            LayoutMode::Scrollbars(ScrollbarsDefId::from_idx(7)),
        ];
        let looks = [
            (Align::new(HAlign::Auto, VAlign::Auto), Visibility::Visible),
            (Align::new(HAlign::Left, VAlign::Center), Visibility::Hidden),
            (
                Align::new(HAlign::Right, VAlign::Bottom),
                Visibility::Collapsed,
            ),
        ];
        for mode in modes {
            for (align, visibility) in looks {
                let packed = PackedLayoutMeta::new(mode, align, visibility);
                assert_eq!(
                    LayoutMode::from(packed),
                    mode,
                    "{mode:?} {align:?} {visibility:?}"
                );
                assert_eq!(packed.align(), align, "{mode:?}");
                assert_eq!(packed.visibility(), visibility, "{mode:?}");
            }
        }
    }
}
