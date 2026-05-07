use super::support;
use super::support::{chat_message, two_hug_cols_with_wrap};
use crate::TextStyle;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::shape::{Shape, TextWrap};
use crate::support::testing::{shapes_of, ui_with_text};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::widgets::{grid::Grid, panel::Panel, text::Text};
use glam::UVec2;
use std::borrow::Cow;
use std::rc::Rc;

const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let mut text_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(60.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new(PARAGRAPH)
                    .style(TextStyle::default().with_font_size(16.0))
                    .wrapping()
                    .show(ui)
                    .node,
            );
        });
    ui.end_frame();

    let node = text_node.unwrap();
    let r = ui.layout.result.rect[node.index()];
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
    );
    // todo refactor
    let shape = shapes_of(&ui.tree, node).next().expect("text shape");
    let wrap = match shape {
        Shape::Text { wrap, .. } => *wrap,
        _ => panic!("expected Shape::Text"),
    };
    assert_eq!(wrap, TextWrap::Wrap);
    let shaped =
        support::first_text(&ui.layout.result, node).expect("layout should have shaped the text");
    assert!(shaped.measured.h > 32.0);
}

/// Pinned by `src/layout/intrinsic.md`: a wrapping `Text` inside a
/// `Grid` `Hug` column constrained by the parent's available width
/// reshapes to fit. The grid column-resolution algorithm runs during
/// measure with the grid's `inner_avail` (200 px here); the wrapping
/// text gets its committed column width before shaping, so the cached
/// shape is multi-line and fits the slot.
#[test]
fn wrapping_text_in_grid_auto_column_wraps_under_constrained_width() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let node = two_hug_cols_with_wrap(&mut ui, PARAGRAPH);
    ui.end_frame();

    let shaped = support::first_text(&ui.layout.result, node).expect("text was shaped");
    // Multi-line height (a 16 px font wraps to 3 lines at the resolved
    // column width — h ≈ 58 px in practice; assert > 32 to allow for
    // line-height variation).
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
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let node = two_hug_cols_with_wrap(&mut ui, PARAGRAPH);
    ui.end_frame();

    let max_w = ui
        .layout
        .intrinsic(&ui.tree, node, Axis::X, LenReq::MaxContent, &mut ui.text);
    let min_w = ui
        .layout
        .intrinsic(&ui.tree, node, Axis::X, LenReq::MinContent, &mut ui.text);
    let max_h = ui
        .layout
        .intrinsic(&ui.tree, node, Axis::Y, LenReq::MaxContent, &mut ui.text);

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
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let msg = chat_message(&mut ui, 40.0, PARAGRAPH, 14.0);
    ui.end_frame();

    let shaped = support::first_text(&ui.layout.result, msg).expect("text was shaped");
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
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let msg = chat_message(&mut ui, 180.0, "supercalifragilistic", 14.0);
    ui.end_frame();

    let shaped = support::first_text(&ui.layout.result, msg).expect("text was shaped");
    assert!(
        shaped.measured.w > 20.0,
        "min-content floor should keep message wider than the cramped slot; got w={}",
        shaped.measured.w,
    );
}

