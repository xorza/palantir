//! What a drag value wears: the scrub chip, and the text field it becomes
//! while it is being typed into.

use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;

/// Theme for [`crate::DragValue`]: the scrub `chip` (a [`ButtonTheme`]) and the
/// inline `editor` (a [`TextEditTheme`]) it swaps to under
/// [`crate::DragValue::editable`]. Bundling both — built from one source via
/// [`Self::from_chip`] — keeps them the same box size, so entering edit mode
/// doesn't resize or restyle the field, and lets the editor's caret / selection
/// match the app's other text fields.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DragValueTheme {
    /// Chrome for the scrub chip — the DragValue-specific `ButtonTheme`
    /// slot (`Button`/`ComboBox` default to `Theme::button` instead).
    pub chip: ButtonTheme,
    /// Chrome for the inline editor. Its box (padding / margin / backgrounds)
    /// mirrors `chip`; its caret / selection come from the app's text-edit look.
    pub editor: TextEditTheme,
}

impl DragValueTheme {
    /// Derive from a `chip` look: the editor inherits the chip's box (padding /
    /// margin / per-state backgrounds) so the two modes are pixel-identical,
    /// while caret / selection / placeholder come from `text_edit` so they match
    /// the app's other fields. The editor's `active` (= focused) maps to the
    /// chip's `hovered` look — the chip is already hovered under the pointer
    /// that clicked it.
    pub fn from_chip(chip: ButtonTheme, text_edit: &TextEditTheme) -> Self {
        let editor = TextEditTheme {
            looks: StatefulLook {
                normal: chip.looks.normal.clone(),
                hovered: chip.looks.hovered.clone(),
                active: chip.looks.hovered.clone(),
                disabled: chip.looks.disabled.clone(),
            },
            defaults: chip.defaults,
            caret: text_edit.caret,
            caret_width: text_edit.caret_width,
            selection: text_edit.selection,
            placeholder: text_edit.placeholder,
        };
        Self { chip, editor }
    }

    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self { chip, editor } = self;
        chip.for_each_text(f);
        editor.for_each_text(f);
    }

    /// One palette drives both halves — the chip from the standard
    /// button recipe, the editor derived from it via [`Self::from_chip`].
    pub fn from_palette(p: &Palette) -> Self {
        Self::from_chip(
            ButtonTheme::from_palette(p),
            &TextEditTheme::from_palette(p),
        )
    }
}

impl Default for DragValueTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
