//! Builders for the recurring widget patterns used by the cross-driver
//! tests in this directory: chat-message HStacks, two-column grids with
//! wrapping text. Local helpers — keep narrow, only generalize when a
//! third caller appears.

use crate::TextStyle;
use crate::Ui;
use crate::layout::result::{LayoutResult, ShapedText};
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, text::Text};
use std::rc::Rc;

/// Test helper: a node's first shaped-text result, or `None` when the
/// layout pass shaped no text on it. Most tests only have one
/// `Shape::Text` per leaf; if a future test needs all of them, index
/// `result.text_shapes[span.start..span.start+span.len]` directly.
pub(crate) fn first_text(result: &LayoutResult, id: NodeId) -> Option<ShapedText> {
    let span = result.text_spans[id.index()];
    (span.len > 0).then(|| result.text_shapes[span.start as usize])
}

/// `Grid` with two `Hug` columns × one `Hug` row. The wrapping `Text`
/// in column 0 is the unit under test; column 1 carries a short label
/// to keep the second column from collapsing. Returns the wrapping
/// node so the test can read its shape afterwards.
pub(crate) fn two_hug_cols_with_wrap(ui: &mut Ui, paragraph: &'static str) -> NodeId {
    let mut text_node = None;
    Grid::new()
        .cols(Rc::from([Track::hug(), Track::hug()]))
        .rows(Rc::from([Track::hug()]))
        .show(ui, |ui| {
            text_node = Some(
                Text::new(paragraph)
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .grid_cell((0, 0))
                    .show(ui)
                    .node,
            );
            Text::new("right column")
                .style(TextStyle::default().with_font_size(16.0))
                .grid_cell((0, 1))
                .show(ui);
        });
    text_node.unwrap()
}

/// VStack containing a `(Fill × Hug)` HStack with a Fixed-size avatar
/// followed by a wrapping `Fill` text. Models the chat-message
/// pattern. Returns the message text node.
pub(crate) fn chat_message(ui: &mut Ui, avatar_w: f32, text: &'static str, text_px: f32) -> NodeId {
    let mut message_node = None;
    Panel::vstack().show(ui, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("avatar")
                    .size((Sizing::Fixed(avatar_w), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new(text)
                        .style(TextStyle::default().with_font_size(text_px))
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node,
                );
            });
    });
    message_node.unwrap()
}
