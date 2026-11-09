//! The chips a page's header and a section's title row wear.

use palantir::{Border, Chrome, Configure, Corners, Panel, RgbaF32, Text, Ui};

use crate::scaffold::tokens::M;
use crate::scaffold::tokens::Tokens;

/// What a chip names.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Chip {
    /// A key or gesture: a keycap, filled and edged, in the ink.
    Key,
    /// An API: edged only, in the dim ink, quieter than the content it labels.
    Tag,
}

impl Chip {
    /// One chip of `text`, keyed by its text.
    pub(crate) fn show(self, ui: &mut Ui, t: Tokens, text: &'static str) {
        let corners = Corners::all(M.radius.control);
        let (chrome, ink) = match self {
            Self::Key => (
                Chrome::rounded(t.raised, corners)
                    .with_border(Border::new(t.border, M.stroke.hairline)),
                t.ink,
            ),
            Self::Tag => (
                Chrome::rounded(RgbaF32::TRANSPARENT, corners)
                    .with_border(Border::new(t.hairline, M.stroke.hairline)),
                t.ink_dim,
            ),
        };
        Panel::hstack()
            .id_salt(text)
            .padding((M.space.sm, M.space.xxs))
            .chrome(chrome)
            .show(ui, |ui| {
                Text::new(text).style(&t.mono(ink)).show(ui);
            });
    }
}