/// Pin (flex semantics): a Stack's Fill child clamps DOWN to its
/// allocated slot when slot < min-content. The shaped text overflows
/// the rect visually (paint extends past the rect's right edge) but
/// the rect itself stays at the slot. Replaces an older test that
/// pinned the WPF "Fill parent grows to contain min-content overflow"
/// rule.
#[test]
fn hstack_fill_clamped_below_min_content_keeps_rect_at_slot() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let msg = chat_message(&mut ui, 180.0, "supercalifragilistic", 14.0);
    ui.end_frame();

    let shaped_w = support::first_text(&ui.layout.result, msg)
        .expect("text was shaped")
        .measured
        .w;
    let rect_w = ui.layout.result.rect[msg.index()].size.w;

    assert!(
        shaped_w > 50.0,
        "measure must floor at MinContent; got shaped_w={shaped_w}"
    );
    assert!(
        rect_w < shaped_w,
        "rect should clamp to slot under flex semantics, paint overflows; \
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
    fn build(ui: &mut crate::Ui) -> (crate::tree::NodeId, crate::tree::NodeId) {
        let mut grid_node = None;
        let mut section_node = None;
        Panel::vstack()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .padding(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                section_node = Some(Panel::vstack()
                                    .size((Sizing::FILL, Sizing::Hug))
                                    .gap(6.0)
                                    .show(ui, |ui| {
                                        Text::new(
                                            "two Hug columns: paragraph wraps to fit, \
                                             label stays natural",
                                        )
                                        .with_id("section-title")
                                        .style(TextStyle::default().with_font_size(12.0))
                                        .show(ui);
                                        grid_node = Some(
                                            Grid::new()
                                                .with_id("grid")
                                                .cols(Rc::from([Track::hug(), Track::hug()]))
                                                .rows(Rc::from([Track::hug()]))
                                                .show(ui, |ui| {
                                                    Text::new(
                                                        "the quick brown fox jumps over the lazy dog",
                                                    )
                                                    .style(TextStyle::default().with_font_size(14.0))
                                                    .wrapping()
                                                    .grid_cell((0, 0))
                                                    .show(ui);
                                                    Text::new("right column")
                                                        .style(
                                                            TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .grid_cell((0, 1))
                                                        .show(ui);
                                                })
                                                .node,
                                        );
                                    }).node);
                            });
                    });
            });
        (grid_node.unwrap(), section_node.unwrap())
    }

    fn measure_at(surface_w: u32) -> (f32, f32) {
        let mut ui = ui_with_text(UVec2::new(surface_w, 400));
        let (grid, section) = build(&mut ui);
        ui.end_frame();
        let grid_w = ui.layout.result.rect[grid.index()].size.w;
        let section_w = ui.layout.result.rect[section.index()].size.w;
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
    let mut ui = ui_with_text(UVec2::new(400, 200));
    let label_node = Text::new("right column")
        .style(TextStyle::default().with_font_size(14.0))
        .show(&mut ui)
        .node;
    ui.end_frame();

    let max_w = ui.layout.intrinsic(
        &ui.tree,
        label_node,
        Axis::X,
        LenReq::MaxContent,
        &mut ui.text,
    );
    let min_w = ui.layout.intrinsic(
        &ui.tree,
        label_node,
        Axis::X,
        LenReq::MinContent,
        &mut ui.text,
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
    fn build(ui: &mut crate::Ui) -> (crate::tree::NodeId, crate::tree::NodeId) {
        let mut paragraph_node = None;
        let mut label_node = None;
        Grid::new()
            .with_id("grid")
            .cols(Rc::from([Track::hug(), Track::hug()]))
            .rows(Rc::from([Track::hug()]))
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                paragraph_node = Some(
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .style(TextStyle::default().with_font_size(14.0))
                        .wrapping()
                        .grid_cell((0, 0))
                        .show(ui)
                        .node,
                );
                label_node = Some(
                    Text::new("right column")
                        .style(TextStyle::default().with_font_size(14.0))
                        .grid_cell((0, 1))
                        .show(ui)
                        .node,
                );
            });
        (paragraph_node.unwrap(), label_node.unwrap())
    }

    // Probe label's natural unbroken width at an unconstrained surface.
    let mut probe = ui_with_text(UVec2::new(2000, 400));
    let (_, probe_label) = build(&mut probe);
    probe.end_frame();
    let label_full = probe.layout.intrinsic(
        &probe.tree,
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &mut probe.text,
    );
    assert!(label_full > 0.0);

    // At a surface narrower than the paragraph max-content but wider
    // than the grid's intrinsic floor, slack distribution kicks in.
    // The label cell must still get at least its full natural width.
    for surface_w in [400u32, 300, 250, 200] {
        let mut ui = ui_with_text(UVec2::new(surface_w, 400));
        let (_, label) = build(&mut ui);
        ui.end_frame();
        let label_rect_w = ui.layout.result.rect[label.index()].size.w;
        assert!(
            label_rect_w >= label_full - 0.5,
            "label cell shrank below the label's natural width — \
             non-wrapping text would visually overflow its column. \
             surface_w={surface_w} label_full={label_full} label_rect_w={label_rect_w}",
        );
    }
}

/// Pin: a custom widget that pushes two `Shape::Text` to the same
/// node has both runs shaped (`text_spans[node].len == 2`) and laid
/// out at distinct positions via per-shape `local_rect`. Replaces the
/// old "one Shape::Text per leaf" hard assert.
#[test]
fn multi_shape_text_per_leaf_shapes_each_run_independently() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let mut leaf = None;
    Panel::vstack().show(&mut ui, |ui| {
        leaf = Some(ui.node(Element::new_auto(LayoutMode::Leaf), None, |ui| {
            ui.add_shape(Shape::Text {
                local_rect: Some(Rect::new(0.0, 0.0, 100.0, 20.0)),
                text: Cow::Borrowed("first"),
                color: Color::WHITE,
                font_size_px: 14.0,
                line_height_px: 16.0,
                wrap: TextWrap::Single,
                align: Default::default(),
            });
            ui.add_shape(Shape::Text {
                local_rect: Some(Rect::new(0.0, 22.0, 100.0, 20.0)),
                text: Cow::Borrowed("second-with-different-text"),
                color: Color::WHITE,
                font_size_px: 14.0,
                line_height_px: 16.0,
                wrap: TextWrap::Single,
                align: Default::default(),
            });
        }));
    });
    ui.end_frame();

    let leaf = leaf.unwrap();
    let span = ui.layout.result.text_spans[leaf.index()];
    assert_eq!(
        span.len, 2,
        "leaf with two Shape::Text should record two text-shape entries"
    );
    let first = ui.layout.result.text_shapes[span.start as usize];
    let second = ui.layout.result.text_shapes[(span.start + 1) as usize];
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
