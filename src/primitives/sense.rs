/// Declares which pointer interactions a widget participates in.
/// Widgets that don't sense any interaction are skipped during hit-testing —
/// clicks/hovers pass through to whatever else is at that point.
///
/// Convention matches egui: containers default to `NONE`, leaf-interactive
/// widgets pick `CLICK`, draggable widgets pick `DRAG` or `CLICK_AND_DRAG`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sense {
    pub click: bool,
    pub drag: bool,
}

impl Sense {
    pub const NONE: Self = Self {
        click: false,
        drag: false,
    };
    pub const CLICK: Self = Self {
        click: true,
        drag: false,
    };
    pub const DRAG: Self = Self {
        click: false,
        drag: true,
    };
    pub const CLICK_AND_DRAG: Self = Self {
        click: true,
        drag: true,
    };

    /// True if the widget participates in any interaction (and thus is hoverable
    /// and a hit-test candidate).
    pub fn is_interactive(self) -> bool {
        self.click || self.drag
    }
}
