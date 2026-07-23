use crate::Ui;
use crate::layout::cache::{ArenaSnapshot, AvailableKey};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, size::Size};
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node::NodeId;
use crate::text::wrap::TextWrap;
use crate::widgets::{frame::Frame, panel::Panel, text::Text};
use glam::UVec2;

fn run_frame(ui: &mut Ui, record: impl FnMut(&mut Ui)) {
    run_frame_at(ui, UVec2::new(200, 200), record);
}

fn run_frame_at(ui: &mut Ui, size: UVec2, mut record: impl FnMut(&mut Ui)) {
    ui.run_at(size, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, &mut record);
    });
}

#[derive(Debug)]
struct SnapView<'a> {
    snap: ArenaSnapshot,
    desired: &'a [Size],
    avail: AvailableKey,
}

fn snap_for(ui: &Ui, wid: WidgetId) -> Option<SnapView<'_>> {
    let snapshot = &ui.layout_engine.cache.previous;
    let descriptor = *snapshot.snapshots.get(&wid)? as usize;
    let snap = snapshot.descriptors[descriptor];
    Some(SnapView {
        snap,
        desired: &snapshot.nodes.desired[snap.nodes.range()],
        avail: snap.available_q,
    })
}

fn assert_snapshot_is_linear(ui: &Ui) {
    let snapshot = &ui.layout_engine.cache.previous;
    let len = snapshot.nodes.desired.len();
    assert_eq!(snapshot.nodes.scroll_content.len(), len);
    assert_eq!(snapshot.nodes.text_spans.len(), len);
    assert_eq!(snapshot.nodes.intrinsics.len(), len);
    assert_eq!(snapshot.nodes.available_q.len(), len);
    let recorded = Layer::PAINT_ORDER
        .iter()
        .map(|layer| ui.forest.trees[*layer].records.len())
        .sum::<usize>();
    assert_eq!(
        len, recorded,
        "the whole-frame snapshot must retain each recorded node once"
    );
}

fn build_wrapped_frame(ui: &mut Ui, panel_id: &str, frame_size: f32, fill: Color) {
    Panel::vstack()
        .id(WidgetId::from_hash(panel_id))
        .show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash((panel_id, "leaf")))
                .size(frame_size)
                .background(Background {
                    fill: fill.into(),
                    ..Default::default()
                })
                .show(ui);
        });
}

#[test]
fn whole_tree_snapshot_populates_subtree_ranges_once() {
    let mut ui = Ui::for_test();
    run_frame(&mut ui, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("group"))
            .show(ui, |ui| {
                for (id, size) in [("c1", 10.0), ("c2", 20.0), ("c3", 30.0)] {
                    Frame::new().id(WidgetId::from_hash(id)).size(size).show(ui);
                }
            });
    });

    assert_snapshot_is_linear(&ui);
    let view = snap_for(&ui, WidgetId::from_hash("group")).unwrap();
    assert_eq!(view.snap.nodes.len, 4);
    assert_eq!(
        view.desired,
        &[
            Size::new(30.0, 60.0),
            Size::new(10.0, 10.0),
            Size::new(20.0, 20.0),
            Size::new(30.0, 30.0),
        ]
    );
}

#[test]
fn unchanged_subtree_hits_and_replays_exact_output() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| build_wrapped_frame(ui, "a", 50.0, Color::rgb(0.2, 0.4, 0.8));

    run_frame(&mut ui, build);
    let first_hash = snap_for(&ui, WidgetId::from_hash("a"))
        .unwrap()
        .snap
        .subtree_hash;
    let first_desired = ui.layout_engine.cache.previous.nodes.desired.clone();
    let first_rects = ui.layout[Layer::Main].rect.clone();

    run_frame(&mut ui, build);
    let second = snap_for(&ui, WidgetId::from_hash("a")).unwrap();

    assert_eq!(first_hash, second.snap.subtree_hash);
    assert_eq!(first_desired, ui.layout_engine.cache.previous.nodes.desired);
    assert_eq!(first_rects, ui.layout[Layer::Main].rect);
    assert_eq!(
        ui.layout_engine.scratch.cache_hits.len(),
        1,
        "the highest unchanged subtree must short-circuit the frame"
    );
    assert_snapshot_is_linear(&ui);
}

