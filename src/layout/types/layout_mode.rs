//! Which driver lays a node's children out, and the per-mode settings that
//! driver reads — grid tracks, scroll axes, wrap direction.

use crate::common::index16::Index16;
use crate::layout::axis::Axis;
use crate::layout::types::align::Align;
use crate::scene::visibility::Visibility;
use glam::BVec2;

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
    Scroll(ScrollSpec),
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
            LayoutMode::Scroll(spec) => (6, spec.0),
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
            6 => Self::Scroll(ScrollSpec(payload)),
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

/// Index into `Tree::scrollbar_defs`. Separate table rather than an
/// inline payload because the def carries nine fields and `LayoutMode`
/// packs into 16 bits — same arrangement as [`GridDefId`].
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

/// Which driver lays a scroll viewport's children out. Derived from the
/// spec's pan mask by [`ScrollSpec::child_layout`], so measure, arrange,
/// and the intrinsic query cannot pick different ones. Spelling the
/// three-way choice out at each of the three sites is what lets the
/// intrinsic copy drift from the other two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollChildLayout {
    /// Both axes pan, so neither constrains the other and children stack
    /// at the origin.
    Layered,
    /// One axis pans; children flow along it.
    Flow(Axis),
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

    /// The pan flags as stored, for a hash that must not see the fit
    /// bits. The one place these two flags' bit positions are written
    /// down, so a consumer folding them cannot invent a second layout.
    #[inline]
    pub(crate) fn pan_bits(self) -> u16 {
        self.0 & (Self::PAN_X | Self::PAN_Y)
    }

    #[inline]
    pub(crate) fn pan_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::PAN_X != 0, self.0 & Self::PAN_Y != 0)
    }

    #[inline]
    pub(crate) fn fit_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::FIT_X != 0, self.0 & Self::FIT_Y != 0)
    }

    /// Which axes fold their measured content into the viewport's own
    /// reported size, as a lane mask — see [`Self::contributes`].
    #[inline]
    pub(crate) fn contributes_mask(self) -> BVec2 {
        BVec2::new(self.contributes(Axis::X), self.contributes(Axis::Y))
    }

    /// The driver that lays this viewport's children out.
    #[inline]
    pub(crate) fn child_layout(self) -> ScrollChildLayout {
        let pan = self.pan_mask();
        match (pan.x, pan.y) {
            (true, true) => ScrollChildLayout::Layered,
            (false, true) => ScrollChildLayout::Flow(Axis::Y),
            // An x-only pan flows along X, and so does the degenerate
            // no-pan spec no constructor can currently produce.
            _ => ScrollChildLayout::Flow(Axis::X),
        }
    }

    #[inline]
    pub(crate) fn pans(self, axis: Axis) -> bool {
        axis.main_b(self.pan_mask())
    }

    /// Whether `axis` folds its measured content extent into the size the
    /// viewport reports for itself.
    ///
    /// A panned axis normally reports nothing — the viewport takes that
    /// axis from its own `Sizing`, not from what it scrolls over — but
    /// `fit` opts back in, which is how a `Hug` scroll sizes to content.
    ///
    /// This is the **max**-content rule, and only that. A panned axis'
    /// *min*-content stays zero whatever `fit` says: `resolve_sizing`
    /// floors a node's own size with its min-content intrinsic, and
    /// shrinking below the content is precisely what scrolling is for —
    /// floor a `Hug` scroll at its content and it pins itself open,
    /// ignoring both `max_size` and the space its parent actually has.
    #[inline]
    pub(crate) fn contributes(self, axis: Axis) -> bool {
        !self.pans(axis) || axis.main_b(self.fit_mask())
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
