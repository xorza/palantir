//! Reading a wrapped child's cell back, and the fill-cross fixtures the
//! cross-floor cases share.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;

/// The arranged rect of whatever recorded under `key`. Generic over the
/// key exactly like `WidgetId::from_hash`, so a fixture that salts by
/// index (`("c", i)`) reads back the same way a named one does. Every
/// fixture in this file keys its cells, so tests read geometry by key
/// instead of threading `NodeId`s out of the record closure.
pub(super) fn rect_of(h: &UiHarness, key: impl std::hash::Hash) -> Rect {
    h.layout_rect(WidgetId::from_hash(key)).expect("arranged")
}

pub(super) fn cell(ui: &mut Ui, id: &'static str, w: f32, h: f32) -> NodeId {
    Frame::new()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(w), Sizing::fixed(h)))
        .background(Background {
            fill: Color::WHITE.into(),
            ..Default::default()
        })
        .show(ui)
        .node()
}
