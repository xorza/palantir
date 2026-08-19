//! The two chrome shapes widgets share: resolving a container's
//! background against the theme, and emitting a background-only leaf.

use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizes;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;

/// Resolve a container widget's chrome + clip against the theme
/// fallbacks, mutating `node`'s clip mode in place. Shared by
/// `Panel`/`Grid`/`Popup` (theme slot `panel_background` / `panel_clip`):
/// an explicit `.background(...)` wins, otherwise the theme default
/// fills in; the clip default only applies when the caller did not configure
/// clipping. Returns the chrome to pass to [`Widget::record`].
///
/// **Not every container wants this.** Tooltip, Modal and ContextMenu
/// resolve their own with `Option::unwrap_or`, and `Frame` has no theme
/// slot to fall back to at all — the three differ in what the consumer
/// needs (a borrow, an owned value, nothing), and none of them has a
/// clip default, which is the only thing this adds over `.or()`.
///
/// [`Widget::record`]: crate::widgets::widget::Widget::record
pub(super) fn resolve_container(
    node: &mut Node,
    explicit: Option<Background>,
    theme_bg: Option<&Background>,
    theme_clip: ClipMode,
) -> Option<Background> {
    let chrome = explicit.or_else(|| theme_bg.cloned());
    node.clip.get_or_insert(theme_clip);
    chrome
}

/// Record a chrome-only leaf: a sized child that paints `bg` and holds
/// nothing.
///
/// The shape every rail / fill / knob segment takes — a `Slider`'s three
/// and a `ProgressBar`'s two. Widgets whose leaf carries more than a size
/// and a background build the node themselves: a `Switch` knob adds a
/// `position`, a `Scroll` track/thumb a `Sense` (and no size at all — its
/// driver assigns the rects), a `Splitter` bar a margin and a grid cell,
/// a `ComboBox` arrow a shape body. Threading those through here would
/// cost more parameters than the sharing saves.
pub(super) fn leaf(ui: &mut Ui, id: WidgetId, size: impl Into<Sizes>, bg: Option<&Background>) {
    let leaf = Node::leaf().id(id).size(size);
    ui.widget(leaf).record(ui, bg, |_| {});
}
