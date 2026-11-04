use crate::internals::layout_rig::{GridTracks, LayoutRig, NodeRecorder, TextRun};
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::plane_input::PlaneInput;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode};
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::layout::track::Track;
use crate::scene::plane::Plane;
use crate::scene::tree::node_id::NodeId;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use std::mem;

const SURFACE: Size = Size::new(400.0, 300.0);

/// A Fill row around a Fill leaf `msg` that wraps `text` in the 16 px mono face, 8 px a char.
fn wrapping_row(rig: &mut LayoutRig, text: &str) {
    rig.run(SURFACE, |rec| {
        rec.node(
            "row",
            Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
            |rec| {
                rec.text_leaf(
                    "msg",
                    Widget::leaf().size((Sizing::FILL, Sizing::HUG)),
                    TextRun::new(text, 16.0, TextWrap::WrapWithOverflow),
                );
            },
        );
    });
}

/// Driver-triggered intrinsic queries during `run` populate the per-node cache; HStack with Fill wrap
/// triggers them (pass 2 of `Stack::measure`).
#[test]
fn intrinsic_cache_populated_after_run() {
    let mut rig = LayoutRig::new();
    wrapping_row(&mut rig, "lorem ipsum dolor sit amet");
    let slot = LenReq::MinContent.slot(Axis::X);
    // Mono's 8 px a char: the widest unbreakable word is five chars, 40.
    assert_eq!(
        rig.engine().scratch.measure.intrinsics[rig.node("msg").idx()][slot],
        40.0,
        "MinContent X for the Fill+wrap child must be cached after run"
    );
}

/// `IntrinsicPass::query` short-circuits on a cache hit: a sentinel in the slot must come back.
#[test]
fn intrinsic_query_short_circuits_on_cache_hit() {
    const SENTINEL: f32 = 1234.5;

    let mut rig = LayoutRig::new();
    wrapping_row(&mut rig, "hello world");
    let child = rig.node("msg");
    let slot = LenReq::MinContent.slot(Axis::X);
    rig.parts().engine.scratch.measure.intrinsics[child.idx()][slot] = SENTINEL;

    let v = rig.intrinsic("msg", Axis::X, LenReq::MinContent);
    assert_eq!(
        v, SENTINEL,
        "cache hit must return the stored value verbatim, not recompute"
    );

    let expected_max = rig.intrinsic("msg", Axis::X, LenReq::MaxContent);
    let max_slot = LenReq::MaxContent.slot(Axis::X);
    let parts = rig.parts();
    parts.engine.scratch.measure.intrinsics[child.idx()][max_slot] = f32::NAN;
    parts.engine.scratch.counters.reset_intrinsic_computes();
    let interned_text = parts.forest.record_store.interned_text();
    let input = PlaneInput {
        tree: &parts.forest.trees[Plane::MAIN],
        interned_text: &interned_text,
    };
    let range = IntrinsicPass::new(parts.engine, input).range(child, Axis::X);
    assert_eq!(
        range,
        IntrinsicRange {
            min: SENTINEL,
            max: expected_max,
        },
        "a partially cached range must preserve the populated side",
    );
    assert_eq!(
        parts.engine.scratch.counters.intrinsic_computes(),
        1,
        "only the missing max-content side should compute",
    );
}

/// Recursive intrinsic queries populate descendant slots too.
#[test]
fn parent_intrinsic_query_populates_descendant_cache() {
    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node("row", Widget::hstack(), |rec| {
            for (id, text) in [("a", "abc"), ("b", "defgh")] {
                rec.text_leaf(
                    id,
                    Widget::leaf(),
                    TextRun::new(text, 16.0, TextWrap::SingleLine),
                );
            }
        });
    });
    // Drop measure-cache snapshots so the root query takes the recursive compute path.
    let engine = rig.parts().engine;
    engine.cache.forget_all();
    let slot = LenReq::MaxContent.slot(Axis::X);
    for entry in &mut engine.scratch.measure.intrinsics {
        entry[slot] = f32::NAN;
    }

    rig.intrinsic("row", Axis::X, LenReq::MaxContent);

    // Mono's 8 px a char: "abc" is 24, "defgh" 40, side by side 64.
    let cached = |id| rig.engine().scratch.measure.intrinsics[rig.node(id).idx()][slot];
    assert_eq!(cached("row"), 64.0, "root slot must be cached");
    assert_eq!(
        [cached("a"), cached("b")],
        [24.0, 40.0],
        "each child slot must be cached after the parent query",
    );
}

