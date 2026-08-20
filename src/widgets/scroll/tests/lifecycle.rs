//! What a scroll records while it lives, and what is swept when it goes.

use crate::Ui;
use crate::layout::axis::Axis;
use crate::layout::scrollbars::BarDomain;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::{ScrollState, ThumbTravel};
use crate::widgets::scroll::tests::support::{
    SURFACE, build, read_state, scroll_content, scroll_viewport,
};
use glam::{UVec2, Vec2};

#[test]
fn scroll_layout_records_viewport_and_content_after_arrange() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 200.0, 800.0));
    let id = WidgetId::from_hash("scroll");
    assert_eq!(scroll_viewport(&h.ui, id).h, 200.0);
    assert_eq!(scroll_content(&h.ui, id).h, 800.0);
    assert_eq!(
        read_state(&mut h).offset,
        Vec2::ZERO,
        "no wheel input → offset stays at 0"
    );
}

#[test]
fn explicit_no_clip_overrides_scroll_default() {
    let mut h = UiHarness::new(UVec2::new(400, 300));
    let unclipped_id = WidgetId::from_hash("unclipped-scroll");
    let clipped_id = WidgetId::from_hash("default-scroll");
    h.frame(|ui| {
        Scroll::vertical()
            .id(unclipped_id)
            .clip(ClipMode::None)
            .size((Sizing::fixed(100.0), Sizing::fixed(80.0)))
            .show(ui, |_| {});
        Scroll::vertical()
            .id(clipped_id)
            .size((Sizing::fixed(100.0), Sizing::fixed(80.0)))
            .show(ui, |_| {});
    });

    let tree = h.ui.tree(Layer::Main);
    let clip_for = |id: WidgetId| {
        let viewport_id = id.with("viewport");
        let index = tree
            .records
            .widget_id()
            .iter()
            .position(|recorded| *recorded == viewport_id)
            .expect("scroll viewport node");
        tree.records.attrs()[index].clip_mode()
    };
    assert_eq!(clip_for(unclipped_id), ClipMode::None);
    assert_eq!(clip_for(clipped_id), ClipMode::Rect);
}

#[test]
fn state_is_swept_when_scroll_disappears() {
    let mut h = UiHarness::new(SURFACE);
    let id = WidgetId::from_hash("scroll");
    let build = |ui: &mut Ui| {
        Scroll::both()
            .id(id)
            .with_zoom()
            .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
            .show(ui, |_| {});
    };

    h.frame(build);
    let state = h.ui.try_state_mut::<ScrollState>(id).unwrap();
    state.offset = Vec2::new(12.0, 34.0);
    state.zoom = 2.0;

    h.frame(|_| {});
    assert!(h.ui.try_state::<ScrollState>(id).is_none());

    h.frame(build);
    let state = h.ui.try_state::<ScrollState>(id).unwrap();
    assert_eq!(state.offset, Vec2::ZERO);
    assert_eq!(state.zoom, 1.0);
    assert!(state.drag_anchor_is_none());
}

/// A thumb drag composes each frame's *cumulative* delta against the
/// offset the press captured, so the anchor cannot outlive the geometry
/// that maps delta → offset. `bar_geometry` answers `None` the moment
/// content fits or the track collapses, which a drag can reach mid-press
/// (the content shrinks under it) while the pointer capture still names
/// the zero-extent thumb.
#[test]
fn thumb_drag_anchor_dies_with_its_geometry() {
    let geom = Some(ThumbTravel {
        factor: 2.0,
        // Track 20, content 120 => the bar's domain is [0, 100].
        domain: BarDomain::new(120.0, 20.0),
    });
    let mut state = ScrollState::default();
    state.offset = Vec2::new(0.0, 10.0);

    // Press, then one tracked step: 10 + 5 * 2 = 20.
    state.apply_thumb_drag(Axis::Y, true, Some(Vec2::ZERO), geom);
    state.apply_thumb_drag(Axis::Y, false, Some(Vec2::new(0.0, 5.0)), geom);
    assert_eq!(state.offset.y, 20.0);
    assert!(!state.drag_anchor_is_none(), "the press is still held");

    // Geometry vanishes while that same press is held.
    state.apply_thumb_drag(Axis::Y, false, Some(Vec2::new(0.0, 40.0)), None);
    assert_eq!(state.offset.y, 20.0, "no geometry, no movement");
    assert!(
        state.drag_anchor_is_none(),
        "the anchor must not outlive the geometry it composes against",
    );

    // Geometry returns under the same held press. The delta is still
    // cumulative from the press, so a surviving anchor would land
    // 10 + 60 * 2 = 130, clamped to the 100 max — a full-track jump.
    state.apply_thumb_drag(Axis::Y, false, Some(Vec2::new(0.0, 60.0)), geom);
    assert_eq!(state.offset.y, 20.0, "a dead anchor must not resume");

    // A fresh press re-anchors against the current offset: 20 + 3 * 2 = 26.
    state.apply_thumb_drag(Axis::Y, true, Some(Vec2::ZERO), geom);
    state.apply_thumb_drag(Axis::Y, false, Some(Vec2::new(0.0, 3.0)), geom);
    assert_eq!(state.offset.y, 26.0);
}

