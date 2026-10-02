use crate::layout::axis::Axis;
use crate::layout::intrinsic::*;
use crate::scene::tree::node_id::NodeId;

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode};
use crate::layout::types::scroll_axes::ScrollAxes;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::scene::layer::Layer;
use crate::text::wrap::TextWrap;
use crate::widgets::configure::Configure;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::{block::Block, grid::Grid, panel::Panel, scroll::Scroll, text::Text};
use glam::UVec2;

/// Driver-triggered intrinsic queries during `run` must populate
/// the per-node cache. Without this, every `engine.intrinsic` call
/// would recompute from scratch — the 9% intrinsic cost in the
/// layout bench would balloon.
///
/// Uses the HStack-with-Fill-wrap pattern: pass-2 of
/// `Stack::measure` queries `MinContent` on each Fill child.
#[test]
fn intrinsic_cache_populated_after_run() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root = h.frame_value(|ui| {
        Panel::hstack()
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
            .node()
    });

    let child =
        h.ui.tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .next()
            .expect("hstack has child");
    let slot = LenReq::MinContent.slot(Axis::X);
    // Mono's 8 px a char: the widest unbreakable word is five chars, 40.
    assert_eq!(
        h.engines.layout.scratch.intrinsics[child.idx()][slot],
        40.0,
        "MinContent X for the Fill+wrap child must be cached after run"
    );
}

/// `engine.intrinsic` must short-circuit on cache hit. We poison
/// the slot with a sentinel and verify the next query returns it
/// — a recompute would overwrite the sentinel with the real value.
#[test]
fn intrinsic_query_short_circuits_on_cache_hit() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root = h.frame_value(|ui| {
        Panel::hstack()
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
            .node()
    });

    let child =
        h.ui.tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .next()
            .unwrap();
    let slot = LenReq::MinContent.slot(Axis::X);

    const SENTINEL: f32 = 1234.5;
    h.engines.layout.scratch.intrinsics[child.idx()][slot] = SENTINEL;

    let v = h.intrinsic(child, Axis::X, LenReq::MinContent);
    assert_eq!(
        v, SENTINEL,
        "cache hit must return the stored value verbatim, not recompute"
    );

    let expected_max = h.intrinsic(child, Axis::X, LenReq::MaxContent);
    let max_slot = LenReq::MaxContent.slot(Axis::X);
    h.engines.layout.scratch.intrinsics[child.idx()][max_slot] = f32::NAN;
    h.engines.layout.scratch.counters.reset_intrinsic_computes();
    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    let range =
        h.engines
            .layout
            .intrinsic_range(h.ui.tree(Layer::Main), child, Axis::X, &interned_text);
    assert_eq!(
        range,
        IntrinsicRange {
            min: SENTINEL,
            max: expected_max,
        },
        "a partially cached range must preserve the populated side",
    );
    assert_eq!(
        h.engines.layout.scratch.counters.intrinsic_computes(),
        1,
        "only the missing max-content side should compute",
    );
}

/// Recursive intrinsic queries must populate descendant slots too,
/// not just the queried node — `Stack::intrinsic` etc. recurse
/// through `engine.intrinsic`, which writes the cache at every
/// level. Without this, deep trees would re-walk on every parent
/// query.
#[test]
fn parent_intrinsic_query_populates_descendant_cache() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    // `run_at` populates `tree.rollups` (leaf intrinsic reads it).
    // Then clear *just the queried slot* on every node so we can
    // observe which nodes the parent query repopulates.
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("abc").id_salt("a").show(ui);
                Text::new("defgh").id_salt("b").show(ui);
            })
            .response
            .node()
    });
    // Drop the measure-cache snapshots so `engine.intrinsic` can't
    // answer the root query from last frame's cached intrinsic — this
    // test pins the *recursive compute* path that populates descendant
    // scratch slots, which the cross-frame lookup would otherwise skip.
    h.engines.layout.cache.forget_all();
    let slot = LenReq::MaxContent.slot(Axis::X);
    for entry in h.engines.layout.scratch.intrinsics.iter_mut() {
        entry[slot] = f32::NAN;
    }

    let _ = h.intrinsic(root, Axis::X, LenReq::MaxContent);

    // Mono's 8 px a char: "abc" is 24, "defgh" 40, side by side 64.
    let cached = |node: NodeId| h.engines.layout.scratch.intrinsics[node.idx()][slot];
    assert_eq!(cached(root), 64.0, "root slot must be cached");
    let children: Vec<_> =
        h.ui.tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .collect();
    assert_eq!(
        children.iter().map(|&c| cached(c)).collect::<Vec<_>>(),
        [24.0, 40.0],
        "each child slot must be cached after the parent query",
    );
}

