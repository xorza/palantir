//! One option of a radio group, over the shared value the whole group writes through.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::text::text_input::TextInput;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::toggle_chrome::ToggleChrome;

/// One option in a radio group: selected when `*current == value`, and clicking assigns `value` into
/// `current`. `T: PartialEq` is the only bound. Layout matches [`crate::Checkbox`] (pip, label); the pip
/// is always a pill. Visuals come from `theme.radio` ([`crate::ToggleTheme`]).
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct RadioButton<'a, T: PartialEq> {
    widget: Widget,
    current: &'a mut T,
    value: T,
    label: TextInput<'a>,
    style: Option<&'a ToggleTheme>,
}

impl<'a, T: PartialEq> RadioButton<'a, T> {
    /// Writes `value` into `current` when picked; selected while the two are equal.
    #[track_caller]
    pub fn new(current: &'a mut T, value: T) -> Self {
        Self {
            widget: ToggleChrome::row(),
            current,
            value,
            label: TextInput::default(),
            style: None,
        }
    }

    /// Label text.
    pub fn label(mut self, label: impl Into<TextInput<'a>>) -> Self {
        self.label = label.into();
        self
    }

    /// Theme slot.
    pub fn style(mut self, s: impl Into<Option<&'a ToggleTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Records the row. A radio latches: `response.clicked()` is true on the already-selected option but
    /// `changed` is not. A pick commits at once, so `committed == changed`.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let response = self.widget.response(ui);
        let activated = ToggleChrome::activated(ui, &mut self.widget, &response);

        // Read ahead of the latch below, which moves `self.value`.
        let theme = ui.theme();
        let slot = self.style.unwrap_or(&theme.radio);
        let pip_size = domain::length_at_least(slot.box_size, 1.0);
        let indicator = slot.indicator;
        let dot_inset = domain::length_at_least(slot.indicator_inset, 0.0);

        let mut selected = *self.current == self.value;
        let mut changed = false;
        // Radios latch: re-clicking the selected option is a no-op. A fresh click flips `selected` now, since
        // `value` moves into `current`, or the pip paints unselected until the next repaint.
        if activated && !selected {
            *self.current = self.value;
            selected = true;
            changed = true;
        }

        let chrome = ToggleChrome {
            plan: slot.plan(&response, selected, theme.text),
            gap: slot.gap,
            boxed: Widget::leaf().size((Sizing::fixed(pip_size), Sizing::fixed(pip_size))),
            // Forces the pip to a circle whatever `radio.checked.normal.background.radius` a theme sets.
            pill: Some(pip_size * 0.5),
        };
        let response = chrome.record_row(ui, self.widget, response, self.label, |ui, _| {
            if selected {
                let dot_size = domain::length_at_least(pip_size - 2.0 * dot_inset, 0.0);
                let dot = Rect::new(dot_inset, dot_inset, dot_size, dot_size);
                ui.add_shape(Shape::rect(dot).corners(dot_size * 0.5).fill(indicator));
            }
        });
        ValueResponse {
            response,
            changed,
            committed: changed,
        }
    }
}

impl<T: PartialEq> Configure for RadioButton<'_, T> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
