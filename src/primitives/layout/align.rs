//! Where a child sits inside the slot its parent gives it, per axis, and the
//! axis-agnostic form the layout math resolves both into.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;

/// Horizontal alignment of a child inside its parent's inner rect. `Auto` defers to
/// the parent's `child_align`, then to the child's cross-axis `Sizing` (Fill
/// stretches, otherwise start); any other variant overrides both. The explicit
/// discriminants are pinned because [`Align`] packs this into three bits and
/// [`Align::halign`] unpacks it positionally.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HAlign {
    /// Inherit: parent's `child_align`, then the child's cross-axis
    /// [`Sizing`](crate::Sizing).
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

/// Vertical alignment of a child inside its parent's inner rect; see [`HAlign`] for
/// `Auto` and the pinned discriminants.
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

/// Both enums' discriminants, which [`Align::new`] packs and [`Align::halign`] /
/// [`Align::valign`] unpack positionally. A reordered variant would decode silently
/// wrong, since the bits stay inside the mask and the `unreachable!` arms never
/// fire.
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

/// Two-axis alignment packed into a byte: `HAlign` in the lower 3 bits, `VAlign` in
/// the next 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Align(u8);

impl Align {
    const VSHIFT: u8 = 3;
    const HMASK: u8 = 0b111;
    const VMASK: u8 = 0b111 << Self::VSHIFT;

    /// Pack both axes. [`Self::h`] / [`Self::v`] set one axis and leave the other
    /// `Auto`.
    pub const fn new(h: HAlign, v: VAlign) -> Self {
        Self((h as u8) | ((v as u8) << Self::VSHIFT))
    }
    #[inline]
    pub(crate) const fn raw(self) -> u8 {
        self.0
    }
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
    /// Position a `content` box of fixed size inside `outer` per `self`: `min`
    /// shifted by the alignment offset, `size` unchanged. `Auto`/`Stretch` collapse
    /// to start and overflow clamps the offset to zero, pinning oversized content
    /// to the leading edge. Coordinate-system agnostic: callers read `.min` back as
    /// the bare offset, so glyphs, caret and selection wash shift together.
    pub(crate) fn place_in(self, outer: Rect, content: Size) -> Rect {
        let dx = self.halign().to_axis().offset_in(outer.size.w, content.w);
        let dy = self.valign().to_axis().offset_in(outer.size.h, content.h);
        Rect::new(
            outer.min.x + dx.max(0.0),
            outer.min.y + dy.max(0.0),
            content.w,
            content.h,
        )
    }
}

/// Axis-agnostic alignment used by the layout math; `HAlign` and `VAlign` both map
/// into it.
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
    /// The offset placing `extent` inside a `slot`, before any overflow policy:
    /// [`Align::place_in`] floors it at zero, while an overlay clamps the position
    /// into its surface.
    #[inline]
    pub(crate) const fn offset_in(self, slot: f32, extent: f32) -> f32 {
        match self {
            AxisAlign::Center => (slot - extent) * 0.5,
            AxisAlign::End => slot - extent,
            AxisAlign::Auto | AxisAlign::Start | AxisAlign::Stretch => 0.0,
        }
    }

    /// `Auto → Stretch`, otherwise unchanged. Grid's per-cell default is stretch
    /// (WPF); other drivers leave `Auto` to the child's `Sizing`.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A content box in a 200×40 leaf at (10, 20): 80×16 of content leaves 120 × 24
    /// of slack, so centring adds 60 and 12, the default keeps the top-left,
    /// right-bottom adds all of it, and oversize content clamps to the top-left.
    #[test]
    fn place_in_splits_the_slack_by_alignment() {
        let leaf = Rect::new(10.0, 20.0, 200.0, 40.0);
        let measured = Size::new(80.0, 16.0);
        for (align, min) in [
            (Align::CENTER, (70.0, 32.0)),
            (Align::default(), (10.0, 20.0)),
            (Align::new(HAlign::Right, VAlign::Bottom), (130.0, 44.0)),
        ] {
            assert_eq!(
                align.place_in(leaf, measured),
                Rect::new(min.0, min.1, 80.0, 16.0),
                "{align:?}"
            );
        }

        let small = Rect::new(0.0, 0.0, 50.0, 10.0);
        assert_eq!(
            Align::CENTER.place_in(small, measured),
            Rect::new(0.0, 0.0, 80.0, 16.0),
            "oversize content clamps to the top-left"
        );
    }
}
