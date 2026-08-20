//! Thumb size and offset against hand-computed cases, and what a travelling
//! thumb keeps.

use crate::Ui;
use crate::layout::scrollbars::bar_geometry;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::bars::support::{theme, thumb_rects};
use glam::UVec2;
use glam::Vec2;

/// `bar_geometry(viewport, content, offset, min_thumb)` returns
/// `None` when content fits the viewport or the viewport collapses to
/// zero; otherwise `Some { thumb_size, thumb_offset }`.
///
/// The track spans the whole viewport extent, so one length drives
/// both the `viewport / content` ratio and the travel:
/// `thumb_size = clamp(viewport² / content, min_thumb, viewport)` and
/// `thumb_offset = clamp(offset / (content - viewport), 0, 1) *
/// (viewport - thumb_size)`.
#[test]
fn bar_geometry_thumb_size_and_offset_cases() {
    #[derive(Debug)]
    struct Want {
        thumb_size: Option<f32>,
        thumb_offset: Option<f32>,
    }
    type Case = (&'static str, f32, f32, f32, Option<Want>);
    let cases: &[Case] = &[
        (
            // 200² / 800 = 50, above the 24 px floor and under the
            // 200 px viewport, so the raw ratio survives both clamps.
            "ratio_above_floor",
            200.0,
            800.0,
            0.0,
            Some(Want {
                thumb_size: Some(50.0),
                thumb_offset: Some(0.0),
            }),
        ),
        (
            // Half of the 600 px scrollable range → half of the
            // 200 - 50 = 150 px travel.
            "midpoint_offset_rides_linearly",
            200.0,
            800.0,
            300.0,
            Some(Want {
                thumb_size: Some(50.0),
                thumb_offset: Some(75.0),
            }),
        ),
        (
            "max_offset_sits_at_track_end",
            200.0,
            800.0,
            600.0,
            Some(Want {
                thumb_size: Some(50.0),
                thumb_offset: Some(200.0 - 50.0),
            }),
        ),
        (
            // 100² / 10000 = 1 px, floored up to the theme minimum.
            "clamped_up_to_min_thumb_px",
            100.0,
            10_000.0,
            0.0,
            Some(Want {
                thumb_size: Some(24.0),
                thumb_offset: None,
            }),
        ),
        (
            // A viewport shorter than `min_thumb`: the floor would
            // overshoot the track, so the viewport cap wins.
            "clamped_down_to_viewport_when_min_exceeds_it",
            10.0,
            200.0,
            0.0,
            Some(Want {
                thumb_size: Some(10.0),
                thumb_offset: None,
            }),
        ),
        ("none_when_content_equals_viewport", 200.0, 200.0, 0.0, None),
        (
            "none_when_content_smaller_than_viewport",
            200.0,
            100.0,
            0.0,
            None,
        ),
        ("none_when_viewport_zero", 0.0, 800.0, 0.0, None),
    ];
    for (label, viewport, content, offset, want) in cases {
        let got = bar_geometry(*viewport, *content, *offset, theme().min_thumb_px);
        match (want, got) {
            (None, None) => {}
            (Some(want), Some(g)) => {
                if let Some(s) = want.thumb_size {
                    assert!((g.thumb_size - s).abs() < 1e-3, "case: {label} thumb_size");
                }
                if let Some(o) = want.thumb_offset {
                    assert!(
                        (g.thumb_offset - o).abs() < 1e-3,
                        "case: {label} thumb_offset"
                    );
                }
            }
            (want, got) => panic!(
                "case: {label} mismatch: want={:?}, got={:?}",
                want.is_some(),
                got.is_some()
            ),
        }
    }
}

/// A travelling thumb must not change *length* on screen. Physical
/// snapping rounds a rect's min and max independently, so a thumb on
/// fractional coordinates grows and shrinks by a pixel as it moves —
/// the shimmer reported against the showcase. `axis_rects` pins the
/// thumb to whole logical pixels to stop it; this asserts the
/// *snapped* extent, since the logical one was already constant and
/// so never caught the bug.
#[test]
fn a_travelling_thumb_keeps_its_snapped_length() {
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(400.0), Sizing::fixed(300.0)))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .overlay_bars()
                    .gap(12.0)
                    .show(ui, |ui| {
                        for i in 0..8 {
                            Frame::new()
                                .id(WidgetId::from_hash(format!("row{i}")))
                                .size((Sizing::FILL, Sizing::fixed(90.0)))
                                .show(ui);
                        }
                    });
            });
    };
    let surface = UVec2::new(400, 300);
    let mut h = UiHarness::new(surface);
    h.frame(build);
    h.frame(build);

    // What the compositor actually rasterizes, per `Rect::scaled_by`.
    let snapped = |r: Rect, scale: f32| (r.max().y * scale).round() - (r.min.y * scale).round();
    let first = thumb_rects(&h.ui, "scroll")[0];
    let expected: Vec<f32> = [1.0, 2.0, 3.0].iter().map(|s| snapped(first, *s)).collect();

    let mut travelled = Vec::new();
    for _ in 0..8 {
        h.scroll_pixels_at(Vec2::new(100.0, 100.0), Vec2::new(0.0, 37.0));
        h.frame(build);
        let r = thumb_rects(&h.ui, "scroll")[0];
        travelled.push(r.min.y);
        for (i, scale) in [1.0f32, 2.0, 3.0].iter().enumerate() {
            assert_eq!(
                snapped(r, *scale),
                expected[i],
                "thumb length changed at DPR {scale} once it moved to y={}",
                r.min.y,
            );
        }
    }
    assert!(
        travelled.windows(2).any(|w| w[0] != w[1]),
        "the thumb must actually travel for this to mean anything: {travelled:?}",
    );
}

