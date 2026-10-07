//! Opening the menu and reading its rows back.

use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use glam::UVec2;

pub(super) const SURFACE: UVec2 = UVec2::new(400, 400);

pub(super) fn trigger_id() -> WidgetId {
    WidgetId::from_hash("trigger")
}

pub(super) fn menu_body(h: &UiHarness, for_id: WidgetId) -> NodeId {
    let body_id = for_id.with("body");
    let index =
        h.ui.tree(Layer::Menu)
            .records
            .widget_id()
            .iter()
            .position(|id| *id == body_id)
            .expect("context menu body recorded");
    NodeId(index as u32)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MenuRow {
    pub(super) node: NodeId,
    pub(super) id: WidgetId,
    pub(super) rect: Rect,
}

/// The open menu's direct children in record order (separators included) with their arranged rects; `subtree_end` skips a row's own label and shortcut leaves.
pub(super) fn menu_rows(h: &UiHarness, for_id: WidgetId) -> Vec<MenuRow> {
    let body = menu_body(h, for_id).idx();
    let tree = h.ui.tree(Layer::Menu);
    let ends = tree.records.subtree_end();
    let body_end = ends[body].end() as usize;
    let ids = tree.records.widget_id();
    let rects = &h.ui.layout(Layer::Menu).rect;
    let mut rows = Vec::new();
    let mut i = body + 1;
    while i < body_end {
        rows.push(MenuRow {
            node: NodeId(i as u32),
            id: ids[i],
            rect: rects[i],
        });
        i = ends[i].end() as usize;
    }
    rows
}
