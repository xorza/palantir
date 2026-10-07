//! The chip that opens a colour picker popup.

use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use std::rc::Rc;

/// A colour chip that opens a [`ColorPicker`] popup when clicked.
///
/// The picker is configured as usual and handed over whole. Open state lives
/// in the response map keyed off the trigger. Click outside or Esc closes it;
/// there is no revert, since every gesture inside already committed.
///
/// ```
/// # use palantir::{ColorButton, ColorPicker, RgbaF32, Ui};
/// # fn f(ui: &mut Ui, color: &mut RgbaF32) {
/// ColorButton::new(ColorPicker::new(color).alpha(true).history(true)).show(ui);
/// # }
/// ```
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorButton<'a> {
    widget: Widget,
    picker: ColorPicker<'a>,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorButton<'a> {
    /// A chip that opens `picker`. The picker's id defaults to `id.with("picker")`; one set on the picker wins.
    pub fn new(picker: ColorPicker<'a>) -> Self {
        Self {
            widget: Widget::leaf()
                .sense(Sense::CLICK)
                .focusable(true)
                .input_scope(KeyFilter::TEXT),
            picker,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `color_picker` for chip, popup chrome and picker; `None` keeps the picker's own style.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Records the button.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        // `Rc` bump so the popup can borrow chrome across the `&mut Ui` the chip's record takes.
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.color_picker);
        let side = domain::length_at_least(slot.chip_size, 1.0);
        let checker = Checkerboard::new(slot);
        let mut widget = self
            .widget
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let mut response = widget.response(ui);
        let id = widget.resolve(ui);
        if !response.disabled && ui.is_focus_within(id) {
            let space = widget.key_pressed(ui, Shortcut::key(Key::Char(' ')));
            let enter = widget.key_pressed(ui, Shortcut::key(Key::Enter));
            if space || enter {
                response.left.phase = ButtonPhase::Up { click: Some(1) };
            }
        }
        let size = response
            .layout_rect
            .map_or(Size::new(side, side), |r| r.size);
        let shown = self.picker.color();

        widget.record(ui, None, |ui| checker.paint_chip(ui, shown, size));

        let mut picker = self.picker.default_id(id.with("picker"));
        if let Some(style) = self.style {
            picker = picker.style(style);
        }
        let trigger = ResponseSnapshot {
            id,
            state: response,
        };
        let opened = PopupTrigger::on(&trigger)
            .id(id.with("panel"))
            .background(slot.popup.clone())
            .padding(slot.popup_padding)
            .show(ui, |ui, _| {
                let r = picker.show(ui);
                (r.changed, r.committed)
            });
        let (changed, committed) = opened.inner.unwrap_or_default();

        ValueResponse {
            response: Response::new(id, ui, response),
            changed,
            committed,
        }
    }
}

impl Configure for ColorButton<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
