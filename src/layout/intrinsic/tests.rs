use crate::layout::intrinsic::*;
use crate::scene::tree::node::NodeId;

use crate::Ui;
use crate::layout::support::TextCtx;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ScrollSpec};
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::widgets::{frame::Frame, grid::Grid, panel::Panel, scroll::Scroll, text::Text};
use glam::UVec2;

/// Driver-triggered intrinsic queries during `run` must populate
/// the per-node cache. Without this, every `engine.intrinsic` call
/// would recompute from scratch — the 9% intrinsic cost in the
/// layout bench would balloon.
///
/// Uses the HStack-with-Fill-wrap pattern: pass-2 of
/// `stack::measure` queries `MinContent` on each Fill child.
#[test]
fn intrinsic_cache_populated_after_run() {
    let mut ui = Ui::for_test();
    let mut root = NodeId(0);
    ui.run_at_without_baseline(UVec2::new(400, 300), |ui| {
        root = Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("lorem ipsum dolor sit amet")
                    .id_salt("msg")
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
            })
            .response
            .node();
    });

    let child = ui.forest.trees[Layer::Main]
        .children(root)
        .map(|c| c.id)
        .next()
        .expect("hstack has child");
    let slot = LenReq::MinContent.slot(Axis::X);
    let cached = ui.layout_engine.scratch.intrinsics[child.idx()][slot];
    assert!(
        !cached.is_nan(),
        "MinContent X for the Fill+wrap child must be cached after run"
    );
}

/// `engine.intrinsic` must short-circuit on cache hit. We poison
/// the slot with a sentinel and verify the next query returns it
/// — a recompute would overwrite the sentinel with the real value.
#[test]
fn intrinsic_query_short_circuits_on_cache_hit() {
    let mut ui = Ui::for_test();
    let mut root = NodeId(0);
    ui.run_at_without_baseline(UVec2::new(400, 300), |ui| {
        root = Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("hello world")
                    .id_salt("msg")
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
            })
            .response
            .node();
    });

    let child = ui.forest.trees[Layer::Main]
        .children(root)
        .map(|c| c.id)
        .next()
        .unwrap();
    let slot = LenReq::MinContent.slot(Axis::X);

    const SENTINEL: f32 = 1234.5;
    ui.layout_engine.scratch.intrinsics[child.idx()][slot] = SENTINEL;

    let payloads = ui.forest.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let tc = TextCtx { bytes: &text_bytes };
    let v = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        child,
        Axis::X,
        LenReq::MinContent,
        &tc,
    );
    assert_eq!(
        v, SENTINEL,
        "cache hit must return the stored value verbatim, not recompute"
    );

    let expected_max = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        child,
        Axis::X,
        LenReq::MaxContent,
        &tc,
    );
    let max_slot = LenReq::MaxContent.slot(Axis::X);
    ui.layout_engine.scratch.intrinsics[child.idx()][max_slot] = f32::NAN;
    ui.layout_engine.scratch.intrinsic_computes = 0;
    let range =
        ui.layout_engine
            .intrinsic_range(&ui.forest.trees[Layer::Main], child, Axis::X, &tc);
    assert_eq!(
        range,
        IntrinsicRange {
            min: SENTINEL,
            max: expected_max,
        },
        "a partially cached range must preserve the populated side",
    );
    assert_eq!(
        ui.layout_engine.scratch.intrinsic_computes, 1,
        "only the missing max-content side should compute",
    );
    drop(text_bytes);
    drop(payloads);
}

/// Recursive intrinsic queries must populate descendant slots too,
/// not just the queried node — `stack::intrinsic` etc. recurse
/// through `engine.intrinsic`, which writes the cache at every
/// level. Without this, deep trees would re-walk on every parent
/// query.
#[test]
fn parent_intrinsic_query_populates_descendant_cache() {
    let mut ui = Ui::for_test();
    let mut root = NodeId(0);
    // `run_at` populates `tree.rollups` (leaf intrinsic reads it).
    // Then clear *just the queried slot* on every node so we can
    // observe which nodes the parent query repopulates.
    ui.run_at_without_baseline(UVec2::new(400, 300), |ui| {
        root = Panel::hstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("abc").id_salt("a").show(ui);
                Text::new("defgh").id_salt("b").show(ui);
            })
            .response
            .node();
    });
    // Drop the measure-cache snapshots so `engine.intrinsic` can't
    // answer the root query from last frame's cached intrinsic — this
    // test pins the *recursive compute* path that populates descendant
    // scratch slots, which the cross-frame lookup would otherwise skip.
    ui.layout_engine.cache.clear();
    let slot = LenReq::MaxContent.slot(Axis::X);
    for entry in ui.layout_engine.scratch.intrinsics.iter_mut() {
        entry[slot] = f32::NAN;
    }

    let payloads = ui.forest.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let _ = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        root,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx { bytes: &text_bytes },
    );
    drop(text_bytes);
    drop(payloads);

    assert!(
        !ui.layout_engine.scratch.intrinsics[root.idx()][slot].is_nan(),
        "root slot must be cached"
    );
    for c in ui.forest.trees[Layer::Main].children(root).map(|c| c.id) {
        assert!(
            !ui.layout_engine.scratch.intrinsics[c.idx()][slot].is_nan(),
            "child {} slot must be cached after parent query",
            c.idx()
        );
    }
}

