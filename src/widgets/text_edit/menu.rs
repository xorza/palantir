//! Default TextEdit context-menu policy.

use crate::input::keyboard::KeyboardEvent;
use crate::ui::Ui;
use crate::widgets::ResponseSnapshot;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::text_edit::action::{ActionAvailability, EditAction};
use crate::widgets::text_edit::model::{EditState, Editor};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MenuResult {
    pub(super) edited: bool,
    pub(super) caret_moved: bool,
}

impl MenuResult {
    fn include(&mut self, other: Self) {
        self.edited |= other.edited;
        self.caret_moved |= other.caret_moved;
    }
}

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
) -> MenuResult {
    let mut result = MenuResult::default();
    let mut clicked_action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        let keyboard_event_count = ui.keyboard_events().len();
        for index in 0..keyboard_event_count {
            let event = ui.keyboard_events()[index];
            let KeyboardEvent::Down(keypress) = event else {
                continue;
            };
            if let Some(action) = EditAction::from_keypress(keypress) {
                result.include(execute_action(ui, text, multiline, max_chars, action, edit));
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
        result.include(execute_action(ui, text, multiline, max_chars, action, edit));
    }
    result
}

fn execute_action(
    ui: &mut Ui,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
    action: EditAction,
    edit: &mut EditState,
) -> MenuResult {
    let clipboard = ui.resources.clipboard.clone();
    let caret_before = edit.caret;
    let selection_before = edit.selection;
    let mut editor = Editor::new(text, edit, multiline, max_chars);
    action.execute(&mut editor, &clipboard);
    MenuResult {
        edited: editor.edited,
        caret_moved: caret_before != editor.state.caret
            || selection_before != editor.state.selection,
    }
}
