//! One recorded text run as the layout pass reads it.

use crate::primitives::layout::align::HAlign;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::shape::record::ShapeRecord;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;

/// One `ShapeRecord::Text`'s layout-side inputs, from [`Self::on_leaf`] and [`Self::on_container`].
#[derive(Debug)]
pub(crate) struct TextShapeInput<'a> {
    pub(crate) ordinal: u16,
    pub(crate) text: &'a str,
    /// Content hash kept on the [`RecordedText`] so [`Self::shape_request`] need not rescan the bytes.
    ///
    /// [`RecordedText`]: crate::primitives::text::recorded_text::RecordedText
    pub(crate) text_hash: u64,
    pub(crate) font: GlyphFont,
    pub(crate) wrap: TextWrap,
    /// From `Shape::Text.align`; with wrap on, cosmic-text bakes per-line offsets into the shaped buffer,
    /// so it must reach `TextSystem::measure` and `TextShapeKey`.
    pub(crate) halign: HAlign,
}

impl<'a> TextShapeInput<'a> {
    /// `TextShape::is_noop` drops empty runs before recording, so the boundary is asserted, not handled.
    pub(crate) fn shape_request(&self) -> TextShapeRequest<'a> {
        TextShapeRequest::for_key(
            self.text,
            TextShapeKey::unbounded(self.text_hash, self.font),
        )
        .expect("a recorded text run has bytes — `TextShape::is_noop` drops the empty one")
    }

    /// Every text shape on a leaf: the one walk behind `MeasureOp::leaf` and `intrinsic::leaf`.
    pub(crate) fn on_leaf(
        tree: &'a Tree,
        interned_text: &'a InternedText<'_>,
        node: NodeId,
    ) -> impl Iterator<Item = TextShapeInput<'a>> {
        debug_assert_eq!(
            tree.subtree_end_of(node.idx()),
            node.idx() + 1,
            "TextShapeInput::on_leaf called on non-leaf node {node:?}",
        );
        let span = tree.records.shape_span()[node.idx()];
        let lo = span.start as usize;
        let hi = lo + span.len as usize;
        text_shape_inputs(tree.shapes.records[lo..hi].iter(), interned_text)
    }

    pub(crate) fn on_container(
        tree: &'a Tree,
        interned_text: &'a InternedText<'_>,
        node: NodeId,
    ) -> impl Iterator<Item = TextShapeInput<'a>> {
        text_shape_inputs(
            tree.tree_items(node).filter_map(|item| match item {
                TreeItem::ShapeRecord(_, shape) => Some(shape),
                TreeItem::Child(_) => None,
            }),
            interned_text,
        )
    }
}

fn text_shape_inputs<'a>(
    shapes: impl Iterator<Item = &'a ShapeRecord> + 'a,
    interned_text: &'a InternedText<'_>,
) -> impl Iterator<Item = TextShapeInput<'a>> + 'a {
    let mut ordinal = 0;
    shapes.filter_map(move |shape| {
        let input = text_shape_input(shape, interned_text, ordinal)?;
        ordinal += 1;
        Some(input)
    })
}

fn text_shape_input<'a>(
    shape: &'a ShapeRecord,
    interned_text: &'a InternedText<'_>,
    ordinal: usize,
) -> Option<TextShapeInput<'a>> {
    match shape {
        ShapeRecord::Text {
            text,
            font,
            wrap,
            align,
            ..
        } => Some(TextShapeInput {
            ordinal: checked_text_ordinal(ordinal),
            text: interned_text.resolve(text.span),
            text_hash: text.hash,
            font: *font,
            wrap: *wrap,
            halign: align.halign(),
        }),
        _ => None,
    }
}

fn checked_text_ordinal(index: usize) -> u16 {
    u16::try_from(index).expect(
        "more than 65536 direct ShapeRecord::Text runs on one node; \
         widen the within-node ordinal width if this trips",
    )
}

#[cfg(test)]
mod tests {
    use crate::common::hash;
    use crate::internals::panic_probe;
    use crate::layout::text::text_shape_input::{TextShapeInput, checked_text_ordinal};
    use crate::primitives::layout::align::HAlign;
    use crate::text::font_family::FontFamily;
    use crate::text::font_slant::FontSlant;
    use crate::text::font_weight::FontWeight;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;
    use crate::text::wrap::TextWrap;

    #[test]
    fn text_ordinal_covers_the_u16_domain_and_rejects_the_next_run() {
        assert_eq!(checked_text_ordinal(0), 0);
        assert_eq!(checked_text_ordinal(usize::from(u16::MAX)), u16::MAX);
        panic_probe::assert_panics_with(
            "more than 65536 direct ShapeRecord::Text runs on one node",
            || checked_text_ordinal(usize::from(u16::MAX) + 1),
        );
    }

    const FACE: GlyphFont = GlyphFont {
        size: 16.0,
        line_height: 19.2,
        family: FontFamily::SANS,
        weight: FontWeight::REGULAR,
        slant: FontSlant::Normal,
    };

    fn input(text_hash: u64) -> TextShapeInput<'static> {
        TextShapeInput {
            ordinal: 0,
            text: "hello",
            text_hash,
            font: FACE,
            wrap: TextWrap::SingleLine,
            halign: HAlign::Auto,
        }
    }

    #[test]
    fn shape_request_reuses_the_recorded_hash() {
        let request = input(hash::hash_str("hello")).shape_request();
        assert_eq!(request.text(), "hello");
        assert_eq!(
            request.key(),
            TextShapeKey::unbounded(hash::hash_str("hello"), FACE),
            "the retained hash must mint the same key re-hashing would",
        );
    }

    /// A retained hash that no longer matches its bytes would replay another run's shaped buffer.
    ///
    /// Debug-only: `for_key` re-hashes the run (O(n) per run per frame).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "text paired with a key minted from different bytes")]
    fn a_stale_retained_hash_is_rejected() {
        let _ = input(1).shape_request();
    }
}
