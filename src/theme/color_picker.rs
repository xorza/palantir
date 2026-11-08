//! What a color picker wears: its surfaces, the handle, and the checker
//! behind translucent colors.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::text::font_family::FontFamily;
use crate::theme::button::ButtonTheme;
use crate::theme::drag_value::DragValueTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::popup::PopupTheme;
use crate::theme::text_edit::TextEditTheme;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Visuals and geometry for [`crate::ColorPicker`] and its parts
/// ([`crate::ColorField`], [`crate::ColorStrip`], [`crate::ColorSwatch`],
/// [`crate::ColorButton`]); one bundle because they are one control.
///
/// The field and bars are sized here, not by layout: they paint a CPU-built
/// texture whose size must be known at record time.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct ColorPickerTheme {
    /// Saturation/value field width in logical px; also the bars' and rows' width.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub field_width: f32,
    /// Saturation/value field height in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub field_height: f32,
    /// Hue and alpha bar height in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub bar_thickness: f32,
    /// Side of the preview chip beside the bars, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub chip_size: f32,
    /// Side of one swatch in the preset row, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub swatch_size: f32,
    /// Radius of the ring marking the field's position, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub handle_radius: f32,
    /// Stroke width of each of the handle's two rings, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub handle_width: f32,
    /// Outer ring of every handle. Dark and not a palette color: a handle sits
    /// over every color the field shows.
    pub handle_outer: RgbaF32,
    /// Inner ring of every handle.
    pub handle_inner: RgbaF32,
    /// Light square of the checker behind a translucent color.
    pub checker_light: RgbaF32,
    /// Dark square of the same checker.
    pub checker_dark: RgbaF32,
    /// Side of one checker square in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::positive")]
    pub checker_cell: f32,
    /// What a [`crate::ColorButton`], a [`crate::ColorSwatch`] and the preview chip wear in each
    /// state, as a button does: the look's chrome frames the color, which fills the chip inside
    /// the chrome's border, so the hairline lets white read against a light panel.
    pub chip: ButtonTheme,
    /// Laid over the colors of a disabled field, bar, chip or swatch, whose colors are data no
    /// look can dim: the panel's color at half alpha, 128/255 on the grid a saved color keeps,
    /// reads as libadwaita's `filter: opacity(0.5)` does over that panel
    /// (<https://gitlab.gnome.org/GNOME/libadwaita/-/merge_requests/307>).
    pub disabled_veil: RgbaF32,
    /// Gap between the panel's rows and between swatches, in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// The popup a [`crate::ColorButton`] drops its panel in: [`Palette::popup_panel`], as for
    /// menus and combo lists, padded wider than [`Self::gap`] so the panel reads as set in a
    /// card.
    pub popup: PopupTheme,
    /// What the channel values wear: [`Theme::drag_value`](crate::Theme) in the
    /// bundled monospace face, so digits don't shuffle as a number changes length
    /// (the panel also pins their boxes to one width).
    ///
    /// Built from the stock bundle at [`Self::from_parts`], so restyling
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

/// Put every state's text in the monospace face at the caption tier, for a dense grid of
/// readouts: four columns, each (208 − 3 × 8) / 4 = 46 px wide at the stock steps.
fn mono_states(looks: &mut StatefulLook, m: &Metrics) {
    for look in looks.each_mut() {
        look.text.family = Some(FontFamily::MONO);
        look.text.font_size = Some(m.text.caption);
    }
}

/// The inset of a value's box and of the hex field.
fn value_padding(m: &Metrics) -> Spacing {
    Spacing::all(m.space.xs)
}

/// A chip's frame: a hairline on no fill, `element_strong` at rest, brighter under the pointer,
/// the focus color pressed, and back to `element` disabled. It shows no selected state, so both
/// packs are one.
fn chip(p: &Palette, m: &Metrics) -> ButtonTheme {
    let ring = |color: RgbaF32| WidgetLook {
        chrome: Chrome::NONE.with_border(Stroke::new(color, m.stroke.hairline)),
        text: TextStyleOverrides::NONE,
    };
    let looks = StatefulLook {
        normal: ring(p.element_strong),
        hovered: ring(p.text_muted),
        active: ring(p.border_focused),
        disabled: ring(p.element),
    };
    ButtonTheme {
        unselected: looks.clone(),
        selected: looks,
        defaults: SlotDefaults {
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            transition: m.transition,
        },
    }
}

fn mono_edit(p: &Palette, m: &Metrics) -> TextEditTheme {
    let mut edit = TextEditTheme::from_parts(p, m);
    mono_states(&mut edit.looks, m);
    edit.defaults.padding = value_padding(m);
    edit
}

impl ColorPickerTheme {
    /// The control's own sizes and the handle's ink are fixed; its lines and popup geometry come from `m`, its checker, border, popup fill and text from `p`.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            field_width: 208.0,
            field_height: 160.0,
            bar_thickness: 14.0,
            chip_size: 38.0,
            swatch_size: 18.0,
            handle_radius: 6.0,
            handle_width: m.stroke.fine,
            handle_outer: RgbaF32::BLACK.with_alpha8(191),
            handle_inner: RgbaF32::WHITE.with_alpha8(242),
            checker_light: p.element_mid,
            checker_dark: p.element,
            checker_cell: 6.0,
            chip: chip(p, m),
            disabled_veil: p.element.with_alpha8(128),
            gap: m.space.sm,
            // Held for the muted ink the channel names below read in.
            popup: PopupTheme::from_parts(p, m),
            // One set of looks for the chip and its editor, so an editable value keeps its box
            // and text in place.
            value: {
                let mut value = DragValueTheme::from_parts(p, m);
                mono_states(&mut value.looks, m);
                value.defaults.padding = value_padding(m);
                value
            },
            hex: mono_edit(p, m),
            label: TextStyleOverrides {
                family: Some(FontFamily::MONO),
                ..TextStyleOverrides::NONE
                    .with_color(p.text_muted)
                    .with_font_size(m.text.caption)
            },
        }
    }
}
