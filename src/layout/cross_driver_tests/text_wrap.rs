use crate::TextStyle;
use crate::Ui;
use crate::WidgetId;
use crate::forest::Layer;
use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::NodeId;
use crate::layout::cross_driver_tests::support;
use crate::layout::cross_driver_tests::support::{chat_message, two_hug_cols_with_wrap};
use crate::layout::support::TextCtx;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::primitives::color::Color;
use crate::renderer::frontend::cmd_buffer::{CmdKind, DrawTextPayload};
use crate::shape::{Shape, TextWrap};
use crate::text::{FontFamily, FontWeight};
use crate::widgets::{button::Button, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::rc::Rc;

const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut text_node = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(60.0), Sizing::Hug))
            .show(ui, |ui| {
                text_node = Some(
                    Text::new(PARAGRAPH)
                        .auto_id()
                        .style(TextStyle::default().with_font_size(16.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui)
                        .node(),
                );
            });
    });
    let node = text_node.unwrap();
    let r = ui.layout[Layer::Main].rect[node.idx()];
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
    );

    let shape = ui
        .forest
        .tree(Layer::Main)
        .shapes_of(node)
        .next()
        .expect("text shape");
    let wrap = match shape {
        ShapeRecord::Text { wrap, .. } => *wrap,
        _ => panic!("expected ShapeRecord::Text"),
    };
    assert_eq!(wrap, TextWrap::WrapWithOverflow);
    let shaped = support::shaped_text(&ui.layout[Layer::Main], node);
    assert!(shaped.measured.h > 32.0);
}

/// A `Button` with a label wider than its `Fixed` width elides to one
/// line instead of overflowing or wrapping *by default*: the body height
/// stays a single line (contrast `wrapping_text_grows_height_in_narrow_frame`,
/// where the same paragraph spans many) and the label shape carries
/// `TextWrap::SingleLine`.
#[test]
fn button_label_truncates_one_line_in_narrow_frame_by_default() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut node = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::Fixed(80.0), Sizing::Hug))
            .show(ui, |ui| {
                node = Some(Button::new().auto_id().label(PARAGRAPH).show(ui).node());
            });
    });
    let node = node.unwrap();

    let wrap = ui
        .forest
        .tree(Layer::Main)
        .shapes_of(node)
        .find_map(|s| match s {
            ShapeRecord::Text { wrap, .. } => Some(*wrap),
            _ => None,
        })
        .expect("button label text shape");
    assert_eq!(
        wrap,
        TextWrap::Truncate,
        "a button label defaults to the truncating wrap mode"
    );

    // The same paragraph wraps to >32 px tall in the wrap test; elided it
    // stays a single ~16 px line.
    let shaped = support::shaped_text(&ui.layout[Layer::Main], node);
    assert!(
        shaped.measured.h <= 32.0,
        "elided label must stay one line, got h={}",
        shaped.measured.h,
    );
    // And the elided line fits the button's fixed width (label width is
    // bounded by the 80 px box minus its padding).
    assert!(
        shaped.measured.w <= 80.0,
        "elided label must fit the button width, got w={}",
        shaped.measured.w,
    );
}

/// Pinned by `src/layout/intrinsic.md`: a wrapping `Text` inside a
/// `Grid` `Hug` column constrained by the parent's available width
/// reshapes to fit. The grid column-resolution algorithm runs during
/// measure with the grid's `inner_avail` (200 px here); the wrapping
/// text gets its committed column width before shaping, so the cached
/// shape is multi-line and fits the slot.
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 400));
    let mut node = None;
    ui.run_at_acked(UVec2::new(200, 400), |ui| {
        node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
    });
    let node = node.unwrap();
    let shaped = support::shaped_text(&ui.layout[Layer::Main], node);
    // 16 px font wraps to 3 lines at resolved col width — h ≈ 58.
    assert!(
        shaped.measured.h > 32.0,
        "expected multi-line wrapped height, got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 200.0,
        "expected text width within the 200 px surface, got w={}",
        shaped.measured.w,
    );
}

