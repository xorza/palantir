//! One sweep: the point under the cursor stays under it at every scale.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::support::{SURFACE, fixed_block};
use glam::Vec2;

/// The content point under the pointer holds still. Measured from the content origin past `padding`, the pointer sits at `pointer - padding`; held there through a 1.5× step from offset 0, the offset becomes `(pointer - padding) × 1.5 - (pointer - padding)`, half of it.
#[test]
fn pointer_zoom_pivot_is_scale_invariant() {
    let id = WidgetId::from_hash("scaled-scroll");
    let logical_pointer = Vec2::new(50.0, 70.0);

    for (scale, padding) in [(0.5, 0.0), (1.0, 0.0), (2.0, 0.0), (1.0, 10.0), (2.0, 10.0)] {
        let mut h = UiHarness::new(SURFACE);
        let build = |ui: &mut Ui| {
            Panel::zstack()
                .id(WidgetId::from_hash("scaled-scroll-parent"))
                .transform(TranslateScale::from_scale(scale))
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .show(ui, |ui| {
                    Scroll::both()
                        .id(id)
                        .zoomable()
                        .padding(padding)
                        .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                        .show(ui, |ui| {
                            fixed_block(
                                ui,
                                WidgetId::from_hash("scaled-scroll-content"),
                                400.0,
                                400.0,
                            );
                        });
                });
        };
        h.frame(build);

        let pointer = h.point_in(id, logical_pointer);
        h.pinch_at(pointer, 1.5);
        h.frame(build);

        let state = *h.state::<ScrollState>(id);
        assert_eq!(state.zoom, 1.5, "zoom at {scale}×, padding {padding}");
        assert_eq!(
            state.offset,
            (logical_pointer - padding) * 0.5,
            "pointer pivot at {scale}×, padding {padding}",
        );
    }
}
