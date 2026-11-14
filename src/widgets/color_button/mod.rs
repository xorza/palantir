//! The chip that opens a color picker popup.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widget_core::value_response::ValueResponse;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::color_swatch::ColorSwatch;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::popup::trigger_state::{Opening, TriggerState};
use std::rc::Rc;

/// A color chip that opens a [`ColorPicker`] popup when clicked.
///
/// The picker is configured as usual and handed over whole. Open state lives
/// in the response map keyed off the trigger. Click outside or Esc closes it;
/// there is no revert, since every gesture inside already committed. The popup is the
/// trigger's [`PopupTrigger::popup_id`], and the picker defaults to `id.with("picker")`.
///
/// ```
/// # use palantir::{ColorButton, ColorPicker, RgbaF32, SwatchRow, Ui};
/// # fn f(ui: &mut Ui, color: &mut RgbaF32) {
/// ColorButton::new(ColorPicker::new(color).alpha(true).swatches(SwatchRow::History)).show(ui);
/// # }
/// ```
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorButton<'a> {
    /// The chip: a swatch of the picker's color at the theme's `chip_size`.
    swatch: ColorSwatch<'a>,
    picker: ColorPicker<'a>,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorButton<'a> {
    /// A chip that opens `picker`. The picker's id defaults to `id.with("picker")`; one set on the picker wins.
    #[track_caller]
    pub fn new(picker: ColorPicker<'a>) -> Self {
        Self {
            swatch: ColorSwatch::new(picker.color()),
            picker,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::color_picker`](crate::Theme::color_picker) for the chip and
    /// the popup. The picker keeps the style it was handed with.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Records the button.
    pub fn show(self, ui: &mut Ui) -> ValueResponse {
        // `Rc` bump so the popup can borrow chrome across the `&mut Ui` the chip's record takes.
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.color_picker);
        let side = domain::length_at_least(slot.chip_size, 1.0);
        let response = self
            .swatch
            .style(slot)
            .default_size((Sizing::fixed(side), Sizing::fixed(side)))
            .show(ui);

        let picker = self.picker.default_id(response.id.with("picker"));
        let opened = PopupTrigger::on(&response)
            .style(&slot.popup)
            .show_as::<ColorButton<'static>, _>(ui, |ui| picker.show(ui));
        let picked = opened.map(|opened| opened.inner);
        ValueResponse {
            response,
            changed: picked.is_some_and(|picked| picked.changed),
            committed: picked.is_some_and(|picked| picked.committed),
        }
    }
}

/// Driving a color button's panel from outside, by the button's id.
impl ColorButton<'static> {
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
        self.swatch.configure()
    }
}

#[cfg(test)]
mod tests;
