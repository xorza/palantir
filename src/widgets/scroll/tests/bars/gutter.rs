//! The width a bar reserves from the viewport, and the padding it lands in.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::tests::bars::support::{record_two_frames, theme, thumb_rects};
use crate::widgets::scroll::tests::support::{scroll_content, scroll_viewport};
use glam::UVec2;

/// Reservation: when content overflows on the V axis, the inner
/// shrinks by exactly `theme.width + theme.gap` on the right.
#[test]
fn vertical_overflow_reserves_bar_width_on_inner() {
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("tall"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build);
    h.frame(build);
    assert_eq!(
        scroll_viewport(&h.ui, WidgetId::from_hash("scroll")),
        Size::new(188.0, 200.0),
        "V overflow reserves theme.width + theme.gap = 12px on the right; H axis untouched"
    );
}

/// User-set padding is preserved — bar reservation adds to it.
#[test]
fn user_padding_is_preserved_when_bar_reserves() {
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .padding(16.0)
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("tall"))
                            .size((Sizing::fixed(100.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build);
    h.frame(build);
    assert_eq!(
        scroll_viewport(&h.ui, WidgetId::from_hash("scroll")),
        Size::new(156.0, 168.0)
    );
}

/// Pin bar positioning: V bar's overlay rect sits flush with
/// `outer.w - theme.width` (the reserved padding strip), NOT
/// inside any user-set padding.
#[test]
fn vertical_bar_overlay_rect_lands_in_right_padding_strip() {
    let (ui, node) = record_two_frames(UVec2::new(400, 600), |ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .padding(16.0)
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("tall"))
                            .size((Sizing::fixed(100.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    });
    let _ = node;
    let theme = theme();
    let expected_x = 200.0 - theme.width;
    let overlays = thumb_rects(&ui.ui, "scroll");
    assert!(!overlays.is_empty(), "expected at least one thumb");
    for r in &overlays {
        assert_eq!(
            r.min.x, expected_x,
            "V bar must sit at outer.w - theme.width (= reserved strip), \
             not inside user padding"
        );
        assert_eq!(r.size.w, theme.width, "V bar width = theme.width");
    }
}

/// Reservation is **constant** across the overflow toggle — the
/// viewport stays at `outer - bar_w` whether or not content
/// overflows. Keeping the viewport size stable prevents Hug
/// ancestors (e.g. a `Popup` body) from shifting by `bar_w` when
/// overflow first appears. Bar visibility (thumb + track drawn or
/// not) still toggles with overflow; only the gutter stays.
#[test]
fn bar_reservation_stays_constant_across_overflow_toggle() {
    let surface = UVec2::new(400, 600);
    let scroll_id = WidgetId::from_hash("scroll");
    let read_viewport = |ui: &Ui| scroll_viewport(ui, scroll_id);

    let build = |ui: &mut Ui, content_h: f32| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("body"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(content_h)))
                            .show(ui);
                    });
            });
    };

    let mut h = UiHarness::new(surface);
    h.frame(|ui| build(ui, 800.0));
    h.frame(|ui| build(ui, 800.0));
    assert_eq!(
        read_viewport(&mut h.ui),
        Size::new(188.0, 200.0),
        "viewport = 200 - (width + gap) when content overflows",
    );

    h.frame(|ui| build(ui, 50.0));
    h.frame(|ui| build(ui, 50.0));
    assert_eq!(
        read_viewport(&mut h.ui),
        Size::new(188.0, 200.0),
        "viewport stays the same when content fits — gutter is constant",
    );
}

/// `BarMode::Overlay`: no gutter is reserved. Viewport gets the
/// full outer width regardless of content/overflow. The bar
/// (when drawn) paints over the content's far-edge strip — same
/// geometry as Reserved mode, but no space taken from the
/// content area. Pinned with overflowing content so the bar
/// would actually appear.
#[test]
fn overlay_mode_skips_gutter_reservation() {
    use crate::BarMode;
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    let scroll_id = WidgetId::from_hash("scroll");
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .bar_mode(BarMode::Overlay)
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("tall"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(scene);
    h.frame(scene);
    assert_eq!(
        scroll_viewport(&h.ui, scroll_id),
        Size::new(200.0, 200.0),
        "Overlay: viewport = full outer (no gutter reservation), \
         even when content overflows and the bar is drawn",
    );
    assert!(
        scroll_content(&h.ui, scroll_id).h > scroll_viewport(&h.ui, scroll_id).h,
        "content > viewport on Y — bar should be drawn"
    );
}
