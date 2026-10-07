//! One chip's whole draw state, and the two small values it carries.

use crate::icons::icon_set::IconHandle;
use crate::primitives::text::interned_str::InternedStr;

/// One tab, as [`TabStrip`](crate::TabStrip) draws it. Every id the strip derives comes from `key`, not the
/// slot: the strip scans last frame's responses, and a reorder or undo may have moved the chips.
#[derive(Clone, Copy, Debug)]
pub struct TabItem {
    /// Stable identity of the tab this chip stands for, unique within one strip.
    pub key: u64,
    /// The chip's text, ellipsised at [`TabsTheme::max_width`](crate::TabsTheme::max_width).
    pub label: InternedStr,
    /// Shows a close button.
    pub closable: bool,
    /// Can be dragged.
    pub draggable: bool,
    /// Status dot.
    pub badge: TabBadge,
    /// Artwork drawn before the label.
    pub icon: Option<IconHandle>,
}

impl TabItem {
    /// A tab with `key` and `label`.
    pub const fn new(key: u64, label: InternedStr) -> Self {
        Self {
            key,
            label,
            closable: true,
            draggable: true,
            badge: TabBadge::None,
            icon: None,
        }
    }
}

/// Whether a chip carries the small status dot, and whether it is inked. Three states, not a `bool`: a tab kind
/// that can ever show the dot reserves its box ([`Self::Idle`]), so the chip does not resize when it appears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabBadge {
    #[default]
    /// No dot, no space.
    None,
    /// Space reserved, dot not inked.
    Idle,
    /// Dot inked.
    On,
}

impl TabBadge {
    /// Whether the dot's space is reserved.
    pub const fn is_reserved(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether the dot is inked.
    pub const fn is_inked(self) -> bool {
        matches!(self, Self::On)
    }
}

/// A strip's per-frame item buffer, kept on the recording view's state row rather than rebuilt.
#[derive(Debug, Default)]
pub(crate) struct TabItemBuf {
    pub(crate) items: Vec<TabItem>,
}
