//! A change of the shown theme, blended frame by frame under `Theme::transition`.

use std::rc::Rc;
use std::time::Duration;

use glam::UVec2;

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::host::harness::Harness;
use crate::primitives::math::lerp::Lerp;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme::{self, Dark, Light};
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;

const SURFACE: UVec2 = UVec2::new(200, 100);

/// 125 ms, linear: a frame of 1/32 s is a quarter of it, each in exact binary fractions.
const SPEC: AnimationSpec = AnimationSpec::duration(Duration::from_millis(125), Easing::Linear);

/// A 32nd of a second.
const QUARTER: Duration = Duration::from_micros(31_250);

fn record(ui: &mut Ui) {
    Button::new().id_salt("apply").label("Apply").show(ui);
}

/// `scheme`'s stock theme under `transition`.
fn under(scheme: ColorScheme, transition: AnimationSpec) -> Rc<Theme> {
    Rc::new(Theme {
        transition,
        ..Theme::stock(scheme)
    })
}

/// A harness past its second frame, its themes under [`SPEC`], with the dark theme it shows.
fn running() -> (Harness, Rc<Theme>) {
    let mut h = Harness::mono(SURFACE);
    h.ui_mut().set_theme(Dark, under(Dark, SPEC));
    h.ui_mut().set_theme(Light, under(Light, SPEC));
    h.prime(2, record);
    let dark = Rc::clone(h.ui().theme());
    (h, dark)
}

fn shows(h: &Harness, theme: &Rc<Theme>) -> bool {
    Rc::ptr_eq(h.ui().theme(), theme)
}

/// A stock theme runs [`AnimationSpec::THEME`], 300 ms `OutCubic`.
#[test]
fn the_default_is_300_ms_out_cubic() {
    assert_eq!(Theme::stock(Dark).transition, AnimationSpec::THEME);
    assert_eq!(Theme::stock(Light).transition, AnimationSpec::THEME);
    assert_eq!(
        AnimationSpec::THEME,
        AnimationSpec::duration(Duration::from_millis(300), Easing::OutCubic)
    );
}

/// The change keeps what the window shows until a frame moves it; each frame shows the lerp at
/// its phase, here ¼ a frame, and the fourth lands on the new theme and asks for no more.
#[test]
fn each_frame_shows_the_lerp_at_its_phase() {
    let (mut h, dark) = running();
    let light = under(Light, SPEC);
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    assert_eq!(
        **h.ui().theme(),
        *dark,
        "the change's own frame keeps its theme"
    );

    let mut blend = None;
    for quarters in 1..4 {
        h.advance(QUARTER);
        let report = h.frame(record);
        blend.get_or_insert_with(|| Rc::as_ptr(h.ui().theme()));
        let t = quarters as f32 / 4.0;
        assert_eq!(
            **h.ui().theme(),
            Theme::lerp(&Theme::clone(&dark), &Theme::clone(&light), t),
            "{t}"
        );
        assert!(report.repaint_requested, "{t}: a blend owes the next frame");
    }
    h.advance(QUARTER);
    h.frame(record);
    assert!(shows(&h, &light));
    let report = h.frame(record);
    assert!(
        !report.repaint_requested,
        "a landed transition asks for nothing"
    );

    // The next run blends into the first run's buffer, so a window allocates for its first alone.
    h.ui_mut().set_theme(Dark, Rc::clone(&dark));
    h.advance(QUARTER).frame(record);
    assert_eq!(Some(Rc::as_ptr(h.ui().theme())), blend);
}

/// Turned back at ¼, a duration runs only the way it came, ¼ of its length: half that later it
/// shows ⅛, and then it lands on the theme it left.
#[test]
fn a_change_back_mid_way_runs_only_the_way_it_came() {
    let (mut h, dark) = running();
    let light = under(Light, SPEC);
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.ui_mut().set_theme(Dark, Rc::clone(&dark));
    h.advance(QUARTER / 2).frame(record);
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&Theme::clone(&dark), &Theme::clone(&light), 0.125)
    );
    h.advance(QUARTER / 2).frame(record);
    assert!(shows(&h, &dark));
}

