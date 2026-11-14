//! A color chip over a checker for translucent colors.

use crate::primitives::geometry::size::Size;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::color_picker::ColorPickerTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::widget::Widget;
use crate::widgets::checkerboard::{Checkerboard, Chip};
use std::rc::Rc;

/// A chip painting one color, over a checkerboard when it is translucent.
///
/// It frames the color in [`ColorPickerTheme::chip`]'s look for its state, and lays
/// [`ColorPickerTheme::disabled_veil`] over it while disabled.
///
/// It writes nothing. It senses a click, read through [`clicked()`](crate::Response::clicked), and takes focus, where Space and Enter click it. With the click sense removed it is display only, and `focusable(false)` takes its Tab stop away too, as for every widget: a caller's setter wins. Sized from [`ColorPickerTheme::swatch_size`] and styled with the rest of the family.
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

    /// Per-instance override of [`Theme::color_picker`](crate::Theme::color_picker).
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the chip and report whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> Response {
        // `Rc` bump so the theme outlives the `&mut Ui` the look's animation takes.
        let ui_theme = Rc::clone(ui.theme());
        let theme = self.style.unwrap_or(&ui_theme.color_picker);
        let side = domain::length_at_least(theme.swatch_size, 1.0);
        let checker = Checkerboard::new(theme);
        let mut widget = self
            .widget
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let response = widget.activation_response(ui, Activation::Button);
        let size = response
            .layout_rect
            .map_or_else(|| Size::new(side, side), |r| r.size);
        let color = self.color;
        let veil = response.disabled.then_some(theme.disabled_veil);
        let plan = theme
            .chip
            .plan(&response, Selection::Unselected, ui_theme.text);
        let look = widget.wear(ui, plan);
        let inset = look.chrome.paint.border.width;

        widget.record(ui, Some(&look.chrome), |ui| {
            checker.paint_chip(
                ui,
                Chip {
                    color,
                    size,
                    inset,
                    veil,
                },
            );
        });
        response
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
