//! Builders for the recurring widget patterns used by the integration
//! tests in this directory: chat-message HStacks, two-column grids
//! with wrapping text. Local helpers — keep narrow, only generalize
//! when a third caller appears.

use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Sizing, Track};
use crate::tree::NodeId;
use crate::widgets::{Frame, Grid, Panel, Text};
use std::rc::Rc;

/// `Grid` with two `Hug` columns × one `Hug` row. The wrapping `Text`
/// in column 0 is the unit under test; column 1 carries a short label
/// to keep the second column from collapsing. Returns the wrapping
/// node so the test can read its shape afterwards.
pub(super) fn two_hug_cols_with_wrap(ui: &mut Ui, paragraph: &'static str) -> NodeId {
    let mut text_node = None;
    Grid::new()
        .cols(Rc::from([Track::hug(), Track::hug()]))
        .rows(Rc::from([Track::hug()]))
        .show(ui, |ui| {
            text_node = Some(
                Text::new(paragraph)
                    .size_px(16.0)
                    .wrapping()
                    .grid_cell((0, 0))
                    .show(ui)
                    .node,
            );
            Text::new("right column")
                .size_px(16.0)
                .grid_cell((0, 1))
                .show(ui);
        });
    text_node.unwrap()
}

/// VStack containing a `(Fill × Hug)` HStack with a Fixed-size avatar
/// followed by a wrapping `Fill` text. Models the chat-message
/// pattern. Returns the message text node.
pub(super) fn chat_message(ui: &mut Ui, avatar_w: f32, text: &'static str, text_px: f32) -> NodeId {
    let mut message_node = None;
    Panel::vstack().show(ui, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                Frame::with_id("avatar")
                    .size((Sizing::Fixed(avatar_w), Sizing::Fixed(40.0)))
                    .show(ui);
                message_node = Some(
                    Text::new(text)
                        .size_px(text_px)
                        .size((Sizing::FILL, Sizing::Hug))
                        .wrapping()
                        .show(ui)
                        .node,
                );
            });
    });
    message_node.unwrap()
}
