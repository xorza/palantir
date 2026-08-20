//! One sweep: the point under the cursor stays under it at every scale.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::SURFACE;
use glam::Vec2;

#[test]
fn pointer_zoom_pivot_is_scale_invariant() {
    let id = WidgetId::from_hash("scaled-scroll");
    let logical_pointer = Vec2::new(50.0, 70.0);

    for scale in [0.5, 1.0, 2.0] {
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-scroll-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(id)
                        .with_zoom()
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("scaled-scroll-content"))
                                .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                                .show(ui);
                        });
                });
        };
        h.frame(build);

        let response = h.ui.response_for(id);
        let layout = response.layout_rect.expect("scroll arranged");
        let pointer = response.transform.apply_point(layout.min + logical_pointer);
        h.pinch_at(pointer, 1.5);
        h.frame(build);

        let state = *h.ui.state_mut::<ScrollState>(id);
        assert_eq!(state.zoom, 1.5, "zoom at {scale}×");
        assert_eq!(
            state.offset,
            logical_pointer * 0.5,
            "pointer pivot at {scale}×",
        );
    }
}
