use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::driver::agent_window::AgentWindow;
use crate::host::harness::Harness;
use crate::primitives::math::domain::internals::assert_close;
use crate::primitives::math::lerp::Lerp;
use crate::theme::Theme;

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::palette::Palette;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::switch::{Switch, SwitchTrack};

use glam::UVec2;
use std::time;

/// The aspect the stock switch ships; expected numbers derive from it.
const ASPECT: f32 = 1.75;

const fn track(height: f32, inset: f32, aspect: f32) -> SwitchTrack {
    SwitchTrack {
        height,
        inset,
        aspect,
    }
}

/// 20 px default, 1 px track border: `track_w = 35`, `knob = 14`. The border insets the content box by 1 px, so `off_x = 2`, `on_x = 17`, `knob_y = 2`; re-adding it puts the knob `inset` (3 px) from every edge.
#[test]
fn knob_geometry_default_dimensions() {
    let (track_h, inset, border) = (20.0_f32, 3.0_f32, 1.0_f32);
    let track_w = track(track_h, inset, ASPECT).width();
    let g = track(track_h, inset, ASPECT).knob(border);
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
fn knob_geometry_without_a_border_is_rect_relative() {
    let g = track(20.0, 3.0, ASPECT).knob(0.0);
    assert_eq!(g.off_x, 3.0);
    assert_eq!(g.on_x, 18.0);
    assert_eq!(g.knob_y, 3.0);
}

/// A wider aspect stretches the track and moves the on knob right; the knob (a function of height) is unchanged. 20 px at 3:1 is 60 px, so `60 - 14 - 3 = 43` vs `35 - 14 - 3 = 18`.
#[test]
fn track_aspect_stretches_the_track_not_the_knob() {
    let wide = track(20.0, 3.0, 3.0).knob(0.0);
    let stock = track(20.0, 3.0, ASPECT).knob(0.0);
    assert_eq!(track(20.0, 3.0, 3.0).width(), 60.0);
    assert_eq!(wide.on_x, 43.0);
    assert_eq!(stock.on_x, 18.0);
    assert_ne!(wide.on_x, stock.on_x);
    assert_eq!(wide.knob, stock.knob);
}

/// A degenerate height can't drive the knob negative: `4 - 2 * 3 = -2` floors at 2 px.
#[test]
fn knob_floors_at_two() {
    let g = track(4.0, 3.0, ASPECT).knob(0.0);
    assert_eq!(g.knob, 2.0);
}

/// The off knob is centered despite the border inset: `inset` (3 px) from every edge, offset (3, 3), 18 px of travel; clicked on, it rests 3 px from the right end.
#[test]
fn knob_rests_inset_from_the_end_it_sits_against() {
    let mut h = Harness::builder(UVec2::new(400, 400))
        .mono()
        .frame_dt(time::Duration::from_millis(16))
        .settle_frames(120)
        .build();
    let id = WidgetId::from_hash("wifi");
    let mut on = false;
    let mut record = |ui: &mut Ui| {
        Switch::new(&mut on).id(id).label("Wi-Fi").show(ui);
    };
    // `[left, top, right, bottom]`, the knob's margins in the track.
    let margins = |h: &Harness| {
        let track = h.arranged(id.with("box"));
        let knob = h.arranged(id.with("knob"));
        [
            knob.unwrap().min.x - track.unwrap().min.x,
            knob.unwrap().min.y - track.unwrap().min.y,
            track.unwrap().max().x - knob.unwrap().max().x,
            track.unwrap().max().y - knob.unwrap().max().y,
        ]
    };
    h.frame(&mut record);
    assert_eq!(
        margins(&h),
        [3.0, 3.0, 18.0, 3.0],
        "off, centered, at the left"
    );

    h.click(id);
    h.frame(&mut record);
    h.session(&mut record)
        .settle()
        .expect("the knob and the look it rides settle inside 2 s");
    assert_eq!(margins(&h), [18.0, 3.0, 3.0, 3.0], "on, at the right");
}

/// The knob wears the theme's off ink while off and, once a click settles, its ink on the accent:
/// the stock dark theme's `text` and `on_accent`, which differ.
#[test]
fn the_knob_wears_the_ink_of_its_state() {
    let mut h = Harness::builder(UVec2::new(400, 400))
        .mono()
        .frame_dt(time::Duration::from_millis(16))
        .settle_frames(120)
        .build();
    let id = WidgetId::from_hash("wifi");
    let mut on = false;
    let mut record = |ui: &mut Ui| {
        Switch::new(&mut on).id(id).label("Wi-Fi").show(ui);
    };
    let knob_fill = |h: &Harness| {
        let chrome = h
            .ui()
            .tree(Layer::Main)
            .chrome(h.node_for_widget_id(id.with("knob")))
            .expect("the knob paints a fill");
        let LoweredBrush::Solid(fill) = chrome.fill else {
            panic!("the knob is a solid fill");
        };
        fill
    };
    h.frame(&mut record);
    assert_eq!(knob_fill(&h), Palette::DARK.text.to_f16(), "off");

    h.click(id);
    h.frame(&mut record);
    h.session(&mut record)
        .settle()
        .expect("the knob's slide and ink settle inside 2 s");
    assert_eq!(knob_fill(&h), Palette::DARK.on_accent.to_f16(), "on");
    assert_ne!(Palette::DARK.text, Palette::DARK.on_accent);
}

/// One share of the travel places the knob and blends its ink. Over a 100 ms linear slide, the
/// click frame spends no time and 16 ms later the share is 0.16: the knob is 15 × 0.16 = 2.4 px
/// into its 15 px travel, and its fill is the off ink 16 % of the way to the on ink.
#[test]
fn one_share_places_the_knob_and_blends_its_ink() {
    let mut style = Theme::stock(ColorScheme::Dark).switch;
    style.defaults.transition =
        AnimationSpec::duration(time::Duration::from_millis(100), Easing::Linear);
    let mut h = Harness::mono(UVec2::new(400, 400));
    let id = WidgetId::from_hash("wifi");
    let mut on = false;
    let mut record = |ui: &mut Ui| {
        Switch::new(&mut on).id(id).style(&style).show(ui);
    };
    h.frame(&mut record);
    h.click(id);
    h.frame(&mut record);
    h.advance(time::Duration::from_millis(16));
    h.frame(&mut record);

    let track = h.arranged(id.with("box"));
    let knob = h.arranged(id.with("knob"));
    // The look moves the track's border on the check too, here 1.00015625 px, which the layout's
    // f16 padding stores as 1: the knob keeps its 3 px inset from the edge it sits against.
    assert_eq!(knob.unwrap().min.y - track.unwrap().min.y, 3.0);
    assert_close(
        knob.unwrap().min.x - track.unwrap().min.x,
        3.0 + 15.0 * 0.16,
        1e-6,
        "the share is 0.016 / 0.1, rounded in f32",
    );
    let chrome = h
        .ui()
        .tree(Layer::Main)
        .chrome(h.node_for_widget_id(id.with("knob")))
        .expect("the knob paints a fill");
    let LoweredBrush::Solid(fill) = chrome.fill else {
        panic!("the knob is a solid fill");
    };
    let ink = Lerp::lerp(&Palette::DARK.text, &Palette::DARK.on_accent, 0.16);
    assert_eq!(fill, ink.to_f16());
}
