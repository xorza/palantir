//! Host-backed editing actions shared by keyboard and menu dispatch.

use crate::common::clipboard::Clipboard;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::shortcut::Shortcut;
use crate::widgets::text_edit::editor::Editor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditAction {
    Undo,
    Redo,
    SelectAll,
    Cut,
    Copy,
    Paste,
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActionAvailability {
    Always,
    Selection,
    Text,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MenuAction {
    pub(super) action: EditAction,
    pub(super) label: &'static str,
    pub(super) availability: ActionAvailability,
    pub(super) separator_before: bool,
}

impl EditAction {
    pub(super) const MENU: [MenuAction; 5] = [
        MenuAction {
            action: Self::Cut,
            label: "Cut",
            availability: ActionAvailability::Selection,
            separator_before: false,
        },
        MenuAction {
            action: Self::Copy,
            label: "Copy",
            availability: ActionAvailability::Selection,
            separator_before: false,
        },
        MenuAction {
            action: Self::Paste,
            label: "Paste",
            availability: ActionAvailability::Always,
            separator_before: false,
        },
        MenuAction {
            action: Self::SelectAll,
            label: "Select All",
            availability: ActionAvailability::Text,
            separator_before: true,
        },
        MenuAction {
            action: Self::Clear,
            label: "Clear",
            availability: ActionAvailability::Text,
            separator_before: false,
        },
    ];

    pub(super) const fn shortcut(self) -> Option<Shortcut> {
        match self {
            Self::Undo => Some(Shortcut::ctrl('Z')),
            Self::Redo => Some(Shortcut::ctrl_shift('Z')),
            Self::SelectAll => Some(Shortcut::ctrl('A')),
            Self::Cut => Some(Shortcut::ctrl('X')),
            Self::Copy => Some(Shortcut::ctrl('C')),
            Self::Paste => Some(Shortcut::ctrl('V')),
            Self::Clear => None,
        }
    }

    /// Every variant, so [`Self::from_keypress`] scans against the one
    /// list. The bound-chord set is [`Self::shortcut`]'s business alone
    /// — an unbound action returns `None` there and simply never
    /// matches, which is what keeps a second hand-curated subset (and
    /// its drift) out of this file.
    const ALL: [Self; 7] = [
        Self::Undo,
        Self::Redo,
        Self::SelectAll,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Clear,
    ];

    pub(super) fn from_keypress(keypress: KeyPress) -> Option<Self> {
        Self::ALL.into_iter().find(|action| {
            action
                .shortcut()
                .is_some_and(|shortcut| shortcut.matches(keypress))
        })
    }

    pub(super) fn execute(self, editor: &mut Editor<'_>, clipboard: &Clipboard) {
        match self {
            Self::Undo => editor.undo(),
            Self::Redo => editor.redo(),
            Self::SelectAll => editor.select_all(),
            Self::Cut => {
                if let Some(selected) = editor.selected_text()
                    && clipboard.set(selected).is_ok()
                {
                    editor.cut_selection();
                }
            }
            Self::Copy => {
                if let Some(selected) = editor.selected_text() {
                    let _ = clipboard.set(selected);
                }
            }
            Self::Paste => editor.paste(&clipboard.get()),
            Self::Clear => editor.clear(),
        }
    }
}
