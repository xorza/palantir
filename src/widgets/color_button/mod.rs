//! The chip that opens a picker: a swatch-styled trigger, and the popup it
//! drops.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_picker::ColorPicker;
use crate::widgets::color_surface;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use std::rc::Rc;

/// A colour chip that opens a [`ColorPicker`] in a popup when clicked.
///
/// The compact form of the picker, for a properties panel or a node's port:
/// one chip the size of a preview, and the panel only while it is wanted.
/// Open state lives in the response map keyed off the trigger, so a caller
/// threads nothing but the colour.
///
/// Clicking outside or pressing Esc closes it. There is no revert, because
/// every gesture inside the panel has already committed — the chip shows what
/// the colour is, not a proposal.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorButton<'a> {
    widget: Widget,
    color: &'a mut RgbaF32,
    alpha: bool,
    model: Option<ColorModel>,
    history: bool,
    swatches: Option<&'a [RgbaF32]>,
    texel_size: u32,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorButton<'a> {
    /// A chip bound to `color`.
    #[track_caller]
    pub fn new(color: &'a mut RgbaF32) -> Self {
        Self {
            widget: Widget::leaf().sense(Sense::CLICK),
            color,
            alpha: false,
            model: None,
            history: true,
            swatches: None,
            texel_size: color_surface::TEXEL_SIZE,
            style: None,
        }
    }

    /// Show the alpha bar and the opacity value in the popup. Off by default,
    /// matching [`ColorPicker::alpha`].
    pub const fn alpha(mut self, on: bool) -> Self {
        self.alpha = on;
        self
    }

    /// Pin the popup's model instead of offering the switch.
    pub const fn model(mut self, model: ColorModel) -> Self {
        self.model = Some(model);
        self
    }

    /// Show the picker's own swatch row. On by default: a chip in a panel is
    /// the case with no room for a preset row of its own.
    pub const fn history(mut self, on: bool) -> Self {
        self.history = on;
        self
    }

    /// Show a swatch row the app owns in the popup, as
    /// [`ColorPicker::swatches`] does. Replaces [`history`](Self::history).
    pub const fn swatches(mut self, colors: &'a [RgbaF32]) -> Self {
        self.swatches = Some(colors);
        self
    }

    /// The edge of one texel of the popup's field and bars, in physical
    /// pixels. See [`ColorField::texel_size`](crate::ColorField::texel_size).
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    #[track_caller]
    pub const fn texel_size(mut self, n: u32) -> Self {
        self.texel_size = domain::power_of_two_in(n, color_surface::MAX_TEXEL_SIZE);
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `color_picker`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the chip, and the popup when it is open.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        // An `Rc` bump on the theme bundle, so the popup can borrow its
        // chrome out of it across the `&mut Ui` the chip's record takes.
        let theme = Rc::clone(ui.theme());
        let slot = self.style.unwrap_or(&theme.color_picker);
        let side = domain::length_at_least(slot.chip_size, 1.0);
        let checker = Checkerboard::new(slot);
        let mut widget = self
            .widget
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let response = widget.response(ui);
        let id = widget.resolve(ui);
        let size = response
            .layout_rect
            .map_or(Size::new(side, side), |r| r.size);
        let color = self.color;
        let shown = *color;

        widget.record(ui, None, |ui| checker.paint_chip(ui, shown, size));

        let alpha = self.alpha;
        let model = self.model;
        let history = self.history;
        let swatches = self.swatches;
        let texel_size = self.texel_size;
        let style = self.style;
        let trigger = ResponseSnapshot {
            id,
            state: response,
        };
        let opened = PopupTrigger::on(&trigger)
            .id(id.with("panel"))
            .background(slot.popup.clone())
            .padding(slot.popup_padding)
            .show(ui, |ui, _| {
                let mut picker = ColorPicker::new(color)
                    .alpha(alpha)
                    .texel_size(texel_size)
                    .style(style);
                picker = match swatches {
                    Some(colors) => picker.swatches(colors),
                    None => picker.history(history),
                };
                if let Some(model) = model {
                    picker = picker.model(model);
                }
                let r = picker.id(id.with("picker")).show(ui);
                (r.changed, r.committed)
            });
        let (changed, committed) = opened.inner.unwrap_or_default();

        ValueResponse {
            response: Response::eager(id, ui, response),
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
