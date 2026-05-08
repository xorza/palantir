/// Declares which pointer interactions a widget participates in. Widgets
/// that don't sense any interaction are skipped during hit-testing —
/// clicks/hovers pass through to whatever else is at that point.
///
/// `Click` / `Drag` / `ClickAndDrag` all imply hover — a clickable widget
/// is always hoverable. Modelling the valid states as enum variants
/// (rather than independent booleans) makes the invariant unrepresentable
/// instead of a documented convention.
///
/// Convention matches egui: containers default to `None`, leaf-interactive
/// widgets pick `Click`, draggable widgets pick `Drag` or `ClickAndDrag`.
/// `Hover` is for widgets that want hover state (tooltips, cursor changes,
/// row highlights) without capturing clicks meant for things below.
/// `Scroll` is orthogonal in spirit but fits this enum because v1 scroll
/// containers don't combine click/hover with scroll capture — children
/// handle those.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sense {
    #[default]
    None,
    Hover,
    Click,
    Drag,
    ClickAndDrag,
    /// Captures wheel/touchpad scroll deltas. Hit-tested independently of
    /// hover/click, so a scrollable container under a clickable child still
    /// receives wheel events when the child is hovered.
    Scroll,
}

impl Sense {
    pub const NONE: Self = Self::None;
    pub const HOVER: Self = Self::Hover;
    pub const CLICK: Self = Self::Click;
    pub const DRAG: Self = Self::Drag;
    pub const CLICK_AND_DRAG: Self = Self::ClickAndDrag;
    pub const SCROLL: Self = Self::Scroll;

    pub const fn drag(self) -> bool {
        matches!(self, Self::Drag | Self::ClickAndDrag)
    }

    /// Visible to hit-test for hover/cursor purposes. `Scroll`-only widgets
    /// stay invisible to hover so the cursor / tooltip layer keeps targeting
    /// content underneath.
    pub const fn hover(self) -> bool {
        matches!(
            self,
            Self::Hover | Self::Click | Self::Drag | Self::ClickAndDrag
        )
    }

    /// Captures press/release. Hover-only widgets return `false`, so clicks
    /// pass through them to whatever clickable widget is beneath.
    pub const fn click(self) -> bool {
        matches!(self, Self::Click | Self::Drag | Self::ClickAndDrag)
    }

    /// Captures wheel/touchpad scroll deltas.
    pub const fn scroll(self) -> bool {
        matches!(self, Self::Scroll)
    }
}
