//! The push button: a labelled, themed leaf that reports what each pointer
//! button did to it.

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
    /// An unlabelled button. Add text with [`Self::label`].
    #[track_caller]
    pub fn new() -> Self {
        Self {
            // A Tab stop, and a focused button takes Space and Enter —
            // which classify as `KeyClass::Text`, so it claims that class,
            // as a focused toggle does.
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

    /// Per-instance override of [`crate::Theme`]'s `button`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ButtonTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The text this widget draws. Empty (the default) draws none —
    /// no text child is recorded at all.
    ///
    /// Drawn inside the button and centered by default;
    /// [`Self::text_align`] moves it.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Set how the label handles a width narrower than its natural line.
    /// Default [`TextWrap::Truncate`] (hard-cut to one line, no marker); pass
    /// [`TextWrap::Ellipsis`] to mark the cut with `…`, [`TextWrap::WrapWithOverflow`] to
    /// reflow onto multiple lines, or [`TextWrap::SingleLine`] to let it run
    /// past the chrome. Only bites on a `Fixed`/`Fill`-width button — a `Hug`
    /// button commits its natural width, so the label always fits.
    pub const fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.label_wrap = wrap;
        self
    }

    /// Position of the label glyphs inside the button's arranged rect.
    /// Distinct from [`Configure::align`], which positions the *button*
    /// inside its parent's slot. Default: [`Align::CENTER`].
    pub const fn text_align(mut self, a: Align) -> Self {
        self.label_align = a;
        self
    }

    /// Record the button. Read the click off the [`Response`].
    ///
    /// Space and Enter on a focused button click it: the response reports
    /// a single left click, as WAI-ARIA's button pattern asks, so a caller
    /// reads keyboard and pointer alike through `clicked()`.
    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let mut response = self.widget.response(ui);
        let id = self.widget.resolve(ui);
        if !response.disabled && ui.is_focus_within(id) {
            // Both sampled: `key_pressed` also keeps each chord subscribed
            // for the wake gate.
            let space = ui.key_pressed(Shortcut::key(Key::Char(' ')));
            let enter = ui.key_pressed(Shortcut::key(Key::Enter));
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
        // Eager: theme picking already paid for `response_for`, so
        // hand the cached response to the caller.
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
