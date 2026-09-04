//! The saturation/value area: the two-axis part of a colour picker, and the
//! texture it paints itself with.

use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
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
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::response::Response;
use crate::widgets::theme::color_picker::ColorPickerTheme;
use crate::widgets::value_response::ValueResponse;
use glam::Vec2;

/// The two-axis area of a colour picker: saturation left to right, value
/// bottom to top, at whatever hue the bound coordinates carry.
///
/// Exact per texel. The field builds a CPU texture and refreshes it in place
/// whenever the hue or the model moves, at a resolution
/// [`downsample`](Self::downsample) below the display's, which the sampler
/// then smooths back out. A gradient stack cannot draw this — it interpolates
/// in linear light, which is neither model's geometry — and a vertex-coloured
/// mesh pays eight *linear* bits, which crushes the darks.
///
/// Sized from [`ColorPickerTheme`], and returns the same
/// [`ValueResponse`] every other value-writing widget does.
#[derive(Debug)]
pub struct ColorField<'a> {
    node: Node,
    coords: &'a mut ColorCoords,
    downsample: u32,
    style: Option<&'a ColorPickerTheme>,
}

impl<'a> ColorField<'a> {
    /// A field driving `coords`. The hue it paints and the axes it writes are
    /// both that value's.
    #[track_caller]
    pub fn new(coords: &'a mut ColorCoords) -> Self {
        Self {
            node: Node::leaf()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true),
            coords,
            downsample: ColorSurface::DOWNSAMPLE,
            style: None,
        }
    }

    /// How far below the display's resolution the texture is built, as a
    /// power of two. Default 4.
    ///
    /// Worst error against the exact colour, in 8-bit sRGB units, over a
    /// 208 × 160 field at display scale 1.5 and twelve hues — measured by
    /// `tests::downsample_four_tracks_the_exact_colour`:
    ///
    /// | divisor | Okhsv | HSV | texels to convert |
    /// |---|---|---|---|
    /// | 1 | 0 | 0 | 74 880 |
    /// | 2 | 4 | 1 | 18 720 |
    /// | **4** | **9** | **3** | **4 680** |
    /// | 8 | 16 | 6 | 1 170 |
    /// | 16 | 25 | 14 | 293 |
    ///
    /// The error is not spread over the field. It sits at `s = 1, v = 1`,
    /// the corner where the gamut edge turns, and falls away from it. Four
    /// costs a sixteenth of the conversions for an error nobody reads a
    /// picker precisely enough to see; a caller that disagrees passes 2.
    ///
    /// # Panics
    ///
    /// Panics unless `n` is a power of two from 1 to 16.
    pub fn downsample(mut self, n: u32) -> Self {
        self.downsample = ColorSurface::checked_downsample(n);
        self
    }

    style_setter!('a, ColorPickerTheme, color_picker);

    /// Record the field and report what the gesture did to the coordinates.
    pub fn show(self, ui: &mut Ui) -> ValueResponse<'_> {
        let theme = self.slot(ui.theme());
        let themed = Size::new(
            theme.field_width.themed_length(1.0),
            theme.field_height.themed_length(1.0),
        );
        let handle_radius = theme.handle_radius.themed_length(1.0);
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

        let coords = self.coords;
        let mut changed = false;
        let released = response.left.released();
        if !response.disabled
            && (response.pressed() || response.left.drag.dragging() || released)
            && let (Some(local), Some(rect)) = (response.pointer_local, response.layout_rect)
        {
            let unit = |at: f32, extent: f32| at.band_fraction(extent, 0.0).unit_fraction_or(0.0);
            let sat = unit(local.x, rect.size.w);
            let val = 1.0 - unit(local.y, rect.size.h);
            changed |= write_axes(coords, sat, val);
        }
        let keyed = !response.disabled && ui.focus_within(id) && keyboard_travel(ui, coords);
        changed |= keyed;
        let committed = !response.disabled && (released || keyed);

        let texels = ColorSurface::texel_size(size, self.downsample, ui);
        let model = coords.model();
        let hue = coords.hue();
        let marker = Vec2::new(coords.sat() * size.w, (1.0 - coords.val()) * size.h);

        widget.record(ui, None, |ui| {
            let image = ui.with_state::<ColorSurface, _>(id.with("surface"), |ui, surface| {
                surface
                    .ensure(ui, texels, (model, hue.to_bits()), |image| {
                        fill(image, model, hue);
                    })
                    .clone()
            });
            ui.add_shape(Shape::image(image).fit(ImageFit::Fill));
            ui.add_shape(Shape::circle(marker, handle_radius, handle_width).brush(handle_outer));
            ui.add_shape(
                Shape::circle(marker, handle_radius - handle_width, handle_width)
                    .brush(handle_inner),
            );
        });
        ValueResponse {
            response: Response::eager(id, ui, response),
            changed,
            committed,
        }
    }
}

impl_configure!(ColorField<'_>);

fn write_axes(coords: &mut ColorCoords, sat: f32, val: f32) -> bool {
    let before = *coords;
    coords.set_sat(sat);
    coords.set_val(val);
    *coords != before
}

fn keyboard_travel(ui: &mut Ui, coords: &mut ColorCoords) -> bool {
    let across = AxisKeys {
        back: Key::ArrowLeft,
        forward: Key::ArrowRight,
    };
    let up = AxisKeys {
        back: Key::ArrowDown,
        forward: Key::ArrowUp,
    };
    let mut sat = coords.sat() + across.travel(ui);
    let mut val = coords.val() + up.travel(ui);
    let home = ui.key_pressed(Shortcut::key(Key::Home));
    let end = ui.key_pressed(Shortcut::key(Key::End));
    let page_up = ui.key_pressed(Shortcut::key(Key::PageUp));
    let page_down = ui.key_pressed(Shortcut::key(Key::PageDown));
    if home {
        sat = 0.0;
    }
    if end {
        sat = 1.0;
    }
    if page_down {
        val = 0.0;
    }
    if page_up {
        val = 1.0;
    }
    write_axes(coords, sat, val)
}

// Every texel shares the hue, so its gamut solve belongs outside the loop.
fn fill(image: &mut Image, model: ColorModel, hue: f32) {
    let slice = model.slice(hue);
    let size = image.size();
    image.fill_with(|column, row| {
        let sat = (column as f32 + 0.5) / size.x as f32;
        let val = 1.0 - (row as f32 + 0.5) / size.y as f32;
        slice.color(sat, val).to_srgba_u8()
    });
}

#[cfg(feature = "bench")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