#[test]
fn changing_descendant_hash_replaces_ancestor_descriptor() {
    let mut ui = Ui::for_test();
    run_frame(&mut ui, |ui| {
        build_wrapped_frame(ui, "a", 50.0, Color::rgb(0.2, 0.4, 0.8));
    });
    let first = snap_for(&ui, WidgetId::from_hash("a")).unwrap().snap;

    run_frame(&mut ui, |ui| {
        build_wrapped_frame(ui, "a", 50.0, Color::rgb(0.9, 0.4, 0.8));
    });
    let second = snap_for(&ui, WidgetId::from_hash("a")).unwrap().snap;

    assert_ne!(first.subtree_hash, second.subtree_hash);
    assert_eq!(
        first.nodes.start, second.nodes.start,
        "stable pre-order position must map to the same whole-tree row"
    );
    assert_snapshot_is_linear(&ui);
}

#[test]
fn removed_widget_is_absent_from_next_snapshot() {
    let mut ui = Ui::for_test();
    run_frame(&mut ui, |ui| {
        build_wrapped_frame(ui, "gone", 40.0, Color::rgb(0.5, 0.5, 0.5));
        build_wrapped_frame(ui, "kept", 40.0, Color::rgb(0.5, 0.5, 0.5));
    });
    assert!(
        ui.layout_engine
            .cache
            .previous
            .snapshots
            .contains_key(&WidgetId::from_hash("gone"))
    );

    run_frame(&mut ui, |ui| {
        build_wrapped_frame(ui, "kept", 40.0, Color::rgb(0.5, 0.5, 0.5));
    });

    assert!(
        !ui.layout_engine
            .cache
            .previous
            .snapshots
            .contains_key(&WidgetId::from_hash("gone"))
    );
    assert!(
        ui.layout_engine
            .cache
            .previous
            .snapshots
            .contains_key(&WidgetId::from_hash("kept"))
    );
    assert_snapshot_is_linear(&ui);
}

#[test]
fn reordered_widgets_rebuild_the_dense_descriptor_index() {
    fn build(ui: &mut Ui, reversed: bool) {
        let mut add = |id: &'static str, size: f32| {
            Panel::vstack().id(WidgetId::from_hash(id)).show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash((id, "leaf")))
                    .size(size)
                    .show(ui);
            });
        };
        if reversed {
            add("b", 20.0);
            add("a", 10.0);
        } else {
            add("a", 10.0);
            add("b", 20.0);
        }
    }

    let mut ui = Ui::for_test();
    run_frame(&mut ui, |ui| build(ui, false));
    run_frame(&mut ui, |ui| build(ui, false));
    run_frame(&mut ui, |ui| build(ui, true));

    let a = snap_for(&ui, WidgetId::from_hash("a")).unwrap();
    let b = snap_for(&ui, WidgetId::from_hash("b")).unwrap();
    assert!(b.snap.nodes.start < a.snap.nodes.start);
    assert_eq!(a.desired, &[Size::new(10.0, 10.0), Size::new(10.0, 10.0)]);
    assert_eq!(b.desired, &[Size::new(20.0, 20.0), Size::new(20.0, 20.0)]);
    assert_snapshot_is_linear(&ui);
}

#[test]
fn changing_available_remeasures_wrapping_text() {
    use crate::TextStyle;

    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("inner"))
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new(
                    "the quick brown fox jumps over the lazy dog \
                     pack my box with five dozen liquor jugs",
                )
                .id(WidgetId::from_hash("fill"))
                .size((Sizing::FILL, Sizing::HUG))
                .style(&TextStyle::default().with_font_size(16.0))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
            });
    };

    run_frame_at(&mut ui, UVec2::new(400, 400), build);
    let first = snap_for(&ui, WidgetId::from_hash("inner")).unwrap();
    let first_avail = first.avail;
    let first_leaf = first.desired[1];

    run_frame_at(&mut ui, UVec2::new(100, 400), build);
    let second = snap_for(&ui, WidgetId::from_hash("inner")).unwrap();

    assert_ne!(first_avail, second.avail);
    assert_ne!(first_leaf, second.desired[1]);
    assert_snapshot_is_linear(&ui);
}

