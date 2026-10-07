use crate::internals::harness::UiHarness;

use crate::Ui;
use crate::primitives::identity::widget_id::WidgetId;
use crate::widget_core::configure::Configure;
use crate::widgets::switch::{Switch, switch_geom, track_width};

/// The aspect `ToggleTheme::switch` ships; expected numbers derive from it.
const ASPECT: f32 = 1.75;
use glam::UVec2;
use std::time;

/// 20 px default, 1 px track border: `track_w = 35`, `knob = 14`. The border insets the content box by 1 px, so `off_x = 2`, `on_x = 17`, `knob_y = 2`; re-adding it puts the knob `inset` (3 px) from every edge.
#[test]
fn switch_geom_default_dimensions() {
    let (track_h, inset, border) = (20.0_f32, 3.0_f32, 1.0_f32);
    let track_w = track_width(track_h, ASPECT);
    let g = switch_geom(track_h, inset, border, ASPECT);
    assert_eq!(track_w, 35.0);
    assert_eq!(g.knob, 14.0);
    assert_eq!(g.off_x, 2.0);
    assert_eq!(g.on_x, 17.0);
    assert_eq!(g.knob_y, 2.0);

    // Rect-relative margins (border re-added) all equal `inset`.
    let margins = [
        ("off left", border + g.off_x),
        ("on right", track_w - (border + g.on_x + g.knob)),
        ("top", border + g.knob_y),
        ("bottom", track_h - (border + g.knob_y + g.knob)),
    ];
    for (name, m) in margins {
        assert_eq!(m, inset, "{name} margin = {m}, want {inset}");
    }
}

/// No track border: content box equals the rect, so `off_x = inset`, `on_x = track_w - knob - inset`. Against the default case this shows `border` moves the coordinates (off_x 3 → 2).
#[test]
fn switch_geom_no_stroke_is_rect_relative() {
    let g = switch_geom(20.0, 3.0, 0.0, ASPECT);
    assert_eq!(g.off_x, 3.0);
    assert_eq!(g.on_x, 18.0);
    assert_eq!(g.knob_y, 3.0);
}

/// A wider aspect stretches the track and moves the on knob right; the knob (a function of height) is unchanged. 20 px at 3:1 is 60 px, so `60 - 14 - 3 = 43` vs `35 - 14 - 3 = 18`.
#[test]
fn track_aspect_stretches_the_track_not_the_knob() {
    let wide = switch_geom(20.0, 3.0, 0.0, 3.0);
    let stock = switch_geom(20.0, 3.0, 0.0, ASPECT);
    assert_eq!(track_width(20.0, 3.0), 60.0);
    assert_eq!(wide.on_x, 43.0);
    assert_eq!(stock.on_x, 18.0);
    assert_ne!(wide.on_x, stock.on_x);
    assert_eq!(wide.knob, stock.knob);
}

/// A degenerate height can't drive the knob negative; it floors at 2 px.
#[test]
fn switch_geom_knob_floors_at_two() {
    let g = switch_geom(4.0, 3.0, 0.0, ASPECT); // 4 - 6 = -2 → floored
    assert_eq!(g.knob, 2.0);
}

/// The off knob is centred despite the border inset: `inset` (3 px) from every edge, offset (3, 3), 18 px of travel; clicked on, it rests 3 px from the right end.
#[test]
fn knob_rests_inset_from_the_end_it_sits_against() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let id = WidgetId::from_hash("wifi");
    let mut on = false;
    let mut record = |ui: &mut Ui| {
        Switch::new(&mut on).id(id).label("Wi-Fi").show(ui);
    };
    // `[left, top, right, bottom]`, the knob's margins in the track.
    let margins = |h: &UiHarness| {
        let track = h.arranged(id.with("box"));
        let knob = h.arranged(id.with("knob"));
        [
            knob.min.x - track.min.x,
            knob.min.y - track.min.y,
            track.max().x - knob.max().x,
            track.max().y - knob.max().y,
        ]
    };
    h.frame(&mut record);
    assert_eq!(
        margins(&h),
        [3.0, 3.0, 18.0, 3.0],
        "off, centred, at the left"
    );

    h.click_on(id);
    h.frame(&mut record);
    let tick = time::Duration::from_millis(16);
    h.frames_until_idle(120, tick, &mut record)
        .expect("the knob and the look it rides settle inside 2 s");
    assert_eq!(margins(&h), [18.0, 3.0, 3.0, 3.0], "on, at the right");
}
