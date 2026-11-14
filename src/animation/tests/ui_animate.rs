//! `Ui::animate` end to end: the repaint it requests and the rows it drops.

use crate::animation::animated::Animated;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::motion_role::MotionRole;
use crate::animation::tests::support::{self, AnimationUi, SLOT};
use crate::host::harness::Harness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::paint::Paint;
use crate::primitives::preference::Preference;
use crate::primitives::reduction::Reduction;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::ui::ui_preferences::UiPreferences;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widgets::block::Block;
use glam::UVec2;
use std::cell;
use std::rc::Rc;
use std::time::Duration;

/// First-touch settled: no repaint; in-flight retarget: repaint; repeated frames settle and stop requesting.
#[test]
fn animate_drives_repaint_until_settle() {
    let AnimationUi { mut h, id } = support::setup_animation_ui("anim-test");

    let repaint = h
        .frame(|ui| {
            let _ = ui.animate(
                AnimationRequest::new(id, SLOT, 0.0_f32),
                AnimationSpec::FAST,
            );
            Block::new().id(id).show(ui);
        })
        .repaint_requested;
    assert!(
        !repaint,
        "first-touch settled animation must not request repaint",
    );

    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            let _ = ui.animate(
                AnimationRequest::new(id, SLOT, 1.0_f32),
                AnimationSpec::FAST,
            );
            Block::new().id(id).show(ui);
        })
        .repaint_requested;
    assert!(repaint, "in-flight animation must request repaint");

    // FAST is 120 ms; frame `n` after the retarget has spent n × 16 ms: 112 at 7, 128 at 8.
    let frames = h.settled(&mut |ui: &mut Ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 1.0_f32),
            AnimationSpec::FAST,
        );
        Block::new().id(id).show(ui);
    });
    assert_eq!(frames, 8, "the 8th 16 ms frame passes 120 ms");
}

/// `Ui::animate(..., SNAP)` returns `target` at rest, allocates no row and requests no repaint.
#[test]
fn animate_with_snap_spec_snaps_and_skips_repaint() {
    let AnimationUi { mut h, id } = support::setup_animation_ui("anim-snap");
    let passes = h.at(Duration::from_millis(16)).frame_passes(|ui| {
        let first = ui.animate(
            AnimationRequest::new(id, SLOT, 7.0_f32),
            AnimationSpec::SNAP,
        );
        let second = ui.animate(
            AnimationRequest::new(id, SLOT, 9.0_f32),
            AnimationSpec::SNAP,
        );
        Block::new().id(id).show(ui);
        [first, second]
    });
    assert_eq!(
        *passes.a(),
        [
            Animated {
                value: 7.0,
                settled: true
            },
            Animated {
                value: 9.0,
                settled: true
            }
        ]
    );
    assert!(!passes.report().repaint_requested);
    assert_eq!(h.animation_row_count::<f32>(), 0);
}

/// Switching from `Some(spec)` to `None` mid-flight drops the stale row, so a later retarget starts fresh.
#[test]
fn animate_some_then_none_drops_stale_row() {
    let AnimationUi { mut h, id } = support::setup_animation_ui("anim-toggle");
    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 0.0_f32),
            AnimationSpec::FAST,
        );
        Block::new().id(id).show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 1.0_f32),
            AnimationSpec::FAST,
        );
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.animation_row_count::<f32>(),
        1,
        "Some(FAST) must allocate a row mid-flight",
    );

    let _ = h.at(Duration::from_millis(60)).frame(|ui| {
        let _ = ui.animate(
            AnimationRequest::new(id, SLOT, 1.0_f32),
            AnimationSpec::SNAP,
        );
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.animation_row_count::<f32>(),
        0,
        "None spec must drop the stale row inserted by a prior Some()",
    );
}

