//! Default TextEdit context-menu policy.

use crate::input::keyboard::KeyboardEvent;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::ResponseSnapshot;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::{ActionAvailability, EditAction};
use crate::widgets::text_edit::model::Editor;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MenuResult {
    pub(crate) edited: bool,
    pub(crate) caret_moved: bool,
}

impl MenuResult {
    fn include(&mut self, other: Self) {
        self.edited |= other.edited;
        self.caret_moved |= other.caret_moved;
    }
}

pub(crate) fn show(
    ui: &mut Ui,
    id: WidgetId,
    snapshot: &ResponseSnapshot,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
) -> MenuResult {
    let mut result = MenuResult::default();
    let mut clicked_action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        let keyboard_event_count = popup.keyboard_events(ui).len();
        for index in 0..keyboard_event_count {
            let event = popup.keyboard_events(ui)[index];
            let KeyboardEvent::Down(keypress) = event else {
                continue;
            };
            if let Some(action) = EditAction::from_keypress(keypress) {
                result.include(execute_action(ui, id, text, multiline, max_chars, action));
                if EditAction::MENU.iter().any(|item| item.action == action) {
                    popup.close();
                }
            }
        }

        let has_selection = ui
            .try_state::<TextEditState>(id)
            .is_some_and(|state| state.edit.sel_range().is_some());
        let has_text = !text.is_empty();
        for item in EditAction::MENU {
            if item.separator_before {
                MenuItem::separator(ui);
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
        result.include(execute_action(ui, id, text, multiline, max_chars, action));
    }
    result
}

fn execute_action(
    ui: &mut Ui,
    id: WidgetId,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
    action: EditAction,
) -> MenuResult {
    let clipboard = ui.resources.clipboard.clone();
    let edit = &mut ui.state_mut::<TextEditState>(id).edit;
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
