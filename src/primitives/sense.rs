/// Declares which pointer interactions a widget participates in.
/// Widgets that don't sense any interaction are skipped during hit-testing —
/// clicks/hovers pass through to whatever else is at that point.
///
/// Convention matches egui: containers default to `NONE`, leaf-interactive
/// widgets pick `CLICK`, draggable widgets pick `DRAG` or `CLICK_AND_DRAG`.
/// `HOVER` is for widgets that want hover state (tooltips, cursor changes,
/// row highlights) without capturing clicks meant for things below.
///
/// `click` and `drag` imply `hover` — a clickable widget is always hoverable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sense {
    pub click: bool,
    pub drag: bool,
    pub hover: bool,
}

impl Sense {
    pub const NONE: Self = Self {
        click: false,
        drag: false,
        hover: false,
    };
    pub const HOVER: Self = Self {
        click: false,
        drag: false,
        hover: true,
    };
    pub const CLICK: Self = Self {
        click: true,
        drag: false,
        hover: true,
    };
    pub const DRAG: Self = Self {
        click: false,
        drag: true,
        hover: true,
    };
    pub const CLICK_AND_DRAG: Self = Self {
        click: true,
        drag: true,
        hover: true,
    };

    /// Visible to hit-test for hover/cursor purposes. Includes hover-only widgets.
    pub fn is_hoverable(self) -> bool {
        self.click || self.drag || self.hover
    }

    /// Captures press/release. Hover-only widgets return `false`, so clicks
    /// pass through them to whatever clickable widget is beneath.
    pub fn is_clickable(self) -> bool {
        self.click || self.drag
    }
}
