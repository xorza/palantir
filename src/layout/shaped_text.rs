//! The layout-side result of shaping one text run.

use crate::primitives::size::Size;
use crate::text::key::TextShapeKey;

/// Result of shaping one `ShapeRecord::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedText {
    pub(crate) measured: Size,
    /// The buffer the renderer replays, or `None` where the run shaped
    /// none — which in production is nothing, and under the gated mono
    /// metric is every run. As wide as a bare key: see
    /// [`TextShapeKey::text_hash`].
    pub(crate) key: Option<TextShapeKey>,
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::layout::shaped_text::ShapedText;
    use crate::text::key::TextShapeKey;

    impl ShapedText {
        /// The key of the buffer this run shaped under. Panics where
        /// none was shaped, which every case reaching here rules out by
        /// driving a cosmic harness; one *about* an absent key reads
        /// [`ShapedText::key`] instead.
        pub(crate) fn buffer_key(&self) -> TextShapeKey {
            self.key.expect("this fixture shapes a buffer")
        }
    }
}
