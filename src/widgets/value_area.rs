//! The shared scaffold behind the color picker's value areas: the field and the strips.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::size::Size;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::image::Image;
use crate::shape::Shape;
use crate::shape::image_paint::ImagePaint;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::configure::{Configure, ConfigureDefaults};
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::color_surface::ColorSurface;
use crate::widgets::{color_surface, press_start};
use glam::Vec2;
use std::rc::Rc;

/// What [`ColorField`](crate::ColorField) and [`ColorStrip`](crate::ColorStrip) share: a
/// focusable leaf that a press, a drag or the arrow keys write a value through, painted with a
/// texture of its value space. Each widget owns one and finishes through [`Self::show`] with its
/// [`AreaValue`].
#[derive(Debug)]
pub(crate) struct ValueArea<'a> {
    pub(crate) widget: Widget,
    /// The edge of one texture texel in physical pixels.
    pub(crate) texel_size: u32,
    pub(crate) style: Option<&'a ColorPickerTheme>,
}

/// What sets one value area apart: the value it writes, the keys that move it, the texture of
/// its value space and its marker.
pub(crate) trait AreaValue {
    /// Everything the texture fill reads, compared exactly; a change refills it.
    type Surface: Copy + PartialEq + 'static;
    /// The value the area writes, compared exactly; a release that leaves it where the press
    /// found it commits nothing.
    type Value: Copy + PartialEq + 'static;
    /// The size the theme gives the area.
    fn themed(theme: &ColorPickerTheme) -> Size;
    /// Writes the value under a press at `at`, a fraction of the area from its top left, and
    /// reports whether it moved.
    fn press(&mut self, at: Vec2) -> bool;
    /// Moves the value by this frame's keys, read through `widget`, and reports whether it moved.
    fn travel(&mut self, widget: &mut Widget, ui: &mut Ui) -> bool;
    /// The value now.
    fn value(&self) -> Self::Value;
    /// What the texture shows now.
    fn surface(&self) -> Self::Surface;
    /// Fills the texture for `surface`, sRGB-encoded.
    fn fill(surface: Self::Surface, image: &mut Image);
    /// Paints under the texture, over an area `size` large; nothing by default.
    fn underlay(&self, _ui: &mut Ui, _theme: &ColorPickerTheme, _size: Size) {}
    /// Paints the marker of the value over the texture.
    fn marker(&self, ui: &mut Ui, theme: &ColorPickerTheme, size: Size);
}

impl ValueArea<'_> {
    #[track_caller]
    pub(crate) fn new() -> Self {
        Self {
            widget: Widget::leaf()
                .sense(Sense::CLICK | Sense::DRAG)
                .focusable(true)
                .input_scope(AxisKeys::SCOPE),
            texel_size: color_surface::TEXEL_SIZE,
            style: None,
        }
    }

    /// Records the area for `value`. A press writes on every gesture frame and commits on the
    /// release that leaves the value apart from where the press found it; a key that moves it
    /// writes and commits at once.
    pub(crate) fn show<V: AreaValue>(self, ui: &mut Ui, mut value: V) -> ValueResponse {
        let Self {
            widget,
            texel_size,
            style,
        } = self;
        // An `Rc` bump, so the theme outlives the `&mut Ui` the record takes.
        let ui_theme = Rc::clone(ui.theme());
        let theme = style.unwrap_or(&ui_theme.color_picker);
        let themed = V::themed(theme);
        let mut widget = widget.default_size((Sizing::fixed(themed.w), Sizing::fixed(themed.h)));
        let response = widget.response(ui);
        let id = response.id;
        let size = response.layout_rect.map_or(themed, |r| r.size);

        let before = value.value();
        let pressed = response
            .press_fraction(0.0)
            .is_some_and(|at| value.press(at));
        let keyed = !response.disabled && ui.is_focus_within(id) && value.travel(&mut widget, ui);
        let release_moved = press_start::moved_since_press(ui, &response, before, value.value());

        let texels = color_surface::texture_size(size, texel_size, ui);
        let surface = value.surface();
        widget.record(ui, None, |ui| {
            value.underlay(ui, theme, size);
            let image = ui.with_state::<ColorSurface<V::Surface>, _>(id, |ui, built| {
                built
                    .ensure(ui, texels, surface, |image| V::fill(surface, image))
                    .clone()
            });
            ui.add_shape(Shape::rect(), ImagePaint::new(image).fit(Fit::Fill));
            value.marker(ui, theme, size);
            if response.disabled {
                ui.add_shape(Shape::rect(), theme.disabled_veil);
            }
        });
        ValueResponse::gesture(response, pressed, keyed, release_moved)
    }
}

