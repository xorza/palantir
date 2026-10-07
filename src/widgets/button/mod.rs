//! The push button: a labelled, themed leaf reporting pointer-button clicks.

use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::layout::align::Align;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::theme::button::ButtonTheme;

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
}

impl<'a> Button<'a> {
    #[track_caller]
    /// A button.
    pub fn new() -> Self {
        Self {
            // Tab stop; a focused button takes Space and Enter (`KeyClass::Text`), so it claims that class.
            widget: Widget::leaf()
                .sense(Sense::CLICK)
                .focusable(true)
                .input_scope(KeyFilter::TEXT),
            style: None,
            label: TextInput::default(),
            label_align: Align::CENTER,
            label_wrap: TextWrap::Truncate,
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

    /// Space and Enter on a focused button report one left click (WAI-ARIA), so `clicked()` covers keyboard.
    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let mut response = self.widget.response(ui);
        let id = self.widget.resolve(ui);
        if !response.disabled && ui.is_focus_within(id) {
            // `key_pressed` also keeps each chord subscribed for the wake gate, so sample both.
            let space = self.widget.key_pressed(ui, Shortcut::key(Key::Char(' ')));
            let enter = self.widget.key_pressed(ui, Shortcut::key(Key::Enter));
            if space || enter {
                response.left.phase = ButtonPhase::Up { click: Some(1) };
            }
        }
        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.button);
        let look = slot
            .plan(&response, (), theme.text)
            .apply(ui, &mut self.widget);
        let label = self.label;
        let label_align = self.label_align;
        let label_wrap = self.label_wrap;

        self.widget.record(ui, Some(&look.background), |ui| {
            if !label.is_empty() {
                let label = ui.intern(label);
                ui.add_shape(
                    Shape::text(label, look.text.font())
                        .color(look.text.color)
                        .wrap(label_wrap)
                        .align(label_align),
                );
            }
        });
        Response::new(id, ui, response)
    }
}

impl Configure for Button<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
