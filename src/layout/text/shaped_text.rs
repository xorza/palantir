//! The layout-side result of shaping one text run.

use crate::text::extent::TextExtent;
use crate::text::key::TextShapeKey;

/// Result of shaping one `ShapeRecord::Text` in the measure pass; `Tree` records only the authoring inputs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedText {
    pub(crate) extent: TextExtent,
    /// The buffer the renderer replays, or `None` where the run shaped none (never in production; every run under the gated mono metric).
    pub(crate) key: Option<TextShapeKey>,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::layout::text::shaped_text::ShapedText;
    use crate::text::key::TextShapeKey;

    impl ShapedText {
        /// The key of the buffer this run shaped under; panics where none was shaped.
        pub(crate) fn buffer_key(&self) -> TextShapeKey {
            self.key.expect("this fixture shapes a buffer")
        }
    }
}
