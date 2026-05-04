use super::support::{chat_message, two_hug_cols_with_wrap};
use crate::layout::types::sizing::Sizing;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::shape::{Shape, TextWrap};
use crate::support::testing::ui_with_text;
use crate::tree::element::Configure;
use crate::widgets::{panel::Panel, text::Text};
use glam::UVec2;

const PARAGRAPH: &str = "the quick brown fox jumps over the lazy dog";

#[test]
fn wrapping_text_grows_height_in_narrow_frame() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let mut text_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(60.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            text_node = Some(Text::new(PARAGRAPH).size_px(16.0).wrapping().show(ui).node);
        });
    ui.end_frame();

    let node = text_node.unwrap();
    let r = ui.layout_engine.result.rect[node.index()];
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
    );
    let shape = ui.tree.shapes_of(node).first().expect("text shape");
    let wrap = match shape {
        Shape::Text { wrap, .. } => *wrap,
        _ => panic!("expected Shape::Text"),
    };
    assert_eq!(wrap, TextWrap::Wrap);
    let shaped = ui.layout_engine.result.text_shapes[node.index()]
        .expect("layout should have shaped the text");
    assert!(shaped.measured.h > 32.0);
}

#[test]
fn wrapping_text_overflows_intrinsic_min_without_breaking_words() {
    let mut ui = ui_with_text(UVec2::new(400, 400));
    let mut text_node = None;
    Panel::vstack()
        .size((Sizing::Fixed(8.0), Sizing::Hug))
        .show(&mut ui, |ui| {
            text_node = Some(
                Text::new("supercalifragilisticexpialidocious")
                    .size_px(16.0)
                    .wrapping()
                    .show(ui)
                    .node,
            );
        });
    ui.end_frame();

    let r = ui.layout_engine.result.rect[text_node.unwrap().index()];
    // The single word can't break — its width must overflow the 8 px slot.
    assert!(
        r.size.w > 8.0,
        "an unbreakable word must overflow the slot, got w={}",
        r.size.w,
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
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let node = two_hug_cols_with_wrap(&mut ui, PARAGRAPH);
    ui.end_frame();

    let shaped = ui.layout_engine.result.text_shapes[node.index()].expect("text was shaped");
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

    let max_w =
        ui.layout_engine
            .intrinsic(&ui.tree, node, Axis::X, LenReq::MaxContent, &mut ui.text);
    let min_w =
        ui.layout_engine
            .intrinsic(&ui.tree, node, Axis::X, LenReq::MinContent, &mut ui.text);
    let max_h =
        ui.layout_engine
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

    let shaped = ui.layout_engine.result.text_shapes[msg.index()].expect("text was shaped");
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

    let shaped = ui.layout_engine.result.text_shapes[msg.index()].expect("text was shaped");
    assert!(
        shaped.measured.w > 20.0,
        "min-content floor should keep message wider than the cramped slot; got w={}",
        shaped.measured.w,
    );
}

/// Pin: when a Stack's Fill child clamps to its `MinContent` floor
/// during pass-2 measure, the Fill HStack parent reports `max(available,
/// hug_with_margin)` so its own desired size includes the floored
/// child. Arrange then sees an inner span that fits, and the Fill
/// child's arranged width matches its measured width — parent stays
/// at-least-as-big-as-child instead of cropping the measure floor.
#[test]
fn hstack_fill_clamped_to_min_content_arranges_at_measured_floor() {
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let msg = chat_message(&mut ui, 180.0, "supercalifragilistic", 14.0);
    ui.end_frame();

    let shaped_w = ui.layout_engine.result.text_shapes[msg.index()]
        .expect("text was shaped")
        .measured
        .w;
    let rect_w = ui.layout_engine.result.rect[msg.index()].size.w;

    assert!(
        shaped_w > 50.0,
        "measure must floor at MinContent; got shaped_w={shaped_w}"
    );
    assert!(
        (rect_w - shaped_w).abs() < 0.5,
        "arrange should match the measured floor now that the Fill \
         parent grows past `available` to contain it; \
         shaped_w={shaped_w} rect_w={rect_w}"
    );
}
