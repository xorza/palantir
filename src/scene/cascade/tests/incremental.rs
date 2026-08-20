//! The incremental walk against a full one, and the gates that bust reuse.

use crate::Ui;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::engine::{
    CascadeContext, CascadePrefixBits, build_cascade_prefix, cascade_fingerprint,
    finish_cascade_input,
};

use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::seen_ids::Endpoint;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::state::ScrollState;
use glam::UVec2;
use glam::Vec2;

#[test]
fn cascade_input_hash_collapses_visual_zero_noise() {
    use crate::primitives::approx::EPS;

    assert_eq!(std::mem::size_of::<CascadePrefixBits>(), 32);
    let hash = |transform, rect| {
        let prefix = build_cascade_prefix(CascadeContext {
            transform,
            ..CascadeContext::ROOT
        });
        finish_cascade_input(&prefix, rect, false)
    };
    let baseline = hash(TranslateScale::IDENTITY, Rect::ZERO);
    assert_eq!(
        baseline,
        hash(
            TranslateScale::new(Vec2::splat(EPS * 0.5), 1.0 + EPS * 0.5),
            Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS),
        ),
    );
    assert_ne!(
        baseline,
        hash(
            TranslateScale::from_translation(Vec2::new(EPS * 2.0, 0.0)),
            Rect::ZERO,
        ),
    );
}

#[test]
fn incremental_matches_full_across_cascade_input_classes() {
    use crate::primitives::background::Background;
    use crate::scene::visibility::Visibility;
    use crate::widgets::frame::Frame;

    fn colored_frame(ui: &mut Ui, color: Color) {
        Frame::new()
            .id(WidgetId::from_hash("paint"))
            .size(50.0)
            .background(Background {
                fill: color.into(),
                ..Default::default()
            })
            .show(ui);
    }

    fn nested_paint(ui: &mut Ui, color: Color) {
        Panel::canvas()
            .id(WidgetId::from_hash("paint-root"))
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("paint-parent"))
                    .show(ui, |ui| colored_frame(ui, color));
            });
    }

    fn reparented(ui: &mut Ui, nested: bool) {
        Panel::canvas()
            .id(WidgetId::from_hash("reparent-root"))
            .size(100.0)
            .show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("reparent-parent"))
                    .size(100.0)
                    .show(ui, |ui| {
                        if nested {
                            colored_frame(ui, Color::WHITE);
                        }
                    });
                if !nested {
                    colored_frame(ui, Color::WHITE);
                }
            });
    }

    fn shape_count(ui: &mut Ui, count: usize) {
        Panel::canvas()
            .id(WidgetId::from_hash("shape-count"))
            .size(100.0)
            .show(ui, |ui| {
                for index in 0..count {
                    let offset = index as f32 * 10.0;
                    ui.add_shape(
                        Shape::line(Vec2::splat(offset), Vec2::splat(offset + 20.0), 2.0)
                            .brush(Color::WHITE)
                            .cap(LineCap::Round),
                    );
                }
            });
    }

    fn transformed(ui: &mut Ui, transform: TranslateScale) {
        Panel::canvas()
            .id(WidgetId::from_hash("transform"))
            .size(100.0)
            .transform(transform)
            .show(ui, |ui| colored_frame(ui, Color::WHITE));
    }

    fn clipped(ui: &mut Ui, clip: ClipMode) {
        Panel::canvas()
            .id(WidgetId::from_hash("clip"))
            .size(100.0)
            .clip(clip)
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("overflow"))
                    .size(50.0)
                    .position((80.0, 0.0))
                    .show(ui);
            });
    }

    fn visible(ui: &mut Ui, visibility: Visibility) {
        Frame::new()
            .id(WidgetId::from_hash("visible"))
            .size(50.0)
            .visibility(visibility)
            .show(ui);
    }

    fn layered(ui: &mut Ui, layer: Layer) {
        ui.layer(layer).at(Vec2::splat(10.0)).show(|ui| {
            colored_frame(ui, Color::WHITE);
        });
    }

    fn ordered(ui: &mut Ui, swap: bool) {
        Panel::hstack()
            .id(WidgetId::from_hash("order"))
            .show(ui, |ui| {
                let paint = |ui: &mut Ui| colored_frame(ui, Color::rgb(0.2, 0.4, 0.8));
                let second = |ui: &mut Ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("second"))
                        .size(50.0)
                        .show(ui);
                };
                if swap {
                    second(ui);
                    paint(ui);
                } else {
                    paint(ui);
                    second(ui);
                }
            });
    }

    assert_incremental_case(
        "paint-only",
        |ui| colored_frame(ui, Color::rgb(0.2, 0.4, 0.8)),
        |ui| colored_frame(ui, Color::rgb(0.8, 0.2, 0.4)),
    );
    assert_incremental_case(
        "nested paint-only",
        |ui| nested_paint(ui, Color::rgb(0.2, 0.4, 0.8)),
        |ui| nested_paint(ui, Color::rgb(0.8, 0.2, 0.4)),
    );
    assert_incremental_case(
        "paint-row cardinality",
        |ui| shape_count(ui, 1),
        |ui| shape_count(ui, 2),
    );
    assert_incremental_case(
        "transform",
        |ui| transformed(ui, TranslateScale::IDENTITY),
        |ui| transformed(ui, TranslateScale::new(Vec2::new(20.0, 10.0), 1.5)),
    );
    assert_incremental_case(
        "clip",
        |ui| clipped(ui, ClipMode::None),
        |ui| clipped(ui, ClipMode::Rect),
    );
    assert_incremental_case(
        "visibility",
        |ui| visible(ui, Visibility::Visible),
        |ui| visible(ui, Visibility::Hidden),
    );
    assert_incremental_case(
        "reparent",
        |ui| reparented(ui, true),
        |ui| reparented(ui, false),
    );
    assert_incremental_case(
        "side-layer migration",
        |ui| layered(ui, Layer::Popup),
        |ui| layered(ui, Layer::Tooltip),
    );
    assert_incremental_case("reorder", |ui| ordered(ui, false), |ui| ordered(ui, true));
}

