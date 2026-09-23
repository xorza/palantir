//! Thumb size and offset against hand-computed cases, and what a travelling
//! thumb keeps.

use crate::Ui;
use crate::layout::axis::Axis;
use crate::layout::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::layout::types::scroll_axes::ScrollAxes;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::scroll::tests::bars::support::{theme, thumb_rects};
use glam::UVec2;
use glam::Vec2;

/// A bare vertical def — no gutter, no padding, no zoom — offset by `offset`.
fn vertical_def(offset: f32) -> ScrollbarsDef {
    ScrollbarsDef {
        content: WidgetId::from_hash("geometry"),
        offset: Vec2::new(0.0, offset),
        zoom: 1.0,
        axes: ScrollAxes::VERTICAL,
        reserve: Spacing::ZERO,
        padding: Spacing::ZERO,
        bar_thickness: 8.0,
        min_thumb: theme().min_thumb_px,
    }
}

/// `ScrollbarsDef::thumb` over a `viewport`-tall overlay returns `None`
/// when content fits the viewport or the viewport collapses to zero;
/// otherwise the thumb, with the track at the viewport's length and the
/// bar's range at `content - viewport`.
///
/// The track spans the whole viewport extent, so one length drives
/// both the `viewport / content` ratio and the travel. Both results are
/// quantized against that one integer track,
/// `track = max(floor(viewport), 1)`:
/// `thumb_size = clamp(round(max(viewport² / content, min_thumb)), 1, track)`
/// and `thumb_offset = round(clamp(offset / (content - viewport), 0, 1) *
/// (track - thumb_size))`.
#[test]
fn thumb_size_and_offset_cases() {
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
        (
            // A track under one logical pixel still overflows, so a bar
            // is drawn: the floor gives it a 1 px thumb with nowhere to
            // travel. The size cap and the offset cap used to floor the
            // viewport separately and disagree about the track length
            // here, placing that thumb at -1.
            "sub_pixel_track_pins_the_thumb_at_zero",
            0.5,
            800.0,
            0.0,
            Some(Want {
                thumb_size: Some(1.0),
                thumb_offset: Some(0.0),
            }),
        ),
        (
            // Same track at the far end of its 799.5 px scrollable
            // range: a full-travel fraction over zero travel is still
            // zero, not a negative offset.
            "sub_pixel_track_holds_at_zero_at_full_offset",
            0.5,
            800.0,
            799.5,
            Some(Want {
                thumb_size: Some(1.0),
                thumb_offset: Some(0.0),
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
        let got = vertical_def(*offset).thumb(
            Axis::Y,
            Size::new(100.0, *viewport),
            Size::new(0.0, *content),
        );
        match (want, got) {
            (None, None) => {}
            (Some(want), Some(g)) => {
                assert_eq!(g.track, *viewport, "case: {label} track");
                assert_eq!(g.max_offset, content - viewport, "case: {label} max_offset");
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

/// What the def adds over the bare arithmetic: its gutter and padding
/// deflate the viewport, its zoom scales the content, and an axis it does
/// not pan shows no bar however far the content overflows.
#[test]
fn the_def_deflates_scales_and_skips_axes_it_does_not_pan() {
    let outer = Size::new(300.0, 200.0);
    let def = ScrollbarsDef {
        reserve: Spacing::new(0.0, 0.0, 10.0, 12.0),
        padding: Spacing::new(4.0, 6.0, 8.0, 2.0),
        ..vertical_def(0.0)
    };
    // Across: 300 - (0 + 10) - (4 + 8) = 278. Down: 200 - (0 + 12) - (6 + 2) = 180.
    assert_eq!(def.viewport(outer), Size::new(278.0, 180.0));
    assert_eq!(def.viewport(Size::new(10.0, 10.0)), Size::ZERO);

    // A 180 track over 720 of content: 180² / 720 = 45, over a 540 range.
    let content = Size::new(1000.0, 720.0);
    let thumb = def
        .thumb(Axis::Y, outer, content)
        .expect("720 overflows 180");
    assert_eq!(
        (thumb.track, thumb.thumb_size, thumb.max_offset),
        (180.0, 45.0, 540.0)
    );
    // Zoom 0.5 halves the content to 360: 180² / 360 = 90, over a 180 range.
    let zoomed = ScrollbarsDef { zoom: 0.5, ..def }
        .thumb(Axis::Y, outer, content)
        .expect("360 overflows 180");
    assert_eq!((zoomed.thumb_size, zoomed.max_offset), (90.0, 180.0));
    assert_ne!(zoomed, thumb);

    // 1000 overflows the 278 across, but a vertical viewport has no
    // horizontal bar. Panning both, it does: 278² / 1000 = 77.28 → 77.
    assert_eq!(def.thumb(Axis::X, outer, content), None);
    let both = ScrollbarsDef {
        axes: ScrollAxes::BOTH,
        ..def
    };
    let across = both
        .thumb(Axis::X, outer, content)
        .expect("1000 overflows 278");
    assert_eq!((across.track, across.thumb_size), (278.0, 77.0));
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
                            Block::new()
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
                        Block::new()
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
                    .zoomable()
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .show(ui, |ui| {
                        Block::new()
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

    h.ui.state_or_default::<ScrollState>(scroll_id).zoom = 2.0;
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
