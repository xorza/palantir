//! One chip's whole draw state, and the two small values it carries.

use crate::icons::icon_set::IconHandle;
use crate::primitives::interned_str::InternedStr;

/// One tab, as [`TabStrip`](crate::TabStrip) draws it.
///
/// `key` is the chip's identity, and every id the strip derives comes
/// from it rather than from the slot the item sits in: a strip scans
/// *last* frame's responses, and a reorder — or an undo — can have moved
/// the chips since, so a slot-keyed id would hand one chip's click to
/// another tab.
#[derive(Clone, Copy, Debug)]
pub struct TabItem {
    /// Stable identity of the tab this chip stands for. Unique within
    /// one strip.
    pub key: u64,
    /// The chip's text. Ellipsised at
    /// [`TabsTheme::max_width`](crate::TabsTheme::max_width).
    pub label: InternedStr,
    /// Whether the chip carries a close button.
    pub closable: bool,
    /// Whether the chip may be dragged out of its strip.
    pub draggable: bool,
    /// The status dot beside the label. See [`TabBadge`].
    pub badge: TabBadge,
    /// Artwork drawn before the label, at
    /// [`TabsTheme::badge_size`](crate::TabsTheme::badge_size)'s
    /// neighbour scale — the label's own line height.
    pub icon: Option<IconHandle>,
}

impl TabItem {
    /// A plain chip: closable, draggable, no badge and no icon.
    pub fn new(key: u64, label: InternedStr) -> Self {
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

/// Whether a chip carries the small status dot, and whether it is inked.
///
/// Three states rather than a `bool`, because reserving the box and
/// inking it are different questions. A tab kind that can *ever* show
/// the dot reserves its box on every frame ([`Self::Idle`]), so the
/// chip is the same width inked or not — a dot that came and went would
/// resize the chip on every save and shuffle every chip to its right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabBadge {
    /// No dot, and no space reserved for one.
    #[default]
    None,
    /// The dot's box, drawn empty.
    Idle,
    /// The dot, inked.
    On,
}

impl TabBadge {
    /// Whether the chip reserves the dot's box.
    pub fn reserved(self) -> bool {
        self != Self::None
    }

    /// Whether the reserved box is inked.
    pub fn inked(self) -> bool {
        self == Self::On
    }
}

/// A strip's per-frame item buffer.
///
/// Kept on the recording view's state row rather than rebuilt as a
/// fresh `Vec`: a view records every frame, and the labels themselves
/// hold no owned text, so one buffer serves every pass.
#[derive(Debug, Default)]
pub(crate) struct TabItemBuf {
    pub(crate) items: Vec<TabItem>,
}
