//! The first frame a scroll is recorded on: bars must be right before any
//! settle.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::tests::bars::support::{theme, thumb_rects};
use crate::widgets::scroll::tests::support::{scroll_content, scroll_viewport};
use glam::UVec2;
use std::time::Duration;

/// The reason `layout::scrollbars` exists. A scroll that has never
/// recorded has no arranged viewport and no measured content — both
/// terms of the thumb's size ratio — so it used to call
/// `Ui::request_relayout` and re-record the entire frame to get them.
/// The driver resolves them after measure instead, so the thumb is
/// placed on the first painted frame *and* the frame stays one pass.
#[test]
fn cold_mount_places_the_thumb_in_one_record_pass() {
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
    let mut h = UiHarness::new(UVec2::new(400, 600));
    let mut passes = 0;
    let report = h.at(Duration::from_millis(16)).frame(|ui| {
        passes += 1;
        build(ui);
    });
    assert_eq!(passes, 1, "a cold-mounted scroll must not re-record");
    assert_eq!(report.processing, FrameProcessing::SingleLayout);

    // The vertical bar's gutter comes out of the *cross* axis (width),
    // so its own main extent is the full 200 — only a horizontal bar
    // would shorten it, and this scroll has none. Thumb is
    // `viewport/content * track` with track == viewport: 200/800*200.
    let theme = theme();
    let track: f32 = 200.0;
    let expected = (track / 800.0 * track).max(theme.min_thumb_px);
    assert_eq!(expected, 50.0, "arithmetic guard on the expectation");
    let thumbs = thumb_rects(&h.ui, "scroll");
    assert_eq!(thumbs.len(), 1, "one vertical thumb, no collapsed peers");
    assert!(
        (thumbs[0].size.h - expected).abs() < 1e-3,
        "first-frame thumb must already be sized from the measured \
         content: expected {expected}, got {}",
        thumbs[0].size.h,
    );
    assert_eq!(thumbs[0].size.w, theme.width);
}

/// Cold-mount overflow must paint with the gutter reservation
/// already in place on frame 1.
#[test]
fn cold_mount_overflow_paints_with_gutter_on_first_frame() {
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    let theme = theme();
    let scroll_id = WidgetId::from_hash("scroll");
    let scene = |ui: &mut Ui| {
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
    h.frame(scene);
    let expected = Size::new(200.0 - theme.width - theme.gap, 200.0);
    assert_eq!(
        scroll_viewport(&h.ui, scroll_id),
        expected,
        "cold-mount overflowing scroll: gutter reservation must be \
         active on the first painted frame; viewport should already \
         be deflated by `theme.width + theme.gap` on the cross axis",
    );
    assert!(
        scroll_content(&h.ui, scroll_id).h > scroll_viewport(&h.ui, scroll_id).h,
        "overflow flag must reflect post-relayout measurement (Y \
         overflows, X doesn't)",
    );
}

/// Cold-mount bar geometry must match steady-state frame-2 bar
/// geometry.
#[test]
fn cold_mount_bar_geometry_matches_frame_two() {
    use crate::primitives::rect::Rect;
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    let scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("big"))
                            .size((Sizing::fixed(800.0), Sizing::fixed(800.0)))
                            .show(ui);
                    });
            });
    };
    let bar_rects = |ui: &Ui| -> Vec<Rect> {
        let mut rects = thumb_rects(ui, "scroll");
        rects.sort_by(|a, b| {
            a.min
                .x
                .total_cmp(&b.min.x)
                .then(a.min.y.total_cmp(&b.min.y))
        });
        rects
    };

    h.frame(scene);
    let f1 = bar_rects(&h.ui);
    assert_eq!(f1.len(), 2, "cold-mount must emit both V + H thumbs");

    h.frame(scene);
    let f2 = bar_rects(&h.ui);

    assert_eq!(
        f1, f2,
        "bar shapes on cold-mount frame must match steady-state \
         frame 2 (regression: pass-B used pass-A's stale viewport \
         → bars shrank by theme.width + theme.gap on next frame)",
    );
}

/// Cold-mount with content that fits in the viewport: the gutter
/// is still reserved (constant), the bar thumb just isn't drawn.
/// Overflow stays `false`.
#[test]
fn cold_mount_fits_reserves_gutter_but_paints_no_thumb() {
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
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("short"))
                            .size((Sizing::fixed(180.0), Sizing::fixed(50.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(scene);
    assert_eq!(
        scroll_viewport(&h.ui, scroll_id),
        Size::new(188.0, 200.0),
        "gutter is constant — viewport = outer - (width + gap) even with no overflow",
    );
    assert_eq!(scroll_content(&h.ui, scroll_id), Size::new(180.0, 50.0));
    assert!(
        scroll_content(&h.ui, scroll_id).w <= scroll_viewport(&h.ui, scroll_id).w
            && scroll_content(&h.ui, scroll_id).h <= scroll_viewport(&h.ui, scroll_id).h
    );
}

/// Repro for "PopClip without matching PushClip" panic — drive
/// the full encode + compose pipeline twice (cold + warm caches)
/// with a Scroll that emits bar shapes.
#[test]
fn scroll_with_bars_composes_through_warm_cache() {
    let surface = UVec2::new(400, 300);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        for i in 0..30u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("row", i)))
                                .size((Sizing::fixed(180.0), Sizing::fixed(28.0)))
                                .show(ui);
                        }
                    });
            });
    };
    h.frame(build);
    h.frame(build);
}

/// Showcase-style nested scroll cards. Pin that the deeper
/// clip-stack walk + warm caches still leave the paint stream balanced.
#[test]
fn nested_clipped_scrolls_compose_through_warm_cache() {
    let surface = UVec2::new(800, 600);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .gap(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for tag in ["v", "h", "xy"] {
                    Panel::vstack()
                        .id(WidgetId::from_hash(("card", tag)))
                        .padding(8.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .background(Background {
                            fill: Color::rgb(0.16, 0.20, 0.28).into(),
                            ..Default::default()
                        })
                        .clip_rect()
                        .show(ui, |ui| {
                            let s = match tag {
                                "v" => Scroll::vertical().id(WidgetId::from_hash(("scroll", tag))),
                                "h" => {
                                    Scroll::horizontal().id(WidgetId::from_hash(("scroll", tag)))
                                }
                                _ => Scroll::both().id(WidgetId::from_hash(("scroll", tag))),
                            };
                            s.size((Sizing::FILL, Sizing::FILL)).show(ui, |ui| {
                                for i in 0..40u32 {
                                    Frame::new()
                                        .id(WidgetId::from_hash((tag, "item", i)))
                                        .size((Sizing::fixed(120.0), Sizing::fixed(28.0)))
                                        .show(ui);
                                }
                            });
                        });
                }
            });
    };
    h.frame(build);
    h.frame(build);
    h.frame(build);
}
