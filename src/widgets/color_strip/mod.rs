//! The one-axis bars of a colour picker: hue, and alpha over its checker.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx;
use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::image::ImageFit;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::scene::node::Node;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::checkerboard::Checkerboard;
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::value_response::ValueResponse;
use glam::{UVec2, Vec2};

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

/// Which axis a bar drives, and what it writes through to do it.
#[derive(Debug)]
enum StripKind<'a> {
    /// Hue needs the whole coordinate, not a bare `f32`: hue alone does not
    /// say which model to paint the ramp in.
    Hue(&'a mut ColorCoords),
    /// Alpha needs the whole colour: it reads three channels for the ramp and
    /// writes the fourth.
    Alpha(&'a mut RgbaF32),
}

/// What `Page up` / `Page down` move.
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
        let mut node = Node::leaf();
        node.flags.set_sense(Sense::CLICK | Sense::DRAG);
        node.flags.set_focusable(true);
        Self {
            node,
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
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        let theme = self.slot(ui.theme());
        let themed = Size::new(
            theme.field_width.themed_length(1.0),
            theme.bar_thickness.themed_length(1.0),
        );
        let handle_width = theme.handle_width.themed_length(0.0);
        let handle_outer = theme.handle_outer;
        let handle_inner = theme.handle_inner;
        let size = response.layout_rect.map_or(themed, |r| r.size);
        let checker = Checkerboard::new(theme, response.layout_rect, themed);

        let mut kind = self.kind;
        let mut changed = false;
        let released = response.left.released();
        if !response.disabled
            && (response.pressed() || response.left.drag.dragging() || released)
            && let (Some(local), Some(rect)) = (response.pointer_local, response.layout_rect)
        {
            let at = approx::ratio(local.x, rect.size.w).unit_fraction_or(0.0);
            changed |= kind.write(at);
        }
        let keyed = !response.disabled && ui.focus_within(id) && keyboard_travel(ui, &mut kind);
        changed |= keyed;
        let committed = !response.disabled && (released || keyed);

        let texels = ColorSurface::texel_size(size, self.downsample, ui);
        let paint = kind.paint();
        let marker = kind.read() * size.w;

        let node = &mut widget.node;
        node.size
            .get_or_insert((Sizing::fixed(themed.w), Sizing::fixed(themed.h)).into());
        widget.record(ui, None, |ui| {
            if paint.wants_checker() {
                checker.paint(ui);
            }
            let image = ui.with_state::<ColorSurface, _>(id.with("surface"), |ui, surface| {
                surface
                    .ensure(ui, texels, paint.stamp(), |texels, size| {
                        paint.fill(texels, size);
                    })
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
    /// Where along the bar the value currently sits, `0..1`.
    fn read(&self) -> f32 {
        match self {
            Self::Hue(coords) => coords.hue(),
            Self::Alpha(color) => color.a,
        }
    }

    /// Move the value to `at`, reporting whether it actually moved.
    fn write(&mut self, at: f32) -> bool {
        let before = self.read();
        match self {
            Self::Hue(coords) => coords.set_hue(at),
            Self::Alpha(color) => color.a = at.clamp(0.0, 1.0),
        }
        self.read() != before
    }

    /// What this bar's texture is built from.
    fn paint(&self) -> StripPaint {
        match self {
            Self::Hue(coords) => StripPaint::Hue(coords.model()),
            Self::Alpha(color) => StripPaint::Alpha(color.with_alpha(1.0)),
        }
    }
}

/// What one bar's texture shows, and everything its fill reads.
#[derive(Clone, Copy, Debug, Hash)]
pub(crate) enum StripPaint {
    Hue(ColorModel),
    Alpha(RgbaF32),
}

impl StripPaint {
    /// Whether this bar needs the checker drawn behind it.
    fn wants_checker(self) -> bool {
        matches!(self, Self::Alpha(_))
    }

    /// The rebuild key: a hue bar follows its model, an alpha bar its colour.
    fn stamp(self) -> u64 {
        ColorSurface::stamp(self)
    }

    /// Write the bar's texels: the three colour channels sRGB-encoded, the
    /// fourth straight alpha.
    ///
    /// Both ramps vary along one axis only, so one row is built and repeated.
    pub(crate) fn fill(self, texels: &mut Vec<u8>, size: UVec2) {
        for column in 0..size.x {
            let along = (column as f32 + 0.5) / size.x as f32;
            let (color, alpha) = match self {
                Self::Hue(model) => (model.slice(along).color(1.0, 1.0), 255),
                Self::Alpha(color) => (color, (along * 255.0).round() as u8),
            };
            let quantized = color.to_srgba_u8();
            texels.extend_from_slice(&[quantized.r, quantized.g, quantized.b, alpha]);
        }
        // Every row is the first one. Copying inside the buffer keeps the
        // rebuild allocation-free, which a drag needs.
        let row = size.x as usize * 4;
        for _ in 1..size.y {
            texels.extend_from_within(..row);
        }
    }
}

/// Key travel along the bar, only while it holds focus. `Home` / `End` run
/// to the ends, `Page up` / `Page down` move a tenth.
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
