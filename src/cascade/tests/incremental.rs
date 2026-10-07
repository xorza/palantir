//! The incremental walk against a full one, and the gates that bust reuse.

use crate::Ui;
use crate::cascade::engine::{CascadeContext, build_cascade_prefix, finish_cascade_input};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;

use crate::internals::harness::UiHarness;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::text::font_scope::internals::INTER;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::state::ScrollState;
use glam::UVec2;
use glam::Vec2;

#[test]
fn cascade_input_hash_collapses_visual_zero_noise() {
    use crate::primitives::math::domain::EPS;

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
    use crate::primitives::layout::visibility::Visibility;
    use crate::primitives::paint::background::Background;
    use crate::widgets::block::Block;

    fn colored_frame(ui: &mut Ui, color: RgbaF32) {
        Block::new()
            .id(WidgetId::from_hash("paint"))
            .size(50.0)
            .background(Background::fill(color))
            .show(ui);
    }

    fn nested_paint(ui: &mut Ui, color: RgbaF32) {
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
                            colored_frame(ui, RgbaF32::WHITE);
                        }
                    });
                if !nested {
                    colored_frame(ui, RgbaF32::WHITE);
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
                        Shape::line(
                            Vec2::splat(offset),
                            Vec2::splat(offset + 20.0),
                            Stroke::new(RgbaF32::WHITE, 2.0),
                        )
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
            .show(ui, |ui| colored_frame(ui, RgbaF32::WHITE));
    }

    fn clipped(ui: &mut Ui, clip: ClipMode) {
        Panel::canvas()
            .id(WidgetId::from_hash("clip"))
            .size(100.0)
            .clip(clip)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("overflow"))
                    .size(50.0)
                    .position((80.0, 0.0))
                    .show(ui);
            });
    }

    fn visible(ui: &mut Ui, visibility: Visibility) {
        Block::new()
            .id(WidgetId::from_hash("visible"))
            .size(50.0)
            .visibility(visibility)
            .show(ui);
    }

    fn layered(ui: &mut Ui, layer: Layer) {
        ui.layer(layer).fixed_at(Vec2::splat(10.0)).show(|ui| {
            colored_frame(ui, RgbaF32::WHITE);
        });
    }

    fn ordered(ui: &mut Ui, swap: bool) {
        Panel::hstack()
            .id(WidgetId::from_hash("order"))
            .show(ui, |ui| {
                let paint = |ui: &mut Ui| colored_frame(ui, RgbaF32::srgb(0.2, 0.4, 0.8));
                let second = |ui: &mut Ui| {
                    Block::new()
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
        |ui| colored_frame(ui, RgbaF32::srgb(0.2, 0.4, 0.8)),
        |ui| colored_frame(ui, RgbaF32::srgb(0.8, 0.2, 0.4)),
    );
    assert_incremental_case(
        "nested paint-only",
        |ui| nested_paint(ui, RgbaF32::srgb(0.2, 0.4, 0.8)),
        |ui| nested_paint(ui, RgbaF32::srgb(0.8, 0.2, 0.4)),
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
    use crate::widgets::block::Block;
    use crate::widgets::scroll::Scroll;

    let build = |ui: &mut Ui| {
        Scroll::vertical()
            .id(WidgetId::from_hash("scroll"))
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("scroll-content"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(300.0)))
                    .show(ui);
            });
    };
    let mut h = UiHarness::new(UVec2::splat(300));
    h.frame(build);
    h.ui.with_state::<ScrollState, _>(WidgetId::from_hash("scroll"), |_, s| s.offset.y = 40.0);
    let rebuilds = h.engines.cascade.counters.full_rebuilds();
    h.frame(build);

    assert_eq!(
        h.engines.cascade.counters.full_rebuilds(),
        rebuilds,
        "a scroll must refresh the cascade in place",
    );
    assert_cascades_match_full(&h.ui, "scroll");
}

