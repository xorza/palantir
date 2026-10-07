//! Wrapping text inside a fill slot: the share it reshapes at, and its floor.

use crate::internals::harness::UiHarness;
use crate::layout::tests::support;
use crate::layout::tests::support::PARAGRAPH;
use crate::layout::tests::support::chat_message;
use crate::scene::layer::Layer;
use glam::UVec2;

/// Chat-message HStack (Fixed avatar + Fill wrapping message): without the Fill min-content floor and width commitment, the message measures at INF and its cached shape disagrees with arrange's slot.
#[test]
fn hstack_fill_wrap_text_reshapes_at_resolved_share() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let msg = h.frame_value(|ui| chat_message(ui, 40.0, PARAGRAPH, 14.0));
    let shaped = support::shaped_text(h.ui.layout(Layer::Main), msg);
    assert!(
        shaped.extent.size.h > 32.0,
        "Fill message should wrap inside its resolved share; got h={}",
        shaped.extent.size.h,
    );
    assert!(
        shaped.extent.size.w <= 160.0,
        "wrapped message width should fit within Fill share; got w={}",
        shaped.extent.size.w,
    );
}

/// A Stack's Fill child respects its `intrinsic_min` floor and grows to fit its content when the slot is smaller, propagating the overflow upward.
#[test]
fn hstack_fill_grows_to_content_when_slot_smaller_than_content() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    let msg = h.frame_value(|ui| chat_message(ui, 180.0, "supercalifragilistic", 14.0));
    let shaped_w = support::shaped_text(h.ui.layout(Layer::Main), msg)
        .extent
        .size
        .w;
    let rect_w = h.ui.arranged_rect(Layer::Main, msg).size.w;

    assert_eq!(shaped_w, 121.0, "measure floors at MinContent");
    assert_eq!(
        rect_w, shaped_w,
        "rect must contain its measured content (no paint outside rect)",
    );
}
