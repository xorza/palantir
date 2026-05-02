/// Declares which pointer interactions a widget participates in. Widgets
/// that don't sense any interaction are skipped during hit-testing —
/// clicks/hovers pass through to whatever else is at that point.
///
/// `Click` / `Drag` / `ClickAndDrag` all imply hover — a clickable widget
/// is always hoverable. Modelling the five valid states as enum variants
/// (rather than three independent booleans) makes the invariant
/// unrepresentable instead of a documented convention.
///
/// Convention matches egui: containers default to `None`, leaf-interactive
/// widgets pick `Click`, draggable widgets pick `Drag` or `ClickAndDrag`.
/// `Hover` is for widgets that want hover state (tooltips, cursor changes,
/// row highlights) without capturing clicks meant for things below.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sense {
    #[default]
    None,
    Hover,
    Click,
    Drag,
    ClickAndDrag,
}

impl Sense {
    pub const NONE: Self = Self::None;
    pub const HOVER: Self = Self::Hover;
    pub const CLICK: Self = Self::Click;
    pub const DRAG: Self = Self::Drag;
    pub const CLICK_AND_DRAG: Self = Self::ClickAndDrag;

    pub const fn drag(self) -> bool {
        matches!(self, Self::Drag | Self::ClickAndDrag)
    }

    /// Visible to hit-test for hover/cursor purposes. Includes hover-only widgets.
    pub const fn hover(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Captures press/release. Hover-only widgets return `false`, so clicks
    /// pass through them to whatever clickable widget is beneath.
    pub const fn click(self) -> bool {
        matches!(self, Self::Click | Self::Drag | Self::ClickAndDrag)
    }
}
