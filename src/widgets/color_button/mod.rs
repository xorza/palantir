//! The chip that opens a color picker popup.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::{ValueEdges, ValueResponse};
use crate::widget_core::widget::Widget;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::popup::trigger_state::{Opening, TriggerState};
use std::rc::Rc;

/// A color chip that opens a [`ColorPicker`] popup when clicked.
///
/// The picker is configured as usual and handed over whole. Open state lives
/// in the response map keyed off the trigger. Click outside or Esc closes it;
/// there is no revert, since every gesture inside already committed. The popup is
/// [`Self::popup_id`], and the picker defaults to `id.with("picker")`.
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
    #[track_caller]
    pub fn new(picker: ColorPicker<'a>) -> Self {
        Self {
            widget: Widget::leaf().sense(Sense::CLICK).focusable(true),
            picker,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::color_picker`](crate::Theme::color_picker) for chip, popup chrome and picker; `None` keeps the picker's own style.
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
        let response = widget.activation_response(ui, Activation::Button);
        let id = response.id;
        let size = response
            .layout_rect
            .map_or_else(|| Size::new(side, side), |r| r.size);
        let shown = self.picker.color();

        widget.record(ui, None, |ui| checker.paint_chip(ui, shown, size));

        let mut picker = self.picker.default_id(id.with("picker"));
        if let Some(style) = self.style {
            picker = picker.style(style);
        }
        let opened = PopupTrigger::on(&response)
            .style(&slot.popup)
            .show_as::<ColorButton<'static>, _>(ui, |ui, _| picker.show(ui).edges());
        let ValueEdges { changed, committed } =
            opened.map(|opened| opened.inner).unwrap_or_default();

        ValueResponse {
            response: Response::new(ui, response),
            changed,
            committed,
        }
    }
}

/// Driving a color button's panel from outside, by the button's id, and the id of the popup.
impl ColorButton<'static> {
    /// The popup the color button `id` drops its panel in.
    pub fn popup_id(id: WidgetId) -> WidgetId {
        PopupTrigger::popup_id(id)
    }

    /// Open the panel of the color button `id`, e.g. from a keyboard shortcut; it records on the
    /// button's next `show`.
    pub fn open(ui: &mut Ui, id: WidgetId) {
        TriggerState::<Self>::open(ui, id, Opening::Below);
    }

    /// Close the panel of the color button `id`, giving back the input it held at once. No-op if
    /// closed.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        TriggerState::<Self>::close(ui, id);
    }

    /// Whether the panel of the color button `id` is open. Allocates no row.
    pub fn is_open(ui: &Ui, id: WidgetId) -> bool {
        TriggerState::<Self>::is_open(ui, id)
    }
}

impl Configure for ColorButton<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