/// `Ui::intrinsic` returns sane values for a wrapping text leaf
/// inside a Grid `Auto` cell. Pure infrastructure test — confirms
/// the API + cache + per-driver functions are wired correctly.
#[test]
fn intrinsic_query_on_wrapping_text_leaf_returns_sensible_values() {
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 400));
    let mut node = None;
    ui.run_at_acked(UVec2::new(200, 400), |ui| {
        node = Some(two_hug_cols_with_wrap(ui, PARAGRAPH));
    });
    let node = node.unwrap();
    let max_w = ui.layout_engine.intrinsic(
        ui.forest.tree(Layer::Main),
        node,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &ui.ctx.frame_arena.inner().fmt_scratch,
            shaper: &ui.ctx.shaper,
        },
    );
    let min_w = ui.layout_engine.intrinsic(
        ui.forest.tree(Layer::Main),
        node,
        Axis::X,
        LenReq::MinContent,
        &TextCtx {
            bytes: &ui.ctx.frame_arena.inner().fmt_scratch,
            shaper: &ui.ctx.shaper,
        },
    );
    let max_h = ui.layout_engine.intrinsic(
        ui.forest.tree(Layer::Main),
        node,
        Axis::Y,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &ui.ctx.frame_arena.inner().fmt_scratch,
            shaper: &ui.ctx.shaper,
        },
    );

    assert!(
        max_w > 200.0,
        "max_w should be the natural unbroken width, got {max_w}"
    );
    assert!(
        min_w > 0.0 && min_w < max_w,
        "min_w should be positive and < max_w, got {min_w}"
    );
    assert!(
        min_w < 100.0,
        "min_w should be a single-word width, got {min_w}"
    );
    assert!(
        max_h > 0.0 && max_h < 30.0,
        "max_h should be single-line height, got {max_h}"
    );
}

/// Chat-message HStack pattern. Avatar (Fixed) + Message (Fill,
/// wrapping text). Without HStack-Fill min-content floor + width
/// commitment, message is measured at INF → shapes at natural width →
/// cached shape disagrees with arrange's slot.
#[test]
fn hstack_fill_wrap_text_reshapes_at_resolved_share() {
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 400));
    let mut msg = None;
    ui.run_at_acked(UVec2::new(200, 400), |ui| {
        msg = Some(chat_message(ui, 40.0, PARAGRAPH, 14.0));
    });
    let msg = msg.unwrap();
    let shaped = support::shaped_text(&ui.layout[Layer::Main], msg);
    assert!(
        shaped.measured.h > 32.0,
        "Fill message should wrap inside its resolved share; got h={}",
        shaped.measured.h,
    );
    assert!(
        shaped.measured.w <= 160.0,
        "wrapped message width should fit within Fill share; got w={}",
        shaped.measured.w,
    );
}

/// Pin: HStack `Fill` child respects `intrinsic_min` floor — when the
/// resolved share is smaller than the longest unbreakable word, the
/// child stays at min-content (overflows) rather than shrinking
/// further.
#[test]
fn hstack_fill_wrap_text_floors_at_min_content() {
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 400));
    let mut msg = None;
    ui.run_at_acked(UVec2::new(200, 400), |ui| {
        msg = Some(chat_message(ui, 180.0, "supercalifragilistic", 14.0));
    });
    let shaped = support::shaped_text(&ui.layout[Layer::Main], msg.unwrap());
    assert!(
        shaped.measured.w > 20.0,
        "min-content floor should keep message wider than the cramped slot; got w={}",
        shaped.measured.w,
    );
}

/// Pin (contains-content rule): a Stack's Fill child grows to fit
/// its measured content when the allocated slot is smaller than the
/// content's rigid min. The rect never paints content outside itself —
/// the overflow propagates upward (the parent stack rect ends up wider
/// than its `available`, and an ancestor that can grow absorbs it).
#[test]
fn hstack_fill_grows_to_content_when_slot_smaller_than_content() {
    let mut ui = Ui::for_test_at_text(UVec2::new(200, 400));
    let mut msg = None;
    ui.run_at_acked(UVec2::new(200, 400), |ui| {
        msg = Some(chat_message(ui, 180.0, "supercalifragilistic", 14.0));
    });
    let msg = msg.unwrap();
    let shaped_w = support::shaped_text(&ui.layout[Layer::Main], msg)
        .measured
        .w;
    let rect_w = ui.layout[Layer::Main].rect[msg.idx()].size.w;

    assert!(
        shaped_w > 50.0,
        "measure must floor at MinContent; got shaped_w={shaped_w}"
    );
    assert!(
        (rect_w - shaped_w).abs() <= 0.5,
        "rect must contain its measured content (no paint outside rect); \
         shaped_w={shaped_w} rect_w={rect_w}"
    );
}

