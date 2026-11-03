use std::rc::Rc;

use crate::animation::animation_spec::AnimationSpec;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme::{Dark, Light};
use crate::ui::window_theme::{Motion, WindowTheme};

fn theme() -> Rc<Theme> {
    Rc::new(Theme::stock(Dark))
}

/// A theme set on the shown scheme lands at once under `Snap` and moves the epoch; one set on the
/// other scheme, or the handle a slot holds already, changes nothing shown and keeps it.
#[test]
fn a_snapped_set_moves_the_epoch_only_for_the_shown_scheme() {
    let mut window = WindowTheme::new(Dark);
    let held = Rc::clone(window.get(Dark));
    assert!(
        !window.set(Dark, held, Motion::Snap),
        "the handle held already"
    );
    assert_eq!(window.epoch(), 0);

    let other = theme();
    assert!(
        !window.set(Light, Rc::clone(&other), Motion::Snap),
        "not shown"
    );
    assert!(Rc::ptr_eq(window.get(Light), &other));
    assert_eq!(window.epoch(), 0);

    let new = theme();
    assert!(window.set(Dark, Rc::clone(&new), Motion::Snap));
    assert!(Rc::ptr_eq(window.shown(), &new));
    assert_eq!(window.epoch(), 1);
    assert!(!window.is_moving());
}

/// Under `Animate` a set shows the old theme on its frame, so the epoch holds; each step after
/// shows a new blend and moves it, and the step that lands shows the new theme and moves it once
/// more. A 300 ms transition is under way at 100 ms and done by a further second.
#[test]
fn an_animated_set_moves_the_epoch_with_each_blend() {
    let mut window = WindowTheme::new(Dark);
    let old = Rc::clone(window.get(Dark));
    let new = theme();
    assert!(window.set(Dark, Rc::clone(&new), Motion::Animate));
    assert!(window.is_moving());
    assert_eq!(window.epoch(), 0, "the set's frame shows the old theme");

    window.advance(0.1, Dark, Motion::Animate);
    assert_eq!(window.epoch(), 1);
    assert!(!Rc::ptr_eq(window.shown(), &old) && !Rc::ptr_eq(window.shown(), &new));

    window.advance(1.0, Dark, Motion::Animate);
    assert_eq!(window.epoch(), 2);
    assert!(Rc::ptr_eq(window.shown(), &new));
    assert!(!window.is_moving());

    window.advance(0.1, Dark, Motion::Animate);
    assert_eq!(window.epoch(), 2, "a frame with no run moves nothing");
}

/// A running transition ends where it heads, moving the epoch, when the spec is taken away, when a
/// frame snaps motion, and when the shown theme is edited in place.
#[test]
fn every_end_of_a_run_moves_the_epoch() {
    let ends: [fn(&mut WindowTheme); 3] = [
        |window| assert!(window.set_spec(AnimationSpec::SNAP)),
        |window| window.advance(0.1, Dark, Motion::Snap),
        |window| _ = window.edit(),
    ];
    for (at, end) in ends.into_iter().enumerate() {
        let mut window = WindowTheme::new(Dark);
        let new = theme();
        window.set(Dark, Rc::clone(&new), Motion::Animate);
        let before = window.epoch();
        end(&mut window);
        assert!(!window.is_moving(), "end {at}");
        assert_eq!(window.epoch(), before + 1, "end {at}");
    }
}

/// A scheme change shows the other scheme's theme, moving the epoch under `Snap`; two schemes that
/// hold one theme change nothing shown, so the epoch holds.
#[test]
fn a_scheme_change_moves_the_epoch_only_when_the_theme_differs() {
    let mut window = WindowTheme::new(Dark);
    window.advance(0.0, Light, Motion::Snap);
    assert_eq!(window.scheme(), Light);
    assert_eq!(window.epoch(), 1);

    let shared = theme();
    window.set(Light, Rc::clone(&shared), Motion::Snap);
    window.set(Dark, shared, Motion::Snap);
    let before = window.epoch();
    window.advance(0.0, Dark, Motion::Snap);
    assert_eq!(window.scheme(), Dark);
    assert_eq!(window.epoch(), before, "one theme for both schemes");
}
