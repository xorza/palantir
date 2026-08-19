//! Default TextEdit context-menu policy.

use crate::input::keyboard::KeyboardEvent;
use crate::ui::Ui;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::response::ResponseSnapshot;
use crate::widgets::text_edit::action::{ActionAvailability, EditAction};
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::Editor;

/// Run the default context menu, returning whether it edited the
/// buffer. Caret motion is *not* reported: `TextEdit::pass` brackets
/// this call and the keyboard pass in one before/after comparison, so
/// there is nothing here for a second one to add.
///
/// `edit` is the caller's already-owned state row, not a fresh lookup:
/// `TextEdit::show` moves the row out for the whole pass, so the body
/// closure below can hold it mutably alongside `&mut Ui` — which a row
/// borrowed *from* `ui` could never do.
pub(super) fn show(
    ui: &mut Ui,
    snapshot: &ResponseSnapshot,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
    edit: &mut EditState,
) -> bool {
    let mut edited = false;
    let mut clicked_action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        let keyboard_event_count = ui.keyboard_events().len();
        for index in 0..keyboard_event_count {
            let event = ui.keyboard_events()[index];
            let KeyboardEvent::Down(keypress) = event else {
                continue;
            };
            if let Some(action) = EditAction::from_keypress(keypress) {
                edited |= execute_action(ui, text, multiline, max_chars, action, edit);
                if EditAction::MENU.iter().any(|item| item.action == action) {
                    popup.close();
                }
            }
        }

        let has_selection = edit.sel_range().is_some();
        let has_text = !text.is_empty();
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
        edited |= execute_action(ui, text, multiline, max_chars, action, edit);
    }
    edited
}

fn execute_action(
    ui: &mut Ui,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
    action: EditAction,
    edit: &mut EditState,
) -> bool {
    let clipboard = ui.resources.clipboard.clone();
    let mut editor = Editor::new(text, edit, multiline, max_chars);
    action.execute(&mut editor, &clipboard);
    editor.edited
}