#[cfg(test)]
mod tests {
    use crate::host::harness::Harness;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::keyboard::key::Key;
    use crate::primitives::input::pointer::PointerButton;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::scene::layer::Layer;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widget_core::value_response::ValueResponse;
    use crate::widgets::color_field::ColorField;
    use crate::widgets::color_strip::ColorStrip;
    use glam::UVec2;

    /// One area of `surface` over `value`, shown by `show` (disabled when its flag is set) and
    /// read by `read`: Home moves the value to 0 and commits in that one frame, a second Home
    /// moves nothing and reports nothing, and a disabled area takes no press at its center and
    /// lays the veil over its colors, one shape past the enabled area's.
    fn check<T>(
        label: &str,
        surface: UVec2,
        mut value: T,
        show: fn(&mut Ui, &mut T, bool) -> ValueResponse,
        read: fn(&T) -> f32,
    ) {
        let id = WidgetId::from_hash("value-area");
        let mut h = Harness::mono(surface);
        let frame = |h: &mut Harness, value: &mut T, disabled: bool| {
            h.frame_value(|ui| show(ui, value, disabled))
        };
        frame(&mut h, &mut value, false);
        h.ui_mut().set_focus(id);
        frame(&mut h, &mut value, false);
        h.key(Key::Home);
        frame(&mut h, &mut value, false).assert_signals(true, true, &format!("{label}: Home"));
        assert_eq!(read(&value), 0.0, "{label}: Home");
        h.key(Key::Home);
        frame(&mut h, &mut value, false).assert_signals(
            false,
            false,
            &format!("{label}: a key that moves nothing"),
        );

        let shapes = |h: &Harness| {
            let node = h.node_of(id).expect("the area recorded").node;
            h.ui().tree(Layer::Main).shapes_of(node).count()
        };
        let enabled = shapes(&h);
        let before = read(&value);
        h.press(surface.as_vec2() / 2.0, PointerButton::Left);
        frame(&mut h, &mut value, true).assert_signals(false, false, &format!("{label}: disabled"));
        assert_eq!(
            read(&value),
            before,
            "{label}: a disabled area writes nothing"
        );
        assert_eq!(shapes(&h), enabled + 1, "{label}: the veil");
    }

    /// Both values the color picker's areas write keep the area's protocol: the field's
    /// saturation and the alpha bar's alpha, each from 0.5.
    #[test]
    fn a_key_commits_at_once_and_a_disabled_area_takes_no_press() {
        let mut coords = ColorCoords::default();
        coords = coords.with_saturation(0.5);
        check(
            "field",
            UVec2::new(208, 160),
            coords,
            |ui, coords, disabled| {
                ColorField::new(coords)
                    .id(WidgetId::from_hash("value-area"))
                    .disabled(disabled)
                    .show(ui)
            },
            |coords| coords.saturation(),
        );
        check(
            "alpha bar",
            UVec2::new(208, 14),
            RgbaF32::hex(0x4cd3ff).with_alpha(0.5),
            |ui, color, disabled| {
                ColorStrip::alpha(color)
                    .id(WidgetId::from_hash("value-area"))
                    .disabled(disabled)
                    .show(ui)
            },
            |color| color.a,
        );
    }
}
