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
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::widget::Widget;

/// A chip painting one colour, with a checkerboard behind it when that colour
/// is translucent.
///
/// The display half of the colour family: it writes nothing and senses only a
/// click, so a caller builds a preset row, a recent-colours strip or a
/// "before / after" pair out of it and reads
/// [`clicked()`](crate::ResponseState::clicked) itself.
///
/// Sized from [`ColorPickerTheme::swatch_size`], and styled from the same
/// bundle as the rest of the family.
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

#[cfg(test)]
mod tests {
    use crate::internals::harness::UiHarness;
    use crate::primitives::color::RgbaF32;
    use crate::primitives::widget_id::WidgetId;
    use crate::scene::layer::Layer;
    use crate::widgets::color_swatch::ColorSwatch;
    use crate::widgets::configure::Configure;
    use crate::widgets::theme::color_picker::ColorPickerTheme;
    use glam::UVec2;

    /// A chip is a `swatch_size` square from the theme, or from the
    /// bundle `style` names. Opaque, it records one shape — the colour.
    /// Translucent, the checker goes behind it first: the light fill,
    /// then a dark cell on every other square of the `checker_cell` grid
    /// (24 / 6 = 4 cells a side, so two dark per row over four rows).
    #[test]
    fn a_chip_is_a_themed_square_with_a_checker_behind_translucency() {
        let id = WidgetId::from_hash("chip");
        let styled = ColorPickerTheme {
            swatch_size: 24.0,
            checker_cell: 6.0,
            ..ColorPickerTheme::default()
        };
        let stock = ColorPickerTheme::default().swatch_size;
        assert_ne!(stock, 24.0, "premise: the style moves the side");
        for (style, color, side, shapes) in [
            (None, RgbaF32::srgb(0.2, 0.4, 0.6), stock, 1),
            (Some(&styled), RgbaF32::srgb(0.2, 0.4, 0.6), 24.0, 1),
            (
                Some(&styled),
                RgbaF32::srgba(0.2, 0.4, 0.6, 0.5),
                24.0,
                1 + 4 * 2 + 1,
            ),
        ] {
            let mut h = UiHarness::new(UVec2::new(100, 100));
            let node =
                h.frame_value(|ui| ColorSwatch::new(color).id(id).style(style).show(ui).node());
            let rect = h.arranged(id);
            assert_eq!((rect.size.w, rect.size.h), (side, side), "{color:?}");
            assert_eq!(
                h.ui.tree(Layer::Main).shapes_of(node).count(),
                shapes,
                "{color:?}: shapes recorded",
            );
        }
    }

    /// A chip senses a click and writes nothing — the caller reads it.
    #[test]
    fn a_chip_reports_its_click() {
        let id = WidgetId::from_hash("chip-click");
        let mut h = UiHarness::new(UVec2::new(100, 100));
        let record = |h: &mut UiHarness| {
            h.frame_value(|ui| {
                ColorSwatch::new(RgbaF32::WHITE)
                    .id(id)
                    .show(ui)
                    .left
                    .clicked()
            })
        };
        assert!(!record(&mut h));
        h.click_on(id);
        assert!(record(&mut h), "the click lands");
        assert!(!record(&mut h), "and is one-shot");
    }
}
