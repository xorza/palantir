//! [`EditableLabel`]: text that opens its own editor on a double click.

use std::rc::Rc;

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
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
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
/// double click, as GTK4's `EditableLabel` does: Enter or a blur commits, Escape keeps the text.
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
            widget: Widget::hstack().sense(Sense::CLICK | Sense::DRAG),
            text: text.into(),
            style: None,
            editable: true,
            max_chars: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `text_edit`, for the label and its editor.
    pub fn style(mut self, s: impl Into<Option<&'a TextEditTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Whether a double click opens the editor. Default `true`; `false` shows the label alone,
    /// and closes an editor that is open.
    pub const fn editable(mut self, on: bool) -> Self {
        self.editable = on;
        self
    }

    /// Cap the editor at `n` characters, as [`TextEdit::max_chars`] does.
    pub const fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
    }

    /// Record the label, or its editor while one is open.
    pub fn show(mut self, ui: &mut Ui) -> EditableLabelResponse<'_> {
        let id = self.widget.resolve(ui);
        let editing = ui
            .state::<EditableLabelState>(id)
            .is_some_and(|state| state.editing);
        if editing && self.editable {
            return self.show_editor(ui, id);
        }
        if editing {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
        }
        self.show_label(ui, id)
    }

    /// The label: the text in the editor's resting look, with the editor's padding and caret
    /// room, floored at one line so an empty one keeps a target. A double click opens the editor
    /// and gives it the focus.
    fn show_label(self, ui: &mut Ui, id: WidgetId) -> EditableLabelResponse<'_> {
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
            .child_align(Align::LEFT)
            .show(ui, Some(&slot.looks.normal.chrome), |ui| {
                Text::new(label)
                    .id(Self::label_id(id))
                    .style(&text)
                    .show(ui);
            })
            .response
            .snapshot();
        if self.editable && response.left.double_clicked() {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = true);
            ui.set_focus(id);
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

    /// The id of the idle label's text inside the label `id`, derived from it so the text keeps
    /// its identity wherever the label records.
    pub fn label_id(id: WidgetId) -> WidgetId {
        id.with("label")
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
