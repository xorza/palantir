//! The saturation/value area of a color picker, and the texture it paints itself with.

use crate::input::keyboard::key::Key;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::{ColorModel, HUE_BLOCK};
use crate::primitives::paint::image::Image;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::{AxisKeys, KeyPair, UNIT_PAGE, UNIT_STEP};
use crate::widgets::color_surface;
use crate::widgets::value_area::{AreaValue, ValueArea};
use glam::Vec2;

/// The two-axis area of a color picker: saturation left to right, value bottom to top, at the bound hue. Exact per texel: a CPU texture refreshed in place when hue or model moves, since a gradient stack interpolates in linear light and a vertex-colored mesh crushes the darks. Returns [`ValueResponse`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorField<'a> {
    area: ValueArea<'a>,
    coords: &'a mut ColorCoords,
}

const ACROSS: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    }],
    page: None,
    ends: Some(KeyPair {
        back: Key::Home,
        forward: Key::End,
    }),
};

const UP: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowDown,
        forward: Key::ArrowUp,
    }],
    page: Some(KeyPair {
        back: Key::PageDown,
        forward: Key::PageUp,
    }),
    ends: None,
};

impl<'a> ColorField<'a> {
    /// A field driving `coords`.
    #[track_caller]
    pub fn new(coords: &'a mut ColorCoords) -> Self {
        Self {
            area: ValueArea::new(),
            coords,
        }
    }

    /// The edge of one texture texel in physical pixels, as a power of two. Default 4. Worst error against the exact color in 8-bit sRGB units (`tests::texel_size_four_tracks_the_exact_color`):
    ///
    /// | texel size | Okhsv | HSV | texels to convert |
    /// |---|---|---|---|
    /// | 1 | 0 | 0 | 74 880 |
    /// | 2 | 4 | 1 | 18 720 |
    /// | **4** | **9** | **3** | **4 680** |
    /// | 8 | 16 | 6 | 1 170 |
    /// | 16 | 25 | 14 | 293 |
    ///
    /// Four costs a sixteenth of the conversions for an error nobody reads off a picker.
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    #[track_caller]
    pub const fn texel_size(mut self, n: u32) -> Self {
        self.area.texel_size = domain::power_of_two_in(n, color_surface::MAX_TEXEL_SIZE);
        self
    }

    /// Per-instance override of `color_picker`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.area.style = s.into();
        self
    }

    /// Record the field and report what the gesture did to the coordinates.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        self.area.show(ui, Field(self.coords))
    }
}

impl Configure for ColorField<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.area.widget.configure()
    }
}

/// Saturation left to right and value bottom to top, at the bound hue.
#[derive(Debug)]
struct Field<'a>(&'a mut ColorCoords);

impl Field<'_> {
    fn write(&mut self, sat: f32, val: f32) -> bool {
        let before = *self.0;
        self.0.set_saturation(sat);
        self.0.set_value(val);
        *self.0 != before
    }
}

impl AreaValue for Field<'_> {
    type Surface = (ColorModel, f32);

    fn themed(theme: &ColorPickerTheme) -> Size {
        Size::new(
            domain::length_at_least(theme.field_width, 1.0),
            domain::length_at_least(theme.field_height, 1.0),
        )
    }

    fn press(&mut self, at: Vec2) -> bool {
        self.write(at.x, 1.0 - at.y)
    }

    fn travel(&mut self, widget: &mut Widget, ui: &mut Ui) -> bool {
        let sat = ACROSS
            .read(widget, ui)
            .on_unit_axis(self.0.saturation(), UNIT_STEP, UNIT_PAGE);
        let val = UP
            .read(widget, ui)
            .on_unit_axis(self.0.value(), UNIT_STEP, UNIT_PAGE);
        self.write(sat, val)
    }

    fn surface(&self) -> Self::Surface {
        (self.0.model(), self.0.hue())
    }

    fn fill((model, hue): Self::Surface, image: &mut Image) {
        fill(image, model, hue);
    }

    fn marker(&self, ui: &mut Ui, theme: &ColorPickerTheme, size: Size) {
        let radius = domain::length_at_least(theme.handle_radius, 1.0);
        let width = domain::length_at_least(theme.handle_width, 0.0);
        let at = Vec2::new(
            self.0.saturation() * size.w,
            (1.0 - self.0.value()) * size.h,
        );
        ui.add_shape(Shape::circle(
            at,
            radius,
            Stroke::new(theme.handle_outer, width),
        ));
        ui.add_shape(Shape::circle(
            at,
            domain::length_at_least(radius - width, 0.0),
            Stroke::new(theme.handle_inner, width),
        ));
    }
}

// Every texel shares the hue, so its gamut solve belongs outside the loop, and every texel of a
// column its saturation, so that column's solve belongs outside the rows.
fn fill(image: &mut Image, model: ColorModel, hue: f32) {
    let slice = model.slice(hue);
    let size = image.size().as_vec2();
    let width = image.size().x as usize;
    let texels = image.texels_mut();
    for start in (0..width).step_by(HUE_BLOCK) {
        let columns = start..width.min(start + HUE_BLOCK);
        let block = slice.block(columns.clone().map(|col| (col as f32 + 0.5) / size.x));
        for (row, line) in texels.chunks_exact_mut(width).enumerate() {
            block.row(
                1.0 - (row as f32 + 0.5) / size.y,
                &mut line[columns.clone()],
            );
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
