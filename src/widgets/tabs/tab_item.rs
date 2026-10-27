//! One tab as a strip reads it, and the buffer a view keeps them in.

use crate::primitives::text::interned_str::InternedStr;

/// One tab, as [`TabStrip`](crate::TabStrip) reads it; what the chip shows is the content [`TabStrip::show`](crate::TabStrip::show) records. Every id the strip derives comes from `key`, not the
/// slot: the strip scans last frame's responses, and a reorder or undo may have moved the chips.
#[derive(Clone, Copy, Debug)]
pub struct TabItem {
    /// Stable identity of the tab this chip stands for, unique within one strip.
    pub key: u64,
    /// The tab's name: what [`TabChip::show_label`](crate::TabChip::show_label) shows, and the overflow menu's row.
    pub label: InternedStr,
    /// Shows a close button.
    pub closable: bool,
    /// Can be dragged.
    pub draggable: bool,
}

impl TabItem {
    /// A tab with `key` and `label`.
    pub const fn new(key: u64, label: InternedStr) -> Self {
        Self {
            key,
            label,
            closable: true,
            draggable: true,
        }
    }
}

/// A strip's per-frame item buffer, kept on the recording view's state row rather than rebuilt.
#[derive(Debug, Default)]
pub(crate) struct TabItemBuf {
    pub(crate) items: Vec<TabItem>,
}
