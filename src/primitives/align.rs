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

/// Two-axis alignment, for the convenience setter that fixes both at once.
/// Held as a plain struct of two `u8`-sized enums (2 bytes total). `Element`
/// stores the components individually (`align_x: HAlign`, `align_y: VAlign`)
/// so that single-axis updates don't disturb the other axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Align {
    pub h: HAlign,
    pub v: VAlign,
}

impl Align {
    pub const fn new(h: HAlign, v: VAlign) -> Self {
        Self { h, v }
    }
    /// Single horizontal axis; vertical defaults to `Auto`.
    pub const fn h(h: HAlign) -> Self {
        Self { h, v: VAlign::Auto }
    }
    /// Single vertical axis; horizontal defaults to `Auto`.
    pub const fn v(v: VAlign) -> Self {
        Self { h: HAlign::Auto, v }
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
    pub(crate) fn to_axis(self) -> AxisAlign {
        match self {
            HAlign::Auto => AxisAlign::Auto,
            HAlign::Left => AxisAlign::Start,
            HAlign::Center => AxisAlign::Center,
            HAlign::Right => AxisAlign::End,
            HAlign::Stretch => AxisAlign::Stretch,
        }
    }
    /// `self` if not `Auto`, else `default`.
    pub(crate) fn or(self, default: HAlign) -> HAlign {
        if matches!(self, HAlign::Auto) {
            default
        } else {
            self
        }
    }
}

impl VAlign {
    pub(crate) fn to_axis(self) -> AxisAlign {
        match self {
            VAlign::Auto => AxisAlign::Auto,
            VAlign::Top => AxisAlign::Start,
            VAlign::Center => AxisAlign::Center,
            VAlign::Bottom => AxisAlign::End,
            VAlign::Stretch => AxisAlign::Stretch,
        }
    }
    pub(crate) fn or(self, default: VAlign) -> VAlign {
        if matches!(self, VAlign::Auto) {
            default
        } else {
            self
        }
    }
}
