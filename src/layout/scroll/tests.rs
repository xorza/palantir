//! Driver-level tests for [`super::measure`] and [`super::arrange`]:
//! INF-axis measure, content-extent recording into
//! [`super::ScrollContent`], and the cache-hit fallback (no entry
//! pushed; widget refresh keeps state from last frame).

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::support::testing::run_at;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(400, 300);

/// Vertical scroll measures children with INF on Y; content extent is
/// the children's full height.
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

    let entries = ui.layout.scroll_content.for_layer(Layer::Main);
    assert_eq!(entries.len(), 1, "one scroll widget = one content entry");
    assert_eq!(
        entries[0].1.h,
        5.0 * 50.0,
        "5 rows × 50 = full content height"
    );
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

    let entries = ui.layout.scroll_content.for_layer(Layer::Main);
    assert_eq!(entries.len(), 1);
    let content_w = entries[0].1.w;
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

    let entries = ui.layout.scroll_content.for_layer(Layer::Main);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1, Size::new(300.0, 250.0));
}

/// On a measure-cache hit at an ancestor, the Scroll's measure arm
/// doesn't fire and `scroll_content` is empty. The widget's `refresh`
/// then falls back to last frame's `ScrollState.content`.
#[test]
fn cache_hit_leaves_scroll_content_empty() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
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
    };
    run_at(&mut ui, SURFACE, build);
    let after_first = ui.layout.scroll_content.for_layer(Layer::Main).to_vec();
    assert_eq!(after_first.len(), 1, "driver fired and pushed an entry");

    // Frame 2: identical content. Cache hit at ancestor → scroll
    // driver doesn't run → no entry this frame.
    run_at(&mut ui, SURFACE, build);
    let after_second = ui.layout.scroll_content.for_layer(Layer::Main);
    assert!(
        after_second.is_empty(),
        "cache hit at ancestor → scroll driver doesn't fire → no entry; got {:?}",
        after_second,
    );
}
