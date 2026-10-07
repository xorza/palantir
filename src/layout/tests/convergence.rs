//! `LayoutPass::measure`'s second pass must not assume
//! `final_desired <= new_available`: a non-monotonic descendant (a `wrap_hstack`
//! beside `Fill` cells that hug padded content) can desire ~10 px more. Sweeps
//! a width range: no frame panics and every toolbar button stays in its toolbar.
use crate::primitives::identity::widget_id::WidgetId;

use crate::internals::harness::UiHarness;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Two FILL/FILL cells in an HStack; the right has a Fixed(180×80) descendant
/// (min-content floor 204). FILL siblings should split by shrink budget
/// (`available - intrinsic_min`), not weight alone, so whenever available >=
/// the summed `intrinsic_min` no child extends past the HStack's right edge.
#[test]
fn fill_siblings_with_unequal_min_content_do_not_overflow_parent() {
    for outer_w in (260u32..=600).step_by(10) {
        let mut h = UiHarness::new(UVec2::new(outer_w, 400));
        let row_node = h.frame_value(|ui| {
            Panel::hstack()
                .auto_id()
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("left"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(12.0)
                        .show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash("left-bg"))
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui);
                        });
                    // Right: Fixed(180×80) descendant, `intrinsic_min` = 204.
                    Panel::vstack()
                        .id(WidgetId::from_hash("right"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(12.0)
                        .show(ui, |ui| {
                            Panel::zstack()
                                .id(WidgetId::from_hash("right-z"))
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    Block::new()
                                        .id(WidgetId::from_hash("right-bg"))
                                        .size((Sizing::FILL, Sizing::FILL))
                                        .show(ui);
                                    Block::new()
                                        .id(WidgetId::from_hash("right-fixed"))
                                        .size((Sizing::fixed(180.0), Sizing::fixed(80.0)))
                                        .show(ui);
                                });
                        });
                })
                .response
                .node()
        });

        let row = h.ui.arranged_rect(Layer::Main, row_node);
        let left = h.arranged(WidgetId::from_hash("left"));
        let right = h.arranged(WidgetId::from_hash("right"));

        // With room for the 204 floor the right cell gets at least 204 and the left absorbs the squeeze (CSS Flexbox).
        assert!(
            right.size.w >= 204.0 - 0.5,
            "outer_w={outer_w}: right cell shrunk below its 204 min-content floor; \
             left.w={} right.w={}",
            left.size.w,
            right.size.w,
        );
        let row_right_edge = row.min.x + row.size.w;
        let right_right_edge = right.min.x + right.size.w;
        assert!(
            right_right_edge <= row_right_edge + 0.5,
            "outer_w={outer_w}: right cell overflows HStack",
        );
    }
}

#[test]
fn second_pass_grow_then_overshoot_does_not_panic() {
    const LABELS: &[&str] = &[
        "text",
        "text layouts",
        "text edit",
        "z-order",
        "panels",
        "scroll",
        "wrap",
        "grid",
        "sizing",
        "alignment",
        "justify",
        "clip",
        "transform",
        "visibility",
        "disabled",
        "gap",
        "spacing",
        "buttons",
    ];
    // Sweep ~620-700 wide plus a wider band in 1 px steps, on one harness so each step is a resize.
    let mut h = UiHarness::new(UVec2::new(480, 600));
    for w in (480u32..=900).step_by(1) {
        h.resize(UVec2::new(w, 600));
        h.frame(|ui| {
            Panel::vstack()
                .auto_id()
                .padding(12.0)
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Toolbar: a `wrap_hstack` wider than `w`, so row count varies with width.
                    Panel::wrap_hstack()
                        .id(WidgetId::from_hash("toolbar"))
                        .gap(6.0)
                        .line_gap(6.0)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui, |ui| {
                            for label in LABELS {
                                Button::new()
                                    .id(WidgetId::from_hash(*label))
                                    .label(*label)
                                    .show(ui);
                            }
                        });

                    // Central panel: 4 padded FILL cells with varying hug widths.
                    Panel::zstack()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .padding(16.0)
                        .show(ui, |ui| {
                            Panel::hstack()
                                .auto_id()
                                .gap(12.0)
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    for (id, content_w) in
                                        [("c1", 132.0), ("c2", 60.0), ("c3", 80.0), ("c4", 100.0)]
                                    {
                                        Panel::vstack()
                                            .id(WidgetId::from_hash(id))
                                            .size((Sizing::FILL, Sizing::FILL))
                                            .padding(12.0)
                                            .show(ui, |ui| {
                                                Block::new()
                                                    .id(WidgetId::from_hash((id, "swatch")))
                                                    .size((
                                                        Sizing::fixed(content_w),
                                                        Sizing::fixed(40.0),
                                                    ))
                                                    .show(ui);
                                            });
                                    }
                                });
                        });
                });
        });
        // Every button sits inside its toolbar; the toolbar itself can exceed
        // the window (the cells below have a rigid 536 px floor).
        let toolbar = h.arranged(WidgetId::from_hash("toolbar"));
        for label in LABELS {
            let button = h.arranged(WidgetId::from_hash(*label));
            assert!(
                toolbar.contains_rect(button),
                "w={w}: {label} at {button:?} leaves the toolbar at {toolbar:?}",
            );
        }
    }
}
