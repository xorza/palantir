//! Wrapping text inside a fill slot: the share it reshapes at, and its floor.

use crate::internals::layout_rig::LayoutRig;
use crate::layout::tests::support::{ChatMessage, PARAGRAPH};
use crate::primitives::geometry::size::Size;

/// Chat-message HStack (Fixed avatar + Fill wrapping message): without the Fill min-content floor and width commitment, the message measures at INF and its cached shape disagrees with arrange's slot. The share is 200 − 20 = 180, 25 chars of 7 px: "the quick brown fox jumps" (175) and "over the lazy dog".
#[test]
fn hstack_fill_wrap_text_reshapes_at_resolved_share() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 400.0), |rec| {
        ChatMessage {
            avatar_w: 20.0,
            text: PARAGRAPH,
            text_px: 14.0,
        }
        .record(rec);
    });
    assert_eq!(
        rig.shaped_text("message").extent.size,
        Size::new(175.0, 2.0 * 14.0),
        "Fill message should wrap to two lines inside its resolved share",
    );
}

/// A Stack's Fill child respects its `intrinsic_min` floor and grows to fit its content when the slot is smaller, propagating the overflow upward: the 20-char word is 140 wide, past the 20 px the avatar leaves.
#[test]
fn hstack_fill_grows_to_content_when_slot_smaller_than_content() {
    let mut rig = LayoutRig::new();
    rig.run(Size::new(200.0, 400.0), |rec| {
        ChatMessage {
            avatar_w: 180.0,
            text: "supercalifragilistic",
            text_px: 14.0,
        }
        .record(rec);
    });
    let shaped_w = rig.shaped_text("message").extent.size.w;
    assert_eq!(shaped_w, 140.0, "measure floors at MinContent");
    assert_eq!(
        rig.rect("message").size.w,
        shaped_w,
        "rect must contain its measured content (no paint outside rect)",
    );
}