/// Repro for the showcase "text layouts" first section: a Hug+Hug grid
/// holding a wrapping paragraph in col 0 and a *non-wrapping* label in
/// col 1, nested under FILL panels that inherit a finite surface width.
/// As the surface narrows below the grid's natural intrinsic floor, the
/// grid must clamp at that floor (col 0 = paragraph longest-word-or-line,
/// col 1 = full label width). It must NOT keep shrinking col 1 below its
/// label's natural width — non-wrapping text cannot be broken.
#[test]
fn two_hug_cols_nonwrapping_label_floors_at_full_width() {
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut grid_node = None;
        let mut section_node = None;
        Panel::vstack().auto_id()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack().auto_id()
                    .padding(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::vstack().auto_id()
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                section_node = Some(Panel::vstack().auto_id()
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .gap(6.0)
                                    .show(ui, |ui| {
                                        Text::new(
                                            "two Hug columns: paragraph wraps to fit, \
                                             label stays natural",
                                        )
                                        .id(WidgetId::from_hash("section-title"))
                                        .style(TextStyle::default().with_font_size(12.0))
                                        .text_wrap(TextWrap::SingleLine)
                                        .show(ui);
                                        grid_node = Some(
                                            Grid::new()
                                                .id(WidgetId::from_hash("grid"))
                                                .cols(Rc::from([Track::hug(), Track::hug()]))
                                                .rows(Rc::from([Track::hug()]))
                                                .show(ui, |ui| {
                                                    Text::new(
                                                        "the quick brown fox jumps over the lazy dog",
                                                    ).auto_id()
                                                    .style(TextStyle::default().with_font_size(14.0))
                                                    .text_wrap(TextWrap::WrapWithOverflow)
                                                    .grid_cell((0, 0))
                                                    .show(ui);
                                                    Text::new("right column").auto_id()
                                                        .style(
                                                            TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .text_wrap(TextWrap::SingleLine)
                                                        .grid_cell((0, 1))
                                                        .show(ui);
                                                })
                                                .node(),
                                        );
                                    }).node());
                            });
                    });
            });
        (grid_node.unwrap(), section_node.unwrap())
    }

    fn measure_at(surface_w: u32) -> (f32, f32) {
        let mut ui = Ui::for_test_at_text(UVec2::new(surface_w, 400));
        let mut nodes = None;
        ui.run_at_acked(UVec2::new(surface_w, 400), |ui| {
            nodes = Some(build(ui));
        });
        let (grid, section) = nodes.unwrap();
        let grid_w = ui.layout[Layer::Main].rect[grid.idx()].size.w;
        let section_w = ui.layout[Layer::Main].rect[section.idx()].size.w;
        (grid_w, section_w)
    }

    // Once the section panel stops shrinking (its intrinsic_min is
    // pinned wider than the grid's alone — by the section title text
    // here), the Hug grid inside must NOT keep shrinking. It should
    // fill the section's committed cross extent, not the smaller
    // surface-derived `available` the measure pass received before
    // flooring.
    let widths: [u32; 5] = [400, 300, 250, 200, 150];
    let mut section_widths = Vec::new();
    let mut grid_widths = Vec::new();
    for w in widths {
        let (g, s) = measure_at(w);
        section_widths.push(s);
        grid_widths.push(g);
    }
    // Find a pair of surface widths where the section width didn't
    // change (panel stopped shrinking). Grid width must also be stable
    // there.
    for i in 1..section_widths.len() {
        if (section_widths[i] - section_widths[i - 1]).abs() < 0.5 {
            let g_prev = grid_widths[i - 1];
            let g_curr = grid_widths[i];
            assert!(
                (g_curr - g_prev).abs() <= 0.5,
                "section panel stopped shrinking at {} but grid kept shrinking: \
                 surfaces {} → {}, grid {} → {}",
                section_widths[i],
                widths[i - 1],
                widths[i],
                g_prev,
                g_curr,
            );
            return;
        }
    }
    panic!(
        "test setup did not produce a regime where section panel stops shrinking; \
         widths={widths:?} section_widths={section_widths:?}"
    );
}

