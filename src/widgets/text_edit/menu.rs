//! Default TextEdit context-menu policy.

use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::ResponseSnapshot;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::model::Editor;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MenuResult {
    pub(crate) edited: bool,
    pub(crate) caret_moved: bool,
}

pub(crate) fn show(
    ui: &mut Ui,
    id: WidgetId,
    snapshot: &ResponseSnapshot,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
) -> MenuResult {
    let has_selection = ui
        .try_state::<TextEditState>(id)
        .is_some_and(|state| state.edit.sel_range().is_some());
    let has_text = !text.is_empty();
    let clipboard = ui.resources.clipboard.clone();
    let mut action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        if MenuItem::new("Cut")
            .shortcut(EditAction::Cut.shortcut().unwrap())
            .enabled(has_selection)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(EditAction::Cut);
        }
        if MenuItem::new("Copy")
            .shortcut(EditAction::Copy.shortcut().unwrap())
            .enabled(has_selection)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(EditAction::Copy);
        }
        if MenuItem::new("Paste")
            .shortcut(EditAction::Paste.shortcut().unwrap())
            .enabled(!clipboard.get().is_empty())
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(EditAction::Paste);
        }
        MenuItem::separator(ui);
        if MenuItem::new("Select All")
            .shortcut(EditAction::SelectAll.shortcut().unwrap())
            .enabled(has_text)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(EditAction::SelectAll);
        }
        if MenuItem::new("Clear")
            .enabled(has_text)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(EditAction::Clear);
        }
    });
    let Some(action) = action else {
        return MenuResult::default();
    };

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
