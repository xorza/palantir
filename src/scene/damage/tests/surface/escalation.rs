//! The surface and output changes that force a full frame.

use crate::Ui;
use crate::display::user_scale::UserScale;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::cascade::CascadeInputHash;
use crate::scene::damage::Damage;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, one_frame};
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use crate::{display::Display, layout::types::sizing::Sizing};
use glam::UVec2;

/// Pin: a Display change between frames (resize, either scale factor, or
/// a snap flip) forces the next compute to `Full` regardless of how few widgets
/// are dirty. The backend recreates the backbuffer / reshapes text
/// and a partial paint over a freshly cleared backbuffer would leave
/// the rest of the screen as clear color — the showcase resize-flicker
/// case.
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
        // The user scale rasterizes exactly as the system one does, so it
        // escalates for the same reason — and it is the half an app can
        // move while the monitor never changes.
        (
            "user_scale",
            Display {
                user_scale: UserScale::new(1.25),
                ..DISPLAY
            },
        ),
        // DPI-monitor move: physical and scale change proportionally,
        // leaving `logical_rect` bit-identical — yet the swapchain is
        // reconfigured to a new pixel size and must repaint. Comparing
        // logical rects alone classified this as Skip and the window
        // kept stale old-DPI content until unrelated damage arrived.
        (
            "dpi_move_constant_logical",
            Display {
                physical: UVec2::new(400, 400),
                system_scale: 2.0,
                ..DISPLAY
            },
        ),
        // Snap flips change compose-time rasterization with identical
        // logical damage — same blind spot as the DPI move.
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

        // Steady-state: Full first frame, then Skip on identical re-record.
        let f1 = h.frame_without_baseline(&mut build).plan;
        assert!(
            matches!(
                f1,
                Some(RenderPlan {
                    damage: Damage::Full,
                    ..
                })
            ),
            "case: {label} f1"
        );
        let f2 = h.frame(&mut build).plan;
        assert!(f2.is_none(), "case: {label} f2 must Skip");
        assert!(
            h.engines.damage.counters.dirty().is_empty(),
            "case: {label} steady"
        );
        // Mutate Display; identical authoring; must short-circuit to Full.
        let mutated_plan = h.set_display(*mutated).frame(&mut build).plan;
        assert!(
            matches!(
                mutated_plan,
                Some(RenderPlan {
                    damage: Damage::Full,
                    ..
                })
            ),
            "case: {label} display change"
        );
        assert!(
            !h.engines.damage.counters.dirty().is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        // Stable surface at the new size, identical authoring → back to Skip.
        let stable = h.frame(&mut build).plan;
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

/// Pin (precise bug reproducer): the showcase resize-flicker fired
/// when surface changed AND the damage rect was small enough to fall
/// below the area threshold — only a few descendants shifted while
/// the root and most others were stable. Without the surface-change
/// short-circuit, `compute` returns `Some(small_rect)` and the
/// encoder produces a damage-filtered partial paint, but the backend
/// force-clears the freshly recreated backbuffer, leaving the rest of
/// the screen as clear color.
///
/// The test uses a Fixed-size root so descendant rects are stable
/// across surface changes; a tiny injected nudge to one descendant's
/// `prev` snapshot would, absent the short-circuit, produce a small
/// partial damage rect on the resize frame.
#[test]
fn small_damage_with_surface_change_forces_full_repaint() {
    let mut h = UiHarness::new(UVec2::new(2000, 2000));
    // Root: Fixed-size VStack containing two Fixed children. Stacked
    // vertically so both children's `paint_rect`s land inside the
    // 2000×2000 surface — required since the Vacant arm in the diff
    // skips inserting an off-surface widget into `prev` (no visible
    // pixels to track). Root rect is stable across surface changes
    // (Fixed never reads `available`), so any damage-rect change
    // must come from the descendant nudge, not the root re-resolving.
    // Frame "small" ends up at (0, 60, 50, 60).
    let mut scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(60.0), Sizing::fixed(120.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("big"))
                    .size((60.0, 60.0))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("small"))
                    .size((50.0, 60.0))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };

    h.frame(&mut scene);
    h.frame(&mut scene);
    assert!(h.engines.damage.counters.dirty().is_empty());

    // Inject: flip widget "small"'s prev `cascade_input` so the next
    // diff sees it as a cascade-state change and damages its paint_rect
    // (50×60 = 3000 area) inside a 2000×2000 surface (4M area) —
    // ratio ≈ 0.075%, well below the full-repaint threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap = h
        .engines
        .damage
        .prev
        .get_mut(&target_wid)
        .expect("small in prev");
    snap.cascade_input = CascadeInputHash(snap.cascade_input.0 ^ 1);

    let resize_plan = h
        .resize(UVec2::new(1999, 2000))
        .frame_without_baseline(&mut scene)
        .plan;

    assert!(
        matches!(
            resize_plan,
            Some(RenderPlan {
                damage: Damage::Full,
                ..
            })
        ),
        "small-damage + surface-change must force full repaint \
         (this is the showcase resize-flicker case — encoder would emit a \
         damage-filtered partial paint over a backend-cleared backbuffer)",
    );
}

/// Pin (negative): a stable surface across many frames does *not*
/// fire the surface-change short-circuit on every frame. This guards
/// the alpha-mode / present-mode / swapchain-recreated-but-backbuffer-
/// kept scenarios from the damage layer's POV — they all leave the
/// surface rect unchanged, so damage must pass through to the normal
/// dirty/threshold logic. Without this guarantee partial repaint
/// would never apply.
#[test]
fn stable_surface_does_not_short_circuit() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui, color: RgbaF32| {
        one_frame(ui, color);
    };

    // Warm up: two identical frames bring damage to steady state.
    h.frame(|ui| build(ui, BLUE));
    let warm = h.frame(|ui| build(ui, BLUE)).plan;
    assert!(warm.is_none(), "warm steady-state must Skip");
    assert!(h.engines.damage.counters.dirty().is_empty());
    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    let changed = h.frame(|ui| build(ui, RED)).plan;
    let Some(RenderPlan {
        damage: Damage::Partial(damage),
        ..
    }) = changed
    else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {changed:?} — surface-change short-circuit fired incorrectly",
        );
    };
    // DamageEngine rect = the 50×50 frame's rect. Well below 50% of 200×200.
    assert!(
        damage.coverage < 0.5,
        "damage region should be small (partial repaint range), got {damage:?}",
    );
}

#[test]
fn invalid_prior_output_forces_full_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    h.frame(|ui| one_frame(ui, BLUE));

    let next = h.frame_without_baseline(|ui| one_frame(ui, RED)).plan;
    assert!(
        matches!(
            next,
            Some(RenderPlan {
                damage: Damage::Full,
                ..
            })
        ),
        "invalid output must discard the incremental baseline: {next:?}",
    );
}

#[test]
fn valid_skip_preserves_incremental_damage_baseline() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let first = h.frame_without_baseline(|ui| one_frame(ui, BLUE)).plan;
    assert!(matches!(
        first,
        Some(RenderPlan {
            damage: Damage::Full,
            ..
        })
    ));
    let skip = h.frame(|ui| one_frame(ui, BLUE)).plan;
    assert!(skip.is_none(), "identical content must Skip");

    let next = h.frame(|ui| one_frame(ui, RED)).plan;
    assert!(
        matches!(
            next,
            Some(RenderPlan {
                damage: Damage::Partial(..),
                ..
            })
        ),
        "valid skip must retain the incremental baseline: {next:?}",
    );
}