/// Pin: a non-wrapping `Text` reports MinContent on the X axis equal to
/// its full unbroken width, not the longest-word width. Wrapping text
/// reports the longest-word width, since it can break between words.
/// A Hug+Hug grid containing a wrapping paragraph and a non-wrapping
/// label must give the label its full natural width as a column floor —
/// otherwise the layout solver's slack distribution shrinks the label
/// column below the label's true width, and the label paint overflows
/// its arranged cell.
#[test]
fn nonwrapping_text_minconent_equals_full_width() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 200));
    let mut label_node = None;
    ui.run_at_acked(UVec2::new(400, 200), |ui| {
        label_node = Some(
            Text::new("right column")
                .auto_id()
                .style(TextStyle::default().with_font_size(14.0))
                .text_wrap(TextWrap::SingleLine)
                .show(ui)
                .node(),
        );
    });
    let label_node = label_node.unwrap();
    let max_w = ui.layout_engine.intrinsic(
        ui.forest.tree(Layer::Main),
        label_node,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &ui.ctx.frame_arena.inner().fmt_scratch,
            shaper: &ui.ctx.shaper,
        },
    );
    let min_w = ui.layout_engine.intrinsic(
        ui.forest.tree(Layer::Main),
        label_node,
        Axis::X,
        LenReq::MinContent,
        &TextCtx {
            bytes: &ui.ctx.frame_arena.inner().fmt_scratch,
            shaper: &ui.ctx.shaper,
        },
    );
    assert!(
        (min_w - max_w).abs() <= 0.5,
        "non-wrapping Text MinContent must equal MaxContent (full width); \
         max_w={max_w} min_w={min_w}",
    );
}

/// Pin issue 1a: in a `Hug+Hug` grid, when the surface is too narrow
/// to fit both columns at their natural max-content widths, the slack
/// distribution must allocate enough to the non-wrapping label column
/// for the label's full text to fit. The wrapping paragraph absorbs
/// the squeeze; the label cell rect width stays >= the label's natural
/// width.
#[test]
fn two_hug_cols_label_cell_never_shrinks_below_label_full_width() {
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut paragraph_node = None;
        let mut label_node = None;
        Grid::new()
            .id(WidgetId::from_hash("grid"))
            .cols(Rc::from([Track::hug(), Track::hug()]))
            .rows(Rc::from([Track::hug()]))
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                paragraph_node = Some(
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 0))
                        .show(ui)
                        .node(),
                );
                label_node = Some(
                    Text::new("right column")
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .text_wrap(TextWrap::SingleLine)
                        .grid_cell((0, 1))
                        .show(ui)
                        .node(),
                );
            });
        (paragraph_node.unwrap(), label_node.unwrap())
    }

    // Probe label's natural unbroken width at an unconstrained surface.
    let mut probe = Ui::for_test_at_text(UVec2::new(2000, 400));
    let mut probe_label = None;
    probe.run_at_acked(UVec2::new(2000, 400), |ui| {
        probe_label = Some(build(ui).1);
    });
    let probe_label = probe_label.unwrap();
    let label_full = probe.layout_engine.intrinsic(
        probe.forest.tree(Layer::Main),
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &probe.ctx.frame_arena.inner().fmt_scratch,
            shaper: &probe.ctx.shaper,
        },
    );
    assert!(label_full > 0.0);

    // At a surface narrower than the paragraph max-content but wider
    // than the grid's intrinsic floor, slack distribution kicks in.
    // The label cell must still get at least its full natural width.
    for surface_w in [400u32, 300, 250, 200] {
        let mut ui = Ui::for_test_at_text(UVec2::new(surface_w, 400));
        let mut label = None;
        ui.run_at_acked(UVec2::new(surface_w, 400), |ui| {
            label = Some(build(ui).1);
        });
        let label_rect_w = ui.layout[Layer::Main].rect[label.unwrap().idx()].size.w;
        assert!(
            label_rect_w >= label_full - 0.5,
            "label cell shrank below the label's natural width — \
             non-wrapping text would visually overflow its column. \
             surface_w={surface_w} label_full={label_full} label_rect_w={label_rect_w}",
        );
    }
}

