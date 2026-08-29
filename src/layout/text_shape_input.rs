//! One recorded text run as the layout pass reads it.

use crate::layout::types::align::HAlign;
use crate::primitives::interned_text::InternedText;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::text::glyph_font::GlyphFont;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::wrap::TextWrap;

/// One `ShapeRecord::Text` worth of layout-side inputs. Yielded by
/// [`Self::on_leaf`] and [`Self::on_container`]; named so the fields
/// aren't a tuple.
#[derive(Debug)]
pub(super) struct TextShapeInput<'a> {
    pub(super) ordinal: u16,
    pub(super) text: &'a str,
    /// Content hash retained on the [`RecordedText`] at record time —
    /// [`Self::shape_request`] reuses it so shaping passes don't rescan
    /// the source bytes.
    ///
    /// [`RecordedText`]: crate::primitives::recorded_text::RecordedText
    pub(super) text_hash: u64,
    /// The face this shapes in, carried whole from
    /// [`ShapeRecord::Text`] so the record and the shaper cannot
    /// disagree about what four separate fields meant.
    pub(super) font: GlyphFont,
    pub(super) wrap: TextWrap,
    /// Horizontal alignment from `Shape::Text.align`. Cosmic-text
    /// bakes per-line offsets into the shaped buffer when wrap is on,
    /// so the layout pass has to thread this all the way down to
    /// `TextSystem::measure` (and into `TextShapeKey`) — two shapes with
    /// identical text/size/wrap but different halign aren't
    /// interchangeable.
    pub(super) halign: HAlign,
}

impl<'a> TextShapeInput<'a> {
    /// A recorded run always has bytes — `TextShape::is_noop` drops an
    /// empty one before it becomes a `ShapeRecord` — so the shaping
    /// boundary is a contract to assert here, not a case layout answers.
    pub(super) fn shape_request(&self) -> TextShapeRequest<'a> {
        TextShapeRequest::for_key(
            self.text,
            TextShapeKey::unbounded(self.text_hash, self.font),
        )
        .expect("a recorded text run has bytes — `TextShape::is_noop` drops the empty one")
    }

    /// Iterate every `ShapeRecord::Text` on a leaf. Single source of truth
    /// for the layout-side leaf walk — `LayoutEngine::measure_dispatch`
    /// drives wrap shaping, `intrinsic::leaf` drives the unbounded content
    /// axis. Filtering and destructuring happen here so neither side can
    /// drift on which shape variants contribute to size.
    pub(super) fn on_leaf(
        tree: &'a Tree,
        interned_text: &'a InternedText<'_>,
        node: NodeId,
    ) -> impl Iterator<Item = TextShapeInput<'a>> {
        // Direct slice into `tree.shapes` for `node`. Leaves have no
        // children, so the `records.shape_span()[i]` span is exactly the
        // leaf's own direct shapes — contiguous, no child boundaries to skip.
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

    /// Iterate the direct text shapes on a container, skipping text
    /// belonging to descendant nodes while preserving this node's
    /// within-owner record order.
    pub(super) fn on_container(
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
    use crate::layout::text_shape_input::{TextShapeInput, checked_text_ordinal};
    use crate::layout::types::align::HAlign;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;
    use crate::text::wrap::TextWrap;
    use crate::text::{FontFamily, FontWeight};

    #[test]
    fn text_ordinal_covers_the_u16_domain_and_rejects_the_next_run() {
        assert_eq!(checked_text_ordinal(0), 0);
        assert_eq!(checked_text_ordinal(usize::from(u16::MAX)), u16::MAX);
        assert!(
            std::panic::catch_unwind(|| checked_text_ordinal(usize::from(u16::MAX) + 1)).is_err(),
            "the 65537th direct text run must exceed the identity key",
        );
    }

    const FACE: GlyphFont = GlyphFont {
        size_px: 16.0,
        line_height_px: 19.2,
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
    };

    /// One recorded run, paired with whatever hash the caller claims for
    /// its bytes.
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

    /// A retained hash that no longer describes the bytes beside it would
    /// let one run replay another's shaped buffer.
    ///
    /// Debug-only, and the crate's one pairing check that is:
    /// `TextShapeRequest::for_key` re-hashes the run to compare, which is
    /// `O(n)` in its bytes per run per frame. `ShapedTextRef::new` asks
    /// the same question of two recorded hashes and holds in release.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "text paired with a key minted from different bytes")]
    fn a_stale_retained_hash_is_rejected() {
        let _ = input(1).shape_request();
    }
}
