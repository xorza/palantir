//! Recording a scroll twice and reading its bar rects back.

use crate::Ui;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use glam::UVec2;

pub(super) fn theme() -> ScrollbarTheme {
    ScrollbarTheme::default()
}

/// Build a scroll over two frames so the second frame's record
/// settles `ScrollState` before the bar-emit check.
pub(super) fn record_two_frames<F: Fn(&mut Ui) + Copy>(
    surface: UVec2,
    build: F,
) -> (UiHarness, NodeId) {
    let mut h = UiHarness::new(surface);
    h.frame(build);
    h.frame(build);
    let scroll_id = WidgetId::from_hash("scroll");
    let idx =
        h.ui.tree(Layer::Main)
            .records
            .widget_id()
            .iter()
            .position(|w| *w == scroll_id)
            .expect("scroll widget recorded");
    (h, NodeId(idx as u32))
}

/// Thumb rects (in *outer-local* coords) for `scroll_key`. Thumbs
/// are real `Sense::DRAG` leaf nodes under an overlay Canvas.
/// Returns 0–2 rects (V and/or H) in vertical-then-horizontal order.
pub(super) fn thumb_rects(ui: &Ui, scroll_key: &str) -> Vec<Rect> {
    let tree = ui.tree(Layer::Main);
    let layout = ui.layout(Layer::Main);
    let outer_id = WidgetId::from_hash(scroll_key);
    let scroll_id = outer_id.with("viewport");
    let widget_ids = tree.records.widget_id();
    let outer_idx = widget_ids
        .iter()
        .position(|w| *w == outer_id)
        .expect("scroll outer recorded");
    let outer_origin = layout.rect[outer_idx].min;
    let mut out = Vec::new();
    for tag in ["vthumb", "hthumb"] {
        let id = scroll_id.with(tag);
        if let Some(idx) = widget_ids.iter().position(|w| *w == id) {
            let r = layout.rect[idx];
            // Both thumbs are recorded every frame — `layout::scrollbars`
            // collapses the ones with nothing to show to zero extent
            // rather than dropping them, so their ids and state rows
            // survive an overflow toggle. A collapsed thumb is not a
            // thumb, so it must not reach an assertion about placement.
            if r.size.w <= 0.0 || r.size.h <= 0.0 {
                continue;
            }
            out.push(Rect {
                min: r.min - outer_origin,
                size: r.size,
            });
        }
    }
    out
}