#[test]
fn intrinsic_range_exactly_matches_separate_queries_for_every_driver() {
    fn fixed(ui: &mut Ui, id: &'static str, size: (f32, f32)) {
        Frame::new().id_salt(id).size(size).show(ui);
    }

    let mut ui = Ui::for_test();
    ui.run_at_without_baseline(UVec2::new(1200, 900), |ui| {
        Panel::vstack().id_salt("range-root").show(ui, |ui| {
            Text::new("leaf alpha-beta")
                .id_salt("range-leaf")
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
            Panel::hstack().id_salt("range-hstack").show(ui, |ui| {
                fixed(ui, "range-hstack-child", (20.0, 10.0));
            });
            Panel::wrap_hstack()
                .id_salt("range-wrap-hstack")
                .gap(3.0)
                .show(ui, |ui| {
                    fixed(ui, "range-wrap-h-child", (30.0, 12.0));
                });
            Panel::wrap_vstack()
                .id_salt("range-wrap-vstack")
                .gap(5.0)
                .show(ui, |ui| {
                    fixed(ui, "range-wrap-v-child", (14.0, 25.0));
                });
            Panel::zstack().id_salt("range-zstack").show(ui, |ui| {
                fixed(ui, "range-zstack-child", (22.0, 18.0));
            });
            Panel::canvas().id_salt("range-canvas").show(ui, |ui| {
                Frame::new()
                    .id_salt("range-canvas-child")
                    .position((7.0, 9.0))
                    .size((19.0, 13.0))
                    .show(ui);
            });
            Grid::new()
                .id_salt("range-grid")
                .cols([Track::hug(), Track::fill()])
                .rows([Track::hug()])
                .gap(4.0)
                .show(ui, |ui| {
                    Text::new("grid label")
                        .id_salt("range-grid-label")
                        .grid_cell((0, 0))
                        .show(ui);
                    Frame::new()
                        .id_salt("range-grid-body")
                        .size((16.0, 11.0))
                        .grid_cell((0, 1))
                        .show(ui);
                });
            Scroll::vertical()
                .id_salt("range-scroll")
                .size((100.0, 60.0))
                .show(ui, |ui| {
                    fixed(ui, "range-scroll-child", (70.0, 90.0));
                });
        });
    });
    ui.layout_engine.cache.clear();

    let expected_modes = [
        LayoutMode::Leaf,
        LayoutMode::HStack,
        LayoutMode::VStack,
        LayoutMode::WrapHStack,
        LayoutMode::WrapVStack,
        LayoutMode::ZStack,
        LayoutMode::Canvas,
        LayoutMode::Grid(GridDefId::from_index(0)),
        LayoutMode::Scroll(ScrollSpec::VERTICAL),
    ];
    let tree = &ui.forest.trees[Layer::Main];
    for expected in expected_modes {
        assert!(
            tree.records.layout().iter().any(|layout| {
                std::mem::discriminant(&LayoutMode::from(layout.meta))
                    == std::mem::discriminant(&expected)
            }),
            "fixture must exercise {expected:?}",
        );
    }

    let payloads = ui.forest.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let tc = TextCtx { bytes: &text_bytes };
    for idx in 0..tree.records.len() {
        let node = NodeId(idx as u32);
        let mode = LayoutMode::from(tree.records.layout()[idx].meta);
        for axis in [Axis::X, Axis::Y] {
            ui.layout_engine
                .scratch
                .intrinsics
                .fill([f32::NAN; SLOT_COUNT]);
            ui.layout_engine.scratch.intrinsic_computes = 0;
            let min = ui
                .layout_engine
                .intrinsic(tree, node, axis, LenReq::MinContent, &tc);
            let max = ui
                .layout_engine
                .intrinsic(tree, node, axis, LenReq::MaxContent, &tc);
            let separate_computes = ui.layout_engine.scratch.intrinsic_computes;

            ui.layout_engine
                .scratch
                .intrinsics
                .fill([f32::NAN; SLOT_COUNT]);
            ui.layout_engine.scratch.intrinsic_computes = 0;
            let range = ui.layout_engine.intrinsic_range(tree, node, axis, &tc);
            let range_computes = ui.layout_engine.scratch.intrinsic_computes;

            assert_eq!(range.min, min, "{mode:?} {axis:?} min-content");
            assert_eq!(range.max, max, "{mode:?} {axis:?} max-content");
            assert_eq!(
                separate_computes,
                range_computes * 2,
                "{mode:?} {axis:?} must visit every computed node once per requested metric",
            );
        }
    }
}