/// Regression for the showcase "two Hug columns" grid: a **bare** label
/// (no `.text_wrap(...)`, so it takes the `Text` default) in a Hug+Hug grid
/// next to a wrapping paragraph must keep its full natural width — the
/// paragraph wraps to absorb the squeeze. This pins the default: `Text`
/// defaults to `TextWrap::Overflow`, whose MinContent equals its full line,
/// so the grid's Hug solver floors the label column at the label width and
/// never shrinks it (the old `SingleLine` default reported MinContent 0 and
/// the slack split clipped "right column" → "right col").
#[test]
fn two_hug_cols_default_label_hugs_full_width() {
    fn build(ui: &mut Ui) -> NodeId {
        let mut label_node = None;
        Grid::new()
            .id(WidgetId::from_hash("grid"))
            .cols(Rc::from([Track::hug(), Track::hug()]))
            .rows(Rc::from([Track::hug()]))
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                Text::new("the quick brown fox jumps over the lazy dog. pack my box with five dozen liquor jugs")
                    .auto_id()
                    .style(TextStyle::default().with_font_size(14.0))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((0, 0))
                    .show(ui);
                // No `.text_wrap(...)` — exercises the default.
                label_node = Some(
                    Text::new("right column")
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((0, 1))
                        .show(ui)
                        .node(),
                );
            });
        label_node.unwrap()
    }

    // Label's natural unbroken width, probed unconstrained.
    let mut probe = Ui::for_test_at_text(UVec2::new(2000, 400));
    let mut probe_label = None;
    probe.run_at_acked(UVec2::new(2000, 400), |ui| {
        probe_label = Some(build(ui));
    });
    let label_full = probe.layout_engine.intrinsic(
        probe.forest.tree(Layer::Main),
        probe_label.unwrap(),
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &probe.ctx.frame_arena.inner().fmt_scratch,
            shaper: &probe.ctx.shaper,
        },
    );
    assert!(label_full > 0.0);

    // The long paragraph's max-content dwarfs these surfaces, so the grid
    // is in the slack-distribution regime (paragraph wraps). The default
    // label must still occupy its full width at each.
    for surface_w in [600u32, 500, 400, 300] {
        let mut ui = Ui::for_test_at_text(UVec2::new(surface_w, 400));
        let mut label = None;
        ui.run_at_acked(UVec2::new(surface_w, 400), |ui| {
            label = Some(build(ui));
        });
        let label_rect_w = ui.layout[Layer::Main].rect[label.unwrap().idx()].size.w;
        assert!(
            label_rect_w >= label_full - 0.5,
            "default-wrap label shrank below its natural width — it would clip. \
             surface_w={surface_w} label_full={label_full} label_rect_w={label_rect_w}",
        );
    }
}

/// Two `ShapeRecord::Text` runs in one leaf:
///   slot 0: "first" at `local_rect: Some((0, 0)+100x20)`,
///   slot 1: "second-with-different-text" at `Some((0, 22)+100x20)`.
/// Returns the leaf NodeId so callers can read `text_spans` /
/// emitted commands. Used by the multi-text-per-leaf pinning tests.
fn build_multi_text_leaf(ui: &mut Ui) -> NodeId {
    let leaf_id = WidgetId::from_hash("multi-text-leaf");
    Panel::vstack().auto_id().show(ui, |ui| {
        let mut element = Element::new(LayoutMode::Leaf);
        element.salt = Salt::Verbatim(leaf_id);
        ui.node(leaf_id, element, None, |ui| {
            ui.add_shape(Shape::Text {
                local_origin: Some(glam::Vec2::new(0.0, 0.0)),
                text: "first".into(),
                brush: Color::WHITE.into(),
                font_size_px: 14.0,
                line_height_px: 16.0,
                wrap: TextWrap::Truncate,
                align: Default::default(),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
            });
            ui.add_shape(Shape::Text {
                local_origin: Some(glam::Vec2::new(0.0, 22.0)),
                text: "second-with-different-text".into(),
                brush: Color::WHITE.into(),
                font_size_px: 14.0,
                line_height_px: 16.0,
                wrap: TextWrap::Truncate,
                align: Default::default(),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
            });
        });
    });
    ui.node_for_widget_id(leaf_id)
}