#[test]
fn solver_order_text_runs_form_contiguous_subtree_snapshots() {
    fn build(ui: &mut Ui, nodes: &mut Vec<NodeId>) {
        nodes.clear();
        Panel::hstack()
            .id(WidgetId::from_hash("solver-order"))
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                nodes.push(
                    Text::new("fill measured second")
                        .id(WidgetId::from_hash("fill-text"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui)
                        .node(),
                );
                nodes.push(
                    Text::new("hug measured first")
                        .id(WidgetId::from_hash("hug-text"))
                        .show(ui)
                        .node(),
                );
            });
    }

    let mut ui = Ui::for_test_at_text(UVec2::new(400, 200));
    let mut nodes = Vec::new();
    run_frame_at(&mut ui, UVec2::new(400, 200), |ui| build(ui, &mut nodes));

    let cold_fill = ui.layout[Layer::Main].text_spans[nodes[0].idx()];
    let cold_hug = ui.layout[Layer::Main].text_spans[nodes[1].idx()];
    assert_eq!(cold_hug, Span::new(0, 1));
    assert_eq!(cold_fill, Span::new(1, 1));
    let inner = snap_for(&ui, WidgetId::from_hash("solver-order")).unwrap();
    assert_eq!(inner.snap.text_shapes, Span::new(0, 2));
    let cold_fill_key = ui.layout[Layer::Main].text_shapes[cold_fill.start as usize].key;
    let cold_hug_key = ui.layout[Layer::Main].text_shapes[cold_hug.start as usize].key;
    assert_ne!(cold_fill_key, cold_hug_key);

    run_frame_at(&mut ui, UVec2::new(400, 200), |ui| build(ui, &mut nodes));

    let warm_fill = ui.layout[Layer::Main].text_spans[nodes[0].idx()];
    let warm_hug = ui.layout[Layer::Main].text_spans[nodes[1].idx()];
    assert_eq!(warm_fill, cold_fill);
    assert_eq!(warm_hug, cold_hug);
    assert_eq!(
        ui.layout[Layer::Main].text_shapes[warm_fill.start as usize].key,
        cold_fill_key
    );
    assert_eq!(
        ui.layout[Layer::Main].text_shapes[warm_hug.start as usize].key,
        cold_hug_key
    );
    assert!(
        ui.layout_engine
            .scratch
            .cache_hits
            .contains(&WidgetId::VIEWPORT)
    );
}

#[test]
fn localized_change_hits_unchanged_sibling() {
    let build = |ui: &mut Ui, color: Color| {
        Panel::vstack()
            .id(WidgetId::from_hash("branch-root"))
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("changing"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("changing-leaf"))
                            .size(20.0)
                            .background(Background {
                                fill: color.into(),
                                ..Default::default()
                            })
                            .show(ui);
                    });
                Panel::vstack()
                    .id(WidgetId::from_hash("stable"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("stable-leaf"))
                            .size(30.0)
                            .show(ui);
                    });
            });
    };
    let mut ui = Ui::for_test();
    run_frame(&mut ui, |ui| build(ui, Color::rgb(1.0, 0.0, 0.0)));
    let stable_hash = snap_for(&ui, WidgetId::from_hash("stable"))
        .unwrap()
        .snap
        .subtree_hash;

    run_frame(&mut ui, |ui| build(ui, Color::rgb(0.0, 1.0, 0.0)));

    assert!(
        ui.layout_engine
            .scratch
            .cache_hits
            .contains(&WidgetId::from_hash("stable"))
    );
    assert!(
        !ui.layout_engine
            .scratch
            .cache_hits
            .contains(&WidgetId::from_hash("branch-root"))
    );
    assert_eq!(
        stable_hash,
        snap_for(&ui, WidgetId::from_hash("stable"))
            .unwrap()
            .snap
            .subtree_hash
    );
    assert_snapshot_is_linear(&ui);
}

