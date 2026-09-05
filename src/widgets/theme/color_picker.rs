//! What a colour picker wears: the surfaces it paints, the handle that rides
//! them, and the checker behind anything translucent.

use crate::primitives::color::RgbaF32;
use crate::primitives::spacing::Spacing;
use crate::text::font_family::FontFamily;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;

/// Visuals and geometry for [`crate::ColorPicker`] and the four widgets it
/// arranges — [`crate::ColorField`], [`crate::ColorStrip`],
/// [`crate::ColorSwatch`] and [`crate::ColorButton`]. One bundle, because the
/// five are one control and a field styled apart from its own hue bar would
/// only ever look broken.
///
/// The field and the bars are sized here rather than by the layout. Both
/// paint a CPU-built texture, and knowing the size at record time is what
/// lets the first frame paint the right one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ColorPickerTheme {
    /// Saturation/value field width in logical px. Also the width of the
    /// bars and of the panel's rows.
    pub field_width: f32,
    /// Saturation/value field height in logical px.
    pub field_height: f32,
    /// Hue and alpha bar height in logical px.
    pub bar_thickness: f32,
    /// Side of the preview chip beside the bars, in logical px.
    pub chip_size: f32,
    /// Side of one swatch in the preset row, in logical px.
    pub swatch_size: f32,
    /// Radius of the ring marking the field's position, in logical px.
    pub handle_radius: f32,
    /// Stroke width of each of the handle's two rings, in logical px.
    pub handle_width: f32,
    /// Outer ring of every handle. Dark, and **not** a palette colour: a
    /// handle sits on top of every colour the field can show, so one taken
    /// from the palette disappears over half of them.
    pub handle_outer: RgbaF32,
    /// Inner ring of every handle. Light, for the same reason.
    pub handle_inner: RgbaF32,
    /// Light square of the checker behind a translucent colour.
    pub checker_light: RgbaF32,
    /// Dark square of the same checker.
    pub checker_dark: RgbaF32,
    /// Side of one checker square in logical px.
    pub checker_cell: f32,
    /// Hairline around the chip and each swatch, so a white colour still
    /// reads as a shape against a light panel.
    pub border: RgbaF32,
    /// Width of that hairline in logical px.
    pub border_width: f32,
    /// Gap between the panel's rows and between swatches, in logical px.
    pub gap: f32,
    /// What the channel values wear: [`Theme::drag_value`](crate::Theme) in
    /// the bundled monospace face.
    ///
    /// Monospace because the values sit under a drag: every digit advances
    /// the same, so a number going from 99 to 100 does not shuffle the ones
    /// beside it. The panel pins their boxes to one width for the same
    /// reason, and the two together are what make a drag read as one number
    /// changing rather than a row rearranging itself.
    ///
    /// Built from the stock bundle at [`Self::from_palette`], so an app that
    /// restyles [`Theme::text`](crate::Theme) does not move these with it.
    /// Rebuild this field alongside it if that matters.
    pub value: DragValueTheme,
    /// What the hex field wears: [`Theme::text_edit`](crate::Theme) in the
    /// same face, for the same reason.
    pub hex: TextEditTheme,
    /// The caption over each channel value.
    ///
    /// Over rather than beside: a four-column row of a 208 px panel leaves
    /// about 35 px beside a label, which is two digits and a half. Above it,
    /// the number gets the whole column.
    pub label: TextStyle,
}

/// Font size the channel values and the hex field are set at.
///
/// Smaller than the ambient style, because the panel is only as wide as its
/// field: a quarter of 208 px is 47.5, and three digits of the 16 px default
/// plus a chip's padding do not fit it.
const VALUE_FONT_PX: f32 = 13.0;

/// Padding a value chip takes, so the column's width goes to the number.
const VALUE_PADDING: f32 = 5.0;

/// Put `look`'s text in the monospace face at the value size, keeping
/// whatever colour it already carries — or the ambient style's, where it
/// carries none.
fn mono_look(look: &mut WidgetLook, ambient: TextStyle) {
    let base = look.text.unwrap_or(ambient);
    look.text = Some(TextStyle {
        family: FontFamily::MONO,
        ..base.with_font_size(VALUE_FONT_PX)
    });
}

fn mono_states(looks: &mut StatefulLook, ambient: TextStyle) {
    for look in [
        &mut looks.normal,
        &mut looks.hovered,
        &mut looks.active,
        &mut looks.disabled,
    ] {
        mono_look(look, ambient);
    }
}

/// The style a look without one of its own inherits — what
/// `Theme::from_palette` builds as its `text`.
fn ambient(p: &Palette) -> TextStyle {
    TextStyle::default().with_color(p.text)
}

fn mono_edit(p: &Palette) -> TextEditTheme {
    let mut edit = TextEditTheme::from_palette(p);
    mono_states(&mut edit.looks, ambient(p));
    edit.defaults.padding = Spacing::xy(VALUE_PADDING, VALUE_PADDING);
    edit
}

impl ColorPickerTheme {
    /// Visit the two bundles that carry text — the channel values and the hex
    /// field. Destructures the whole struct so a new field has to be
    /// classified here before it compiles, which is the guarantee
    /// [`Theme::scale_text`](crate::Theme::scale_text) rides on.
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            value,
            hex,
            label,
            field_width: _,
            field_height: _,
            bar_thickness: _,
            chip_size: _,
            swatch_size: _,
            handle_radius: _,
            handle_width: _,
            handle_outer: _,
            handle_inner: _,
            checker_light: _,
            checker_dark: _,
            checker_cell: _,
            border: _,
            border_width: _,
            gap: _,
        } = self;
        value.for_each_text(f);
        hex.for_each_text(f);
        f(label);
    }

    pub fn from_palette(p: &Palette) -> Self {
        Self {
            field_width: 208.0,
            field_height: 160.0,
            bar_thickness: 14.0,
            chip_size: 38.0,
            swatch_size: 18.0,
            handle_radius: 6.0,
            handle_width: 1.5,
            handle_outer: RgbaF32::new(0.0, 0.0, 0.0, 0.75),
            handle_inner: RgbaF32::new(1.0, 1.0, 1.0, 0.95),
            checker_light: p.elem_mid,
            checker_dark: p.elem,
            checker_cell: 6.0,
            border: p.elem_strong,
            border_width: 1.0,
            gap: 6.0,
            value: {
                let mut value = DragValueTheme::from_palette(p);
                mono_states(&mut value.chip.looks, ambient(p));
                value.chip.defaults.padding = Spacing::xy(VALUE_PADDING, VALUE_PADDING);
                value.editor = mono_edit(p);
                value
            },
            hex: mono_edit(p),
            label: TextStyle {
                family: FontFamily::MONO,
                ..TextStyle::default()
                    .with_color(p.text_muted)
                    .with_font_size(10.0)
            },
        }
    }
}

impl Default for ColorPickerTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