/// Pin: a custom widget that pushes two `ShapeRecord::Text` to the same
/// node has both runs shaped (`text_spans[node].len == 2`) at distinct
/// `TextCacheKey`s (no `TextShaper.reuse` collision). Replaces the
/// old "one ShapeRecord::Text per leaf" hard assert.
#[test]
fn multi_shape_text_per_leaf_shapes_each_run_independently() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut leaf = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        leaf = Some(build_multi_text_leaf(ui));
    });
    let leaf = leaf.unwrap();
    let span = ui.layout[Layer::Main].text_spans[leaf.idx()];
    assert_eq!(
        span.len, 2,
        "leaf with two ShapeRecord::Text should record two text-shape entries"
    );
    let first = ui.layout[Layer::Main].text_shapes[span.start as usize];
    let second = ui.layout[Layer::Main].text_shapes[(span.start + 1) as usize];
    assert!(
        first.measured.w > 0.0 && second.measured.w > 0.0,
        "both runs must have measured nonzero width: first={:?} second={:?}",
        first.measured,
        second.measured,
    );
    assert!(
        second.measured.w > first.measured.w,
        "second run is longer text and should measure wider; first={} second={}",
        first.measured.w,
        second.measured.w,
    );
    assert_ne!(
        first.key, second.key,
        "different text inputs must produce distinct TextCacheKeys — \
         a collision would mean the second shape clobbered the first's cache slot",
    );
}

/// Pin: encoder emits one `DrawText` per `ShapeRecord::Text` in record
/// order, and `local_rect: Some(lr)` shifts the emitted rect by
/// `lr.min` (relative to the owner). Without per-shape `text_ordinal`
/// indexing or the `local_rect` branch, the second run would either
/// re-paint the first's shaped buffer or sit on top of the first.
#[test]
fn multi_shape_text_per_leaf_emits_one_drawtext_per_run_at_local_rect() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut leaf = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        leaf = Some(build_multi_text_leaf(ui));
    });
    let leaf = leaf.unwrap();
    let owner_min = ui.layout[Layer::Main].rect[leaf.idx()].min;
    let cmds = ui.encode_cmds();
    let mut drawn: Vec<glam::Vec2> = (0..cmds.kinds.len())
        .filter(|&i| cmds.kinds[i] == CmdKind::DrawText)
        .map(|i| cmds.read::<DrawTextPayload>(cmds.starts[i]).rect.min)
        .collect();
    assert_eq!(
        drawn.len(),
        2,
        "leaf with two ShapeRecord::Text must emit two DrawText cmds; got {drawn:?}"
    );
    drawn.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    let [low, high] = [drawn[0], drawn[1]];
    // Slot 0 (`local_rect.min = (0, 0)`) → DrawText.min == owner.min.
    assert!(
        (low.y - owner_min.y).abs() < 0.5,
        "slot 0 with local_rect=(0,0) should emit at owner_min.y; \
         owner_min={owner_min:?} low={low:?}",
    );
    // Slot 1 (`local_rect.min = (0, 22)`) → DrawText.min.y == owner.min.y + 22.
    assert!(
        (high.y - (owner_min.y + 22.0)).abs() < 0.5,
        "slot 1 with local_rect.y=22 should emit shifted by 22 from owner_min.y; \
         owner_min={owner_min:?} high={high:?}",
    );
    // Distinct y proves the two emissions are not aliased.
    assert!(
        (high.y - low.y).abs() >= 20.0,
        "two DrawText must paint at distinct y; got {low:?} {high:?}",
    );
}

