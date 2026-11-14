//! What a drag value wears: one set of looks for the scrub chip and the text field it becomes
//! while typed into, and the field's caret, selection and placeholder.

use crate::primitives::input::interaction::response::Response;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::button::ButtonTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_edit::TextEditTheme;

/// Theme for [`crate::DragValue`]: one set of `looks` the scrub chip wears and the inline editor
/// under [`crate::DragValue::editable`] wears too, so editing neither resizes nor restyles the
/// field; the editor's caret, selection and placeholder beside them. The chip's `active` look is
/// pressed; the editor wears `hovered` while it is focused, as the pointer that clicked it is
/// already over it ([`Self::editor`]).
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct DragValueTheme {
    /// The four per-state looks (`active` = pressed on the chip; `[drag_value.looks.hovered]`).
    pub looks: StatefulLook,
    /// Spacing and transition spec — see [`SlotDefaults`].
    pub defaults: SlotDefaults,
    /// Ink for the editor's caret.
    pub caret: RgbaF32,
    /// Width of the editor's caret in logical px.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub caret_width: f32,
    /// The editor's selection wash, behind the selected glyphs.
    pub selection: RgbaF32,
    /// Ink for the placeholder an empty editor shows.
    pub placeholder: RgbaF32,
}

impl DragValueTheme {
    /// The button recipe's resting pack and defaults over `p` and `m`, with the text-edit recipe's
    /// caret, selection and placeholder.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let chip = ButtonTheme::from_parts(p, m);
        let field = TextEditTheme::from_parts(p, m);
        Self {
            looks: chip.unselected,
            defaults: chip.defaults,
            caret: field.caret,
            caret_width: field.caret_width,
            selection: field.selection,
            placeholder: field.placeholder,
        }
    }

    /// The text field the editor shows: these looks, with focus worn as hover, and the caret,
    /// selection and placeholder.
    pub fn editor(&self) -> TextEditTheme {
        TextEditTheme {
            looks: StatefulLook {
                normal: self.looks.normal.clone(),
                hovered: self.looks.hovered.clone(),
                active: self.looks.hovered.clone(),
                disabled: self.looks.disabled.clone(),
            },
            placeholder: self.placeholder,
            caret: self.caret,
            caret_width: self.caret_width,
            selection: self.selection,
            defaults: self.defaults,
        }
    }
}

impl ThemeSlot for DragValueTheme {
    type Pick = ();

    /// The state's look: `active` = pressed, as a button's.
    fn look(&self, response: &Response, (): ()) -> &WidgetLook {
        self.looks.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
