//! The one-axis bars of a color picker: hue, and alpha over its checker.

use crate::input::keyboard::key::Key;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
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
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_surface;
use crate::widgets::value_area::{AreaValue, ValueArea};
use glam::Vec2;

/// A one-axis bar of a color picker: the hue ramp, or one color's alpha ramp over its checker.
///
/// Exact per texel, as [`ColorField`](crate::ColorField) explains; the alpha bar writes real alpha and
/// lets the GPU composite it over the checker.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorStrip<'a> {
    area: ValueArea<'a>,
    kind: StripKind<'a>,
}

#[derive(Debug)]
enum StripKind<'a> {
    Hue(&'a mut ColorCoords),
    Alpha(&'a mut RgbaF32),
}

const ALONG: AxisKeys = AxisKeys {
    steps: &[KeyPair {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    }],
    page: Some(KeyPair {
        back: Key::PageDown,
        forward: Key::PageUp,
    }),
    ends: Some(KeyPair {
        back: Key::Home,
        forward: Key::End,
    }),
};

impl<'a> ColorStrip<'a> {
    #[track_caller]
    /// A strip over the hue of `coords`.
    pub fn for_hue(coords: &'a mut ColorCoords) -> Self {
        Self::new(StripKind::Hue(coords))
    }

    /// An alpha bar over `color`. Not named `alpha`, which is a picker setter.
    #[track_caller]
    pub fn for_alpha(color: &'a mut RgbaF32) -> Self {
        Self::new(StripKind::Alpha(color))
    }

    #[track_caller]
    fn new(kind: StripKind<'a>) -> Self {
        Self {
            area: ValueArea::new(),
            kind,
        }
    }

    /// Edge of one texture texel in physical pixels, a power of two; default 4. See
    /// [`ColorField::texel_size`](crate::ColorField::texel_size).
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    #[track_caller]
    pub const fn texel_size(mut self, n: u32) -> Self {
        self.area.texel_size = domain::power_of_two_in(n, color_surface::MAX_TEXEL_SIZE);
        self
    }

    /// Theme slot.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.area.style = s.into();
        self
    }

    /// Records the strip.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        self.area.show(ui, self.kind)
    }
}

impl Configure for ColorStrip<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.area.widget.configure()
    }
}

impl StripKind<'_> {
    const fn read(&self) -> f32 {
        match self {
            Self::Hue(coords) => coords.hue(),
            Self::Alpha(color) => color.a,
        }
    }

    fn write(&mut self, at: f32) -> bool {
        let before = self.read();
        match self {
            Self::Hue(coords) => coords.set_hue(at),
            Self::Alpha(color) => color.a = at.clamp(0.0, 1.0),
        }
        self.read() != before
    }

    const fn paint(&self) -> StripPaint {
        match self {
            Self::Hue(coords) => StripPaint::Hue(coords.model()),
            Self::Alpha(color) => StripPaint::Alpha(color.with_alpha(1.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StripPaint {
    Hue(ColorModel),
    Alpha(RgbaF32),
}

impl StripPaint {
    const fn wants_checker(self) -> bool {
        matches!(self, Self::Alpha(_))
    }

    fn fill(self, image: &mut Image) {
        let width = image.size().x;
        for (col, texel) in image.row_mut(0).iter_mut().enumerate() {
            let along = (col as f32 + 0.5) / width as f32;
            *texel = match self {
                Self::Hue(model) => model.slice(along).color(1.0, 1.0).to_srgba8(),
                Self::Alpha(color) => color.with_alpha(along).to_srgba8(),
            };
        }
        image.repeat_row(0);
    }
}

impl AreaValue for StripKind<'_> {
    type Surface = StripPaint;

    fn themed(theme: &ColorPickerTheme) -> Size {
        Size::new(
            domain::length_at_least(theme.field_width, 1.0),
            domain::length_at_least(theme.bar_thickness, 1.0),
        )
    }

    fn press(&mut self, at: Vec2) -> bool {
        self.write(at.x)
    }

    fn travel(&mut self, widget: &mut Widget, ui: &mut Ui) -> bool {
        let travel = ALONG.read(widget, ui);
        let mut at = travel.on_unit_axis(self.read(), UNIT_STEP, UNIT_PAGE);
        if travel.end.is_none() && matches!(self, Self::Hue(_)) && !(0.0..=1.0).contains(&at) {
            // Steps wrap round the hue circle; positions do not (drag, Home, End clamp in
            // `ColorCoords::set_hue`).
            at = at.rem_euclid(1.0);
        }
        self.write(at)
    }

    fn surface(&self) -> StripPaint {
        self.paint()
    }

    fn fill(surface: StripPaint, image: &mut Image) {
        surface.fill(image);
    }

    fn underlay(&self, ui: &mut Ui, theme: &ColorPickerTheme, size: Size) {
        if self.paint().wants_checker() {
            Checkerboard::new(theme).paint(ui, size);
        }
    }

    fn marker(&self, ui: &mut Ui, theme: &ColorPickerTheme, size: Size) {
        let width = domain::length_at_least(theme.handle_width, 0.0);
        let x = self.read() * size.w;
        let (top, bottom) = (Vec2::new(x, 0.0), Vec2::new(x, size.h));
        ui.add_shape(Shape::line(
            top,
            bottom,
            Stroke::new(theme.handle_outer, width * 2.5),
        ));
        ui.add_shape(Shape::line(
            top,
            bottom,
            Stroke::new(theme.handle_inner, width),
        ));
    }
}

#[cfg(test)]
mod tests;