/// Pin: the cross-frame measure cache replays multi-text leaves
/// correctly. Frame 1 populates the cache; frame 2 hits and rebases
/// the snapshot's subtree-local spans + flat text-shapes back into
/// the per-frame buffer. Without correct rebase (e.g. forgetting
/// `dest_start += text_shapes.len()` or storing global indices in
/// the snapshot), frame 2 would either read from the wrong slot or
/// see stale `TextCacheKey`s.
#[test]
fn multi_shape_text_per_leaf_round_trips_through_measure_cache() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut f1_leaf = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        f1_leaf = Some(build_multi_text_leaf(ui));
    });
    let f1_leaf = f1_leaf.unwrap();
    let f1_span = ui.layout[Layer::Main].text_spans[f1_leaf.idx()];
    let f1_first = ui.layout[Layer::Main].text_shapes[f1_span.start as usize];
    let f1_second = ui.layout[Layer::Main].text_shapes[(f1_span.start + 1) as usize];

    let mut f2_leaf = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        f2_leaf = Some(build_multi_text_leaf(ui));
    });
    let f2_leaf = f2_leaf.unwrap();
    let f2_span = ui.layout[Layer::Main].text_spans[f2_leaf.idx()];
    assert_eq!(f2_span.len, 2, "frame 2 must restore both text-shape slots");
    let f2_first = ui.layout[Layer::Main].text_shapes[f2_span.start as usize];
    let f2_second = ui.layout[Layer::Main].text_shapes[(f2_span.start + 1) as usize];

    assert_eq!(
        (f1_first.key, f1_second.key),
        (f2_first.key, f2_second.key),
        "cache hit must replay the exact same TextCacheKeys per slot",
    );
    assert!(
        (f1_first.measured.w - f2_first.measured.w).abs() < 0.01
            && (f1_second.measured.w - f2_second.measured.w).abs() < 0.01,
        "cache hit must replay the exact same measured sizes per slot; \
         f1=({:?}, {:?}) f2=({:?}, {:?})",
        f1_first.measured,
        f1_second.measured,
        f2_first.measured,
        f2_second.measured,
    );
}

/// Pin (contains-content rule, cross axis): a FILL chrome panel
/// wrapping a paragraph in a Fixed(width) inner panel must grow on Y
/// to contain its wrapped content, even when surface_h is smaller.
/// The intrinsic-min query alone underestimates this (wrapping text
/// intrinsic runs at INF width → single-line height), so the floor
/// has to come from the post-dispatch measured content. Without the
/// fix, surface_h < natural content height makes the chrome panel
/// rect shorter than its content, visibly clipping at the bottom.
#[test]
fn fill_panel_grows_to_contain_wrapped_content_on_y() {
    use crate::forest::tree::NodeId;
    use crate::widgets::panel::Panel;
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut inner = NodeId(0);
        Panel::zstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                inner = Panel::vstack()
                    .id_salt("inner")
                    .size((Sizing::Fixed(360.0), Sizing::Hug))
                    .padding(8.0)
                    .show(ui, |ui| {
                        Text::new(
                            "The quick brown fox jumps over the lazy dog. \
                             Pack my box with five dozen liquor jugs. \
                             How vexingly quick daft zebras jump!",
                        )
                        .auto_id()
                        .style(TextStyle::default().with_font_size(14.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .show(ui);
                    })
                    .node();
            });
        // The chrome panel is the first child of the implicit root.
        (NodeId(1), inner)
    }
    // The inner Fixed-width panel is Hug on Y, so its rect.size.h is the
    // measured wrapped-paragraph height (+ inner padding). Chrome must
    // be at least that + chrome padding (16*2 = 32) on Y, at every
    // surface height — including ones smaller than the natural content.
    for h in [800u32, 400, 300, 200, 150, 100, 50] {
        let mut ui = Ui::for_test_at_text(UVec2::new(800, h));
        let mut nodes = (NodeId(0), NodeId(0));
        ui.run_at_acked(UVec2::new(800, h), |ui| {
            nodes = build(ui);
        });
        let (chrome, inner) = nodes;
        let chrome_h = ui.layout[Layer::Main].rect[chrome.idx()].size.h;
        let inner_h = ui.layout[Layer::Main].rect[inner.idx()].size.h;
        let floor = inner_h + 32.0;
        assert!(
            chrome_h + 0.5 >= floor,
            "FILL chrome panel must contain its inner panel on Y at surface_h={h}; \
             chrome_h={chrome_h} inner_h={inner_h} required_floor={floor}",
        );
    }
}
