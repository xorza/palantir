//! A change of the shown theme, blended frame by frame under `Ui::set_theme_transition`.

use std::rc::Rc;
use std::time::Duration;

use glam::UVec2;

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::host::harness::Harness;
use crate::primitives::math::animatable::Animatable;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme::{Dark, Light};
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

/// A harness past its second frame under [`SPEC`], with the dark theme it shows.
fn running() -> (Harness, Rc<Theme>) {
    let mut h = Harness::mono(SURFACE);
    h.ui().set_theme_transition(SPEC);
    h.prime(2, record);
    let dark = Rc::clone(h.ui_ref().theme());
    (h, dark)
}

fn shows(h: &Harness, theme: &Rc<Theme>) -> bool {
    Rc::ptr_eq(h.ui_ref().theme(), theme)
}

/// Until set, a change runs [`AnimationSpec::THEME`], 300 ms `OutCubic`.
#[test]
fn the_default_is_300_ms_out_cubic() {
    let h = Harness::mono(SURFACE);
    assert_eq!(h.ui_ref().theme_transition(), Some(AnimationSpec::THEME));
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
    let light = Rc::new(Theme::stock(Light));
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    assert_eq!(
        **h.ui_ref().theme(),
        *dark,
        "the change's own frame keeps its theme"
    );

    let mut blend = None;
    for quarters in 1..4 {
        h.advance(QUARTER);
        let report = h.frame(record);
        blend.get_or_insert_with(|| Rc::as_ptr(h.ui_ref().theme()));
        let t = quarters as f32 / 4.0;
        assert_eq!(
            **h.ui_ref().theme(),
            Theme::lerp(Theme::clone(&dark), Theme::clone(&light), t),
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
    h.ui().set_theme_for(Dark, Rc::clone(&dark));
    h.advance(QUARTER).frame(record);
    assert_eq!(Some(Rc::as_ptr(h.ui_ref().theme())), blend);
}

/// Turned back at ¼, a duration runs only the way it came, ¼ of its length: half that later it
/// shows ⅛, and then it lands on the theme it left.
#[test]
fn a_change_back_mid_way_runs_only_the_way_it_came() {
    let (mut h, dark) = running();
    let light = Rc::new(Theme::stock(Light));
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.ui().set_theme_for(Dark, Rc::clone(&dark));
    h.advance(QUARTER / 2).frame(record);
    assert_eq!(
        **h.ui_ref().theme(),
        Theme::lerp(Theme::clone(&dark), Theme::clone(&light), 0.125)
    );
    h.advance(QUARTER / 2).frame(record);
    assert!(shows(&h, &dark));
}

/// A third theme mid-way starts from the blend shown: a quarter on, the lerp from that blend.
#[test]
fn a_third_theme_mid_way_starts_from_the_blend_shown() {
    let (mut h, dark) = running();
    let light = Rc::new(Theme::stock(Light));
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    let blend = Theme::clone(h.ui_ref().theme());
    let mut third = Theme::stock(Dark);
    third.focus_ring.width = 9.0;
    let third = Rc::new(third);
    h.ui().set_theme_for(Dark, Rc::clone(&third));
    assert_eq!(**h.ui_ref().theme(), blend, "the change keeps the blend");
    h.advance(QUARTER).frame(record);
    assert_eq!(
        **h.ui_ref().theme(),
        Theme::lerp(blend, Theme::clone(&third), 0.25)
    );
    assert_ne!(**h.ui_ref().theme(), *dark);
}

/// A scheme change starts at the theme shown: the frame that adopts the scheme shows the old
/// theme, and the next a quarter of the way.
#[test]
fn a_scheme_change_blends_from_the_next_frame() {
    let (mut h, dark) = running();
    h.set_system_color_scheme(Some(Light));
    h.advance(QUARTER).frame(record);
    assert_eq!(h.ui_ref().color_scheme(), Light);
    assert!(shows(&h, &dark), "no step yet");
    h.advance(QUARTER).frame(record);
    let light = Rc::clone(h.ui_ref().theme_for(Light));
    assert_eq!(
        **h.ui_ref().theme(),
        Theme::lerp(Theme::clone(&dark), Theme::clone(&light), 0.25)
    );

    // Two schemes that hold one theme change nothing: no run, so no frame is owed, and the theme
    // epoch stays, so nothing records again, whether the change would run or snap.
    for reduced in [false, true] {
        let (mut h, dark) = running();
        h.set_motion_reduced(reduced);
        h.ui().set_theme_for(Light, Rc::clone(&dark));
        let epoch = h.ui_ref().window_theme.epoch();
        h.set_system_color_scheme(Some(Light));
        let report = h.advance(QUARTER).frame(record);
        assert_eq!(h.ui_ref().color_scheme(), Light);
        assert!(shows(&h, &dark));
        assert_eq!(h.ui_ref().window_theme.epoch(), epoch, "reduced {reduced}");
        assert!(!report.repaint_requested, "reduced {reduced}: nothing runs");
    }
}

/// A change snaps before the window's second frame, with no transition, with motion reduced,
/// and on a frame after a hidden span; setting `None` or reducing motion ends a running one where
/// it heads, at once or on the next frame.
#[test]
fn a_change_snaps_where_nothing_should_move() {
    let light = Rc::new(Theme::stock(Light));

    let mut fresh = Harness::mono(SURFACE);
    fresh.ui().set_theme_for(Dark, Rc::clone(&light));
    assert!(shows(&fresh, &light), "before the first frame");

    let (mut h, _) = running();
    h.ui().set_theme_transition(None);
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    assert!(shows(&h, &light), "no transition");

    let (mut h, _) = running();
    h.set_motion_reduced(true);
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    assert!(shows(&h, &light), "motion reduced");

    let (mut h, _) = running();
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.ui().set_theme_transition(None);
    assert!(shows(&h, &light), "`None` ends a running one");

    let (mut h, _) = running();
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    h.advance(QUARTER).frame(record);
    h.set_motion_reduced(true);
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light), "reduced mid-way");

    let (mut h, _) = running();
    h.ui().set_theme_for(Dark, Rc::clone(&light));
    h.ui().window_frame.facts.shown_again = true;
    h.advance(QUARTER).frame(record);
    assert!(shows(&h, &light), "a frame after a hidden span");
}

/// An underdamped spring swings past its end, and the blend shows the end there, never past it:
/// a theme past one could hold a negative length.
#[test]
fn a_spring_swing_past_the_end_shows_the_end() {
    let (mut h, dark) = running();
    h.ui()
        .set_theme_transition(AnimationSpec::spring(300.0, 10.0));
    let mut wide = Theme::clone(&dark);
    wide.focus_ring.width = dark.focus_ring.width + 4.0;
    let wide = Rc::new(wide);
    h.ui().set_theme_for(Dark, Rc::clone(&wide));
    let mut held_at_end = false;
    for _ in 0..240 {
        let report = h.advance(Duration::from_millis(16)).frame(record);
        let width = h.ui_ref().theme().focus_ring.width;
        assert!(width <= wide.focus_ring.width, "{width}");
        held_at_end |= report.repaint_requested && width == wide.focus_ring.width;
    }
    assert!(held_at_end, "the swing passed the end while the spring ran");
    assert!(shows(&h, &wide));
}