#[test]
fn incremental_scroll_matches_full() {
    use crate::widgets::frame::Frame;
    use crate::widgets::scroll::Scroll;

    let build = |ui: &mut Ui| {
        Scroll::vertical()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("scroll-content"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(300.0)))
                    .show(ui);
            });
    };
    let mut h = UiHarness::new(UVec2::splat(300));
    h.frame(build);
    h.ui.state_mut::<ScrollState>(WidgetId::from_hash("scroll"))
        .offset
        .y = 40.0;
    h.frame(build);

    assert_cascades_match_full(&h.ui, "scroll");
}

/// Pin: a widget that adds a shape without moving goes straight to the
/// full rebuild instead of attempting an incremental walk first.
///
/// `cascade_static` deliberately excludes chrome and direct shapes, so
/// paint-only edits can stay on the incremental path — but the
/// incremental walk repairs a node's paint rows *in place* and can only
/// bail once a row count changes, which it discovers mid-tree, after
/// having duplicated part of the work the rebuild then redoes. Both
/// paths end in the same cascade, so only the counter can tell them
/// apart.
#[test]
fn adding_a_shape_skips_the_doomed_incremental_walk() {
    fn build(ui: &mut Ui, extra_shape: bool) {
        Panel::vstack()
            .id(WidgetId::from_hash("host"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(Color::WHITE));
                if extra_shape {
                    // Same layout, same rects, one more paint row — the
                    // caret / focus-ring / hover-highlight shape.
                    ui.add_shape(Shape::rect(Rect::new(20.0, 0.0, 10.0, 10.0)).fill(Color::WHITE));
                }
            });
    }

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| build(ui, false));
    let baseline = h.engines.cascade.counters.abandoned_incrementals();

    h.frame(|ui| build(ui, true));
    assert_eq!(
        h.engines.cascade.counters.abandoned_incrementals(),
        baseline,
        "a row-count change must be caught by `can_update`, not discovered mid-walk",
    );

    // And the cascade it produced is still right: the host now owns two
    // shape rows where it owned one.
    let rows = h.ui.cascade().layers[Layer::Main]
        .paint_arena
        .node_spans
        .iter()
        .map(|span| span.len)
        .max()
        .expect("nodes recorded");
    assert!(
        rows >= 2,
        "the rebuilt cascade must carry both shape rows, got {rows}",
    );
}

