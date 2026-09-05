//! When a bar exists at all, and what retires it.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::node_id::NodeId;
use crate::shape::rect::RectKind;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::bars::support::{record_two_frames, theme, thumb_rects};
use crate::widgets::scroll::tests::support::scroll_viewport;
use glam::UVec2;
use glam::Vec2;

#[test]
fn hidden_scroll_skips_bar_ids_and_cold_relayout_but_keeps_pan_and_zoom() {
    let surface = UVec2::new(400, 400);
    let outer_id = WidgetId::from_hash("hidden-scroll");
    let scroll_id = outer_id.with("viewport");
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(outer_id)
            .hide_bars()
            .with_zoom()
            .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("hidden-content"))
                    .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                    .show(ui);
            });
    };

    let mut h = UiHarness::new(surface);
    let mut records = 0;
    let report = h.frame(|ui| {
        records += 1;
        build(ui);
    });
    assert_eq!(report.processing, FrameProcessing::SingleLayout);
    assert_eq!(
        records, 1,
        "hidden cold mount must not settle bar visibility"
    );

    let tree = h.ui.tree(Layer::Main);
    for tag in ["bars", "vtrack", "htrack", "vthumb", "hthumb"] {
        assert!(
            !tree
                .records
                .widget_id()
                .iter()
                .any(|widget_id| *widget_id == scroll_id.with(tag)),
            "hidden scroll recorded bar id {tag}",
        );
    }

    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(40.0, 60.0));
    h.pinch(1.5);
    h.frame(build);
    let state = *h.ui.state_mut::<ScrollState>(outer_id);
    assert_eq!(scroll_viewport(&h.ui, outer_id), Size::new(200.0, 200.0));
    assert_eq!(state.zoom, 1.5);
    assert_eq!(
        state.offset,
        Vec2::new(65.0, 85.0),
        "pivot adds (25, 25), then wheel pan adds (40, 60)",
    );
}

#[test]
fn vertical_overflow_emits_thumb_shape_after_settle() {
    let (ui, _node) = record_two_frames(UVec2::new(400, 600), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("tall"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    });
    assert!(
        !thumb_rects(&ui.ui, "scroll").is_empty(),
        "vertical overflow should emit at least one bar thumb"
    );
}

/// Content that stops overflowing must retire its bar, even though
/// the bar overlay's own subtree hash and slot are unchanged — the
/// showcase symptom was a scrollbar surviving a page switch.
///
/// The bars' placement reads a *sibling's* measured `scroll_content`,
/// so it is not the pure function of its own slot that arrange replay
/// assumes; `LayoutEngine::arrange` exempts `Scrollbars` for exactly
/// this. Asserting the raw rects (not `thumb_rects`, which filters
/// collapsed bars) is what makes a stale bar visible to the test.
#[test]
fn content_that_stops_overflowing_retires_its_bar() {
    let build = |tall: bool| {
        move |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::fixed(400.0), Sizing::fixed(300.0)))
                .show(ui, |ui| {
                    Scroll::vertical()
                        .id(WidgetId::from_hash("scroll"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .overlay_bars()
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("body"))
                                .size((
                                    Sizing::FILL,
                                    Sizing::fixed(if tall { 900.0 } else { 50.0 }),
                                ))
                                .show(ui);
                        });
                });
        }
    };
    let surface = UVec2::new(400, 300);
    let mut h = UiHarness::new(surface);
    h.frame(build(true));
    h.frame(build(true));
    assert_eq!(
        thumb_rects(&h.ui, "scroll").len(),
        1,
        "900px of content in a 300px viewport must show a thumb",
    );

    h.frame(build(false));
    for (tag, rect) in raw_bar_rects(&h.ui, "scroll") {
        assert_eq!(
            rect.size,
            Size::ZERO,
            "{tag} must collapse once the content fits, got {rect:?}",
        );
    }

    // ...and come back, so the collapse isn't a one-way latch.
    h.frame(build(true));
    assert_eq!(
        thumb_rects(&h.ui, "scroll").len(),
        1,
        "the thumb must return when the content overflows again",
    );
}

#[test]
fn no_bar_when_content_fits_viewport() {
    let (ui, node) = record_two_frames(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("short"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(50.0)))
                            .show(ui);
                    });
            });
    });
    assert_eq!(
        count_positioned(&ui.ui, node),
        0,
        "non-overflowing content should produce no bar shapes"
    );
}

#[test]
fn both_axes_overflow_emits_two_thumbs() {
    let (ui, _node) = record_two_frames(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("big"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    });
    assert_eq!(
        thumb_rects(&ui.ui, "scroll").len(),
        2,
        "ScrollXY with overflow on both axes should emit two thumbs"
    );
}

/// `ScrollXY` with both axes overflowing must NOT have its V and H
/// bars overlap at the bottom-right corner.
#[test]
fn both_axes_bars_dont_overlap_at_corner() {
    let (ui, _node) = record_two_frames(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("big"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    });
    let theme = theme();
    let inner = 200.0 - theme.thickness - theme.gap;
    let outer_far = 200.0 - theme.thickness;
    let overlays = thumb_rects(&ui.ui, "scroll");
    assert_eq!(overlays.len(), 2, "expected V + H thumbs");
    let v = overlays
        .iter()
        .find(|r| r.min.x == outer_far)
        .expect("V bar at right edge");
    let h = overlays
        .iter()
        .find(|r| r.min.y == outer_far)
        .expect("H bar at bottom edge");
    assert!(
        v.max().y <= inner,
        "V bar must not extend into the H bar's reserved strip; \
         v.max.y={}, inner={inner}",
        v.max().y,
    );
    assert!(
        h.max().x <= inner,
        "H bar must not extend into the V bar's reserved strip; \
         h.max.x={}, inner={inner}",
        h.max().x,
    );
}

fn count_positioned(ui: &Ui, node: NodeId) -> usize {
    ui.tree(Layer::Main)
        .shapes_of(node)
        .filter(|s| {
            matches!(
                s,
                ShapeRecord::Quad(QuadShape::Rect {
                    kind: RectKind::Rounded,
                    local_rect: Some(_),
                    ..
                })
            )
        })
        .count()
}

/// Every bar node's arranged rect, collapsed ones included.
fn raw_bar_rects(ui: &Ui, scroll_key: &str) -> Vec<(&'static str, Rect)> {
    let tree = ui.tree(Layer::Main);
    let layout = ui.layout(Layer::Main);
    let scroll_id = WidgetId::from_hash(scroll_key).with("viewport");
    let widget_ids = tree.records.widget_id();
    let mut out = Vec::new();
    for tag in ["vtrack", "vthumb", "htrack", "hthumb"] {
        let id = scroll_id.with(tag);
        if let Some(idx) = widget_ids.iter().position(|w| *w == id) {
            out.push((tag, layout.rect[idx]));
        }
    }
    out
}