#[test]
fn widget_reappearance_matches_cold_snapshot() {
    let with_widget = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("inner"))
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("blip"))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("blip-leaf"))
                            .size(40.0)
                            .show(ui);
                    });
            });
    };
    let mut ui = Ui::for_test();
    let blip = WidgetId::from_hash("blip");

    run_frame(&mut ui, with_widget);
    run_frame(&mut ui, |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("inner"))
            .show(ui, |_| {});
    });
    assert!(snap_for(&ui, blip).is_none());

    run_frame(&mut ui, with_widget);
    let warm = snap_for(&ui, blip).unwrap().desired.to_vec();

    ui.layout_engine.cache.clear();
    run_frame(&mut ui, with_widget);
    let cold = snap_for(&ui, blip).unwrap().desired.to_vec();

    assert_eq!(warm, cold);
    assert_snapshot_is_linear(&ui);
}

#[test]
fn oscillating_tree_size_reuses_both_snapshot_buffers() {
    fn render(ui: &mut Ui, extra: bool) {
        run_frame(ui, |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("oscillating"))
                .show(ui, |ui| {
                    for index in 0..10 {
                        Frame::new()
                            .id(WidgetId::from_hash(("row", index)))
                            .size(10.0)
                            .show(ui);
                    }
                    if extra {
                        Frame::new()
                            .id(WidgetId::from_hash("extra"))
                            .size(10.0)
                            .show(ui);
                    }
                });
        });
    }

    let mut ui = Ui::for_test();
    for frame in 0..8 {
        render(&mut ui, frame % 2 == 0);
    }
    let mut node_capacities = [
        ui.layout_engine.cache.previous.nodes.desired.capacity(),
        ui.layout_engine.cache.current.nodes.desired.capacity(),
        ui.layout_engine.scratch.desired.capacity(),
    ];
    node_capacities.sort_unstable();
    let mut descriptor_capacities = [
        ui.layout_engine.cache.previous.snapshots.capacity(),
        ui.layout_engine.cache.current.snapshots.capacity(),
    ];
    descriptor_capacities.sort_unstable();

    for frame in 8..40 {
        render(&mut ui, frame % 2 == 0);
        assert_snapshot_is_linear(&ui);
        let mut current_node_capacities = [
            ui.layout_engine.cache.previous.nodes.desired.capacity(),
            ui.layout_engine.cache.current.nodes.desired.capacity(),
            ui.layout_engine.scratch.desired.capacity(),
        ];
        current_node_capacities.sort_unstable();
        let mut current_descriptor_capacities = [
            ui.layout_engine.cache.previous.snapshots.capacity(),
            ui.layout_engine.cache.current.snapshots.capacity(),
        ];
        current_descriptor_capacities.sort_unstable();
        assert_eq!(
            node_capacities, current_node_capacities,
            "both alternating buffers must retain their warmed capacities"
        );
        assert_eq!(descriptor_capacities, current_descriptor_capacities);
    }
}

#[test]
fn quantize_available_axis_invariants() {
    use crate::layout::cache::quantize_available;

    let inf = f32::INFINITY;
    assert_eq!(
        quantize_available(Size::new(inf, 100.4)),
        glam::IVec2::new(i32::MAX, 100),
    );
    assert_eq!(
        quantize_available(Size::new(50.7, inf)),
        glam::IVec2::new(51, i32::MAX),
    );
    assert_eq!(
        quantize_available(Size::new(inf, inf)),
        glam::IVec2::splat(i32::MAX),
    );
    assert_eq!(quantize_available(Size::ZERO), glam::IVec2::ZERO);
}
