//! Default TextEdit context-menu policy.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::KeyboardEvent;
use crate::ui::Ui;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::text_edit::action::{ActionAvailability, EditAction};
use crate::widgets::text_edit::editor::Editor;

/// Run the default context menu, returning whether it edited the
/// buffer. Caret motion is *not* reported: `TextEdit::pass` brackets
/// this call and the keyboard pass in one before/after comparison, so
/// there is nothing here for a second one to add.
///
/// `editor` is the caller's session over the host buffer, not one built
/// per action: `TextEdit::show` moves the state row out for the whole
/// pass, so the session can be held mutably alongside `&mut Ui` — which
/// a row borrowed *from* `ui` could never do. One session also means the
/// undo history is reconciled against the buffer once for the menu,
/// exactly as it is once for the key pass.
///
/// `filter` is the field's own — the menu drains the same layer-wide
/// stream `run_input` does, so it owes the same
/// [`KeyFilter::accepts`] gate against double dispatch.
pub(super) fn show(
    ui: &mut Ui,
    snapshot: &ResponseSnapshot,
    editor: &mut Editor<'_>,
    filter: KeyFilter,
) -> bool {
    let clipboard = ui.resources.clipboard.clone();
    let mut clicked_action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        ui.each_keyboard_event(|_, event| {
            let Some(KeyboardEvent::Down(keypress)) = filter.accepts(event) else {
                return;
            };
            if let Some(action) = EditAction::from_keypress(keypress) {
                action.execute(editor, &clipboard);
                if EditAction::MENU.iter().any(|item| item.action == action) {
                    popup.close();
                }
            }
        });

        let has_selection = editor.state.sel_range().is_some();
        let has_text = !editor.text.is_empty();
        for item in EditAction::MENU {
            if item.separator_before {
                MenuItem::separator().show(ui);
            }
            let enabled = match item.availability {
                ActionAvailability::Always => true,
                ActionAvailability::Selection => has_selection,
                ActionAvailability::Text => has_text,
            };
            let mut row = MenuItem::new(item.label).enabled(enabled);
            if let Some(shortcut) = item.action.shortcut() {
                row = row.shortcut_hint(shortcut);
            }
            if row.show(ui, popup).left.clicked() {
                clicked_action = Some(item.action);
            }
        }
    });
    if let Some(action) = clicked_action {
        action.execute(editor, &clipboard);
    }
    editor.edited
}
