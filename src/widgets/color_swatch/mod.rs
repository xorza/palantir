//! A color chip over a checker for translucent colors.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::widget::Widget;
use crate::widgets::checkerboard::Checkerboard;

/// A chip painting one color, over a checkerboard when it is translucent.
///
/// It writes nothing. It senses a click, read through [`clicked()`](crate::ResponseState::clicked), and takes focus, where Space and Enter click it; with the click sense removed it is display only and no Tab stop. Sized from [`ColorPickerTheme::swatch_size`] and styled with the rest of the family.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorSwatch<'a> {
    widget: Widget,
    color: RgbaF32,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorSwatch<'a> {
    /// A chip showing `color`.
    #[track_caller]
    pub fn new(color: RgbaF32) -> Self {
        Self {
            widget: Widget::leaf().sense(Sense::CLICK).focusable(true),
            color,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `color_picker`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the chip and report whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().color_picker);
        let side = domain::length_at_least(theme.swatch_size, 1.0);
        let checker = Checkerboard::new(theme);
        let mut widget = self
            .widget
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        if !widget.authored_sense().contains(Sense::CLICK) {
            widget.configure().focusable(false);
        }
        let response = widget.activation_response(ui, Activation::Button);
        let size = response
            .layout_rect
            .map_or_else(|| Size::new(side, side), |r| r.size);
        let color = self.color;

        widget.record(ui, None, |ui| checker.paint_chip(ui, color, size));
        Response::new(ui, response)
    }
}

impl Configure for ColorSwatch<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
