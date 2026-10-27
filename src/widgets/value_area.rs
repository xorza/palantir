//! The shared scaffold behind the color picker's value areas: the field and the strips.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::image::{Image, ImageFit};
use crate::shape::Shape;
use crate::theme::color_picker::ColorPickerTheme;
use crate::ui::Ui;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::axis_keys::AxisKeys;
use crate::widgets::color_surface;
use crate::widgets::color_surface::ColorSurface;
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
    /// The size the theme gives the area.
    fn themed(theme: &ColorPickerTheme) -> Size;
    /// Writes the value under a press at `at`, a fraction of the area from its top left, and
    /// reports whether it moved.
    fn press(&mut self, at: Vec2) -> bool;
    /// Moves the value by this frame's keys, read through `widget`, and reports whether it moved.
    fn travel(&mut self, widget: &mut Widget, ui: &mut Ui) -> bool;
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
    /// release; a key writes and commits at once.
    pub(crate) fn show<V: AreaValue>(self, ui: &mut Ui, mut value: V) -> ValueResponse<'_> {
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

        let pressed = response
            .press_fraction(0.0)
            .is_some_and(|at| value.press(at));
        let keyed = !response.disabled && ui.is_focus_within(id) && value.travel(&mut widget, ui);
        let committed = !response.disabled && (response.left.released() || keyed);

        let texels = color_surface::texture_size(size, texel_size, ui);
        let surface = value.surface();
        widget.record(ui, None, |ui| {
            value.underlay(ui, theme, size);
            let image = ui.with_state::<ColorSurface<V::Surface>, _>(id, |ui, built| {
                built
                    .ensure(ui, texels, surface, |image| V::fill(surface, image))
                    .clone()
            });
            ui.add_shape(Shape::image(image).fit(ImageFit::Fill));
            value.marker(ui, theme, size);
        });
        ValueResponse {
            response: Response::new(ui, response),
            changed: pressed | keyed,
            committed,
        }
    }
}
