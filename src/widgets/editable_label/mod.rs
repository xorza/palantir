//! [`EditableLabel`]: text that opens its own editor on a double click, Enter or F2.

use std::rc::Rc;

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::text::text_input::TextInput;
use crate::theme::text_edit::TextEditTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::widget::Widget;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::text_edit::edit_end::EditEnd;
use crate::widgets::text_edit::text_layout;

/// Whether a label shows its editor, kept under its id.
#[derive(Clone, Copy, Debug, Default)]
struct EditableLabelState {
    editing: bool,
}

/// Text that shows as a label and opens a [buffered](TextEdit::buffered) editor of itself on a
/// double click, or on Enter or F2 while it holds focus, as GTK4's `EditableLabel` does: Enter or
/// a blur commits, Escape keeps the text. [`Self::start_editing`], [`Self::stop_editing`] and
/// [`Self::is_editing`] drive it from outside, under GTK4's names.
/// The editor opens with the text selected, so the first key replaces it, and as in any
/// [`TextEdit`] the first Escape drops a selection and the next one the edit.
/// The label wears the editor theme's resting look and reserves the editor's caret room, so the
/// glyphs hold their place across the swap. Both halves record under one id, which must be
/// stable across frames, as the editor's draft is kept under it.
///
/// The label senses a click and a drag, so a drag that starts on it reports, for a caller that
/// moves what the label names.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct EditableLabel<'a> {
    widget: Widget,
    text: TextInput<'a>,
    style: Option<&'a TextEditTheme>,
    editable: bool,
    max_chars: Option<usize>,
}

