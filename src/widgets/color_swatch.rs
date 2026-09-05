//! A colour chip: the smallest thing that shows a colour, and the checker it
//! shows a translucent one against.

use crate::input::sense::Sense;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::ui::Ui;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::widget::Widget;

/// A chip painting one colour, with a checkerboard behind it when that colour
/// is translucent.
///
/// The display half of the colour family: it writes nothing and senses only a
/// click, so a caller builds a preset row, a recent-colours strip or a
/// "before / after" pair out of it and reads
/// [`clicked`](crate::Response::clicked) itself.
///
/// Sized from [`ColorPickerTheme::swatch_size`], and styled from the same
/// bundle as the rest of the family.
#[derive(Debug)]
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
            widget: Widget::leaf().sense(Sense::CLICK),
            color,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `color_picker`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the chip and report whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let theme = self.style.unwrap_or(&ui.theme().color_picker);
        let side = theme.swatch_size.themed_length(1.0);
        let checker = Checkerboard::new(theme);
        let mut widget = self
            .widget
            .default_size((Sizing::fixed(side), Sizing::fixed(side)));
        let response = widget.response(ui);
        let id = widget.resolve(ui);
        let size = response
            .layout_rect
            .map_or(Size::new(side, side), |r| r.size);
        let color = self.color;

        widget.record(ui, None, |ui| checker.paint_chip(ui, color, size));
        Response::eager(id, ui, response)
    }
}

impl Configure for ColorSwatch<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}
