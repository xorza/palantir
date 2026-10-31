//! A page's header, and the chips it and a section's title row wear.

use palantir::{
    Align, Chrome, Configure, Corners, Metrics, Panel, RgbaF32, Sizing, Stroke, Text, TextWrap, Ui,
    VAlign,
};

use crate::scaffold::tokens::{MEASURE, Tokens};

const M: Metrics = Metrics::DEFAULT;

/// What a page's header says: what it is, the keys it answers to, the API it is built from, the
/// other example's pages that show its pieces, and where its source is. A row with no items is
/// left out.
#[derive(Debug)]
pub(crate) struct Header<'a, R> {
    pub(crate) title: &'static str,
    pub(crate) blurb: &'static str,
    pub(crate) keys: &'static [&'static str],
    /// `api!` spellings, which a `const` cannot hold, so the header borrows them.
    pub(crate) apis: &'a [&'static str],
    /// The other example's name, which labels [`Self::related`].
    pub(crate) related_label: &'static str,
    /// The other example's pages that show this one's pieces, read from
    /// [`LINKS`](crate::scaffold::links::LINKS).
    pub(crate) related: R,
    pub(crate) source: &'static str,
}

impl<R: IntoIterator<Item = &'static str>> Header<'_, R> {
    pub(crate) fn show(self, ui: &mut Ui, t: Tokens) {
        Panel::vstack()
            .id_salt(("header", self.title))
            .size((Sizing::FILL, Sizing::HUG))
            .gap(M.space.sm)
            .show(ui, |ui| {
                Text::new(self.title).style(&t.title()).show(ui);
                Text::new(self.blurb)
                    .style(&t.dim())
                    .size((Sizing::FILL, Sizing::HUG))
                    .max_size((MEASURE, f32::INFINITY))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
                chips(ui, t, "keys", self.keys.iter().copied(), Chip::Key);
                chips(ui, t, "built from", self.apis.iter().copied(), Chip::Tag);
                chips(ui, t, self.related_label, self.related, Chip::Tag);
                Text::new(self.source)
                    .id_salt("source")
                    .style(&t.mono(t.ink_faint))
                    .show(ui);
            });
    }
}

/// A labeled row of `kind` chips; nothing for an empty list.
fn chips(
    ui: &mut Ui,
    t: Tokens,
    label: &'static str,
    items: impl IntoIterator<Item = &'static str>,
    kind: Chip,
) {
    let mut items = items.into_iter().peekable();
    if items.peek().is_none() {
        return;
    }
    Panel::wrap_hstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(M.space.xs)
        .line_gap(M.space.xs)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            Text::new(label)
                .style(&t.caption())
                .margin((0.0, 0.0, M.space.xxs, 0.0))
                .show(ui);
            for item in items {
                kind.show(ui, t, item);
            }
        });
}

/// What a chip names.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Chip {
    /// A key or gesture: a keycap, filled and edged, in the ink.
    Key,
    /// An API or a page: edged only, in the dim ink, quieter than the content it labels.
    Tag,
}

impl Chip {
    /// One chip of `text`, keyed by its text.
    pub(crate) fn show(self, ui: &mut Ui, t: Tokens, text: &'static str) {
        let corners = Corners::all(M.radius.control);
        let (chrome, ink) = match self {
            Self::Key => (
                Chrome::rounded(t.raised, corners)
                    .with_border(Stroke::new(t.border, M.stroke.hairline)),
                t.ink,
            ),
            Self::Tag => (
                Chrome::rounded(RgbaF32::TRANSPARENT, corners)
                    .with_border(Stroke::new(t.hairline, M.stroke.hairline)),
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