/// A widget that adds a shape without moving goes straight to the full
/// rebuild: the incremental walk only discovers a changed row count mid-tree,
/// after duplicating work. Only the counter tells the paths apart.
#[test]
fn adding_a_shape_skips_the_doomed_incremental_walk() {
    fn build(ui: &mut Ui, extra_shape: bool) {
        Panel::vstack()
            .id(WidgetId::from_hash("host"))
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(RgbaF32::WHITE));
                if extra_shape {
                    ui.add_shape(
                        Shape::rect(Rect::new(20.0, 0.0, 10.0, 10.0)).fill(RgbaF32::WHITE),
                    );
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

/// Every cascade input moves the key and takes the path its kind calls for. An
/// equal key keeps last frame's `Cascade`; a key equal in structure refreshes in
/// place. Both fail silently. The control case keeps this from passing vacuously.
#[test]
fn every_cascade_input_moves_the_key_and_takes_its_path() {
    #[derive(Clone, Copy, Debug)]
    struct Scene {
        size: f32,
        transformed: bool,
        disabled: bool,
        tab_index: i16,
    }
    const BASE: Scene = Scene {
        size: 100.0,
        transformed: false,
        disabled: false,
        tab_index: 0,
    };
    fn scene(ui: &mut Ui, s: Scene) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .transform(if s.transformed {
                TranslateScale::from_translation(Vec2::new(7.0, 0.0))
            } else {
                TranslateScale::IDENTITY
            })
            .show(ui, |ui| {
                Panel::vstack()
                    .id(WidgetId::from_hash("body"))
                    .size((Sizing::fixed(s.size), Sizing::fixed(40.0)))
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .focusable(true)
                    .tab_index(s.tab_index)
                    .disabled(s.disabled)
                    .show(ui, |_| {});
            });
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Path {
        /// A structural table changed: everything is rebuilt.
        Rebuild,
        /// Only geometry or paint changed: rows are rewritten in place.
        Refresh,
    }
    /// `(label, the path it must take, mutation applied to the base scene)`.
    type Mutation = (&'static str, Path, fn(&mut UiHarness));
    const RESIZED: Scene = Scene {
        size: 120.0,
        ..BASE
    };
    const TRANSFORMED: Scene = Scene {
        transformed: true,
        ..BASE
    };
    const DISABLED: Scene = Scene {
        disabled: true,
        ..BASE
    };
    const TAB_INDEXED: Scene = Scene {
        tab_index: 3,
        ..BASE
    };
    let mutations: &[Mutation] = &[
        ("resized child", Path::Refresh, |h| {
            h.frame(|ui| scene(ui, RESIZED));
        }),
        ("root transform", Path::Refresh, |h| {
            h.frame(|ui| scene(ui, TRANSFORMED));
        }),
        ("surface resize", Path::Refresh, |h| {
            h.resize(UVec2::new(260, 200));
            h.frame(|ui| scene(ui, BASE));
        }),
        ("disabled", Path::Rebuild, |h| {
            h.frame(|ui| scene(ui, DISABLED));
        }),
        ("tab index", Path::Rebuild, |h| {
            h.frame(|ui| scene(ui, TAB_INDEXED));
        }),
        ("font load", Path::Rebuild, |h| {
            h.ui.load_font(INTER).expect("the bundled Inter loads");
            h.frame(|ui| scene(ui, BASE));
        }),
    ];

    for &(label, path, mutate) in mutations {
        let mut h = UiHarness::new(UVec2::new(200, 200));
        h.frame(|ui| scene(ui, BASE));
        let base_key = h.ui.cascade().key;
        let rebuilds = h.engines.cascade.counters.full_rebuilds();
        let abandoned = h.engines.cascade.counters.abandoned_incrementals();

        mutate(&mut h);

        assert_ne!(
            base_key,
            h.ui.cascade().key,
            "`{label}` left the key unmoved — the frame would reuse a stale cascade",
        );
        let rebuilt = h.engines.cascade.counters.full_rebuilds() > rebuilds;
        assert_eq!(
            if rebuilt {
                Path::Rebuild
            } else {
                Path::Refresh
            },
            path,
            "`{label}` took the wrong path",
        );
        assert_eq!(
            h.engines.cascade.counters.abandoned_incrementals(),
            abandoned,
            "`{label}` should be caught by `can_update`, not discovered mid-walk",
        );
        assert_cascades_match_full(&h.ui, label);
    }

    let mut h = UiHarness::new(UVec2::new(200, 200));
    h.frame(|ui| scene(ui, BASE));
    let base_key = h.ui.cascade().key;
    let rebuilds = h.engines.cascade.counters.full_rebuilds();
    h.frame(|ui| scene(ui, BASE));
    assert_eq!(
        base_key,
        h.ui.cascade().key,
        "an unchanged frame must keep its key",
    );
    assert!(!h.engines.cascade.counters.ran(), "an unchanged key skips");
    assert_eq!(
        h.engines.cascade.counters.full_rebuilds(),
        rebuilds,
        "an unchanged frame must not rebuild",
    );
}

/// A refresh recomputes exactly the nodes whose inputs moved. Two sibling
/// canvases of three blocks under a fixed root:
/// - Transforming `a` moves no rect: viewport, root, `a` and its 3 blocks
///   recompute (6); `b` is skipped.
/// - Growing `a0` moves a rect: all are visited but only viewport, root, `a`
///   and `a0` recompute (4).
#[test]
fn refresh_recomputes_only_what_moved() {
    use crate::widgets::block::Block;

    fn scene(ui: &mut Ui, transformed: bool, a0_size: f32) {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size(Sizing::fixed(200.0))
            .show(ui, |ui| {
                for name in ["a", "b"] {
                    let transform = if transformed && name == "a" {
                        TranslateScale::from_translation(Vec2::new(5.0, 3.0))
                    } else {
                        TranslateScale::IDENTITY
                    };
                    Panel::canvas()
                        .id(WidgetId::from_hash(name))
                        .size(Sizing::fixed(100.0))
                        .transform(transform)
                        .show(ui, |ui| {
                            for k in 0..3u8 {
                                let size = if name == "a" && k == 0 { a0_size } else { 10.0 };
                                Block::new()
                                    .id_salt((name, k))
                                    .size(size)
                                    .position((f32::from(k) * 30.0, 0.0))
                                    .background(Background::fill(RgbaF32::WHITE))
                                    .show(ui);
                            }
                        });
                }
            });
    }

    for (label, transformed, a0_size, refreshed) in
        [("transform", true, 10.0, 6), ("resize", false, 20.0, 4)]
    {
        let mut h = UiHarness::new(UVec2::splat(300));
        h.frame(|ui| scene(ui, false, 10.0));
        let rebuilds = h.engines.cascade.counters.full_rebuilds();
        let before = h.engines.cascade.counters.refreshed_nodes();

        h.frame(|ui| scene(ui, transformed, a0_size));

        assert_eq!(
            h.engines.cascade.counters.full_rebuilds(),
            rebuilds,
            "{label}: rebuilt",
        );
        assert_eq!(
            h.engines.cascade.counters.refreshed_nodes() - before,
            refreshed,
            "{label}: recomputed nodes",
        );
        assert_cascades_match_full(&h.ui, label);
    }
}

fn assert_cascades_match_full(ui: &Ui, label: &str) {
    use crate::cascade::Cascade;
    use crate::cascade::cascade_key::CascadeKey;
    use crate::cascade::engine::CascadeEngine;

    let mut engine = CascadeEngine::default();
    let mut full = Cascade::default();
    let key = CascadeKey::new(
        ui.forest(),
        ui.layout_tables(),
        ui.display(),
        ui.font_epoch(),
    );
    engine.run_full(
        ui.forest(),
        ui.layout_tables(),
        ui.display(),
        &key,
        &mut full,
    );
    assert_eq!(ui.cascade().key, full.key, "{label}: key");

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
            actual.arena_hashes, expected.arena_hashes,
            "{label}: {layer:?} arena hashes"
        );
        assert_eq!(
            actual.subtree_ends, expected.subtree_ends,
            "{label}: {layer:?} subtree ends"
        );
        assert_eq!(
            actual.paint_rects, expected.paint_rects,
            "{label}: {layer:?} own paint rects"
        );
        assert_eq!(
            actual.hit_rows, expected.hit_rows,
            "{label}: {layer:?} hit rows"
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
