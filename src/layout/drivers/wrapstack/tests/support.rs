//! The keyed cells the wrapstack cases share.

use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::bg::Bg;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::tree::node_id::NodeId;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;

/// A fixed-size white cell keyed by `id`, so a test reads its geometry
/// back with `UiHarness::arranged` instead of threading a `NodeId` out.
pub(super) fn cell(ui: &mut Ui, id: &'static str, size: Size) -> NodeId {
    Block::new()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(size.w), Sizing::fixed(size.h)))
        .bg(Bg::fill(RgbaF32::WHITE))
        .show(ui)
        .node()
}