/// Pin: every cascade input busts **both** gates.
///
/// Two hand-maintained enumerations decide whether cascade output can be
/// reused. `cascade_fingerprint` is the outer one — a match in
/// `FrameCycle::post_record` skips `CascadeEngine::run` outright and reuses
/// last
/// frame's `Cascade` verbatim. `can_update` is the inner one, choosing
/// between repairing paint in place and rebuilding every row. Neither
/// references the other, and both fail silently: the outer one by
/// serving a stale cascade, the inner by keeping `entries` / `hits` /
/// `cascade_inputs` that no longer describe the frame.
///
/// So for each input, assert it moves the fingerprint *and* forces a
/// full rebuild. The control case at the end is what stops this passing
/// vacuously — an unchanged frame must move neither.
#[test]
fn every_cascade_input_busts_both_reuse_gates() {
    fn scene(ui: &mut Ui, size: f32, transformed: bool) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .transform(if transformed {
                TranslateScale::from_translation(Vec2::new(7.0, 0.0))
            } else {
                TranslateScale::IDENTITY
            })
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("body"))
                    .size((Sizing::fixed(size), Sizing::fixed(40.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
            });
    }

    /// `(label, apply the mutation to a harness already showing the base
    /// scene)`.
    type Mutation = (&'static str, fn(&mut UiHarness));
    let mutations: &[Mutation] = &[
        // Authoring: reaches the fingerprint through the root's
        // subtree_hash and `can_update` through `cascade_static`.
        ("resized child", |h| {
            h.frame(|ui| scene(ui, 120.0, false));
        }),
        // Ancestor transform: same two columns, different field.
        ("root transform", |h| {
            h.frame(|ui| scene(ui, 100.0, true));
        }),
        // Surface: reaches the fingerprint directly and `can_update`
        // through the arranged rects it hashes.
        ("surface resize", |h| {
            h.resize(UVec2::new(260, 200));
            h.frame(|ui| scene(ui, 100.0, false));
        }),
    ];

    for &(label, mutate) in mutations {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| scene(ui, 100.0, false));
        let base_fp = cascade_fingerprint(h.ui.forest(), h.ui.display());
        let rebuilds = h.engines.cascade.counters.full_rebuilds();
        let abandoned = h.engines.cascade.counters.abandoned_incrementals();

        mutate(&mut h);

        assert_ne!(
            base_fp,
            cascade_fingerprint(h.ui.forest(), h.ui.display()),
            "`{label}` left the fingerprint unmoved — the frame would reuse a stale cascade",
        );
        assert!(
            h.engines.cascade.counters.full_rebuilds() > rebuilds,
            "`{label}` did not force a full rebuild — `can_update` kept columns \
             that no longer describe the frame",
        );
        assert_eq!(
            h.engines.cascade.counters.abandoned_incrementals(),
            abandoned,
            "`{label}` should be caught by `can_update`, not discovered mid-walk",
        );
    }

    // Control: an identical frame must move neither gate, or the
    // assertions above would hold for any frame at all.
    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| scene(ui, 100.0, false));
    let base_fp = cascade_fingerprint(h.ui.forest(), h.ui.display());
    let rebuilds = h.engines.cascade.counters.full_rebuilds();
    h.frame(|ui| scene(ui, 100.0, false));
    assert_eq!(
        base_fp,
        cascade_fingerprint(h.ui.forest(), h.ui.display()),
        "an unchanged frame must keep its fingerprint",
    );
    assert_eq!(
        h.engines.cascade.counters.full_rebuilds(),
        rebuilds,
        "an unchanged frame must not rebuild",
    );
}

fn assert_cascades_match_full(ui: &Ui, label: &str) {
    use crate::scene::cascade::Cascade;
    use crate::scene::cascade::engine::CascadeEngine;

    let mut engine = CascadeEngine::default();
    let mut full = Cascade::default();
    engine.run_full(ui.forest(), ui.layout_tables(), ui.display(), &mut full);

    // Whole-row compares: `entries` / `hits` are AoS and `PartialEq`,
    // so this covers every field and keeps covering any field added
    // later — the previous column-by-column form silently skipped new
    // ones.
    assert_eq!(ui.cascade().entries, full.entries, "{label}");
    assert_eq!(ui.cascade().hits, full.hits, "{label}");

    let mut id_count = 0;
    for layer in Layer::PAINT_ORDER {
        let widget_ids = ui.tree(layer).records.widget_id();
        id_count += widget_ids.len();
        for (index, wid) in widget_ids.iter().copied().enumerate() {
            assert_eq!(
                ui.cascade().by_id[&wid],
                Endpoint {
                    layer,
                    node: NodeId(index as u32),
                },
                "{label}: {layer:?} by-id endpoint"
            );
        }
        let actual = &ui.cascade().layers[layer];
        let expected = &full.layers[layer];
        assert_eq!(
            actual.cascade_inputs, expected.cascade_inputs,
            "{label}: {layer:?} cascade inputs"
        );
        assert_eq!(
            actual.subtree_paint_rects, expected.subtree_paint_rects,
            "{label}: {layer:?} subtree paint rects"
        );
        assert_eq!(
            actual.subtree_hashes, expected.subtree_hashes,
            "{label}: {layer:?} subtree hashes"
        );
        assert_eq!(
            actual.static_hash, expected.static_hash,
            "{label}: {layer:?} static hash"
        );
        assert_eq!(
            actual.subtree_ends, expected.subtree_ends,
            "{label}: {layer:?} subtree ends"
        );
        assert_eq!(
            actual.paint_arena.node_spans, expected.paint_arena.node_spans,
            "{label}: {layer:?} paint spans"
        );
        assert_eq!(
            actual.paint_arena.rows, expected.paint_arena.rows,
            "{label}: {layer:?} paint rows"
        );
        assert_eq!(
            actual.entries_base, expected.entries_base,
            "{label}: {layer:?} entry base"
        );
    }
    assert_eq!(ui.cascade().by_id.len(), id_count, "{label}: by-id length");
}

fn assert_incremental_case(label: &str, base: impl Fn(&mut Ui), changed: impl Fn(&mut Ui)) {
    let mut h = UiHarness::new(UVec2::splat(300));
    h.frame(base);
    h.frame(changed);
    assert_cascades_match_full(&h.ui, label);
}
