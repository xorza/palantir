//! Recording a scroll twice and reading its bar rects back.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use glam::UVec2;

pub(super) fn theme() -> ScrollbarTheme {
    ScrollbarTheme::default()
}

/// Build a scroll over two frames so the second settles `ScrollState`.
pub(super) fn record_two_frames<F: Fn(&mut Ui) + Copy>(surface: UVec2, build: F) -> UiHarness {
    let mut h = UiHarness::new(surface);
    h.prime(2, build);
    h
}

/// Thumb rects (outer-local) for `scroll_key`: `Sense::DRAG` leaves under an overlay Canvas; 0–2 rects, vertical then horizontal.
pub(super) fn thumb_rects(ui: &Ui, scroll_key: &str) -> Vec<Rect> {
    let layout = ui.layout(Layer::Main);
    let outer_id = WidgetId::from_hash(scroll_key);
    let scroll_id = outer_id.with("viewport");
    let node = |id: WidgetId| ui.cascade().endpoint(id).map(|at| at.node);
    let outer = node(outer_id).expect("scroll outer recorded");
    let outer_origin = layout.rect[outer.idx()].min;
    let mut out = Vec::new();
    for tag in ["vthumb", "hthumb"] {
        if let Some(thumb) = node(scroll_id.with(tag)) {
            let r = layout.rect[thumb.idx()];
            // Both thumbs are recorded every frame; collapsed ones (zero extent) mustn't reach a placement assertion.
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
