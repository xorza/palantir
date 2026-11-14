//! [`EditableLabel`]: text that opens its own editor on a double click, Enter or F2.

use std::rc::Rc;

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::{Align, HAlign};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::text::text_input::TextInput;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::{Configure, ThemeDefaults};
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
/// stable across frames, as the editor's draft is kept under it; the idle label's text is
/// `id.with("label")`.
///
/// The label senses a click and a drag, so a drag that starts on it reports, for a caller that
/// moves what the label names.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct EditableLabel<'a> {
    widget: Widget,
    text: TextInput<'a>,
    style: Option<&'a TextEditTheme>,
    overrides: TextStyleOverrides,
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
            overrides: TextStyleOverrides::NONE,
            editable: true,
            max_chars: None,
        }
    }

    /// Per-instance override of [`Theme::text_edit`](crate::Theme::text_edit), for the label and its editor.
    pub fn style(mut self, s: impl Into<Option<&'a TextEditTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Fill color of the text, overriding the resolved look's, on the label and in its editor.
    ///
    /// # Panics
    ///
    /// Panics unless `color` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn color(mut self, color: RgbaF32) -> Self {
        self.overrides.color = Some(domain::color(color));
        self
    }

    /// Font size in logical px, a *length*, overriding the resolved look's; named apart from
    /// [`Configure::size`].
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn font_size(mut self, px: f32) -> Self {
        self.overrides.font_size = Some(domain::length(px));
        self
    }

    /// Line height as a multiple of the font size, overriding the resolved look's. `factor`:
    /// *positive*.
    ///
    /// # Panics
    ///
    /// Panics unless `factor` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn line_height_factor(mut self, factor: f32) -> Self {
        self.overrides.line_height_factor = Some(domain::positive(factor));
        self
    }

    /// Family to shape against, overriding the resolved look's.
    pub const fn family(mut self, family: FontFamily) -> Self {
        self.overrides.family = Some(family);
        self
    }

    /// Weight to shape against; [`Self::bold`] is `FontWeight::BOLD`.
    pub const fn weight(mut self, weight: FontWeight) -> Self {
        self.overrides.weight = Some(weight);
        self
    }

    /// Upright or italic; [`Self::italic`] is `FontSlant::Italic`.
    pub const fn slant(mut self, slant: FontSlant) -> Self {
        self.overrides.slant = Some(slant);
        self
    }

    /// Shape the text bold.
    pub const fn bold(mut self) -> Self {
        self.overrides.weight = Some(FontWeight::BOLD);
        self
    }

    /// Shape the text italic; the weight is untouched, so `.bold().italic()` is bold italic.
    pub const fn italic(mut self) -> Self {
        self.overrides.slant = Some(FontSlant::Italic);
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
    pub fn show(mut self, ui: &mut Ui) -> EditableLabelResponse {
        let response = self.widget.response(ui);
        let id = response.id;
        let editing = Self::is_editing(ui, id);
        let editable = self.editable && !response.disabled;
        if editing && editable {
            return self.show_editor(ui, id);
        }
        if editing {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
            if ui.focus() == Some(id) {
                ui.clear_focus();
            }
        }
        self.show_label(ui, response, editable)
    }

    /// The label: the text in the editor's resting look, with the editor's padding and caret
    /// room, its text floored at one line's block as the editor's is, so an empty one keeps a
    /// target and the label takes the editor's rect whatever it holds. A double click, or Enter
    /// or F2 while the label holds focus, opens the editor and gives it the focus. F2 is read as
    /// the enclosing node reads it, as the label takes no accelerators from the app.
    fn show_label(
        mut self,
        ui: &mut Ui,
        response: Response,
        editable: bool,
    ) -> EditableLabelResponse {
        let id = response.id;
        let keyed = editable
            && ui.focus() == Some(id)
            && (self.widget.shortcut_pressed(ui, Key::Enter) | ui.shortcut_pressed(Key::F2));
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.text_edit);
        let text = self
            .overrides
            .apply(&slot.looks.normal.text.apply(&theme.text));
        let padding = text_layout::label_padding(
            self.widget
                .authored()
                .padding()
                .unwrap_or(slot.defaults.padding),
            slot.caret_width,
            HAlign::Left,
        );
        let size = self.extent();
        let label = self.text;
        self.widget
            .size(size)
            .padding(padding)
            .default_child_align(Align::LEFT)
            .record(ui, Some(&slot.looks.normal.chrome), |ui| {
                Text::new(label)
                    .id(id.with("label"))
                    .style(&text)
                    .min_size((0.0, text.block_height(1)))
                    .show(ui);
            });
        if keyed || (editable && response.left.double_clicked()) {
            Self::start_editing(ui, id);
        }
        EditableLabelResponse {
            response,
            editing: false,
            committed: false,
            committed_text: None,
            ended: None,
        }
    }

    /// The editor, closed on the frame it commits or Escape cancels it.
    fn show_editor(self, ui: &mut Ui, id: WidgetId) -> EditableLabelResponse {
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.text_edit);
        let extent = self.extent();
        let mut edit = TextEdit::buffered(self.text)
            .id(id)
            .style(slot)
            .select_all_on_focus(true)
            .text_align(Align::LEFT)
            .size(extent);
        if let Some(n) = self.max_chars {
            edit = edit.max_chars(n);
        }
        edit = with_overrides(edit, self.overrides);
        if let Some(min) = self.widget.authored().min_size() {
            edit = edit.min_size(min);
        }
        if let Some(max) = self.widget.authored().max_size() {
            edit = edit.max_size(max);
        }
        if let Some(padding) = self.widget.authored().padding() {
            edit = edit.padding(padding);
        }
        let edited = edit.adopt_placement(&self.widget).show(ui);
        let (committed, ended) = (edited.committed, edited.ended);
        let committed_text = edited.committed_text;
        // An editor without the focus after its frame never had it, as one whose opening focus
        // another widget took first: no blur will come to close it.
        let closed = committed || ended == Some(EditEnd::Canceled) || ui.focus() != Some(id);
        if closed {
            ui.with_state::<EditableLabelState, _>(id, |_, state| state.editing = false);
            if ui.focus() == Some(id) {
                ui.clear_focus();
            }
        }
        EditableLabelResponse {
            response: edited.response,
            editing: true,
            committed,
            committed_text,
            ended,
        }
    }

    /// The caller's size, else a hug on both axes.
    fn extent(&self) -> (Sizing, Sizing) {
        let size = self.widget.authored().size().unwrap_or_default();
        (size.w(), size.h())
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

/// `edit` with each axis `overrides` sets, through the editor's own setters.
const fn with_overrides(mut edit: TextEdit<'_>, overrides: TextStyleOverrides) -> TextEdit<'_> {
    let TextStyleOverrides {
        color,
        font_size,
        line_height_factor,
        family,
        weight,
        slant,
    } = overrides;
    if let Some(color) = color {
        edit = edit.color(color);
    }
    if let Some(px) = font_size {
        edit = edit.font_size(px);
    }
    if let Some(factor) = line_height_factor {
        edit = edit.line_height_factor(factor);
    }
    if let Some(family) = family {
        edit = edit.family(family);
    }
    if let Some(weight) = weight {
        edit = edit.weight(weight);
    }
    if let Some(slant) = slant {
        edit = edit.slant(slant);
    }
    edit
}

impl Configure for EditableLabel<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

/// What [`EditableLabel::show`] returns.
#[derive(Clone, Debug)]
pub struct EditableLabelResponse {
    /// The label's, or while it edits the editor's, pointer/click/hover [`Response`].
    pub response: Response,
    /// The label showed its editor this frame.
    pub editing: bool,
    /// The edit finished this frame and kept its result, as
    /// [`TextEditResponse::committed`](crate::TextEditResponse::committed): on Enter, on a blur,
    /// or on [`EditEnd::Submitted`] from [`EditableLabel::stop_editing`]; never on Escape.
    pub committed: bool,
    /// The edited text, on the frame an edit that changed it commits.
    pub committed_text: Option<String>,
    /// How the frame ended the edit, if it did, as
    /// [`TextEditResponse::ended`](crate::TextEditResponse::ended).
    pub ended: Option<EditEnd>,
}

#[cfg(test)]
mod tests;