/// `WidgetLook::to_animated` folds text overrides onto the ambient style into an `AnimatedLook`;
/// covers `spec = None` (no rows) and a real spec.
#[test]
fn widget_look_animate_resolves_components_and_falls_back() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::border::Border;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::shadow::Shadow;
    use crate::theme::look::WidgetLook;
    use crate::theme::look::animated_look::AnimatedLook;
    use crate::theme::text_style::TextStyle;
    use crate::theme::text_style_overrides::TextStyleOverrides;
    use crate::widget_core::widget::Widget;
    use std::cell::Cell;

    let AnimationUi { mut h, id } = support::setup_animation_ui("look-test");

    let chrome = Chrome {
        corners: Corners::all(4.0),
        paint: Paint {
            fill: RgbaF32::hex(0x336699).into(),
            border: Border::new(RgbaF32::hex(0xffffff), 2.0),
            shadow: Shadow::NONE,
        },
    };
    let look = WidgetLook::new(chrome.clone());
    let fallback = TextStyle::stock(ColorScheme::Dark);

    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(16)).frame(|ui| {
        let target = look.to_animated(fallback);
        captured.set(Some(
            ui.animate(
                AnimationRequest::new(id, Widget::LOOK_SLOT, target),
                AnimationSpec::SNAP,
            )
            .value,
        ));
        Block::new().id(id).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(
        snap.chrome.paint.fill, chrome.paint.fill,
        "None: fill snaps to target"
    );
    assert_eq!(
        snap.chrome.paint.border.width, 2.0,
        "None: border width snaps"
    );
    assert_eq!(snap.chrome.paint.border.color, chrome.paint.border.color);
    assert_eq!(snap.chrome.corners, chrome.corners);
    assert_eq!(
        snap.text, fallback,
        "a look that overrides nothing takes the ambient style whole",
    );
    assert_eq!(
        h.animation_row_count::<AnimatedLook>(),
        0,
        "None spec: WidgetLook::animate must allocate no AnimatedLook row",
    );

    // Retargeting the fill allocates one `AnimatedLook` row.
    let look2 = WidgetLook::new(Chrome {
        paint: Paint {
            fill: RgbaF32::hex(0xff0000).into(),
            ..chrome.paint.clone()
        },
        ..chrome
    });
    let _ = h.at(Duration::from_millis(32)).frame(|ui| {
        let target = look2.to_animated(fallback);
        let _ = ui.animate(
            AnimationRequest::new(id, Widget::LOOK_SLOT, target),
            AnimationSpec::FAST,
        );
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.animation_row_count::<AnimatedLook>(),
        1,
        "Some(FAST) on changed fill must allocate an AnimatedLook row",
    );

    // Axes the look names win; the rest come from the ambient style. Every axis differs, so a wrong source shows.
    let own_size = fallback.font_size + 7.0;
    let own_color = RgbaF32::hex(0x00ff00);
    let ambient = TextStyle {
        font_size: fallback.font_size + 99.0,
        color: RgbaF32::hex(0xff00ff),
        line_height_factor: fallback.line_height_factor + 9.0,
        ..fallback
    };
    let look3 = WidgetLook {
        chrome,
        text: TextStyleOverrides::NONE
            .with_font_size(own_size)
            .with_color(own_color),
    };
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(48)).frame(|ui| {
        let target = look3.to_animated(ambient);
        captured.set(Some(
            ui.animate(
                AnimationRequest::new(id.with("own"), Widget::LOOK_SLOT, target),
                AnimationSpec::SNAP,
            )
            .value,
        ));
        Block::new().id(id).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(
        snap.text,
        TextStyle {
            font_size: own_size,
            color: own_color,
            ..ambient
        },
        "named axes come from the look, the leading from the ambient style",
    );
}

/// `FAST` from rest after a second idle: the change frame shows the start value (not the clamped 0.1 s); the next frame, 16 ms later, shows `OutCubic(16 / 120)`.
#[test]
fn a_motion_from_rest_starts_on_the_frame_of_the_change() {
    use crate::animation::easing::Easing;
    let AnimationUi { mut h, id } = support::setup_animation_ui("from-rest");
    let record = |h: &mut Harness, at: Duration, target: f32| {
        let value = cell::Cell::new(f32::NAN);
        h.at(at).frame(|ui| {
            value.set(
                ui.animate(AnimationRequest::new(id, SLOT, target), AnimationSpec::FAST)
                    .value,
            );
            Block::new().id(id).show(ui);
        });
        value.get()
    };
    assert_eq!(record(&mut h, Duration::ZERO, 0.0), 0.0);
    let idle = Duration::from_secs(1);
    assert_eq!(
        record(&mut h, idle, 1.0),
        0.0,
        "the change's frame shows the start"
    );
    let next = record(&mut h, idle + Duration::from_millis(16), 1.0);
    assert_eq!(next, Easing::OutCubic.apply(0.016 / 0.12));
}

/// With motion reduced, a retarget lands on its target in the frame that asks, with no repaint to
/// finish it; with full motion, the same retarget is in flight: its frame shows the start, 0, and
/// asks for the next. The motion preference decides: the system's ask under `System`, and a pick
/// by hand over it either way. Reduced power snaps it too under `System`, and a pick of full
/// motion moves it still. An essential request moves under reduced motion and reduced power
/// alike. A motion in flight when the window hides lands on its target on the first frame shown
/// again, and stays there, essential or not.
#[test]
fn reduced_motion_and_a_hidden_span_snap_every_transition() {
    let step = |h: &mut Harness, id: WidgetId, target: f32, role: MotionRole| {
        let mut value = f32::NAN;
        let report = h.frame(|ui| {
            value = ui
                .animate(
                    AnimationRequest::new(id, SLOT, target).with_motion(role),
                    AnimationSpec::FAST,
                )
                .value;
            Block::new().id(id).show(ui);
        });
        (value, report.repaint_requested)
    };
    let retarget =
        |system_reduced: bool, power: bool, preference: Preference<Reduction>, role: MotionRole| {
            let AnimationUi { mut h, id } = support::setup_animation_ui("reduced");
            h.set_system_motion_reduced(system_reduced);
            h.set_system_power_reduced(power);
            h.ui_mut().set_preferences(UiPreferences {
                motion: preference,
                ..UiPreferences::default()
            });
            step(&mut h, id, 0.0, role);
            let moved = step(h.at(Duration::from_millis(16)), id, 1.0, role);
            (h, id, moved)
        };
    let decorative = MotionRole::Decorative;
    assert_eq!(
        retarget(true, false, Preference::System, decorative).2,
        (1.0, false)
    );
    assert_eq!(
        retarget(
            false,
            false,
            Preference::Fixed(Reduction::Reduced),
            decorative
        )
        .2,
        (1.0, false)
    );
    assert_eq!(
        retarget(false, true, Preference::System, decorative).2,
        (1.0, false),
        "reduced power snaps a transition that follows the system"
    );
    for (system_reduced, power, preference, role) in [
        (
            true,
            false,
            Preference::Fixed(Reduction::Unreduced),
            decorative,
        ),
        (
            false,
            true,
            Preference::Fixed(Reduction::Unreduced),
            decorative,
        ),
        (true, false, Preference::System, MotionRole::Essential),
        (false, true, Preference::System, MotionRole::Essential),
    ] {
        assert_eq!(
            retarget(system_reduced, power, preference, role).2,
            (0.0, true),
            "motion {system_reduced}, power {power}, {preference:?}, {role:?}: in flight"
        );
    }
    for role in [decorative, MotionRole::Essential] {
        let (mut h, id, moved) = retarget(false, false, Preference::System, role);
        assert_eq!(moved, (0.0, true), "in flight");
        h.ui_mut().window_frame_mut().facts.shown_again = true;
        assert_eq!(
            step(h.at(Duration::from_millis(32)), id, 1.0, role),
            (1.0, false)
        );
        assert_eq!(
            step(h.at(Duration::from_millis(48)), id, 1.0, role),
            (1.0, false)
        );
    }
}

/// A look follows a theme change and fades a state change. A themed leaf at rest wears the dark
/// button look; the shown theme turns light, and the next frame wears the light look exactly, as
/// the state that picks it did not change. Then the pointer moves over it: the state changes, so
/// 16 ms later the look is part way to the hovered one, not on it.
#[test]
fn a_look_follows_a_theme_change_and_fades_a_state_change() {
    let mut h = Harness::builder(UVec2::new(100, 100))
        .mono()
        .frame_dt(Duration::from_millis(16))
        .build();
    let id = WidgetId::from_hash("themed-leaf");
    let wear = |h: &mut Harness| -> (AnimatedLook, AnimatedLook) {
        h.frame_value(|ui| {
            let mut leaf = Widget::leaf()
                .id(id)
                .sense(Sense::CLICK)
                .size((Sizing::fixed(40.0), Sizing::fixed(40.0)));
            let response = leaf.response(ui);
            let theme = Rc::clone(ui.theme());
            let plan = theme
                .button
                .plan(&response, Selection::Unselected, theme.text);
            let want = plan.target.clone();
            let look = leaf.wear(ui, plan);
            leaf.record(ui, Some(&look.chrome), |_| {});
            (look, want)
        })
    };
    let (look, want) = wear(&mut h);
    assert_eq!(look, want, "first touch rests on its look");

    let shown = h.ui().color_scheme();
    let other = match shown {
        ColorScheme::Light => ColorScheme::Dark,
        ColorScheme::Dark => ColorScheme::Light,
    };
    h.ui_mut()
        .set_scheme_theme(shown, Theme::stock(other))
        .unwrap();
    let (look, want) = wear(h.advance(Duration::from_millis(16)));
    assert_eq!(look, want, "a theme change is followed at once");

    h.hover(id);
    let _ = wear(h.advance(Duration::from_millis(16)));
    let (look, want) = wear(h.advance(Duration::from_millis(16)));
    assert_ne!(look, want, "a state change fades");
}
