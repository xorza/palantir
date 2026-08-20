//! Wrapping text inside a fill slot: the share it reshapes at, and the
//! floor under it.

use crate::layout::cross_driver_tests::support;
use crate::layout::cross_driver_tests::support::chat_message;
use crate::layout::cross_driver_tests::text_wrap::support::PARAGRAPH;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use glam::UVec2;

/// Chat-message HStack pattern. Avatar (Fixed) + Message (Fill,
/// wrapping text). Without HStack-Fill min-content floor + width
/// commitment, message is measured at INF → shapes at natural width →
/// cached shape disagrees with arrange's slot.
#[test]
fn hstack_fill_wrap_text_reshapes_at_resolved_share() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let msg = h.frame_value(|ui| chat_message(ui, 40.0, PARAGRAPH, 14.0));
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), msg);
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
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let msg = h.frame_value(|ui| chat_message(ui, 180.0, "supercalifragilistic", 14.0));
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), msg);
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
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let msg = h.frame_value(|ui| chat_message(ui, 180.0, "supercalifragilistic", 14.0));
    let shaped_w = support::shaped_text(h.ui.layout(Layer::Main), msg)
        .measured
        .w;
    let rect_w = h.ui.arranged_rect(Layer::Main, msg).size.w;

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