/// A run keeps its spec, and a turn takes the spec of the theme it heads for. Two quarters into
/// the 125 ms run to light it shows ½. The change back takes dark's 250 ms: the turn runs the ½ it
/// covered, 125 ms, so a quarter frame, 31.25 ms, later it shows `0.5 · (1 − 0.25) = 0.375`, where
/// 125 ms would show ¼.
#[test]
fn a_turn_takes_the_spec_of_the_theme_it_heads_for() {
    let mut h = Harness::mono(SURFACE);
    let dark = under(
        Dark,
        AnimationSpec::duration(Duration::from_millis(250), Easing::Linear),
    );
    h.ui_mut().set_theme(Dark, Rc::clone(&dark));
    h.prime(2, record);
    let light = under(Light, SPEC);
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.advance(QUARTER).frame(record);
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&Theme::clone(&dark), &Theme::clone(&light), 0.5)
    );
    h.ui_mut().set_theme(Dark, Rc::clone(&dark));
    h.advance(QUARTER).frame(record);
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&Theme::clone(&dark), &Theme::clone(&light), 0.375)
    );
}

/// A theme set every frame trails its edit by a steady lag, in frames `(1 − e) / e` for the share
/// `e` a frame closes. At 60 Hz under 300 ms `OutCubic` a frame is 1/18 of the spec, so
/// `e = 1 − (17/18)³ = 919/5832`, and the lag is `4913/919 = 5.346` frames, 89 ms. The tolerance is
/// the sum's rounding in `f32` over the 240 frames that reach the steady lag.
#[test]
fn a_theme_set_every_frame_trails_by_a_steady_lag() {
    const STEP: f32 = 0.01;
    let mut h = Harness::mono(SURFACE);
    h.prime(2, record);
    let mut lag_frames = 0.0;
    for n in 1..=240u16 {
        let target = 1.0 + f32::from(n) * STEP;
        let mut theme = Theme::stock(Dark);
        theme.focus_ring.width = target;
        h.ui_mut().set_theme(Dark, theme);
        h.advance(Duration::from_nanos(16_666_667)).frame(record);
        lag_frames = (target - h.ui().theme().focus_ring.width) / STEP;
    }
    assert!((lag_frames - 4913.0 / 919.0).abs() < 0.01, "{lag_frames}");
}

/// A turn of the scheme runs under the theme it turns to: a still light theme lands on the frame
/// that adopts it, and the turn back to a dark theme under [`SPEC`] blends.
#[test]
fn a_scheme_change_takes_the_spec_of_the_theme_it_turns_to() {
    let (mut h, dark) = running();
    let light = under(Light, AnimationSpec::SNAP);
    h.ui_mut().set_theme(Light, Rc::clone(&light));
    h.set_system_color_scheme(Some(Light));
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light));

    h.set_system_color_scheme(Some(Dark));
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light), "no step yet");
    h.advance(QUARTER).frame(record);
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&Theme::clone(&light), &Theme::clone(&dark), 0.25)
    );
}

/// A theme that differs in its transition alone is a change: one under a moving spec runs, and one
/// under [`AnimationSpec::SNAP`] lands. An equal theme in a new handle changes nothing.
#[test]
fn a_change_of_the_transition_alone_is_a_change() {
    let (mut h, dark) = running();
    h.ui_mut().set_theme(Dark, Theme::clone(&dark));
    assert!(!h.ui().window_theme.is_moving(), "an equal theme");

    let slower = under(
        Dark,
        AnimationSpec::duration(Duration::from_millis(250), Easing::Linear),
    );
    h.ui_mut().set_theme(Dark, Rc::clone(&slower));
    assert!(h.ui().window_theme.is_moving(), "a moving spec");

    let still = under(Dark, AnimationSpec::SNAP);
    h.ui_mut().set_theme(Dark, Rc::clone(&still));
    assert!(!h.ui().window_theme.is_moving(), "no transition");
    assert!(shows(&h, &still));
}