#[test]
fn intrinsic_range_exactly_matches_separate_queries_for_every_driver() {
    fn fixed(ui: &mut Ui, id: &'static str, size: (f32, f32)) {
        Block::new().id_salt(id).size(size).show(ui);
    }

    let mut h = UiHarness::new(UVec2::new(1200, 900));
    h.frame(|ui| {
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
                Block::new()
                    .id_salt("range-canvas-child")
                    .position((7.0, 9.0))
                    .size((19.0, 13.0))
                    .show(ui);
            });
            Grid::new()
                .id_salt("range-grid")
                .cols([Track::HUG, Track::FILL])
                .rows([Track::HUG])
                .gap(4.0)
                .show(ui, |ui| {
                    Text::new("grid label")
                        .id_salt("range-grid-label")
                        .grid_cell((0, 0))
                        .show(ui);
                    Block::new()
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
    h.engines.layout.cache.forget_all();

    let expected_modes = [
        LayoutMode::Leaf,
        LayoutMode::Stack(Axis::X),
        LayoutMode::Stack(Axis::Y),
        LayoutMode::WrapStack(Axis::X),
        LayoutMode::WrapStack(Axis::Y),
        LayoutMode::ZStack,
        LayoutMode::Canvas,
        LayoutMode::Grid(GridDefId::from_index(0)),
        LayoutMode::Scroll(ScrollAxes::VERTICAL),
    ];
    let tree = h.ui.tree(Layer::Main);
    for expected in expected_modes {
        assert!(
            tree.records.layout().iter().any(|layout| {
                std::mem::discriminant(&LayoutMode::from(layout.meta))
                    == std::mem::discriminant(&expected)
            }),
            "fixture must exercise {expected:?}",
        );
    }

    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    for idx in 0..tree.records.len() {
        let node = NodeId(idx as u32);
        let mode = LayoutMode::from(tree.records.layout()[idx].meta);
        for axis in [Axis::X, Axis::Y] {
            h.engines.layout.forget_intrinsics();
            let min =
                h.engines
                    .layout
                    .intrinsic(tree, node, axis, LenReq::MinContent, &interned_text);
            let max =
                h.engines
                    .layout
                    .intrinsic(tree, node, axis, LenReq::MaxContent, &interned_text);
            let separate_computes = h.engines.layout.scratch.counters.intrinsic_computes();

            h.engines.layout.forget_intrinsics();
            let range = h
                .engines
                .layout
                .intrinsic_range(tree, node, axis, &interned_text);
            let range_computes = h.engines.layout.scratch.counters.intrinsic_computes();

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

/// One walk over a text leaf answers both axes, and the engine records
/// the axis the query did not name. A run's min-content and max-content
/// are `Size`s, so the named axis only picks a lane of what the walk
/// already holds; without the record, `LayoutPass::measure`'s pair of
/// min-content queries shapes the same runs twice.
///
/// Both lanes are pinned to hand-computed values, because a lane swap
/// would otherwise make the free lane agree with a cold query that swaps
/// it the same way. Under the mono metric at 16 px with a 1.0 line-height
/// multiplier a glyph is 8 px wide and a line is 16 px tall, so with
/// `WrapWithOverflow` the leaf demands its longest word, `lorem`, on X
/// and one line on Y:
///
/// - X: `5 * 8 + 2 * 3` padding `+ 2 * 1` margin = 48
/// - Y: `16 + 2 * 5` padding `+ 2 * 2` margin = 30
#[test]
fn a_leaf_intrinsic_walk_records_the_axis_it_was_not_asked_about() {
    const EXPECT_X: f32 = 48.0;
    const EXPECT_Y: f32 = 30.0;

    let mut h = UiHarness::new(UVec2::new(400, 300));
    let root = h.frame_value(|ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("lorem ipsum dolor sit amet")
                    .id_salt("msg")
                    .style(
                        &TextStyle::default()
                            .with_font_size(16.0)
                            .with_line_height_mult(1.0),
                    )
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .size((Sizing::HUG, Sizing::HUG))
                    .padding((3.0, 5.0))
                    .margin((1.0, 2.0))
                    .show(ui);
            })
            .response
            .node()
    });

    let leaf =
        h.ui.tree(Layer::Main)
            .children(root)
            .map(|c| c.id)
            .next()
            .expect("hstack has child");

    h.engines.layout.forget_intrinsics();
    let x = h.intrinsic(leaf, Axis::X, LenReq::MinContent);
    assert_eq!(
        x, EXPECT_X,
        "min-content X is the longest word plus the box"
    );

    let recorded =
        h.engines.layout.scratch.intrinsics[leaf.idx()][LenReq::MinContent.slot(Axis::Y)];
    assert_eq!(
        recorded, EXPECT_Y,
        "the X walk must record Y's outer min-content, padding and margin folded in",
    );
    assert!(
        h.engines.layout.scratch.intrinsics[leaf.idx()][LenReq::MaxContent.slot(Axis::Y)].is_nan(),
        "a min-content query may record only the half it asked for",
    );

    let y = h.intrinsic(leaf, Axis::Y, LenReq::MinContent);
    assert_eq!(y, EXPECT_Y, "the recorded lane is Y's own min-content");
    assert_eq!(
        h.engines.layout.scratch.counters.intrinsic_computes(),
        1,
        "the second axis must read the recorded lane, not shape the runs again",
    );
}
