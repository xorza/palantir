//! The surface and output changes that force a full frame.

use crate::Ui;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::damage::Damage;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame, frame_without_baseline, one_frame};
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::render_plan::RenderPlan;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

/// A Display change between frames (resize, scale factor, snap flip) forces
/// `Full` however few widgets are dirty: the backend recreates the backbuffer,
/// and a partial paint over it would leave the rest clear colour.
#[test]
fn display_change_forces_full_repaint() {
    let cases: &[(&str, Display)] = &[
        (
            "resize_1px",
            Display {
                physical: UVec2::new(199, 200),
                ..DISPLAY
            },
        ),
        (
            "system_scale",
            Display {
                system_scale: 2.0,
                ..DISPLAY
            },
        ),
        // The user scale rasterizes like the system one and can move alone.
        (
            "user_scale",
            Display {
                user_scale: UserScale::new(1.25).unwrap(),
                ..DISPLAY
            },
        ),
        // A DPI-monitor move leaves `logical_rect` identical, yet the swapchain
        // reconfigures; comparing logical rects alone gave Skip and stale content.
        (
            "dpi_move_constant_logical",
            Display {
                physical: UVec2::new(400, 400),
                system_scale: 2.0,
                ..DISPLAY
            },
        ),
        (
            "pixel_snap_flip",
            Display {
                pixel_snap: false,
                ..DISPLAY
            },
        ),
    ];
    for (label, mutated) in cases {
        let mut h = UiHarness::new(DISPLAY.physical);
        let mut build = |ui: &mut Ui| {
            one_frame(ui, BLUE);
        };

        let f1 = frame_without_baseline(&mut h, &mut build);
        assert!(matches!(f1, Some(Damage::Full)), "case: {label} f1");
        let f2 = frame(&mut h, &mut build);
        assert!(f2.is_none(), "case: {label} f2 must Skip");
        assert!(
            h.engines.damage.counters.dirty().is_empty(),
            "case: {label} steady"
        );
        let mutated_plan = frame(h.set_display(*mutated), &mut build);
        assert!(
            matches!(mutated_plan, Some(Damage::Full)),
            "case: {label} display change"
        );
        assert!(
            !h.engines.damage.counters.dirty().is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        let stable = frame(&mut h, &mut build);
        assert!(
            stable.is_none(),
            "case: {label} post-mutation steady must Skip",
        );
        assert!(
            h.engines.damage.counters.dirty().is_empty(),
            "case: {label} post-mutation dirty empty"
        );
    }
}

/// Reproducer of the resize flicker: the surface changed while the damage rect
/// fell below the area threshold. Without the surface-change short-circuit a
/// small partial paint follows a backbuffer clear. A Fixed-size root keeps
/// descendant rects stable, so the `prev` nudge is the only damage.
#[test]
fn small_damage_with_surface_change_forces_full_repaint() {
    let mut h = UiHarness::new(UVec2::new(2000, 2000));
    // Two Fixed children in a Fixed VStack, both inside the 2000×2000 surface
    // (the Vacant arm skips off-surface widgets). "small" ends at (0, 60, 50, 60).
    let mut scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(60.0), Sizing::fixed(120.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("big"))
                    .size((60.0, 60.0))
                    .background(Background::fill(BLUE))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("small"))
                    .size((50.0, 60.0))
                    .background(Background::fill(BLUE))
                    .show(ui);
            });
    };

    h.prime(2, &mut scene);
    assert!(h.engines.damage.counters.dirty().is_empty());

    // Flip "small"'s prev `cascade_input`: 3000 of 4M area, far below the full-repaint threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap = h
        .engines
        .damage
        .prev
        .get_mut(&target_wid)
        .expect("small in prev");
    snap.cascade_input = CascadeInputHash(snap.cascade_input.0 ^ 1);

    let resize_plan = frame_without_baseline(h.resize(UVec2::new(1999, 2000)), &mut scene);

    assert!(
        matches!(resize_plan, Some(Damage::Full)),
        "small-damage + surface-change must force full repaint \
         (this is the showcase resize-flicker case — encoder would emit a \
         damage-filtered partial paint over a backend-cleared backbuffer)",
    );
}

/// A stable surface across frames does not fire the short-circuit, or partial
/// repaint would never apply.
#[test]
fn stable_surface_does_not_short_circuit() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui, color: RgbaF32| {
        one_frame(ui, color);
    };

    h.frame(|ui| build(ui, BLUE));
    let warm = frame(&mut h, |ui| build(ui, BLUE));
    assert!(warm.is_none(), "warm steady-state must Skip");
    assert!(h.engines.damage.counters.dirty().is_empty());
    // Frame 3: one leaf changes colour; `Partial` proves the short-circuit didn't fire.
    let changed = frame(&mut h, |ui| build(ui, RED));
    let Some(Damage::Partial(damage)) = changed else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {changed:?} — surface-change short-circuit fired incorrectly",
        );
    };
    assert!(
        damage.coverage < 0.5,
        "damage region should be small (partial repaint range), got {damage:?}",
    );
}

/// The clear colour is the bottom paint layer and no widget carries it; a
/// partial frame only pre-fills scissors, so a new colour escalates once.
#[test]
fn a_new_clear_colour_forces_full_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    h.frame(|ui| one_frame(ui, BLUE));
    assert!(
        frame(&mut h, |ui| one_frame(ui, BLUE)).is_none(),
        "premise: an unchanged scene skips",
    );

    h.ui().theme_mut().window_clear = RED;
    assert_eq!(
        h.frame(|ui| one_frame(ui, BLUE)).plan,
        Some(RenderPlan {
            clear: RED,
            damage: Damage::Full,
        }),
        "a clear colour reaches the screen through a full repaint or not \
         at all, and the repaint clears to the colour it escalated for",
    );

    assert!(
        frame(&mut h, |ui| one_frame(ui, BLUE)).is_none(),
        "the colour the last frame presented under is the baseline now",
    );
}

#[test]
fn invalid_prior_output_forces_full_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    h.frame(|ui| one_frame(ui, BLUE));

    let next = frame_without_baseline(&mut h, |ui| one_frame(ui, RED));
    assert!(
        matches!(next, Some(Damage::Full)),
        "invalid output must discard the incremental baseline: {next:?}",
    );
}

#[test]
fn valid_skip_preserves_incremental_damage_baseline() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let first = frame_without_baseline(&mut h, |ui| one_frame(ui, BLUE));
    assert!(matches!(first, Some(Damage::Full)));
    let skip = frame(&mut h, |ui| one_frame(ui, BLUE));
    assert!(skip.is_none(), "identical content must Skip");

    let next = frame(&mut h, |ui| one_frame(ui, RED));
    assert!(
        matches!(next, Some(Damage::Partial(..))),
        "valid skip must retain the incremental baseline: {next:?}",
    );
}
