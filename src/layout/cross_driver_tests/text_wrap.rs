use super::support::{chat_message, two_hug_cols_with_wrap};
use crate::TextStyle;
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
    let r = ui.pipeline.layout.result.rect[node.index()];
    assert!(
        r.size.h > 32.0,
        "wrapped paragraph should span multiple lines, got h={}",
        r.size.h,
    );
    let shape = ui
        .tree
        .shapes
        .slice_of(node.index())
        .first()
        .expect("text shape");
    let wrap = match shape {
        Shape::Text { wrap, .. } => *wrap,
        _ => panic!("expected Shape::Text"),
    };
    assert_eq!(wrap, TextWrap::Wrap);
    let shaped = ui.pipeline.layout.result.text_shapes[node.index()]
        .expect("layout should have shaped the text");
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

    let shaped = ui.pipeline.layout.result.text_shapes[node.index()].expect("text was shaped");
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

    let max_w = ui.pipeline.layout.intrinsic(
        &ui.tree,
        node,
        Axis::X,
        LenReq::MaxContent,
        &mut ui.pipeline.text,
    );
    let min_w = ui.pipeline.layout.intrinsic(
        &ui.tree,
        node,
        Axis::X,
        LenReq::MinContent,
        &mut ui.pipeline.text,
    );
    let max_h = ui.pipeline.layout.intrinsic(
        &ui.tree,
        node,
        Axis::Y,
        LenReq::MaxContent,
        &mut ui.pipeline.text,
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
    let mut ui = ui_with_text(UVec2::new(200, 400));
    let msg = chat_message(&mut ui, 40.0, PARAGRAPH, 14.0);
    ui.end_frame();

    let shaped = ui.pipeline.layout.result.text_shapes[msg.index()].expect("text was shaped");
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

    let shaped = ui.pipeline.layout.result.text_shapes[msg.index()].expect("text was shaped");
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

    let shaped_w = ui.pipeline.layout.result.text_shapes[msg.index()]
        .expect("text was shaped")
        .measured
        .w;
    let rect_w = ui.pipeline.layout.result.rect[msg.index()].size.w;

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