/// Thumb *extent* is `viewport / content * track` — no offset term.
/// Scrolling moves the thumb; it must never resize it.
#[test]
fn scrolling_moves_the_thumb_without_resizing_it() {
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
    let surface = UVec2::new(400, 600);
    let mut h = UiHarness::new(surface);
    h.frame(build);
    h.frame(build);
    let before = thumb_rects(&h.ui, "scroll");
    assert_eq!(before.len(), 1, "one vertical thumb");

    let mut seen = Vec::new();
    for _ in 0..4 {
        // Wheel input routes to whatever the pointer is over.
        h.scroll_pixels_at(Vec2::new(100.0, 100.0), Vec2::new(0.0, 50.0));
        h.frame(build);
        let now = thumb_rects(&h.ui, "scroll");
        assert_eq!(now.len(), 1, "thumb must not vanish mid-scroll");
        seen.push((now[0].min.y, now[0].size.h));
    }
    for (offset, height) in &seen {
        assert!(
            (height - before[0].size.h).abs() < 1e-3,
            "thumb resized while scrolling: {} -> {height} (offsets so far {seen:?})",
            before[0].size.h,
        );
        let _ = offset;
    }
    assert!(
        seen.windows(2).any(|w| w[0].0 != w[1].0),
        "the thumb should actually travel: {seen:?}",
    );
}

/// Zooming a `Scroll::both` shrinks the thumb proportionally to
/// the content growth.
#[test]
fn zoomed_content_shrinks_thumb_proportionally() {
    let surface = UVec2::new(400, 400);
    let mut h = UiHarness::new(surface);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::both()
                    .id(WidgetId::from_hash("scroll"))
                    .with_zoom()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("big"))
                            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                            .show(ui);
                    });
            });
    };
    h.frame(build);
    h.frame(build);
    let scroll_id = WidgetId::from_hash("scroll");
    let z1_thumbs = thumb_rects(&h.ui, "scroll");
    assert_eq!(z1_thumbs.len(), 2, "z=1: V + H thumbs");
    let v1 = z1_thumbs
        .iter()
        .find(|r| r.size.h > r.size.w)
        .unwrap()
        .size
        .h;

    h.ui.state_mut::<ScrollState>(scroll_id).zoom = 2.0;
    h.frame(build);
    h.frame(build);
    let z2_thumbs = thumb_rects(&h.ui, "scroll");
    assert_eq!(z2_thumbs.len(), 2, "z=2: V + H thumbs");
    let v2 = z2_thumbs
        .iter()
        .find(|r| r.size.h > r.size.w)
        .unwrap()
        .size
        .h;
    assert!(v2 < v1, "thumb should shrink under zoom (v1={v1}, v2={v2})");
    let ratio = v2 / v1;
    assert!(
        (0.45..=0.55).contains(&ratio),
        "thumb shrink ratio off; v1={v1} v2={v2} ratio={ratio}"
    );
}
