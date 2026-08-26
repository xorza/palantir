use crate::primitives::rect::Rect;
use crate::primitives::size::Size;

/// Horizontal alignment of a child inside its parent's inner rect.
///
/// `Auto` defers to the parent's `child_align` (if set) and then to the
/// child's own cross-axis `Sizing` (Fill → stretch, otherwise → start). Any
/// non-`Auto` variant overrides both.
///
/// `#[repr(u8)]` with explicit discriminants pins the on-disk tag:
/// `TextShapeKey::halign_q` stores this as a byte and decodes it back
/// positionally, so a reordered variant would resolve every cached shaped
/// buffer to the wrong alignment. The same reason
/// [`FontFamily`](crate::FontFamily) pins its own.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HAlign {
    /// Inherit: parent's `child_align` first, then the child's own
    /// cross-axis [`Sizing`](crate::Sizing) — `Fill` stretches, anything
    /// else starts.
    #[default]
    Auto = 0,
    /// Pin to the inner rect's left edge.
    Left = 1,
    /// Center within the inner rect.
    Center = 2,
    /// Pin to the inner rect's right edge.
    Right = 3,
    /// Fill the inner rect's width, overriding the child's measured width.
    Stretch = 4,
}

/// Vertical alignment of a child inside its parent's inner rect. See
/// [`HAlign`] for the `Auto` resolution rule.
///
/// Discriminants are pinned for the reason [`HAlign`]'s are: [`Align`]
/// packs this into three bits and [`Align::valign`] unpacks it
/// positionally.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    /// Inherit, by the same rule as [`HAlign::Auto`].
    #[default]
    Auto = 0,
    /// Pin to the inner rect's top edge.
    Top = 1,
    /// Center within the inner rect.
    Center = 2,
    /// Pin to the inner rect's bottom edge.
    Bottom = 3,
    /// Fill the inner rect's height, overriding the child's measured height.
    Stretch = 4,
}

/// Both alignment enums' discriminants, which [`Align::new`] packs into
/// bits and [`Align::halign`] / [`Align::valign`] unpack positionally.
///
/// A reordered variant would decode every packed `Align` to the wrong one
/// — silently, since the bits stay inside the mask and the `unreachable!`
/// arms never fire. `TextShapeKey` decodes `HAlign` the same way off a
/// stored tag byte, so this guards that too.
const _: () = {
    assert!(
        HAlign::Auto as u8 == 0
            && HAlign::Left as u8 == 1
            && HAlign::Center as u8 == 2
            && HAlign::Right as u8 == 3
            && HAlign::Stretch as u8 == 4
    );
    assert!(
        VAlign::Auto as u8 == 0
            && VAlign::Top as u8 == 1
            && VAlign::Center as u8 == 2
            && VAlign::Bottom as u8 == 3
            && VAlign::Stretch as u8 == 4
    );
};

/// Two-axis alignment packed into a single byte. Lower 3 bits hold the
/// `HAlign`, next 3 hold the `VAlign`. Recorded inside `PackedLayoutMeta`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Align(u8);

impl Align {
    const VSHIFT: u8 = 3;
    const HMASK: u8 = 0b111;
    const VMASK: u8 = 0b111 << Self::VSHIFT;

