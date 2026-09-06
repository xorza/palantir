//! The scroll panel a test drives, and what its offsets are read back through.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::endpoint::Endpoint;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use glam::{UVec2, Vec2};

pub(super) const SURFACE: UVec2 = UVec2::new(400, 600);

pub(super) fn build(ui: &mut Ui, viewport_h: f32, content_h: f32) {
    driven(ui, viewport_h, content_h, Vec2::ZERO);
}

/// [`build`] with a pan request folded in — the viewport driven by
/// authoring code rather than by a pointer. `build` passes a zero one,
/// which the widget takes as the no-op it is.
pub(super) fn driven(ui: &mut Ui, viewport_h: f32, content_h: f32, pan: Vec2) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            Scroll::vertical()
                .id(WidgetId::from_hash("scroll"))
                .scroll_by(pan)
                .size((Sizing::fixed(200.0), Sizing::fixed(viewport_h)))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("content"))
                        .size((Sizing::fixed(200.0), Sizing::fixed(content_h)))
                        .show(ui);
                });
        });
}

/// A zoomable viewport, taking one [`Scroll::zoom_by`] per entry of
/// `factors` — so a case can ask what several requests on one builder
/// compose to. Read back through [`read_state`], like its panning peer.
pub(super) fn zoom_driven(ui: &mut Ui, factors: &[f32]) {
    let mut scroll = Scroll::both()
        .id(WidgetId::from_hash("scroll"))
        .zoom()
        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)));
    for factor in factors {
        scroll = scroll.zoom_by(*factor);
    }
    scroll.show(ui, |ui| {
        Frame::new()
            .id(WidgetId::from_hash("content"))
            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
            .show(ui);
    });
}

pub(super) fn read_state(h: &mut UiHarness) -> ScrollState {
    *h.ui
        .state_or_default::<ScrollState>(WidgetId::from_hash("scroll"))
}

fn scroll_viewport_endpoint(ui: &Ui, outer_id: WidgetId) -> Endpoint {
    ui.cascade()
        .endpoint(outer_id.with("viewport"))
        .expect("scroll viewport endpoint")
}

pub(super) fn scroll_content(ui: &Ui, outer_id: WidgetId) -> Size {
    ui.scroll_content(outer_id.with("viewport"))
}

pub(super) fn scroll_viewport(ui: &Ui, outer_id: WidgetId) -> Size {
    let endpoint = scroll_viewport_endpoint(ui, outer_id);
    let tree = ui.tree(endpoint.layer);
    ui.arranged_rect(endpoint.layer, endpoint.node)
        .deflated_by(tree.records.layout()[endpoint.node.idx()].padding)
        .size
}