/// `LayerLayout::scroll_content` records the extent the scroll
/// viewport sees. V-axis and H-axis behave like a Stack: sum along
/// the panned axis, max on the cross. XY behaves like a ZStack: max
/// per axis. An empty scroll records zero.
#[test]
fn scroll_records_content_extent() {
    #[derive(Debug)]
    enum Axis {
        V,
        H,
        XY,
        Empty,
    }
    let cases: &[(&str, Axis, &str, Size)] = &[
        (
            "v_axis_sum_main_max_cross",
            Axis::V,
            "scroll",
            Size::new(180.0, 92.0),
        ),
        (
            "h_axis_sum_main_max_cross",
            Axis::H,
            "scroll",
            Size::new(128.0, 40.0),
        ),
        (
            "xy_max_per_axis",
            Axis::XY,
            "scroll",
            Size::new(300.0, 250.0),
        ),
        ("empty_records_zero", Axis::Empty, "empty", Size::ZERO),
    ];
    for (label, axis, scroll_key, expected) in cases {
        let surface = match axis {
            Axis::V | Axis::Empty => UVec2::new(400, 600),
            Axis::H => UVec2::new(800, 200),
            Axis::XY => UVec2::new(400, 400),
        };
        let mut h = UiHarness::new(surface);
        let scroll_node = h.under_outer(|ui| match axis {
            Axis::V => Scroll::vertical()
                .id(WidgetId::from_hash("scroll"))
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .gap(4.0)
                .show(ui, |ui| {
                    for i in 0..3u32 {
                        Frame::new()
                            .id(WidgetId::from_hash(("row", i)))
                            .size((Sizing::fixed(180.0), Sizing::fixed(28.0)))
                            .show(ui);
                    }
                })
                .response
                .node(),
            Axis::H => Scroll::horizontal()
                .id(WidgetId::from_hash("scroll"))
                .size((Sizing::fixed(200.0), Sizing::fixed(60.0)))
                .gap(8.0)
                .show(ui, |ui| {
                    for i in 0..2u32 {
                        Frame::new()
                            .id(WidgetId::from_hash(("col", i)))
                            .size((Sizing::fixed(60.0), Sizing::fixed(40.0)))
                            .show(ui);
                    }
                })
                .response
                .node(),
            Axis::XY => Scroll::both()
                .id(WidgetId::from_hash("scroll"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |ui| {
                    Frame::new()
                        .id(WidgetId::from_hash("wide"))
                        .size((Sizing::fixed(300.0), Sizing::fixed(60.0)))
                        .show(ui);
                    Frame::new()
                        .id(WidgetId::from_hash("tall"))
                        .size((Sizing::fixed(80.0), Sizing::fixed(250.0)))
                        .show(ui);
                })
                .response
                .node(),
            Axis::Empty => Scroll::vertical()
                .id(WidgetId::from_hash("empty"))
                .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
                .show(ui, |_| {})
                .response
                .node(),
        });
        let scroll_id = WidgetId::from_hash(scroll_key);
        assert_eq!(
            scroll_content(&h.ui, scroll_id),
            *expected,
            "case: {label} content"
        );
        let rect = h.ui.arranged_rect(Layer::Main, scroll_node);
        let want_view = match axis {
            Axis::V => (200.0, 200.0),
            Axis::H => (200.0, 60.0),
            Axis::XY | Axis::Empty => (100.0, 100.0),
        };
        assert_eq!(
            (rect.size.w, rect.size.h),
            want_view,
            "case: {label} viewport"
        );
    }
}

/// A measure-cache hit restores the scroll content column.
#[test]
fn scroll_content_is_restored_on_measure_cache_hit() {
    let surface = UVec2::new(400, 600);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id(WidgetId::from_hash("scroll"))
                    .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                    .gap(4.0)
                    .show(ui, |ui| {
                        for i in 0..3u32 {
                            Frame::new()
                                .id(WidgetId::from_hash(("row", i)))
                                .size((Sizing::fixed(180.0), Sizing::fixed(28.0)))
                                .show(ui);
                        }
                    });
            });
    };

    let mut h = UiHarness::new(surface);
    h.frame(build);
    let scroll_id = WidgetId::from_hash("scroll");
    let after_first = scroll_content(&h.ui, scroll_id);
    let viewport_first = scroll_viewport(&h.ui, scroll_id);
    assert_eq!(after_first.h, 92.0);

    h.frame(build);
    let after_second = scroll_content(&h.ui, scroll_id);
    assert!(
        h.engines
            .layout
            .scratch
            .counters
            .cache_hits()
            .contains(&WidgetId::VIEWPORT),
        "warm frame must restore scroll content from an ancestor cache hit"
    );
    assert_eq!(
        after_second, after_first,
        "scroll content survives a measure-cache hit",
    );
    assert_eq!(scroll_viewport(&h.ui, scroll_id), viewport_first);
}

