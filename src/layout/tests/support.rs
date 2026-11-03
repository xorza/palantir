//! Builders for patterns the cross-driver tests share: chat-message HStacks, two-column grids with wrapping text, and the paragraph they wrap. Keep narrow; generalize on a third caller.

use crate::internals::layout_rig::{GridTracks, NodeRecorder, TextRun};
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// The paragraph the wrapping cases shape: nine words, so every narrow width breaks it.
pub(super) const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

/// A grid with two `Hug` columns and one `Hug` row: `paragraph` wrapping at 16 px in column 0,
/// under the id `wrapping`, and a "right column" label in column 1 that keeps it from collapsing.
pub(super) fn two_hug_cols_with_wrap(rec: &mut NodeRecorder<'_>, paragraph: &str) {
    let tracks = GridTracks {
        rows: &[Track::HUG],
        cols: &[Track::HUG, Track::HUG],
    };
    rec.grid("grid", Widget::grid(), tracks, |rec| {
        let wrapping = TextRun::new(paragraph, 16.0, TextWrap::WrapWithOverflow);
        rec.text_leaf("wrapping", Widget::leaf().grid_cell((0, 0)), wrapping);
        let label = TextRun::new("right column", 16.0, TextWrap::SingleLine);
        rec.text_leaf("right", Widget::leaf().grid_cell((0, 1)), label);
    });
}

/// A VStack holding a `(Fill × Hug)` HStack of a Fixed `avatar_w` avatar and a `Fill` text
/// `message` wrapping at `text_px` (the chat-message pattern).
#[derive(Clone, Copy, Debug)]
pub(super) struct ChatMessage {
    pub(super) avatar_w: f32,
    pub(super) text: &'static str,
    pub(super) text_px: f32,
}

impl ChatMessage {
    pub(super) fn record(self, rec: &mut NodeRecorder<'_>) {
        rec.node("column", Widget::vstack(), |rec| {
            rec.node(
                "row",
                Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
                |rec| {
                    rec.leaf("avatar", Widget::leaf().size((self.avatar_w, 40.0)));
                    let message = TextRun::new(self.text, self.text_px, TextWrap::WrapWithOverflow);
                    rec.text_leaf(
                        "message",
                        Widget::leaf().size((Sizing::FILL, Sizing::HUG)),
                        message,
                    );
                },
            );
        });
    }
}
