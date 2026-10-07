//! The per-node rows the walk fills, and what each covers.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// A chrome panel emits a Paint row at the start of its `node_spans` span; a chromeless childless panel an empty span; a chromeless parent one zero-screen marker row per child, hash = the child's `WidgetId` bits.
#[test]
fn node_spans_rows_mirror_chrome_and_children() {
    use crate::primitives::paint::background::Background;

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("chrome"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("bare"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .show(ui, |_| {});
            Panel::hstack()
                .id(WidgetId::from_hash("parent"))
                .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("kid"))
                        .size((Sizing::fixed(10.0), Sizing::fixed(10.0)))
                        .show(ui, |_| {});
                });
        });
    });

    let layer = Layer::Main;
    let cascade = h.ui.cascade();
    let arena = &cascade.layers[layer].paint_arena;
    let chrome_idx = cascade.by_id[&WidgetId::from_hash("chrome")].node.idx();
    let bare_idx = cascade.by_id[&WidgetId::from_hash("bare")].node.idx();
    let parent_idx = cascade.by_id[&WidgetId::from_hash("parent")].node.idx();
    let chrome_span = arena.node_spans[chrome_idx];
    let bare_span = arena.node_spans[bare_idx];
    let parent_span = arena.node_spans[parent_idx];

    assert!(
        chrome_span.len > 0 && arena.rows[chrome_span.start as usize].screen.area() > 0.0,
        "chromed panel must have a non-empty paint span with non-zero chrome rect",
    );
    let chrome_entry = cascade
        .locate(WidgetId::from_hash("chrome"))
        .unwrap()
        .entry_idx as usize;
    assert_eq!(
        arena.rows[chrome_span.start as usize].screen, cascade.entries[chrome_entry].rect,
        "no-shadow chrome must reuse the node's transformed and clipped visible rect",
    );
    assert_eq!(
        bare_span.len, 0,
        "chromeless childless panel must have empty paint span; got {bare_span:?}",
    );
    assert_eq!(
        parent_span.len, 1,
        "chromeless one-child parent must have exactly its marker row; got {parent_span:?}",
    );
    let marker = arena.rows[parent_span.start as usize];
    assert!(
        marker.screen.is_paint_empty(),
        "child marker row must carry no pixels; got {:?}",
        marker.screen,
    );
    assert_eq!(
        marker.hash.0,
        WidgetId::from_hash("kid").0,
        "child marker hash must be the child's WidgetId bits",
    );
}

/// Every per-node output column follows tree size changes exactly and every
/// retained slot is overwritten with a valid row for the current tree.
#[test]
fn per_node_columns_track_tree_size() {
    let mut h = UiHarness::new(UVec2::new(100, 100));
    for child_count in [3usize, 1, 4] {
        h.frame(|ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("column-root"))
                .show(ui, |ui| {
                    for i in 0..child_count {
                        Panel::hstack()
                            .id(WidgetId::from_hash(("column-child", i)))
                            .show(ui, |_| {});
                    }
                });
        });
        let layer = Layer::Main;
        let nodes = h.ui.tree(layer).records.len();
        let cascade = &h.ui.cascade().layers[layer];
        assert_eq!(cascade.cascade_inputs.len(), nodes);
        assert_eq!(cascade.subtree_paint_rects.len(), nodes);
        assert_eq!(cascade.subtree_ends.len(), nodes);
        assert_eq!(cascade.paint_arena.node_spans.len(), nodes);
        for (i, (&end, &span)) in cascade
            .subtree_ends
            .iter()
            .zip(&cascade.paint_arena.node_spans)
            .enumerate()
        {
            assert!(end as usize > i && end as usize <= nodes);
            assert!(span.start as usize + span.len as usize <= cascade.paint_arena.rows.len());
        }
    }
}

/// A non-painting sibling seeds `Rect::ZERO`; folding it into the parent rollup must not anchor `subtree_paint_rects` at the origin, which would defeat the encoder's subtree cull.
#[test]
fn non_painting_sibling_does_not_origin_anchor_subtree_rollup() {
    use crate::primitives::paint::background::Background;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;
    let row = WidgetId::from_hash("row");
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| {
        Panel::hstack().id(row).show(ui, |ui| {
            // Layout-only spacer: 50 px, paints nothing.
            Panel::hstack()
                .id(WidgetId::from_hash("spacer"))
                .size(50.0)
                .show(ui, |_| {});
            Block::new()
                .id(WidgetId::from_hash("painted"))
                .size(50.0)
                .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                .show(ui);
        });
    });
    let ep = h.ui.cascade().by_id[&row];
    let rollup = h.ui.cascade().layers[ep.layer].subtree_paint_rects[ep.node.idx()];
    assert_eq!(
        rollup,
        Rect::new(50.0, 0.0, 50.0, 50.0),
        "spacer's ZERO seed must not drag the rollup's min to the origin",
    );
}

/// `LayerLayout::rect_hash` lets the incremental walk skip clean subtrees, so it must discriminate on geometry only: identical geometry hashes equal despite paint changes, any moved rect hashes differently.
#[test]
fn rect_hash_tracks_geometry_and_ignores_paint() {
    fn build(size: f32, fill: RgbaF32) -> impl FnMut(&mut Ui) {
        move |ui: &mut Ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .size(Sizing::fixed(200.0))
                .show(ui, |ui| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("child"))
                        .size(Sizing::fixed(size))
                        .background(Background::fill(fill))
                        .show(ui, |_| {});
                });
        }
    }

    let mut h = UiHarness::new(UVec2::splat(300));
    h.frame(build(50.0, RgbaF32::srgb(1.0, 0.0, 0.0)));
    let base = h.ui.layout(Layer::Main).rect_hash();

    // Same geometry, same paint.
    h.frame(build(50.0, RgbaF32::srgb(1.0, 0.0, 0.0)));
    assert_eq!(
        h.ui.layout(Layer::Main).rect_hash(),
        base,
        "an identical frame must hash equal, or the cascade walks every node",
    );

    // Same geometry, different paint: clean subtrees must still skip.
    h.frame(build(50.0, RgbaF32::srgb(0.0, 1.0, 0.0)));
    assert_eq!(
        h.ui.layout(Layer::Main).rect_hash(),
        base,
        "a paint-only change must not move the rect hash",
    );

    // Geometry moved: must invalidate.
    h.frame(build(80.0, RgbaF32::srgb(1.0, 0.0, 0.0)));
    let moved = h.ui.layout(Layer::Main).rect_hash();
    assert_ne!(
        moved, base,
        "a resized child must move the rect hash, or its rows go stale under a clean ancestor",
    );

    // A function of geometry, not a counter: the original size returns the original hash.
    h.frame(build(50.0, RgbaF32::srgb(1.0, 0.0, 0.0)));
    assert_eq!(
        h.ui.layout(Layer::Main).rect_hash(),
        base,
        "the hash must be a pure function of the arranged rects",
    );
}