/// A scroll-offset change updates the authored viewport transform, so
/// its subtree hash must bust the cascade skip.
#[test]
fn cascade_skip_busts_on_scroll_offset_change() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| build(ui, 200.0, 800.0));
    assert!(
        h.ui.frame_runtime().cascade_ran(),
        "first frame runs the cascade"
    );

    h.frame(|ui| build(ui, 200.0, 800.0));
    assert!(
        !h.ui.frame_runtime().cascade_ran(),
        "unchanged scroll frame skips the cascade"
    );

    // Scroll the viewport: the offset shifts, so the content re-arranges
    // and the cascade must re-run.
    h.scroll_pixels_at(Vec2::new(50.0, 50.0), Vec2::new(0.0, 50.0));
    h.frame(|ui| build(ui, 200.0, 800.0));
    assert_eq!(read_state(&mut h).offset.y, 50.0, "offset advanced");
    assert!(
        h.ui.frame_runtime().cascade_ran(),
        "scroll offset change must re-run the cascade (offset is in the fingerprint)",
    );
}

/// A thumb drag that starts while the offset sits inside a
/// `content_margin` leading band moves the thumb on the very first
/// tracked pixel.
///
/// The bar's domain is `[0, max_off]` — `content_margin` is documented
/// as not showing extra thumb travel — but the offset's runs lower, into
/// the negative leading band the wheel can reach. Anchoring the drag at
/// the raw offset mixed the two: the target was composed from a negative
/// anchor and then clamped to the bar domain, so the first
/// `-offset / factor` px of the gesture were spent climbing back to zero
/// with nothing moving. Anchoring in the bar domain is what makes the
/// gesture start where the thumb is.
#[test]
fn thumb_drag_anchors_in_the_bar_domain_not_the_offset_domain() {
    let geom = Some(ThumbTravel {
        factor: 2.0,
        // Track 20, content 120 => the bar's domain is [0, 100].
        domain: BarDomain::new(120.0, 20.0),
    });
    let mut state = ScrollState::default();
    // Panned into the leading band, as a wheel over a scroll with a
    // `content_margin` can leave it.
    state.offset = Vec2::new(0.0, -30.0);

    state.apply_thumb_drag(Axis::Y, true, Some(Vec2::ZERO), geom);
    // One pixel of thumb travel buys `factor` px of offset, from the
    // clamped anchor 0 — not from -30, which would have needed 15 px of
    // drag before the offset left zero at all.
    state.apply_thumb_drag(Axis::Y, false, Some(Vec2::new(0.0, 1.0)), geom);
    assert_eq!(
        state.offset.y, 2.0,
        "the first tracked pixel must move the thumb, not repay the band",
    );
}