/// A third theme mid-way starts from the blend shown: a quarter on, the lerp from that blend.
#[test]
fn a_third_theme_mid_way_starts_from_the_blend_shown() {
    let (mut h, dark) = running();
    let light = under(Light, SPEC);
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    let blend = Theme::clone(h.ui().theme());
    let mut third = Theme::clone(&dark);
    third.focus_ring.width = 9.0;
    let third = Rc::new(third);
    h.ui_mut().set_theme(Dark, Rc::clone(&third));
    assert_eq!(**h.ui().theme(), blend, "the change keeps the blend");
    h.advance(QUARTER).frame(record);
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&blend, &Theme::clone(&third), 0.25)
    );
    assert_ne!(**h.ui().theme(), *dark);
}

/// A scheme change starts at the theme shown: the frame that adopts the scheme shows the old
/// theme, and the next a quarter of the way.
#[test]
fn a_scheme_change_blends_from_the_next_frame() {
    let (mut h, dark) = running();
    h.set_system_color_scheme(Some(Light));
    h.advance(QUARTER).frame(record);
    assert_eq!(h.ui().color_scheme(), Light);
    assert!(shows(&h, &dark), "no step yet");
    h.advance(QUARTER).frame(record);
    let light = Rc::clone(h.ui().scheme_theme(Light));
    assert_eq!(
        **h.ui().theme(),
        Theme::lerp(&Theme::clone(&dark), &Theme::clone(&light), 0.25)
    );

    // Two schemes that hold one theme change nothing: no run, so no frame is owed, and the theme
    // epoch stays, so nothing records again, whether the change would run or snap.
    for reduced in [false, true] {
        let (mut h, dark) = running();
        h.set_system_motion_reduced(reduced);
        h.ui_mut().set_theme(Light, Rc::clone(&dark));
        let epoch = h.ui().window_theme.epoch();
        h.set_system_color_scheme(Some(Light));
        let report = h.advance(QUARTER).frame(record);
        assert_eq!(h.ui().color_scheme(), Light);
        assert!(shows(&h, &dark));
        assert_eq!(h.ui().window_theme.epoch(), epoch, "reduced {reduced}");
        assert!(!report.repaint_requested, "reduced {reduced}: nothing runs");
    }
}

/// A change snaps before the window's second frame, to a theme under no transition, with motion
/// reduced, and on a frame after a hidden span; a change to a theme under no transition ends a
/// running one at once, and reducing motion ends it where it heads on the next frame.
#[test]
fn a_change_snaps_where_nothing_should_move() {
    let light = under(Light, SPEC);

    let mut fresh = Harness::mono(SURFACE);
    fresh.ui_mut().set_theme(Dark, Rc::clone(&light));
    assert!(shows(&fresh, &light), "before the first frame");

    let still = under(Light, AnimationSpec::SNAP);
    let (mut h, _) = running();
    h.ui_mut().set_theme(Dark, Rc::clone(&still));
    assert!(shows(&h, &still), "no transition");

    let (mut h, _) = running();
    h.set_system_motion_reduced(true);
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    assert!(shows(&h, &light), "motion reduced");

    let (mut h, _) = running();
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.ui_mut().set_theme(Dark, Rc::clone(&still));
    assert!(shows(&h, &still), "no transition ends a running one");

    let (mut h, _) = running();
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.set_system_motion_reduced(true);
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light), "reduced mid-way");

    let (mut h, _) = running();
    h.ui_mut().set_theme(Dark, Rc::clone(&light));
    h.ui_mut().host.window_frame.facts.shown_again = true;
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light), "a frame after a hidden span");
}

/// An underdamped spring swings past its end, and the blend shows the end there, never past it:
/// a theme past one could hold a negative length.
#[test]
fn a_spring_swing_past_the_end_shows_the_end() {
    let (mut h, dark) = running();
    let mut wide = Theme::clone(&dark);
    wide.transition = AnimationSpec::spring(300.0, 10.0);
    wide.focus_ring.width = dark.focus_ring.width + 4.0;
    let wide = Rc::new(wide);
    h.ui_mut().set_theme(Dark, Rc::clone(&wide));
    let mut held_at_end = false;
    for _ in 0..240 {
        let report = h.advance(Duration::from_millis(16)).frame(record);
        let width = h.ui().theme().focus_ring.width;
        assert!(width <= wide.focus_ring.width, "{width}");
        held_at_end |= report.repaint_requested && width == wide.focus_ring.width;
    }
    assert!(held_at_end, "the swing passed the end while the spring ran");
    assert!(shows(&h, &wide));
}
