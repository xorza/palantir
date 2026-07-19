use crate::TextStyle;
use crate::Ui;
use crate::WidgetId;
use crate::forest::element::{Configure, Element, Salt};
use crate::forest::layer::Layer;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::node::NodeId;
use crate::forest::visibility::Visibility;
use crate::layout::cross_driver_tests::support;
use crate::layout::cross_driver_tests::support::{chat_message, two_hug_cols_with_wrap};
use crate::layout::support::TextCtx;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::renderer::frontend::cmd_buffer::Command;
use crate::shape::{Shape, TextWrap};
use crate::text::{FontFamily, FontWeight};
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};
use glam::UVec2;

const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

fn add_direct_text(
    ui: &mut Ui,
    text: &'static str,
    font_size_px: f32,
    line_height_px: f32,
    wrap: TextWrap,
    local_origin: Option<glam::Vec2>,
) {
    let text = ui.intern(text);
    ui.add_shape(Shape::Text {
        local_origin,
        text,
        color: Color::WHITE,
        font_size_px,
        line_height_px,
        wrap,
        align: Default::default(),
        family: FontFamily::Sans,
        weight: FontWeight::Regular,
    });
}

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let mut text_node = None;
    ui.run_at_acked(UVec2::new(400, 400), |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::fixed(60.0), Sizing::HUG))
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

    let shape = ui.forest.trees[Layer::Main]
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
            .size((Sizing::fixed(80.0), Sizing::HUG))
            .show(ui, |ui| {
                node = Some(Button::new().auto_id().label(PARAGRAPH).show(ui).node());
            });
    });
    let node = node.unwrap();

    let wrap = ui.forest.trees[Layer::Main]
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
    let node = ui.run_at_value_acked(UVec2::new(200, 400), |ui| {
        two_hug_cols_with_wrap(ui, PARAGRAPH)
    });
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
    let node = ui.run_at_value_acked(UVec2::new(200, 400), |ui| {
        two_hug_cols_with_wrap(ui, PARAGRAPH)
    });
    let payloads = ui.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let max_w = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        node,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &ui.shared.text,
        },
    );
    let min_w = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        node,
        Axis::X,
        LenReq::MinContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &ui.shared.text,
        },
    );
    let max_h = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        node,
        Axis::Y,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &ui.shared.text,
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
    let msg = ui.run_at_value_acked(UVec2::new(200, 400), |ui| {
        chat_message(ui, 40.0, PARAGRAPH, 14.0)
    });
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
    let msg = ui.run_at_value_acked(UVec2::new(200, 400), |ui| {
        chat_message(ui, 180.0, "supercalifragilistic", 14.0)
    });
    let shaped = support::shaped_text(&ui.layout[Layer::Main], msg);
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
    let msg = ui.run_at_value_acked(UVec2::new(200, 400), |ui| {
        chat_message(ui, 180.0, "supercalifragilistic", 14.0)
    });
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
                                    .size((Sizing::FILL, Sizing::HUG))
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
                                                .cols([Track::hug(), Track::hug()])
                                                .rows([Track::hug()])
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
        let nodes = ui.run_at_value_acked(UVec2::new(surface_w, 400), build);
        let (grid, section) = nodes;
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
    let label_node = ui.run_at_value_acked(UVec2::new(400, 200), |ui| {
        Text::new("right column")
            .auto_id()
            .style(TextStyle::default().with_font_size(14.0))
            .text_wrap(TextWrap::SingleLine)
            .show(ui)
            .node()
    });
    let payloads = ui.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let max_w = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        label_node,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &ui.shared.text,
        },
    );
    let min_w = ui.layout_engine.intrinsic(
        &ui.forest.trees[Layer::Main],
        label_node,
        Axis::X,
        LenReq::MinContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &ui.shared.text,
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
            .cols([Track::hug(), Track::hug()])
            .rows([Track::hug()])
            .size((Sizing::FILL, Sizing::HUG))
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
    let probe_label = probe.run_at_value_acked(UVec2::new(2000, 400), |ui| build(ui).1);
    let payloads = probe.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let label_full = probe.layout_engine.intrinsic(
        &probe.forest.trees[Layer::Main],
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &probe.shared.text,
        },
    );
    assert!(label_full > 0.0);

    // At a surface narrower than the paragraph max-content but wider
    // than the grid's intrinsic floor, slack distribution kicks in.
    // The label cell must still get at least its full natural width.
    for surface_w in [400u32, 300, 250, 200] {
        let mut ui = Ui::for_test_at_text(UVec2::new(surface_w, 400));
        let label = ui.run_at_value_acked(UVec2::new(surface_w, 400), |ui| build(ui).1);
        let label_rect_w = ui.layout[Layer::Main].rect[label.idx()].size.w;
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
        Grid::new()
          .id(WidgetId::from_hash("grid"))
          .cols([Track::hug(), Track::hug()])
          .rows([Track::hug()])
          .size((Sizing::FILL, Sizing::HUG))
          .show(ui, |ui| {
              Text::new("the quick brown fox jumps over the lazy dog. pack my box with five dozen liquor jugs")
                  .auto_id()
                  .style(TextStyle::default().with_font_size(14.0))
                  .text_wrap(TextWrap::WrapWithOverflow)
                  .grid_cell((0, 0))
                  .show(ui);
              // No `.text_wrap(...)` — exercises the default.
              Text::new("right column")
                  .auto_id()
                  .style(TextStyle::default().with_font_size(14.0))
                  .grid_cell((0, 1))
                  .show(ui)
                  .node()
          })
          .inner
    }

    // Label's natural unbroken width, probed unconstrained.
    let mut probe = Ui::for_test_at_text(UVec2::new(2000, 400));
    let probe_label = probe.run_at_value_acked(UVec2::new(2000, 400), build);
    let payloads = probe.record_store.payloads.borrow();
    let text_bytes = payloads.text_bytes();
    let label_full = probe.layout_engine.intrinsic(
        &probe.forest.trees[Layer::Main],
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &TextCtx {
            bytes: &text_bytes,
            shaper: &probe.shared.text,
        },
    );
    assert!(label_full > 0.0);

    // The long paragraph's max-content dwarfs these surfaces, so the grid
    // is in the slack-distribution regime (paragraph wraps). The default
    // label must still occupy its full width at each.
    for surface_w in [600u32, 500, 400, 300] {
        let mut ui = Ui::for_test_at_text(UVec2::new(surface_w, 400));
        let label = ui.run_at_value_acked(UVec2::new(surface_w, 400), build);
        let label_rect_w = ui.layout[Layer::Main].rect[label.idx()].size.w;
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
        let mut element = Element::leaf();
        element.salt = Salt::Verbatim(leaf_id);
        ui.node(leaf_id, element, None, |ui| {
            let first = ui.intern("first");
            ui.add_shape(Shape::Text {
                local_origin: Some(glam::Vec2::new(0.0, 0.0)),
                text: first,
                color: Color::WHITE,
                font_size_px: 14.0,
                line_height_px: 16.0,
                wrap: TextWrap::Truncate,
                align: Default::default(),
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
            });
            ui.add_shape(Shape::RoundedRect {
                local_rect: Some(crate::Rect::new(0.0, 20.0, 4.0, 2.0)),
                corners: crate::Corners::ZERO,
                fill: Color::WHITE.into(),
                stroke: crate::Stroke::ZERO,
            });
            let second = ui.intern("second-with-different-text");
            ui.add_shape(Shape::Text {
                local_origin: Some(glam::Vec2::new(0.0, 22.0)),
                text: second,
                color: Color::WHITE,
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

#[derive(Debug)]
struct ContainerTextScene {
    container: NodeId,
    child: NodeId,
}

fn build_wrapping_container_text(ui: &mut Ui) -> ContainerTextScene {
    let mut child = None;
    let container = Panel::vstack()
        .id_salt("wrapping-container-text")
        .size((Sizing::HUG, Sizing::HUG))
        .padding(10.0)
        .show(ui, |ui| {
            add_direct_text(ui, PARAGRAPH, 14.0, 16.0, TextWrap::Wrap, None);
            child = Some(
                Frame::new()
                    .id_salt("container-size-driver")
                    .size((Sizing::fixed(80.0), Sizing::fixed(20.0)))
                    .show(ui)
                    .node(),
            );
        })
        .node();
    ContainerTextScene {
        container,
        child: child.unwrap(),
    }
}

fn build_interleaved_container_text(ui: &mut Ui) -> ContainerTextScene {
    let mut child = None;
    let container = Panel::vstack()
        .id_salt("interleaved-container-text")
        .size((Sizing::fixed(240.0), Sizing::fixed(100.0)))
        .show(ui, |ui| {
            add_direct_text(
                ui,
                "parent-before",
                12.0,
                14.0,
                TextWrap::SingleLine,
                Some(glam::Vec2::new(0.0, 0.0)),
            );
            child = Some(
                Text::new("child-between")
                    .id_salt("interleaved-child")
                    .style(TextStyle::default().with_font_size(18.0))
                    .show(ui)
                    .node(),
            );
            add_direct_text(
                ui,
                "parent-after-is-longer",
                14.0,
                16.0,
                TextWrap::SingleLine,
                Some(glam::Vec2::new(0.0, 60.0)),
            );
        })
        .node();
    ContainerTextScene {
        container,
        child: child.unwrap(),
    }
}

fn build_container_text_with_visibility(ui: &mut Ui, visibility: Visibility) -> NodeId {
    Panel::vstack()
        .id_salt("container-text-visibility")
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .padding(10.0)
        .visibility(visibility)
        .show(ui, |ui| {
            add_direct_text(ui, PARAGRAPH, 14.0, 16.0, TextWrap::Wrap, None);
        })
        .node()
}

#[test]
fn container_text_is_paint_only_and_wraps_to_final_inner_width() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let scene = ui.run_at_value_acked(UVec2::new(400, 400), build_wrapping_container_text);
    let layout = &ui.layout[Layer::Main];
    assert_eq!(layout.text_shapes.len(), 1);
    let container_rect = layout.rect[scene.container.idx()];
    let child_rect = layout.rect[scene.child.idx()];

    assert_eq!(child_rect.size, Size::new(80.0, 20.0));
    assert_eq!(container_rect.size, Size::new(100.0, 40.0));

    let span = layout.text_spans[scene.container.idx()];
    assert_eq!(span.len, 1, "container owns one direct text run");
    let shaped = layout.text_shapes[span.start as usize];
    assert_eq!(shaped.measured, Size::new(73.0, 80.0));

    let draw_keys: Vec<_> = ui
        .encode_cmds()
        .iter()
        .filter_map(|command| match command {
            Command::DrawText(payload) => Some(payload.key),
            _ => None,
        })
        .collect();
    assert_eq!(draw_keys, [shaped.key]);
    let leaf = ui.run_at_value_acked(UVec2::new(400, 400), |ui| {
        Text::new("leaf-only").show(ui).node()
    });
    let layout = &ui.layout[Layer::Main];
    assert_eq!(layout.text_shapes.len(), 1);
    assert_eq!(layout.text_spans[leaf.idx()].len, 1);
}

#[test]
fn container_text_visibility_distinguishes_hidden_from_collapsed() {
    let surface = UVec2::new(400, 400);
    let mut ui = Ui::for_test_at_text(surface);
    let hidden_node = ui.run_at_value_acked(surface, |ui| {
        build_container_text_with_visibility(ui, Visibility::Hidden)
    });
    let hidden_layout = &ui.layout[Layer::Main];
    assert_eq!(
        hidden_layout.rect[hidden_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    assert_eq!(hidden_layout.text_spans[hidden_node.idx()].len, 0);
    assert!(hidden_layout.text_shapes.is_empty());

    let collapsed_node = ui.run_at_value_acked(surface, |ui| {
        build_container_text_with_visibility(ui, Visibility::Collapsed)
    });
    let collapsed_layout = &ui.layout[Layer::Main];
    assert_eq!(collapsed_layout.rect[collapsed_node.idx()].size, Size::ZERO);
    assert_eq!(collapsed_layout.text_spans[collapsed_node.idx()].len, 0);
    assert!(collapsed_layout.text_shapes.is_empty());

    let visible_node = ui.run_at_value_acked(surface, |ui| {
        build_container_text_with_visibility(ui, Visibility::Visible)
    });
    let visible_layout = &ui.layout[Layer::Main];
    assert_eq!(
        visible_layout.rect[visible_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    let span = visible_layout.text_spans[visible_node.idx()];
    assert_eq!(span.len, 1);
    assert_eq!(
        visible_layout.text_shapes[span.start as usize].measured,
        Size::new(73.0, 80.0),
    );
}

#[test]
fn container_and_child_text_keep_independent_order_across_cache_hit() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let first_scene = ui.run_at_value_acked(UVec2::new(400, 400), |ui| {
        build_interleaved_container_text(ui)
    });
    let first_layout = &ui.layout[Layer::Main];
    assert_eq!(first_layout.text_shapes.len(), 3);
    let first_parent_span = first_layout.text_spans[first_scene.container.idx()];
    let first_child_span = first_layout.text_spans[first_scene.child.idx()];
    assert_eq!(first_parent_span.len, 2);
    assert_eq!(first_child_span.len, 1);
    let first_parent_keys = [
        first_layout.text_shapes[first_parent_span.start as usize].key,
        first_layout.text_shapes[(first_parent_span.start + 1) as usize].key,
    ];
    let first_child_key = first_layout.text_shapes[first_child_span.start as usize].key;
    assert_ne!(first_parent_keys[0], first_child_key);
    assert_ne!(first_parent_keys[1], first_child_key);
    let first_draw_keys: Vec<_> = ui
        .encode_cmds()
        .iter()
        .filter_map(|command| match command {
            Command::DrawText(payload) => Some(payload.key),
            _ => None,
        })
        .collect();
    assert_eq!(
        first_draw_keys,
        [first_parent_keys[0], first_child_key, first_parent_keys[1]],
    );
    let second_scene = ui.run_at_value_acked(UVec2::new(400, 400), |ui| {
        build_interleaved_container_text(ui)
    });
    assert!(
        !ui.layout_engine.scratch.cache_hits.is_empty(),
        "second identical frame should exercise measure-cache replay",
    );
    let second_layout = &ui.layout[Layer::Main];
    assert_eq!(second_layout.text_shapes.len(), 3);
    let second_parent_span = second_layout.text_spans[second_scene.container.idx()];
    let second_child_span = second_layout.text_spans[second_scene.child.idx()];
    assert_eq!(second_parent_span.len, 2);
    assert_eq!(second_child_span.len, 1);
    let second_parent_keys = [
        second_layout.text_shapes[second_parent_span.start as usize].key,
        second_layout.text_shapes[(second_parent_span.start + 1) as usize].key,
    ];
    let second_child_key = second_layout.text_shapes[second_child_span.start as usize].key;
    assert_eq!(second_parent_keys, first_parent_keys);
    assert_eq!(second_child_key, first_child_key);
    let second_draw_keys: Vec<_> = ui
        .encode_cmds()
        .iter()
        .filter_map(|command| match command {
            Command::DrawText(payload) => Some(payload.key),
            _ => None,
        })
        .collect();
    assert_eq!(
        second_draw_keys,
        [
            second_parent_keys[0],
            second_child_key,
            second_parent_keys[1],
        ],
    );
}

/// Pin: a custom widget that pushes two `ShapeRecord::Text` to the same
/// node has both runs shaped (`text_spans[node].len == 2`) at distinct
/// `TextCacheKey`s (no `TextReuseCache` collision). Replaces the
/// old "one ShapeRecord::Text per leaf" hard assert.
#[test]
fn multi_shape_text_per_leaf_shapes_each_run_independently() {
    let mut ui = Ui::for_test_at_text(UVec2::new(400, 400));
    let leaf = ui.run_at_value_acked(UVec2::new(400, 400), build_multi_text_leaf);
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
    let leaf = ui.run_at_value_acked(UVec2::new(400, 400), build_multi_text_leaf);
    let owner_min = ui.layout[Layer::Main].rect[leaf.idx()].min;
    let cmds = ui.encode_cmds();
    let mut drawn: Vec<glam::Vec2> = cmds
        .iter()
        .filter_map(|command| match command {
            Command::DrawText(payload) => Some(payload.rect.min),
            _ => None,
        })
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
    let f1_leaf = ui.run_at_value_acked(UVec2::new(400, 400), build_multi_text_leaf);
    let f1_span = ui.layout[Layer::Main].text_spans[f1_leaf.idx()];
    let f1_first = ui.layout[Layer::Main].text_shapes[f1_span.start as usize];
    let f1_second = ui.layout[Layer::Main].text_shapes[(f1_span.start + 1) as usize];
    let f2_leaf = ui.run_at_value_acked(UVec2::new(400, 400), build_multi_text_leaf);
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
    use crate::forest::tree::node::NodeId;
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
                    .size((Sizing::fixed(360.0), Sizing::HUG))
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
