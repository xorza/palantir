//! The keyed cells and fill-cross fixtures the wrapstack cases share.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::scene::tree::node_id::NodeId;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;

/// A fixed-size white cell keyed by `id`, so a test reads its geometry
/// back with `UiHarness::arranged` instead of threading a `NodeId` out.
pub(super) fn cell(ui: &mut Ui, id: &'static str, w: f32, h: f32) -> NodeId {
    Block::new()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(w), Sizing::fixed(h)))
        .background(Background {
            fill: RgbaF32::WHITE.into(),
            ..Default::default()
        })
        .show(ui)
        .node()
}
