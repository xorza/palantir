//! Builders for the recurring widget patterns used by the cross-driver
//! tests in this directory: chat-message HStacks, two-column grids with
//! wrapping text. Local helpers — keep narrow, only generalize when a
//! third caller appears.
use crate::primitives::widget_id::WidgetId;

use crate::TextStyle;
use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::NodeId;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::layout::{LayerLayout, ShapedText};
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, text::Text};
use std::rc::Rc;

/// Test helper: the leaf's single shaped-text result. Asserts the
/// span holds exactly one entry — every cross-driver test today builds
/// single-Text leaves; a multi-text caller should index
/// `result.text_shapes[span.range()]` itself rather than pretend index
/// 0 is meaningful.
pub(crate) fn shaped_text(result: &LayerLayout, id: NodeId) -> ShapedText {
    let span = result.text_spans[id.index()];
    assert_eq!(
        span.len, 1,
        "shaped_text expects a single-Text leaf; got {} shapes",
        span.len,
    );
    result.text_shapes[span.start as usize]
}

/// `Grid` with two `Hug` columns × one `Hug` row. The wrapping `Text`
/// in column 0 is the unit under test; column 1 carries a short label
/// to keep the second column from collapsing. Returns the wrapping
/// node so the test can read its shape afterwards.
pub(crate) fn two_hug_cols_with_wrap(ui: &mut Ui, paragraph: &'static str) -> NodeId {
    let mut text_node = None;
    Grid::new()
        .auto_id()
        .cols(Rc::from([Track::hug(), Track::hug()]))
        .rows(Rc::from([Track::hug()]))
        .show(ui, |ui| {
            text_node = Some(
                Text::new(paragraph)
                    .auto_id()
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .grid_cell((0, 0))
                    .show(ui)
                    .node(ui),
            );
            Text::new("right column")
                .auto_id()
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
    Panel::vstack().auto_id().show(ui, |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("avatar"))
                    .size((Sizing::Fixed(avatar_w), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new(text)
                        .auto_id()
                        .style(TextStyle::default().with_font_size(text_px))
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node(ui),
                );
            });
    });
    message_node.unwrap()
}