    /// Pack both axes. [`Self::h`] / [`Self::v`] set one axis and leave
    /// the other `Auto`; the named constants below cover the nine corners
    /// plus [`Self::STRETCH`].
    pub const fn new(h: HAlign, v: VAlign) -> Self {
        Self((h as u8) | ((v as u8) << Self::VSHIFT))
    }
    /// Raw packed byte (lower 3 bits HAlign, next 3 VAlign). Used by
    /// the per-node hash to fold `Align` into a single byte without
    /// going through `Hash` derives.
    #[inline]
    pub(crate) const fn raw(self) -> u8 {
        self.0
    }
    /// Rebuild from the raw packed byte (lower 3 bits HAlign, next 3
    /// VAlign). Used when `Align` is stored inside a larger packed
    /// `u16`/`u32` field (e.g.
    /// [`PackedLayoutMeta`](crate::layout::types::layout_mode::PackedLayoutMeta))
    /// — the caller masks the
    /// slot and hands the 6 valid bits straight back here, no
    /// `HAlign`/`VAlign` round-trip required.
    #[inline]
    pub(crate) const fn from_raw(b: u8) -> Self {
        Self(b)
    }
    /// Single horizontal axis; vertical defaults to `Auto`.
    pub const fn h(h: HAlign) -> Self {
        Self::new(h, VAlign::Auto)
    }
    /// Single vertical axis; horizontal defaults to `Auto`.
    pub const fn v(v: VAlign) -> Self {
        Self::new(HAlign::Auto, v)
    }
    /// Unpack the horizontal axis.
    pub const fn halign(self) -> HAlign {
        match self.0 & Self::HMASK {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            4 => HAlign::Stretch,
            _ => unreachable!(),
        }
    }
    /// Unpack the vertical axis.
    pub const fn valign(self) -> VAlign {
        match (self.0 & Self::VMASK) >> Self::VSHIFT {
            0 => VAlign::Auto,
            1 => VAlign::Top,
            2 => VAlign::Center,
            3 => VAlign::Bottom,
            4 => VAlign::Stretch,
            _ => unreachable!(),
        }
    }
    /// Top-left corner.
    pub const TOP_LEFT: Self = Self::new(HAlign::Left, VAlign::Top);
    /// Top edge, horizontally centered.
    pub const TOP: Self = Self::new(HAlign::Center, VAlign::Top);
    /// Top-right corner.
    pub const TOP_RIGHT: Self = Self::new(HAlign::Right, VAlign::Top);
    /// Left edge, vertically centered.
    pub const LEFT: Self = Self::new(HAlign::Left, VAlign::Center);
    /// Centered on both axes.
    pub const CENTER: Self = Self::new(HAlign::Center, VAlign::Center);
    /// Right edge, vertically centered.
    pub const RIGHT: Self = Self::new(HAlign::Right, VAlign::Center);
    /// Bottom-left corner.
    pub const BOTTOM_LEFT: Self = Self::new(HAlign::Left, VAlign::Bottom);
    /// Bottom edge, horizontally centered.
    pub const BOTTOM: Self = Self::new(HAlign::Center, VAlign::Bottom);
    /// Bottom-right corner.
    pub const BOTTOM_RIGHT: Self = Self::new(HAlign::Right, VAlign::Bottom);
    /// Fill the inner rect on both axes.
    pub const STRETCH: Self = Self::new(HAlign::Stretch, VAlign::Stretch);
    /// Position a `content` box of fixed size inside `outer` per `self`:
    /// `min` shifted by the alignment offset, `size` = `content` unchanged.
    ///
    /// For content that cannot stretch, so `Auto`/`Stretch` collapse to start —
    /// matching arrange-axis placement for non-stretchable children — and
    /// overflow on an axis clamps that axis's offset to zero, pinning oversized
    /// content to the leading edge instead of letting it drift negative.
    ///
    /// Coordinate-system agnostic: callers pass owner-local, screen-space, or
    /// zero-origin rects and read `.min` back as the bare offset. Text is the
    /// motivating consumer and needs one definition for all of them — glyphs,
    /// caret, and selection wash must shift by the same offset or the caret
    /// drifts off its glyph.
    pub(crate) fn place_in(self, outer: Rect, content: Size) -> Rect {
        let dx = match self.halign() {
            HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
            HAlign::Center => (outer.size.w - content.w) * 0.5,
            HAlign::Right => outer.size.w - content.w,
        };
        let dy = match self.valign() {
            VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
            VAlign::Center => (outer.size.h - content.h) * 0.5,
            VAlign::Bottom => outer.size.h - content.h,
        };
        Rect::new(
            outer.min.x + dx.max(0.0),
            outer.min.y + dy.max(0.0),
            content.w,
            content.h,
        )
    }
}

/// Internal axis-agnostic alignment used by the layout math. Both `HAlign`
/// and `VAlign` map into this so arrange-axis resolution is single-sourced.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisAlign {
    Auto,
    Start,
    Center,
    End,
    Stretch,
}

impl AxisAlign {
    /// `Auto → Stretch`, otherwise unchanged. Grid's per-cell default
    /// is stretch (WPF semantics): a non-Fixed child with no explicit
    /// alignment fills its cell on each axis. Other drivers leave `Auto`
    /// alone (arrange-axis resolution consults the child's `Sizing`).
    #[inline]
    pub(crate) const fn or_stretch_if_auto(self) -> AxisAlign {
        match self {
            AxisAlign::Auto => AxisAlign::Stretch,
            other => other,
        }
    }
}

impl HAlign {
    pub(crate) const fn to_axis(self) -> AxisAlign {
        match self {
            HAlign::Auto => AxisAlign::Auto,
            HAlign::Left => AxisAlign::Start,
            HAlign::Center => AxisAlign::Center,
            HAlign::Right => AxisAlign::End,
            HAlign::Stretch => AxisAlign::Stretch,
        }
    }
    /// `self` if not `Auto`, else `default`.
    pub(crate) const fn or(self, default: HAlign) -> HAlign {
        if matches!(self, HAlign::Auto) {
            default
        } else {
            self
        }
    }
}

impl VAlign {
    pub(crate) const fn to_axis(self) -> AxisAlign {
        match self {
            VAlign::Auto => AxisAlign::Auto,
            VAlign::Top => AxisAlign::Start,
            VAlign::Center => AxisAlign::Center,
            VAlign::Bottom => AxisAlign::End,
            VAlign::Stretch => AxisAlign::Stretch,
        }
    }
    pub(crate) const fn or(self, default: VAlign) -> VAlign {
        if matches!(self, VAlign::Auto) {
            default
        } else {
            self
        }
    }
}
