//! What a drag value wears: the scrub chip and the text field it becomes while typed into.

use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_edit::TextEditTheme;

/// Theme for [`crate::DragValue`]: scrub `chip` ([`ButtonTheme`]) and inline `editor` ([`TextEditTheme`]) used under [`crate::DragValue::editable`]. Built from one source via [`Self::from_chip`], so both modes share a box size and edit mode doesn't resize or restyle.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DragValueTheme {
    /// Chrome for the scrub chip: the DragValue-specific `ButtonTheme` slot (`Button`/`ComboBox` use `Theme::button`).
    pub chip: ButtonTheme,
    /// Chrome for the inline editor; box mirrors `chip`, caret / selection come from the text-edit look.
    pub editor: TextEditTheme,
}

impl DragValueTheme {
    /// Derive from a `chip`: the editor inherits its box for pixel-identical modes, caret / selection / placeholder come from `text_edit`. The editor's `active` (focused) maps to the chip's `hovered` look, since the pointer that clicked it is already over it.
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

    /// Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self { chip, editor } = self;
        chip.for_each_text(f);
        editor.for_each_text(f);
    }

    /// One palette drives both: the chip from the button recipe, the editor via [`Self::from_chip`].
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
