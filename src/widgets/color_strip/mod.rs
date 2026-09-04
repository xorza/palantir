//! The one-axis bars of a colour picker: hue, and alpha over its checker.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::image::Image;
use crate::primitives::image::ImageFit;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::value_response::ValueResponse;
use glam::Vec2;
use std::rc::Rc;

/// A one-axis bar of a colour picker: the hue ramp, or the alpha ramp of one
/// colour over its checker.
///
/// Both are exact per texel, for the reason [`ColorField`](crate::ColorField)
/// gives. The hue ramp especially: it runs along the sRGB gamut edge, which
/// turns a corner at each primary and secondary, and a gradient chording
/// across those corners misses by up to 73/255.
///
/// The alpha bar carries **straight alpha** in its texture and lets the GPU
/// composite it over the checker behind — the same blend the colour will get
/// wherever it is used, rather than a CPU imitation of it.
#[derive(Debug)]
pub struct ColorStrip<'a> {
    node: Node,
    kind: StripKind<'a>,
    downsample: u32,
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

const KEY_PAGE: f32 = 0.1;

impl<'a> ColorStrip<'a> {
    /// A hue bar driving `coords`, painted in that value's model.
    #[track_caller]
    pub fn hue(coords: &'a mut ColorCoords) -> Self {
        Self::new(StripKind::Hue(coords))
    }

    /// An alpha bar over `color`, showing that colour from transparent to
    /// opaque and writing its alpha.
    #[track_caller]
    pub fn alpha(color: &'a mut RgbaF32) -> Self {
        Self::new(StripKind::Alpha(color))
    }

    #[track_caller]
    fn new(kind: StripKind<'a>) -> Self {
        Self {
            node: Node::leaf()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            kind,
            downsample: ColorSurface::DOWNSAMPLE,
            style: None,
        }
    }

    /// How far below the display's resolution the texture is built, as a
    /// power of two. Default 4. See
    /// [`ColorField::downsample`](crate::ColorField::downsample).
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    pub fn downsample(mut self, n: u32) -> Self {
        self.downsample = ColorSurface::checked_downsample(n);
        self
    }

    style_setter!('a, ColorPickerTheme, color_picker);

    /// Record the bar and report what the gesture did to the value it writes.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        // The theme handle is cloned so the slot outlives the `&mut Ui` the
        // widget opening below takes: the checker reads it after that.
        let bundle = Rc::clone(ui.theme());
        let theme = self.slot(&bundle);
        let themed = Size::new(
            theme.field_width.themed_length(1.0),
            theme.bar_thickness.themed_length(1.0),
        );
        let handle_width = theme.handle_width.themed_length(0.0);
        let handle_outer = theme.handle_outer;
        let handle_inner = theme.handle_inner;

        let node = self
            .node
            .default_size((Sizing::fixed(themed.w), Sizing::fixed(themed.h)));
        let widget = ui.widget(node);
        let response = widget.response(ui);
        let id = widget.id();
        let size = response.layout_rect.map_or(themed, |r| r.size);
        let checker = Checkerboard::new(theme, response.layout_rect, themed);

        let mut kind = self.kind;
        let mut changed = false;
        let released = response.left.released();
        if !response.disabled
            && (response.pressed() || response.left.drag.dragging() || released)
            && let (Some(local), Some(rect)) = (response.pointer_local, response.layout_rect)
        {
            let at = local
                .x
                .band_fraction(rect.size.w, 0.0)
                .unit_fraction_or(0.0);
            changed |= kind.write(at);
        }
        let keyed = !response.disabled && ui.focus_within(id) && keyboard_travel(ui, &mut kind);
        changed |= keyed;
        let committed = !response.disabled && (released || keyed);

        let texels = ColorSurface::texel_size(size, self.downsample, ui);
        let paint = kind.paint();
        let marker = kind.read() * size.w;

        widget.record(ui, None, |ui| {
            if paint.wants_checker() {
                checker.paint(ui);
            }
            let image = ui.with_state::<ColorSurface, _>(id.with("surface"), |ui, surface| {
                surface
                    .ensure(ui, texels, paint, |image| paint.fill(image))
                    .clone()
            });
            ui.add_shape(Shape::image(image).fit(ImageFit::Fill));
            let top = Vec2::new(marker, 0.0);
            let bottom = Vec2::new(marker, size.h);
            ui.add_shape(Shape::line(top, bottom, handle_width * 2.5).brush(handle_outer));
            ui.add_shape(Shape::line(top, bottom, handle_width).brush(handle_inner));
        });
        ValueResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl_configure!(ColorStrip<'_>);

impl StripKind<'_> {
    fn read(&self) -> f32 {
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

    fn paint(&self) -> StripPaint {
        match self {
            Self::Hue(coords) => StripPaint::Hue(coords.model()),
            Self::Alpha(color) => StripPaint::Alpha(color.with_alpha(1.0)),
        }
    }
}

/// What one bar's texture shows, and everything its fill reads — so, hashed,
/// the rebuild key: a hue bar follows its model, an alpha bar its colour.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum StripPaint {
    Hue(ColorModel),
    Alpha(RgbaF32),
}

impl StripPaint {
    fn wants_checker(self) -> bool {
        matches!(self, Self::Alpha(_))
    }

    // Both ramps vary along one axis; reuse each column conversion for every row.
    pub(crate) fn fill(self, image: &mut Image) {
        let width = image.size().x;
        for (column, texel) in image.row_mut(0).iter_mut().enumerate() {
            let along = (column as f32 + 0.5) / width as f32;
            *texel = match self {
                Self::Hue(model) => model.slice(along).color(1.0, 1.0).to_srgba_u8(),
                Self::Alpha(color) => color.with_alpha(along).to_srgba_u8(),
            };
        }
        image.repeat_row(0);
    }
}

fn keyboard_travel(ui: &mut Ui, kind: &mut StripKind<'_>) -> bool {
    let along = AxisKeys {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    };
    let mut at = kind.read() + along.travel(ui);
    let home = ui.key_pressed(Shortcut::key(Key::Home));
    let end = ui.key_pressed(Shortcut::key(Key::End));
    let page_up = ui.key_pressed(Shortcut::key(Key::PageUp));
    let page_down = ui.key_pressed(Shortcut::key(Key::PageDown));
    if page_down {
        at -= KEY_PAGE;
    }
    if page_up {
        at += KEY_PAGE;
    }
    if home {
        at = 0.0;
    }
    if end {
        at = 1.0;
    }
    kind.write(at)
}

#[cfg(test)]
mod tests;
