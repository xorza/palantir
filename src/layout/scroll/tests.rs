//! Driver-level tests for [`super::measure`] and [`super::arrange`]:
//! INF-axis measure, content-extent recording into the persistent
//! [`super::ScrollLayoutState`] row, and the cache-hit fallback
//! (driver doesn't fire; row keeps last frame's `content`).

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::widget_id::WidgetId;
use crate::layout::scroll::ScrollLayoutState as ScrollState;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::support::testing::run_at;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 300);

/// Read the post-frame `ScrollState` for the scroll widget at
/// `id_salt`. State is what the codebase reads at record time and is
/// the stable observation point — on measure-cache hits the driver
/// doesn't run, but the persisted row keeps last frame's value.
fn state_for(ui: &mut Ui, id_salt: &'static str) -> ScrollState {
    *ui.scroll_state(WidgetId::from_hash(id_salt).with("__viewport"))
}

/// Vertical scroll measures children with INF on Y; content extent is
/// the children's full height. State is populated post-arrange.
#[test]
fn vertical_scroll_records_content_extent() {
    let mut ui = Ui::new();
    run_at(&mut ui, SURFACE, |ui| {
        Scroll::vertical()
            .id_salt("scroll")
            .size((Sizing::Fixed(200.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                for i in 0..5u32 {
                    Frame::new()
                        .id_salt(("row", i))
                        .size((Sizing::FILL, Sizing::Fixed(50.0)))
                        .show(ui);
                }
            });
    });
    assert_eq!(state_for(&mut ui, "scroll").content.h, 5.0 * 50.0);
}

/// Horizontal scroll measures children with INF on X.
#[test]
fn horizontal_scroll_records_content_extent() {
    let mut ui = Ui::new();
    run_at(&mut ui, SURFACE, |ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::horizontal()
                .id_salt("scroll")
                .size((Sizing::Fixed(200.0), Sizing::Fixed(80.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..10u32 {
                        Frame::new()
                            .id_salt(("col", i))
                            .size((Sizing::Fixed(40.0), Sizing::FILL))
                            .show(ui);
                    }
                });
        });
    });
    let content_w = state_for(&mut ui, "scroll").content.w;
    assert!(
        content_w > 200.0,
        "content overflows the 200 viewport on X: got {}",
        content_w,
    );
}

/// Both-axis scroll measures with both axes unbounded.
#[test]
fn both_axis_scroll_records_content_extent() {
    let mut ui = Ui::new();
    run_at(&mut ui, SURFACE, |ui| {
        Scroll::both()
            .id_salt("scroll")
            .size((Sizing::Fixed(100.0), Sizing::Fixed(100.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("wide-tall")
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(250.0)))
                    .show(ui);
            });
    });
    assert_eq!(
        state_for(&mut ui, "scroll").content,
        Size::new(300.0, 250.0)
    );
}

/// `ScrollState` survives across frames — record time reads it for
/// offset clamp + reservation guess + bar geometry.
#[test]
fn state_survives_across_frames() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::Fixed(150.0), Sizing::Fixed(100.0)))
                .show(ui, |ui| {
                    for i in 0..4u32 {
                        Frame::new()
                            .id_salt(("row", i))
                            .size((Sizing::FILL, Sizing::Fixed(40.0)))
                            .show(ui);
                    }
                });
        });
    };
    run_at(&mut ui, SURFACE, build);
    let f1 = state_for(&mut ui, "scroll");
    run_at(&mut ui, SURFACE, build);
    let f2 = state_for(&mut ui, "scroll");
    assert_eq!(f1.content, f2.content);
    assert_eq!(f1.viewport, f2.viewport);
    assert_eq!(f1.outer, f2.outer);
    assert!(f1.seen, "first frame's relayout populated state");
    assert!(f2.seen);
    // Sanity: pinned numbers.
    assert_eq!(f1.content.h, 4.0 * 40.0);
}
