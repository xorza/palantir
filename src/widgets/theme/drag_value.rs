use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::StatefulLook;

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
            padding: chip.padding,
            margin: chip.margin,
            anim: chip.anim,
            caret: text_edit.caret,
            caret_width: text_edit.caret_width,
            selection: text_edit.selection,
            placeholder: text_edit.placeholder,
        };
        Self { chip, editor }
    }

    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.chip.for_each_text(f);
        self.editor.for_each_text(f);
    }
}

impl Default for DragValueTheme {
    fn default() -> Self {
        Self::from_chip(ButtonTheme::default(), &TextEditTheme::default())
    }
}
