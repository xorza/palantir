//! In-process clipboard. Backed by a sentinel row in
//! [`crate::ui::state::StateMap`] — same decoupling pattern as
//! `PopupCtx`, no new `Ui` fields. Single-process scope; OS-level
//! integration (cross-app paste) is a future host concern.
//!
//! Text-editing widgets (`TextEdit`'s default context menu, future
//! shortcut routing) use [`get`] / [`set`] for their cut/copy/paste
//! handlers.

use crate::forest::widget_id::WidgetId;
use crate::ui::Ui;

/// Sentinel `WidgetId` for the [`Clipboard`] row inside `StateMap`.
/// Picked to avoid colliding with any user id (auto ids hash
/// file/line/column, explicit ids hash user keys — neither produces
/// this fixed value).
const CLIPBOARD_ID: WidgetId = WidgetId(0xC11B_0A4D_5EED_u64);

#[derive(Default)]
pub(crate) struct Clipboard {
    pub(crate) text: String,
}

/// Current clipboard contents (empty when nothing has been written
/// in this `Ui` instance).
pub fn get(ui: &Ui) -> &str {
    ui.try_state::<Clipboard>(CLIPBOARD_ID)
        .map(|c| c.text.as_str())
        .unwrap_or("")
}

/// Overwrite the clipboard with `s`. Allocates only when the new
/// contents differ in length from the existing buffer (reuses
/// capacity via `String::replace_range` otherwise).
pub fn set(ui: &mut Ui, s: &str) {
    let buf = &mut ui.state_mut::<Clipboard>(CLIPBOARD_ID).text;
    buf.clear();
    buf.push_str(s);
}
