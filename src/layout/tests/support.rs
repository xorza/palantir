//! Builders for patterns the cross-driver tests share: chat-message HStacks, two-column grids with wrapping text, and the paragraph they wrap. Keep narrow; generalize on a third caller.
use crate::primitives::identity::widget_id::WidgetId;
use crate::text::wrap::TextWrap;

use crate::TextStyle;
use crate::Ui;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::scene::tree::node_id::NodeId;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, grid::Grid, panel::Panel, text::Text};

/// The paragraph the wrapping cases shape: nine words, so every narrow width breaks it.
pub(super) const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

/// Measured height of `lines` lines of `font_px` text: the default line height per line on the shaper's 1/64-px grid, ceiled to whole pixels as the measurer does. Widths are the bundled faces' advances.
pub(super) fn lines_h(lines: u32, font_px: f32) -> f32 {
    (lines as f32 * TextStyle::default().line_height_for(font_px)).ceil()
}

/// The leaf's single shaped-text result; asserts the span holds exactly one entry (multi-text callers should index `result.text_shapes[span.range()]`).
pub(super) fn shaped_text(result: &LayerLayout, id: NodeId) -> ShapedText {
    let span = result.text_spans[id.idx()];
    assert_eq!(
        span.len, 1,
        "shaped_text expects a single-Text leaf; got {} shapes",
        span.len,
    );
    result.text_shapes[span.start as usize]
}

/// `Grid` with two `Hug` columns and one `Hug` row; the wrapping `Text` in column 0 is under test, column 1 a short label that keeps it from collapsing. Returns the wrapping node.
pub(super) fn two_hug_cols_with_wrap(ui: &mut Ui, paragraph: &'static str) -> NodeId {
    let mut text_node = None;
    Grid::new()
        .auto_id()
        .cols([Track::HUG, Track::HUG])
        .rows([Track::HUG])
        .show(ui, |ui| {
            text_node = Some(
                Text::new(paragraph)
                    .auto_id()
                    .font_size(16.0)
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((0, 0))
                    .show(ui)
                    .node(),
            );
            Text::new("right column")
                .auto_id()
                .font_size(16.0)
                .grid_cell((0, 1))
                .show(ui);
        });
    text_node.unwrap()
}

/// VStack holding a `(Fill × Hug)` HStack of a Fixed avatar and a wrapping `Fill` text (the chat-message pattern). Returns the text node.
pub(super) fn chat_message(ui: &mut Ui, avatar_w: f32, text: &'static str, text_px: f32) -> NodeId {
    let mut message_node = None;
    Panel::vstack().auto_id().show(ui, |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("avatar"))
                    .size((Sizing::fixed(avatar_w), Sizing::fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new(text)
                        .auto_id()
                        .font_size(text_px)
                        .size((Sizing::FILL, Sizing::HUG))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui)
                        .node(),
                );
            });
    });
    message_node.unwrap()
}
