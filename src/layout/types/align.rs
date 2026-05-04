/// Horizontal alignment of a child inside its parent's inner rect.
///
/// `Auto` defers to the parent's `child_align` (if set) and then to the
/// child's own cross-axis `Sizing` (Fill → stretch, otherwise → start). Any
/// non-`Auto` variant overrides both.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    Auto,
    Left,
    Center,
    Right,
    Stretch,
}

/// Vertical alignment of a child inside its parent's inner rect. See
/// [`HAlign`] for the `Auto` resolution rule.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Auto,
    Top,
    Center,
    Bottom,
    Stretch,
}

/// Two-axis alignment packed into a single byte. Lower 3 bits hold the
/// `HAlign`, next 3 hold the `VAlign`. Stored on `NodeAttrs` (not directly on
/// `ElementCore`) so the layout pass reads it through `flags.align()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Align(u8);

impl Align {
    const VSHIFT: u8 = 3;
    const HMASK: u8 = 0b111;
    const VMASK: u8 = 0b111 << Self::VSHIFT;

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
    /// Single horizontal axis; vertical defaults to `Auto`.
    pub const fn h(h: HAlign) -> Self {
        Self::new(h, VAlign::Auto)
    }
    /// Single vertical axis; horizontal defaults to `Auto`.
    pub const fn v(v: VAlign) -> Self {
        Self::new(HAlign::Auto, v)
    }
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
    pub const TOP_LEFT: Self = Self::new(HAlign::Left, VAlign::Top);
    pub const TOP: Self = Self::new(HAlign::Center, VAlign::Top);
    pub const TOP_RIGHT: Self = Self::new(HAlign::Right, VAlign::Top);
    pub const LEFT: Self = Self::new(HAlign::Left, VAlign::Center);
    pub const CENTER: Self = Self::new(HAlign::Center, VAlign::Center);
    pub const RIGHT: Self = Self::new(HAlign::Right, VAlign::Center);
    pub const BOTTOM_LEFT: Self = Self::new(HAlign::Left, VAlign::Bottom);
    pub const BOTTOM: Self = Self::new(HAlign::Center, VAlign::Bottom);
    pub const BOTTOM_RIGHT: Self = Self::new(HAlign::Right, VAlign::Bottom);
    pub const STRETCH: Self = Self::new(HAlign::Stretch, VAlign::Stretch);
}

/// Internal axis-agnostic alignment used by the layout math. Both `HAlign`
/// and `VAlign` map into this so `place_axis` is single-sourced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AxisAlign {
    Auto,
    Start,
    Center,
    End,
    Stretch,
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