#[test]
fn intrinsic_range_exactly_matches_separate_queries_for_every_driver() {
    fn fixed(rec: &mut NodeRecorder<'_>, id: &str, size: (f32, f32)) {
        rec.leaf(id, Widget::leaf().size(size));
    }

    let mut rig = LayoutRig::new();
    rig.run(Size::new(1200.0, 900.0), |rec| {
        rec.node("range-root", Widget::vstack(), |rec| {
            let wrap = TextWrap::WrapWithOverflow;
            rec.text_leaf(
                "range-leaf",
                Widget::leaf(),
                TextRun::new("leaf alpha-beta", 16.0, wrap),
            );
            rec.node("range-hstack", Widget::hstack(), |rec| {
                fixed(rec, "range-hstack-child", (20.0, 10.0));
            });
            rec.node("range-wrap-hstack", Widget::wrap_hstack().gap(3.0), |rec| {
                fixed(rec, "range-wrap-h-child", (30.0, 12.0));
            });
            rec.node("range-wrap-vstack", Widget::wrap_vstack().gap(5.0), |rec| {
                fixed(rec, "range-wrap-v-child", (14.0, 25.0));
            });
            rec.node("range-zstack", Widget::zstack(), |rec| {
                fixed(rec, "range-zstack-child", (22.0, 18.0));
            });
            rec.node("range-canvas", Widget::canvas(), |rec| {
                let child = Widget::leaf().at((7.0, 9.0)).size((19.0, 13.0));
                rec.leaf("range-canvas-child", child);
            });
            let tracks = GridTracks {
                rows: &[Track::HUG],
                cols: &[Track::HUG, Track::FILL],
            };
            rec.grid("range-grid", Widget::grid().gap(4.0), tracks, |rec| {
                let cell = Widget::leaf().grid_cell((0, 0));
                rec.text_leaf(
                    "range-grid-label",
                    cell,
                    TextRun::new("grid label", 16.0, TextWrap::SingleLine),
                );
                let body = Widget::leaf().size((16.0, 11.0)).grid_cell((0, 1));
                rec.leaf("range-grid-body", body);
            });
            let size = SizeSpec::new(Sizing::fixed(100.0), Sizing::fixed(60.0));
            let scroll = Widget::scroll(ScrollAxes::VERTICAL.fit_content(size)).size(size);
            rec.node("range-scroll", scroll, |rec| {
                fixed(rec, "range-scroll-child", (70.0, 90.0));
            });
        });
    });
    let parts = rig.parts();
    parts.engine.cache.forget_all();

    let expected_modes = [
        LayoutMode::Leaf,
        LayoutMode::Stack(Axis::X),
        LayoutMode::Stack(Axis::Y),
        LayoutMode::WrapStack(Axis::X),
        LayoutMode::WrapStack(Axis::Y),
        LayoutMode::ZStack,
        LayoutMode::Canvas,
        LayoutMode::Grid(GridDefId::from_idx(0)),
        LayoutMode::Scroll(ScrollAxes::VERTICAL),
    ];
    let tree = &parts.forest.trees[Plane::MAIN];
    for expected in expected_modes {
        assert!(
            tree.records.layout().iter().any(|layout| {
                mem::discriminant(&LayoutMode::from(layout.meta)) == mem::discriminant(&expected)
            }),
            "fixture must exercise {expected:?}",
        );
    }

    let interned_text = parts.forest.record_store.interned_text();
    let input = PlaneInput {
        tree,
        interned_text: &interned_text,
    };
    let layout = parts.engine;
    for idx in 0..tree.records.len() {
        let node = NodeId(u32::try_from(idx).unwrap());
        let mode = LayoutMode::from(tree.records.layout()[idx].meta);
        for axis in [Axis::X, Axis::Y] {
            layout.forget_intrinsics();
            let mut pass = IntrinsicPass::new(layout, input);
            let min = pass.single(node, axis, LenReq::MinContent);
            let max = pass.single(node, axis, LenReq::MaxContent);
            let separate_computes = layout.scratch.counters.intrinsic_computes();

            layout.forget_intrinsics();
            let range = IntrinsicPass::new(layout, input).range(node, axis);
            let range_computes = layout.scratch.counters.intrinsic_computes();

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

/// One walk over a text leaf answers both axes and records the axis not asked about, else
/// `LayoutPass::measure`'s pair of min-content queries shapes twice. Both lanes are pinned to
/// hand-computed values: mono at 16 px has 8-wide glyphs and 16-tall lines; the longest word `lorem` on X:
/// - X: `5 * 8 + 2 * 3` padding `+ 2 * 1` margin = 48
/// - Y: `16 + 2 * 5` padding `+ 2 * 2` margin = 30
#[test]
fn a_leaf_intrinsic_walk_records_the_axis_it_was_not_asked_about() {
    const EXPECT_X: f32 = 48.0;
    const EXPECT_Y: f32 = 30.0;

    let mut rig = LayoutRig::new();
    rig.run(SURFACE, |rec| {
        rec.node(
            "row",
            Widget::hstack().size((Sizing::FILL, Sizing::HUG)),
            |rec| {
                let leaf = Widget::leaf().padding((3.0, 5.0)).margin((1.0, 2.0));
                rec.text_leaf(
                    "msg",
                    leaf,
                    TextRun::new(
                        "lorem ipsum dolor sit amet",
                        16.0,
                        TextWrap::WrapWithOverflow,
                    ),
                );
            },
        );
    });
    let leaf = rig.node("msg");
    let slots = |rig: &LayoutRig| rig.engine().scratch.measure.intrinsics[leaf.idx()];

    rig.parts().engine.forget_intrinsics();
    let x = rig.intrinsic("msg", Axis::X, LenReq::MinContent);
    assert_eq!(
        x, EXPECT_X,
        "min-content X is the longest word plus the box"
    );

    assert_eq!(
        slots(&rig)[LenReq::MinContent.slot(Axis::Y)],
        EXPECT_Y,
        "the X walk must record Y's outer min-content, padding and margin folded in",
    );
    assert!(
        slots(&rig)[LenReq::MaxContent.slot(Axis::Y)].is_nan(),
        "a min-content query may record only the half it asked for",
    );

    let y = rig.intrinsic("msg", Axis::Y, LenReq::MinContent);
    assert_eq!(y, EXPECT_Y, "the recorded lane is Y's own min-content");
    assert_eq!(
        rig.engine().scratch.counters.intrinsic_computes(),
        1,
        "the second axis must read the recorded lane, not shape the runs again",
    );
}
