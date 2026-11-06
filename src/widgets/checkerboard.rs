//! The checkerboard a translucent color is read against.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;

/// The light ground with dark squares drawn over it, behind translucent color.
///
/// One type so the chip, preview and alpha bar share a cell size. The painted rect arrives with each paint (from the themed size before the first arrange) and only decides how many squares are drawn.
#[derive(Debug)]
pub(crate) struct Checkerboard {
    light: RgbaF32,
    dark: RgbaF32,
    cell: f32,
}

/// A color chip as [`Checkerboard::paint_chip`] paints it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Chip {
    pub(crate) color: RgbaF32,
    pub(crate) size: Size,
    pub(crate) inset: f32,
    pub(crate) veil: Option<RgbaF32>,
}

impl Checkerboard {
    pub(crate) const fn new(theme: &ColorPickerTheme) -> Self {
        Self {
            light: theme.checker_light,
            dark: theme.checker_dark,
            cell: domain::length_at_least(theme.checker_cell, 1.0),
        }
    }

    /// A chip's content inside its frame's border, `inset` wide: the pattern when `color` is
    /// translucent, then the color, then `veil` over both when the chip is disabled.
    pub(crate) fn paint_chip(&self, ui: &mut Ui, chip: Chip) {
        let Chip {
            color,
            size,
            inset,
            veil,
        } = chip;
        let inside = Rect::new(
            inset,
            inset,
            (size.w - 2.0 * inset).max(0.0),
            (size.h - 2.0 * inset).max(0.0),
        );
        if color.a < 1.0 {
            self.paint(ui, inside);
        }
        ui.add_shape(Shape::rect().at(inside).fill(color));
        if let Some(veil) = veil {
            ui.add_shape(Shape::rect().at(inside).fill(veil));
        }
    }

    /// Paint the pattern across `rect`, in the owner's space, its squares counted from its top
    /// left.
    #[expect(
        clippy::cast_sign_loss,
        reason = "the cell count is held at zero or above before the cast"
    )]
    pub(crate) fn paint(&self, ui: &mut Ui, rect: Rect) {
        ui.add_shape(Shape::rect().at(rect).fill(self.light));
        let cols = (rect.size.w / self.cell).ceil().max(0.0) as u32;
        let rows = (rect.size.h / self.cell).ceil().max(0.0) as u32;
        for row in 0..rows {
            for col in (row % 2..cols).step_by(2) {
                let x = col as f32 * self.cell;
                let y = row as f32 * self.cell;
                let w = self.cell.min(rect.size.w - x);
                let h = self.cell.min(rect.size.h - y);
                ui.add_shape(
                    Shape::rect()
                        .at(Rect::new(rect.min.x + x, rect.min.y + y, w, h))
                        .fill(self.dark),
                );
            }
        }
    }
}