impl<'a> EditableLabel<'a> {
    /// A label showing `text`.
    #[track_caller]
    pub fn new(text: impl Into<TextInput<'a>>) -> Self {
        Self {
            widget: Widget::hstack()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true)
                .input_scope(KeyFilter::ENTER),
            text: text.into(),
            style: None,
            editable: true,
            max_chars: None,
        }
    }

    /// Per-instance override of [`Theme::text_edit`](crate::Theme::text_edit), for the label and its editor.
    pub fn style(mut self, s: impl Into<Option<&'a TextEditTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Whether a double click, Enter or F2 opens the editor. Default `true`; `false` shows the
    /// label alone, and closes an editor that is open.
    pub const fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// Cap the editor at `n` characters, as [`TextEdit::max_chars`] does.
    pub const fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
    }

    /// Record the label, or its editor while one is open. A label that is not
    /// [editable](Self::editable) or is disabled, itself or by an ancestor, closes an open editor
    /// and gives its focus back.
    pub fn show(mut self, ui: &mut Ui) -> EditableLabelResponse<'_> {
        let ResponseSnapshot { id, state } = self.widget.response(ui);
        let editing = Self::is_editing(ui, id);
        let editable = self.editable && !state.disabled;
        if editing && editable {
            return self.show_editor(ui, id);
        }
        if editing {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
            if ui.focus() == Some(id) {
                ui.clear_focus();
            }
        }
        self.show_label(ui, id)
    }

    /// The label: the text in the editor's resting look, with the editor's padding and caret
    /// room, floored at one line so an empty one keeps a target. A double click, or Enter or F2
    /// while the label holds focus, opens the editor and gives it the focus. F2 is read as the
    /// enclosing node reads it, as the label takes no accelerators from the app.
    fn show_label(mut self, ui: &mut Ui, id: WidgetId) -> EditableLabelResponse<'_> {
        let keyed = self.editable
            && ui.focus() == Some(id)
            && (self.widget.shortcut_pressed(ui, Key::Enter) | ui.shortcut_pressed(Key::F2));
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.text_edit);
        let text = slot.looks.normal.text.apply(&theme.text);
        let min = self.floor(text.line_height());
        let reserve =
            text_layout::single_line_caret_reserve(text_layout::caret_room(slot.caret_width));
        let padding = self
            .widget
            .authored_padding()
            .unwrap_or(slot.defaults.padding);
        let [left, top, right, bottom] = padding.to_array();
        let size = self.extent();
        let label = self.text;
        let response = self
            .widget
            .size(size)
            .min_size(min)
            .padding(Spacing::new(left, top, right + reserve, bottom))
            .default_child_align(Align::LEFT)
            .show(ui, Some(&slot.looks.normal.chrome), |ui| {
                Text::new(label)
                    .id(Self::label_id(id))
                    .style(&text)
                    .show(ui);
            })
            .response
            .snapshot();
        if keyed || (self.editable && response.left.double_clicked()) {
            Self::start_editing(ui, id);
        }
        EditableLabelResponse {
            response: Response::new(ui, response),
            editing: false,
            committed_text: None,
        }
    }

    /// The editor, closed on the frame it commits or Escape cancels it.
    fn show_editor(self, ui: &mut Ui, id: WidgetId) -> EditableLabelResponse<'_> {
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.text_edit);
        let text = slot.looks.normal.text.apply(&theme.text);
        let min = self.floor(text.line_height());
        let extent = self.extent();
        let mut edit = TextEdit::buffered(self.text)
            .id(id)
            .style(slot)
            .select_all_on_focus(true)
            .text_align(Align::LEFT)
            .size(extent)
            .min_size(min);
        if let Some(n) = self.max_chars {
            edit = edit.max_chars(n);
        }
        if let Some(max) = self.widget.authored_max_size() {
            edit = edit.max_size(max);
        }
        if let Some(padding) = self.widget.authored_padding() {
            edit = edit.padding(padding);
        }
        let edited = edit.adopt_placement(&self.widget).show(ui);
        let snapshot = edited.response.snapshot();
        let closed = edited.committed || edited.ended == Some(EditEnd::Canceled);
        let committed_text = edited.committed_text;
        if closed {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
            if ui.focus() == Some(id) {
                ui.clear_focus();
            }
        }
        EditableLabelResponse {
            response: Response::new(ui, snapshot),
            editing: true,
            committed_text,
        }
    }

    /// The caller's size, else a hug on both axes.
    fn extent(&self) -> (Sizing, Sizing) {
        let size = self.widget.authored_size().unwrap_or_default();
        (size.w(), size.h())
    }

    /// The caller's minimum, its height floored at `line_height`.
    fn floor(&self, line_height: f32) -> Size {
        let min = self.widget.authored_min_size().unwrap_or(Size::ZERO);
        Size::new(min.w, min.h.max(line_height))
    }

    /// The id of the idle label's text inside `editable_label`, derived from it so the text keeps
    /// its identity wherever the label records.
    pub fn label_id(editable_label: WidgetId) -> WidgetId {
        editable_label.with("label")
    }

    /// Open the editor of the label `id` and give it the focus, as a double click does, for a
    /// rename a menu or a key asks for; the label's next frame records it, its text selected. A
    /// label that is not [editable](Self::editable) or is disabled stays a label.
    pub fn start_editing(ui: &mut Ui, id: WidgetId) {
        ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = true);
        ui.set_focus(id);
    }

    /// End the edit of the label `id` as `end` says: [`EditEnd::Submitted`] commits the draft,
    /// as a blur does, and [`EditEnd::Canceled`] keeps the text, as Escape does. The label's next
    /// frame shows it. No-op for a label that does not edit.
    pub fn stop_editing(ui: &mut Ui, id: WidgetId, end: EditEnd) {
        if !Self::is_editing(ui, id) {
            return;
        }
        if end == EditEnd::Canceled {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
        }
        if ui.focus() == Some(id) {
            ui.clear_focus();
        }
    }

    /// Whether the label `id` shows its editor. Allocates no row.
    pub fn is_editing(ui: &Ui, id: WidgetId) -> bool {
        ui.state::<EditableLabelState>(id)
            .is_some_and(|state| state.editing)
    }
}

impl Configure for EditableLabel<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What [`EditableLabel::show`] returns.
#[derive(Debug)]
pub struct EditableLabelResponse<'a> {
    /// The label's, or while it edits the editor's, pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The label showed its editor this frame.
    pub editing: bool,
    /// The edited text, on the frame an edit that changed it commits.
    pub committed_text: Option<String>,
}

#[cfg(test)]
mod tests;
