//! The saturation/value area of a colour picker, and the texture it paints itself with.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::image::ImageFit;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::axis_keys::KeyPair;
use crate::widgets::color_surface;
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::Vec2;

/// The two-axis area of a colour picker: saturation left to right, value bottom to top, at the bound hue. Exact per texel: a CPU texture refreshed in place when hue or model moves, since a gradient stack interpolates in linear light and a vertex-coloured mesh crushes the darks. Returns [`ValueResponse`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorField<'a> {
    widget: Widget,
    coords: &'a mut ColorCoords,
    texel_size: u32,
    style: Option<&'a ColorPickerTheme>,
}

const ACROSS: AxisKeys = AxisKeys {
    step: KeyPair {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    },
    page: None,
    ends: Some(KeyPair {
        back: Key::Home,
        forward: Key::End,
    }),
};

const UP: AxisKeys = AxisKeys {
    step: KeyPair {
        back: Key::ArrowDown,
        forward: Key::ArrowUp,
    },
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
            widget: Widget::leaf()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            coords,
            texel_size: color_surface::TEXEL_SIZE,
            style: None,
        }
    }

    /// The edge of one texture texel in physical pixels, as a power of two. Default 4. Worst error against the exact colour in 8-bit sRGB units (`tests::texel_size_four_tracks_the_exact_colour`):
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
        self.texel_size = domain::power_of_two_in(n, color_surface::MAX_TEXEL_SIZE);
        self
    }

    /// Per-instance override of `color_picker`.
    pub fn style(mut self, s: impl Into<Option<&'a ColorPickerTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record the field and report what the gesture did to the coordinates.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        let theme = self.style.unwrap_or(&ui.theme().color_picker);
        let themed = Size::new(
            domain::length_at_least(theme.field_width, 1.0),
            domain::length_at_least(theme.field_height, 1.0),
        );
        let handle_radius = domain::length_at_least(theme.handle_radius, 1.0);
        let handle_width = domain::length_at_least(theme.handle_width, 0.0);
        let handle_outer = theme.handle_outer;
        let handle_inner = theme.handle_inner;

        let mut widget = self
            .widget
            .default_size((Sizing::fixed(themed.w), Sizing::fixed(themed.h)));
        let response = widget.response(ui);
        let id = widget.resolve(ui);
        let size = response.layout_rect.map_or(themed, |r| r.size);

        let coords = self.coords;
        let mut changed = false;
        if let Some(at) = response.press_fraction(0.0) {
            changed |= write_axes(coords, at.x, 1.0 - at.y);
        }
        let keyed = !response.disabled && ui.is_focus_within(id) && keyboard_travel(ui, coords);
        changed |= keyed;
        let committed = !response.disabled && (response.left.released() || keyed);

        let texels = color_surface::texture_size(size, self.texel_size, ui);
        let model = coords.model();
        let hue = coords.hue();
        let marker = Vec2::new(
            coords.saturation() * size.w,
            (1.0 - coords.value()) * size.h,
        );

        widget.record(ui, None, |ui| {
            let image = ui.with_state::<ColorSurface<(ColorModel, f32)>, _>(id, |ui, surface| {
                surface
                    .ensure(ui, texels, (model, hue), |image| fill(image, model, hue))
                    .clone()
            });
            ui.add_shape(Shape::image(image).fit(ImageFit::Fill));
            ui.add_shape(Shape::circle(
                marker,
                handle_radius,
                Stroke::new(handle_outer, handle_width),
            ));
            ui.add_shape(Shape::circle(
                marker,
                domain::length_at_least(handle_radius - handle_width, 0.0),
                Stroke::new(handle_inner, handle_width),
            ));
        });
        ValueResponse {
            response: Response::new(id, ui, response),
            changed,
            committed,
        }
    }
}

impl Configure for ColorField<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

fn write_axes(coords: &mut ColorCoords, sat: f32, val: f32) -> bool {
    let before = *coords;
    coords.set_saturation(sat);
    coords.set_value(val);
    *coords != before
}

fn keyboard_travel(ui: &mut Ui, coords: &mut ColorCoords) -> bool {
    let sat = ACROSS.travel(ui, coords.saturation());
    let val = UP.travel(ui, coords.value());
    write_axes(coords, sat.to, val.to)
}

// Every texel shares the hue, so its gamut solve belongs outside the loop.
fn fill(image: &mut Image, model: ColorModel, hue: f32) {
    let slice = model.slice(hue);
    let size = image.size();
    image.fill_with(|column, row| {
        let sat = (column as f32 + 0.5) / size.x as f32;
        let val = 1.0 - (row as f32 + 0.5) / size.y as f32;
        slice.color(sat, val).into()
    });
}

#[cfg(feature = "bench")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
