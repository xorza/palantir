//! The push button: a labeled, themed leaf reporting pointer-button clicks.

use crate::input::sense::Sense;
use crate::primitives::layout::align::Align;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::theme::button::ButtonTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::selection::Selection;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::{InnerResponse, Response};
use crate::widget_core::widget::Widget;

/// A clickable, themed rectangle carrying an optional label.
///
/// ```
/// # use palantir::{Button, Ui};
/// # fn demo(ui: &mut Ui) {
/// if Button::new().label("Save").show(ui).clicked() {
///     // …
/// }
/// # }
/// ```
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Button<'a> {
    widget: Widget,
    style: Option<&'a ButtonTheme>,
    label: TextInput<'a>,
    label_align: Align,
    label_wrap: TextWrap,
    selection: Selection,
}

impl<'a> Button<'a> {
    #[track_caller]
    /// A button.
    pub fn new() -> Self {
        Self {
            widget: Widget::leaf().sense(Sense::CLICK).focusable(true),
            style: None,
            label: TextInput::default(),
            label_align: Align::CENTER,
            label_wrap: TextWrap::Truncate,
            selection: Selection::Unselected,
        }
    }

    /// Theme slot.
    pub fn style(mut self, s: impl Into<Option<&'a ButtonTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Label text.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Label wrap policy for a width narrower than its line (default [`TextWrap::Truncate`]); a `Hug` button never narrows.
    pub const fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.label_wrap = wrap;
        self
    }

    /// Label position inside the button's rect (unlike [`Configure::align`], which places the button).
    pub const fn text_align(mut self, a: Align) -> Self {
        self.label_align = a;
        self
    }

    /// Whether the button shows its on state, as WAI-ARIA's `aria-pressed` reports one: it then
    /// wears [`ButtonTheme::selected`]. A click reports as it does on any button; the caller
    /// changes the state it shows. Default `false`.
    pub const fn selected(mut self, selected: bool) -> Self {
        self.selection = if selected {
            Selection::Selected
        } else {
            Selection::Unselected
        };
        self
    }

    /// Space and Enter on a focused button report one left click (WAI-ARIA), so `clicked()` covers keyboard.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        self.show_with(ui, |_, _| {}).response
    }

    /// [`Self::show`], then `body` after the label: it adds shapes in the button's own space,
    /// and gets the text style the button's state resolved, so an icon takes the label's ink.
    /// The button is a leaf, so `body` adds shapes alone; a widget recorded in it is not laid
    /// out.
    pub fn show_with<R>(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &TextStyle) -> R,
    ) -> InnerResponse<'_, R> {
        let response = self.widget.activation_response(ui, Activation::Button);
        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.button);
        let plan = slot.plan(&response, self.selection, theme.text);
        let look = self.widget.wear(ui, plan);
        let label = self.label;
        let label_align = self.label_align;
        let label_wrap = self.label_wrap;

        let inner = self.widget.record(ui, Some(&look.chrome), |ui| {
            if !label.is_empty() {
                let label = ui.intern(label);
                ui.add_shape(
                    Shape::text(label, look.text.font())
                        .color(look.text.color)
                        .wrap(label_wrap)
                        .align(label_align),
                );
            }
            body(ui, &look.text)
        });
        InnerResponse {
            response: Response::new(ui, response),
            inner,
        }
    }
}

impl Configure for Button<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
