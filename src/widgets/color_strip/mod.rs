//! The one-axis bars of a colour picker: hue, and alpha over its checker.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
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
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_surface;
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use glam::Vec2;

/// A one-axis bar of a colour picker: the hue ramp, or the alpha ramp of one
/// colour over its checker.
///
/// Both are exact per texel, for the reason [`ColorField`](crate::ColorField)
/// gives. The hue ramp especially: it runs along the sRGB gamut edge, which
/// turns a corner at each primary and secondary, and a gradient chording
/// across those corners misses by up to 73/255.
///
/// The alpha bar writes **real alpha** into its image and lets the GPU
/// composite it over the checker behind — the same blend the colour will get
/// wherever it is used, rather than a CPU imitation of it.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ColorStrip<'a> {
    widget: Widget,
    kind: StripKind<'a>,
    texel_size: u32,
    style: Option<&'a ColorPickerTheme>,
}

#[derive(Debug)]
enum StripKind<'a> {
    /// Hue needs the whole coordinate, not a bare `f32`: hue alone does not
    /// say which model to paint the ramp in.
    Hue(&'a mut ColorCoords),
    /// Alpha needs the whole colour: it reads three channels for the ramp and
    /// writes the fourth.
    Alpha(&'a mut RgbaF32),
}

const ALONG: AxisKeys = AxisKeys {
    step: KeyPair {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    },
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
    /// A hue bar driving `coords`, painted in that value's model.
    #[track_caller]
    pub fn for_hue(coords: &'a mut ColorCoords) -> Self {
        Self::new(StripKind::Hue(coords))
    }

    /// An alpha bar over `color`, showing that colour from transparent to
    /// opaque and writing its alpha.
    ///
    /// Not `alpha`: that is a *setter* on the two colour widgets next door
    /// ([`ColorPicker::alpha`](crate::ColorPicker::alpha),
    /// [`ColorButton::alpha`](crate::ColorButton::alpha)), and one word
    /// cannot mean both a setter and a constructor.
    #[track_caller]
    pub fn for_alpha(color: &'a mut RgbaF32) -> Self {
        Self::new(StripKind::Alpha(color))
    }

    #[track_caller]
    fn new(kind: StripKind<'a>) -> Self {
        Self {
            widget: Widget::leaf()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            kind,
            texel_size: color_surface::TEXEL_SIZE,
            style: None,
        }
    }

    /// The edge of one texture texel, in physical pixels: how far below the
    /// display's resolution the texture is built, as a power of two.
    /// Default 4. See
    /// [`ColorField::texel_size`](crate::ColorField::texel_size).
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

    /// Record the bar and report what the gesture did to the value it writes.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        let theme = self.style.unwrap_or(&ui.theme().color_picker);
        let themed = Size::new(
            domain::length_at_least(theme.field_width, 1.0),
            domain::length_at_least(theme.bar_thickness, 1.0),
        );
        let handle_width = domain::length_at_least(theme.handle_width, 0.0);
        let handle_outer = theme.handle_outer;
        let handle_inner = theme.handle_inner;
        let checker = Checkerboard::new(theme);

        let mut widget = self
            .widget
            .default_size((Sizing::fixed(themed.w), Sizing::fixed(themed.h)));
        let response = widget.response(ui);
        let id = widget.resolve(ui);
        let size = response.layout_rect.map_or(themed, |r| r.size);

        let mut kind = self.kind;
        let mut changed = false;
        if let Some(at) = response.press_fraction(0.0) {
            changed |= kind.write(at.x);
        }
        let keyed = !response.disabled && ui.is_focus_within(id) && keyboard_travel(ui, &mut kind);
        changed |= keyed;
        let committed = !response.disabled && (response.left.released() || keyed);

        let texels = color_surface::texture_size(size, self.texel_size, ui);
        let paint = kind.paint();
        let marker = kind.read() * size.w;

        widget.record(ui, None, |ui| {
            if paint.wants_checker() {
                checker.paint(ui, size);
            }
            let image =
                ui.with_state::<ColorSurface<StripPaint>, _>(id.with("surface"), |ui, surface| {
                    surface
                        .ensure(ui, texels, paint, |image| paint.fill(image))
                        .clone()
                });
            ui.add_shape(Shape::image(image).fit(ImageFit::Fill));
            let top = Vec2::new(marker, 0.0);
            let bottom = Vec2::new(marker, size.h);
            ui.add_shape(Shape::line(
                top,
                bottom,
                Stroke::new(handle_outer, handle_width * 2.5),
            ));
            ui.add_shape(Shape::line(
                top,
                bottom,
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

impl Configure for ColorStrip<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
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

/// What one bar's texture shows, and everything its fill reads — so the
/// rebuild key: a hue bar follows its model, an alpha bar its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StripPaint {
    Hue(ColorModel),
    Alpha(RgbaF32),
}

impl StripPaint {
    const fn wants_checker(self) -> bool {
        matches!(self, Self::Alpha(_))
    }

    // Both ramps vary along one axis; reuse each column conversion for every row.
    fn fill(self, image: &mut Image) {
        let width = image.size().x;
        for (column, texel) in image.row_mut(0).iter_mut().enumerate() {
            let along = (column as f32 + 0.5) / width as f32;
            *texel = match self {
                Self::Hue(model) => model.slice(along).color(1.0, 1.0).into(),
                Self::Alpha(color) => color.with_alpha(along).into(),
            };
        }
        image.repeat_row(0);
    }
}

fn keyboard_travel(ui: &mut Ui, kind: &mut StripKind<'_>) -> bool {
    let travel = ALONG.travel(ui, kind.read());
    let mut at = travel.to;
    if !travel.jumped && matches!(kind, StripKind::Hue(_)) && !(0.0..=1.0).contains(&at) {
        // Steps go round the hue circle; positions do not. A step past an
        // end wraps here, and every write — a drag to the edge, Home, End —
        // clamps in `ColorCoords::set_hue`.
        at = at.rem_euclid(1.0);
    }
    kind.write(at)
}

#[cfg(test)]
mod tests;
