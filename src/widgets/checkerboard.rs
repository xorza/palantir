//! The checkerboard a translucent colour is read against.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::theme::color_picker::ColorPickerTheme;

/// The pattern behind anything see-through in a colour picker: one light
/// ground with its dark squares drawn over it.
///
/// One type for the chip, the preview and the alpha bar. All three owe the
/// viewer the same answer to "how much of this is transparent", and two
/// checkers of different cell sizes in one panel read as a bug.
///
/// Holds only what the theme says, so a widget builds it before it opens its
/// node and paints it after. The painted size arrives with each paint: the
/// arranged rect's this frame, or the themed one on the first frame, before
/// there is one. The size only decides how many squares are drawn — wrong
/// for one frame, it draws the pattern over the wrong extent, never in the
/// wrong colour.
#[derive(Debug)]
pub(crate) struct Checkerboard {
    light: RgbaF32,
    dark: RgbaF32,
    cell: f32,
    border: Stroke,
}

impl Checkerboard {
    pub(crate) const fn new(theme: &ColorPickerTheme) -> Self {
        Self {
            light: theme.checker_light,
            dark: theme.checker_dark,
            cell: domain::length_at_least(theme.checker_cell, 1.0),
            border: Stroke::new(
                theme.border,
                domain::length_at_least(theme.border_width, 0.0),
            ),
        }
    }

    /// A chip: the pattern when `color` is translucent, then the colour and
    /// the theme's hairline over it. What a swatch and a picker's trigger
    /// both are.
    pub(crate) fn paint_chip(&self, ui: &mut Ui, color: RgbaF32, size: Size) {
        if color.a < 1.0 {
            self.paint(ui, size);
        }
        ui.add_shape(Shape::owner_rect().fill(color).border(self.border));
    }

    /// Paint the pattern across the owner's rect. The caller decides whether
    /// it is needed; an opaque colour hides it either way.
    #[expect(
        clippy::cast_sign_loss,
        reason = "the cell count is held at zero or above before the cast"
    )]
    pub(crate) fn paint(&self, ui: &mut Ui, size: Size) {
        ui.add_shape(Shape::owner_rect().fill(self.light));
        let columns = (size.w / self.cell).ceil().max(0.0) as u32;
        let rows = (size.h / self.cell).ceil().max(0.0) as u32;
        for row in 0..rows {
            for column in (row % 2..columns).step_by(2) {
                let x = column as f32 * self.cell;
                let y = row as f32 * self.cell;
                let w = self.cell.min(size.w - x);
                let h = self.cell.min(size.h - y);
                ui.add_shape(Shape::rect(Rect::new(x, y, w, h)).fill(self.dark));
            }
        }
    }
}
