//! What a colour picker wears: its surfaces, the handle, and the checker
//! behind translucent colours.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::text::font_family::FontFamily;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::drag_value::DragValueTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyleOverrides;

/// Visuals and geometry for [`crate::ColorPicker`] and its parts
/// ([`crate::ColorField`], [`crate::ColorStrip`], [`crate::ColorSwatch`],
/// [`crate::ColorButton`]); one bundle because they are one control.
///
/// The field and bars are sized here, not by layout: they paint a CPU-built
/// texture whose size must be known at record time.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ColorPickerTheme {
    /// Saturation/value field width in logical px; also the bars' and rows' width.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub field_width: f32,
    /// Saturation/value field height in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub field_height: f32,
    /// Hue and alpha bar height in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub bar_thickness: f32,
    /// Side of the preview chip beside the bars, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub chip_size: f32,
    /// Side of one swatch in the preset row, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub swatch_size: f32,
    /// Radius of the ring marking the field's position, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub handle_radius: f32,
    /// Stroke width of each of the handle's two rings, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub handle_width: f32,
    /// Outer ring of every handle. Dark and not a palette colour: a handle sits
    /// over every colour the field shows.
    pub handle_outer: RgbaF32,
    /// Inner ring of every handle.
    pub handle_inner: RgbaF32,
    /// Light square of the checker behind a translucent colour.
    pub checker_light: RgbaF32,
    /// Dark square of the same checker.
    pub checker_dark: RgbaF32,
    /// Side of one checker square in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::positive")]
    pub checker_cell: f32,
    /// Hairline around the chip and swatches, so white reads against a light panel.
    pub border: RgbaF32,
    /// Width of that hairline in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub border_width: f32,
    /// Gap between the panel's rows and between swatches, in logical px.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Chrome of the popup a [`crate::ColorButton`] drops its panel in:
    /// [`Palette::popup_panel`], as for menus and combo lists.
    pub popup: Background,
    /// Padding inside the popup chrome; wider than [`Self::gap`] so the panel reads
    /// as set in a card.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub popup_padding: Spacing,
    /// What the channel values wear: [`Theme::drag_value`](crate::Theme) in the
    /// bundled monospace face, so digits don't shuffle as a number changes length
    /// (the panel also pins their boxes to one width).
    ///
    /// Built from the stock bundle at [`Self::from_palette`], so restyling
    /// [`Theme::drag_value`](crate::Theme) doesn't move these. Only face and size
    /// are named; other axes follow [`Theme::text`](crate::Theme).
    pub value: DragValueTheme,
    /// What the hex field wears: [`Theme::text_edit`](crate::Theme) in the same face.
    pub hex: TextEditTheme,
    /// The caption over each channel value. Over rather than beside: a
    /// four-column row of a 208 px panel leaves about 35 px beside a label.
    ///
    /// Text axes set over [`Theme::text`](crate::Theme).
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub label: TextStyleOverrides,
}

/// Font size of the channel values and hex field. Smaller than ambient: each
/// of four columns is (208 - 3 gaps) / 4 = 47.5 px, too narrow for three digits
/// of the 16 px default plus padding.
const VALUE_FONT_PX: f32 = 13.0;

const VALUE_PADDING: f32 = 5.0;

/// Put every state's text in the monospace face at the value size.
fn mono_states(looks: &mut StatefulLook) {
    for look in [
        &mut looks.normal,
        &mut looks.hovered,
        &mut looks.active,
        &mut looks.disabled,
    ] {
        look.text.family = Some(FontFamily::MONO);
        look.text.font_size = Some(VALUE_FONT_PX);
    }
}

fn mono_edit(p: &Palette) -> TextEditTheme {
    let mut edit = TextEditTheme::from_palette(p);
    mono_states(&mut edit.looks);
    edit.defaults.padding = Spacing::xy(VALUE_PADDING, VALUE_PADDING);
    edit
}

impl ColorPickerTheme {
    /// Visit the two text-bearing bundles. Destructures the whole struct so a new
    /// field must be classified here, which [`Theme::scale_text`](crate::Theme::scale_text)
    /// relies on.
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
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
            popup: _,
            popup_padding: _,
        } = self;
        value.for_each_text(f);
        hex.for_each_text(f);
        f(ThemeText::Overrides(label));
    }

    /// Geometry is fixed; only handle and swatch colours come from `p`.
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
            checker_light: p.element_mid,
            checker_dark: p.element,
            checker_cell: 6.0,
            border: p.element_strong,
            border_width: 1.0,
            gap: 6.0,
            popup: p.popup_panel(),
            popup_padding: Spacing::all(8.0),
            // The editor derives from the chip, as `DragValueTheme` promises, so an
            // editable value keeps its box and text in place.
            value: {
                let mut chip = DragValueTheme::from_palette(p).chip;
                mono_states(&mut chip.looks);
                chip.defaults.padding = Spacing::xy(VALUE_PADDING, VALUE_PADDING);
                DragValueTheme::from_chip(chip, &TextEditTheme::from_palette(p))
            },
            hex: mono_edit(p),
            label: TextStyleOverrides {
                family: Some(FontFamily::MONO),
                ..TextStyleOverrides::NONE
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
