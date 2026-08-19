//! When damage stops being partial and the whole surface is repainted.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, translate_scale::TranslateScale};
use crate::renderer::render_plan::{RenderKind, RenderPlan};
use crate::scene::cascade::CascadeInputHash;
use crate::scene::cascade::paint::Paint;
use crate::scene::cascade::paint::PaintRows;
use crate::scene::damage::region::DamageRegion;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, TEST_SURFACE, frame, one_frame};
use crate::scene::damage::{Damage, DamageEngine};
use crate::scene::node::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::popup::Popup;
use crate::widgets::{frame::Frame, panel::Panel};
use crate::{display::Display, layout::types::sizing::Sizing};
use glam::{UVec2, Vec2};

/// Pin: when a subtree's `(paint_rect, node_hash, subtree_hash,
/// cascade_input)` all match the prev-frame snapshot at its painting
/// root, the damage diff jumps to `subtree_end` instead of walking every
/// descendant. The fast path's correctness is already covered by every
/// "unchanged → no damage" test in this file; this pin specifically
/// guards that the jump *fires* — without it the path silently degrades
/// to a per-node walk that still produces correct damage.
#[test]
fn stable_painting_subtree_triggers_skip_jump() {
    let mut h = UiHarness::new(DISPLAY.physical);
    // Frame with a painting parent (background) wrapping painting
    // children — both root and children land in `prev` with matching
    // snapshots on the second frame, so the root's Occupied-equal arm
    // is reached with a span > 1 and the skip counter increments.
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("painting_parent"))
                    .size((Sizing::fixed(80.0), Sizing::fixed(60.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("child_a"))
                            .size(20.0)
                            .background(Background {
                                fill: RED.into(),
                                ..Default::default()
                            })
                            .show(ui);
                        Frame::new()
                            .id(WidgetId::from_hash("child_b"))
                            .size(20.0)
                            .background(Background {
                                fill: RED.into(),
                                ..Default::default()
                            })
                            .show(ui);
                    });
            });
    };
    frame(&mut h, build);
    assert_eq!(
        h.ui.damage_engine.counters.subtree_skips(),
        0,
        "first frame populates prev — no prior snapshots to skip against"
    );

    frame(&mut h, build);
    assert!(
        h.ui.damage_engine.counters.subtree_skips() >= 1,
        "identical second frame must skip at least the painting_parent subtree, got {}",
        h.ui.damage_engine.counters.subtree_skips(),
    );
    assert!(h.ui.damage_engine.counters.dirty().is_empty());
}

/// Pin: a widget that loses its background between frames flips from
/// painting to non-painting. The diff must (a) contribute its prev
/// rect to damage so the prior pixels get cleared, (b) drop the entry
/// from `prev` so the next frame sees it as truly absent, and (c)
/// contribute no curr rect.
#[test]
fn paints_to_non_paints_transition_evicts_and_clears() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let with_bg = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    let no_bg = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .show(ui);
            });
    };
    frame(&mut h, with_bg);
    let id = WidgetId::from_hash("a");
    assert!(h.ui.damage_engine.prev.contains_key(&id));

    frame(&mut h, no_bg);
    assert!(
        !h.ui.damage_engine.prev.contains_key(&id),
        "paints→non-paints transition must evict the prev entry"
    );
    let rects: Vec<_> = h.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 50.0, 50.0)],
        "damage must contain only the prev rect (curr doesn't paint)"
    );
}

/// Regression: a popup's full-surface invisible click-eater leaf must
/// not contribute to damage on add or remove. Otherwise opening or
/// dismissing a popup blows past the full-repaint coverage threshold.
/// Sole signal here is that filter stays `Partial` — no full-surface
/// rect lands in `region`.
#[test]
fn popup_eater_does_not_force_full_repaint() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let anchor = glam::Vec2::new(40.0, 40.0);
    // Frame 1: popup open. Eater (full-surface) + body (small).
    frame(&mut h, |ui| {
        Popup::anchored_to(anchor)
            .id(WidgetId::from_hash("p"))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui, _popup| {
                Frame::new()
                    .id(WidgetId::from_hash("body-leaf"))
                    .size(60.0)
                    .background(Background {
                        fill: RED.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });

    // Frame 2: popup gone. Body + eater both removed. Without the
    // paints-gate, the eater's full-surface prev rect would dominate
    // the region.
    let out = h.frame(|ui| {
        Frame::new()
            .id(WidgetId::from_hash("placeholder"))
            .size(10.0)
            .show(ui);
    });
    let Some(RenderPlan {
        kind: RenderKind::Partial { region },
        ..
    }) = out.plan
    else {
        panic!(
            "popup dismissal escalated to {:?}; eater contributed full-surface \
             rect despite painting nothing",
            out.plan
        );
    };
    assert!(
        region.coverage < 0.5,
        "damage region covers {:.1}% of surface — eater leaked into damage",
        100.0 * region.coverage
    );
}

/// Regression: a click on empty background has no route, so it must not set
/// `frame_had_action`, run a discarded pre-pass, and force the next paint to
/// `Full` through the dropped-frame recovery path.
#[test]
fn click_on_empty_bg_does_not_force_full() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    // Frame 0 (cold): expect Full. Submit.
    h.frame(build);
    // Frame 1 (warm): nothing changed → Skip.
    let warm = h.frame(build).plan;
    assert!(warm.is_none(), "warm frame must Skip");

    // Click on empty background (far from the 50×50 frame at origin).
    h.press_at(Vec2::new(180.0, 180.0));
    h.release();
    let click_plan = h.frame(build).plan;
    assert!(
        !matches!(
            click_plan,
            Some(RenderPlan {
                kind: RenderKind::Full,
                ..
            })
        ),
        "click on empty bg escalated to Full repaint: {click_plan:?}",
    );
}

#[test]
fn valid_skip_preserves_incremental_damage_baseline() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let first = h.frame_without_baseline(|ui| one_frame(ui, BLUE)).plan;
    assert!(matches!(
        first,
        Some(RenderPlan {
            kind: RenderKind::Full,
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
                kind: RenderKind::Partial { .. },
                ..
            })
        ),
        "valid skip must retain the incremental baseline: {next:?}",
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
                kind: RenderKind::Full,
                ..
            })
        ),
        "invalid output must discard the incremental baseline: {next:?}",
    );
}

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns `Partial(rect)`, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_partial_when_small() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut h, |ui| {
        one_frame(ui, RED);
    });
    let region = h.damage_region();
    let r = region
        .iter_rects()
        .next()
        .expect("single-leaf change → some damage");
    assert_eq!(Damage::new(h.damage_region()), Damage::Partial(r.into()),);
}

// DamageEngine rects must be in *screen space*. When an ancestor has a
// transform, the rendered position of a node differs from its layout
// rect; the damage rect, the prev_frame snapshot, and the encoder/
// backend scissor all need to track that screen-space position.

/// Soundness pin for the tier's entry-less leg: a node skipped by the
/// Vacant-arm off-surface filter (no `prev` snapshot) that scrolls
/// *into* view under tier 1.5 is covered by the curr-extent push and
/// gets its snapshot inserted in the same pass, a following still
/// frame is a clean Skip (tier 1 at the subtree root), a second move
/// clears its previous position (the inserted snapshot feeds the
/// prev-extent fold), a content change on it lands its rect, and
/// removing it while visible clears its pixels (the eviction tail
/// finds the snapshot). The last two legs regress without the tier-1.5
/// insert: the second move smears (old pixels stay) and the removal
/// computes `Damage::Skip` outright.
#[test]
fn offscreen_node_scrolling_into_view_is_covered_and_stays_sound() {
    let mut h = UiHarness::new(DISPLAY.physical);
    // Surface is 200×200 (test DISPLAY). Three 100-wide frames: "c"
    // starts at x = 200 — exactly off-surface (edge-touching rects
    // don't intersect), so its Vacant visit skips the snapshot insert.
    let build = |dx: f32, c_fill: Option<Color>, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("outer"))
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        let cells = [("a", Some(BLUE)), ("b", Some(BLUE)), ("c", c_fill)];
                        for (key, fill) in cells {
                            let Some(fill) = fill else { continue };
                            Frame::new()
                                .id(WidgetId::from_hash(key))
                                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            });
    };
    frame(&mut h, |ui| build(0.0, Some(RED), ui));

    // Scroll left: "c" enters at (100..200). Tier 1.5 fires at
    // "inner"; "c" had no snapshot (off-surface skip last frame) — the
    // curr-extent push covers its pixels and the insert leg snapshots
    // it now that it's visible.
    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    let covers_c = region
        .iter_rects()
        .any(|r| r.min.x <= 100.5 && r.max().x >= 200.0 - 0.5 && r.max().y >= 40.0 - 0.5);
    assert!(
        covers_c,
        "curr-extent push must cover the newly revealed node. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Still frame: nothing changed — tier 1 skips at the root.
    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    assert_eq!(damage, Damage::Skip, "still frame after the move");

    // Second move: "c" shifts to (0..100). Its just-inserted snapshot
    // joins the prev-extent fold, so its old pixels at (100..200)
    // repaint alongside the new position.
    let damage = frame(&mut h, |ui| build(-200.0, Some(RED), ui));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    for (label, probe) in [
        ("old", Rect::new(150.0, 0.0, 10.0, 40.0)),
        ("new", Rect::new(50.0, 0.0, 10.0, 40.0)),
    ] {
        assert!(
            region.any_intersects(probe),
            "second move must damage c's {label} position; region = {:?}",
            region.iter_rects().collect::<Vec<_>>(),
        );
    }

    // Content change on "c" (now snapshotted, at 0..100): the walk
    // descends and the changed-paints arm damages its rect.
    let damage = frame(&mut h, |ui| build(-200.0, Some(BLUE), ui));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    let rects: Vec<Rect> = region.iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 100.0, 40.0)],
        "content change on the revealed node damages its rect",
    );

    // Remove "c" while visible: the eviction tail finds the inserted
    // snapshot and clears its pixels.
    let damage = frame(&mut h, |ui| build(-200.0, None, ui));
    let covers_removed = match damage {
        Damage::Full => true,
        Damage::Partial(region) => region.any_intersects(Rect::new(50.0, 0.0, 10.0, 40.0)),
        Damage::Skip => false,
    };
    assert!(
        covers_removed,
        "removing the revealed node must damage its pixels; got {damage:?}",
    );
}

#[test]
fn no_damage_means_skip() {
    let d = DamageEngine::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what coverage above
    // [`FULL_REPAINT_THRESHOLD`] produces.
    assert_eq!(
        Damage::new(DamageRegion::collapse_from(
            &d.raw_rects,
            d.budget_px,
            TEST_SURFACE
        )),
        Damage::Skip,
    );
}

/// Heuristic: total coverage = `sum(rect.area()) / surface_area`;
/// strictly above `FULL_REPAINT_THRESHOLD` (0.7) ⇒ Full, otherwise
/// Partial. The check is `>`, not `>=`, so coverage exactly at the
/// threshold stays Partial. `total_area` sums per-rect areas of the
/// post-merge region, so adjacent rects that the proximity-merge
/// rule collapses contribute their merged-bbox area (which here
/// equals the input sum since they tile cleanly). Inputs go through
/// `collapse_from` (the only constructor that seals `coverage`); the
/// `region()` helper builds the unsealed *expected* values, which
/// still match because coverage is excluded from `PartialEq`.
#[test]
fn damage_filter_threshold_cases() {
    use crate::scene::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    fn region(rects: &[Rect]) -> DamageRegion {
        let mut r = DamageRegion::default();
        for rect in rects {
            r.add(*rect);
        }
        r
    }
    // Adjacent halves on the 100×100 surface — a perfectly adjacent
    // pair has `union_excess = bbox − a − b = 0`, below any positive
    // SAH budget, so each pair collapses into one rect whose area
    // equals the input sum. The region's `total_area` then lands
    // exactly at the threshold (or just above) and the strict `>`
    // decision logic is what's under test; the merge is guaranteed by
    // the zero excess alone, independent of the budget's exact value.
    const PAIR_BELOW: [Rect; 2] = [
        // Merges to Rect(0,0,70,100); total_area = 7000 / 10000 = 0.70
        // → stays Partial (`>` is strict).
        Rect::new(0.0, 0.0, 35.0, 100.0),
        Rect::new(35.0, 0.0, 35.0, 100.0),
    ];
    const PAIR_ABOVE: [Rect; 2] = [
        // Merges to Rect(0,0,72,100); total_area = 7200 / 10000 = 0.72
        // → escalates Full.
        Rect::new(0.0, 0.0, 36.0, 100.0),
        Rect::new(36.0, 0.0, 36.0, 100.0),
    ];
    let cases: &[(&str, &[Rect], Rect, Damage)] = &[
        (
            "small_1pct",
            &[Rect::new(0.0, 0.0, 10.0, 10.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 10.0, 10.0).into()),
        ),
        (
            "large_81pct_above_threshold",
            &[Rect::new(0.0, 0.0, 90.0, 90.0)],
            TEST_SURFACE,
            Damage::Full,
        ),
        (
            "below_threshold_64pct_stays_partial",
            &[Rect::new(0.0, 0.0, 80.0, 80.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 80.0, 80.0).into()),
        ),
        (
            "exact_70pct_stays_partial",
            &[Rect::new(0.0, 0.0, 70.0, 100.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 70.0, 100.0).into()),
        ),
        (
            "two_rect_sum_at_threshold_stays_partial",
            &PAIR_BELOW,
            TEST_SURFACE,
            Damage::Partial(region(&PAIR_BELOW)),
        ),
        (
            "two_rect_sum_above_threshold_escalates_full",
            &PAIR_ABOVE,
            TEST_SURFACE,
            Damage::Full,
        ),
        // Zero-area-surface case dropped: `collapse_from` now asserts
        // `surface_area > EPS` (host filters resize-to-zero before we
        // ever reach this layer), so the prior `Damage::Full` fallback
        // became unreachable.
    ];
    for (label, rects, surface, want) in cases {
        let region = DamageRegion::collapse_from(rects, DEFAULT_PASS_BUDGET_PX, *surface);
        assert_eq!(Damage::new(region), *want, "case: {label}");
    }
}

/// Pin: a Display change between frames (resize or scale-factor)
/// forces the next compute to `Full` regardless of how few widgets
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
            "scale_factor",
            Display {
                scale_factor: 2.0,
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
                scale_factor: 2.0,
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
                    kind: RenderKind::Full,
                    ..
                })
            ),
            "case: {label} f1"
        );
        let f2 = h.frame(&mut build).plan;
        assert!(f2.is_none(), "case: {label} f2 must Skip");
        assert!(
            h.ui.damage_engine.counters.dirty().is_empty(),
            "case: {label} steady"
        );
        // Mutate Display; identical authoring; must short-circuit to Full.
        let mutated_plan = h.set_display(*mutated).frame(&mut build).plan;
        assert!(
            matches!(
                mutated_plan,
                Some(RenderPlan {
                    kind: RenderKind::Full,
                    ..
                })
            ),
            "case: {label} display change"
        );
        assert!(
            !h.ui.damage_engine.counters.dirty().is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        // Stable surface at the new size, identical authoring → back to Skip.
        let stable = h.frame(&mut build).plan;
        assert!(
            stable.is_none(),
            "case: {label} post-mutation steady must Skip",
        );
        assert!(
            h.ui.damage_engine.counters.dirty().is_empty(),
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
    assert!(h.ui.damage_engine.counters.dirty().is_empty());

    // Inject: flip widget "small"'s prev `cascade_input` so the next
    // diff sees it as a cascade-state change and damages its paint_rect
    // (50×60 = 3000 area) inside a 2000×2000 surface (4M area) —
    // ratio ≈ 0.075%, well below the full-repaint threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap =
        h.ui.damage_engine
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
                kind: RenderKind::Full,
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
    let build = |ui: &mut Ui, color: Color| {
        one_frame(ui, color);
    };

    // Warm up: two identical frames bring damage to steady state.
    h.frame(|ui| build(ui, BLUE));
    let warm = h.frame(|ui| build(ui, BLUE)).plan;
    assert!(warm.is_none(), "warm steady-state must Skip");
    assert!(h.ui.damage_engine.counters.dirty().is_empty());
    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    let changed = h.frame(|ui| build(ui, RED)).plan;
    let Some(RenderPlan {
        kind: RenderKind::Partial { region },
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
        region.coverage < 0.5,
        "damage region should be small (partial repaint range), got {region:?}",
    );
}

/// `DamageRegion::collapse_from` intersects each input rect with the
/// surface before folding it into the region. Without this, a
/// paint_rect whose bounds extend past the viewport (root-level
/// transformed canvas with no clip ancestor, plus high zoom —
/// `parent_clip` stays `None` so `cascade::compute_paint_rect` never
/// clips down) would inflate `total_area` past the threshold despite
/// only a tiny visible fraction. Reproduces the darkroom graph
/// pan/zoom regression where a few zoomed-up node panels off-screen
/// would force `Damage::Full` each pan tick.
#[test]
fn partial_when_oversized_rect_lies_mostly_off_surface() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    // 1000×1000 paint_rect anchored at (90, 90): only a 10×10 corner
    // pokes into the surface, the rest sticks off-screen. Pre-fix:
    // rect.area() = 1e6, ratio = 1e6 / 1e4 = 100 ⇒ Full. Post-fix:
    // collapse_from clips to (90,90,10,10), area = 100, ratio = 0.01
    // ≪ 0.7 ⇒ Partial.
    let oversized = Rect::new(90.0, 90.0, 1000.0, 1000.0);
    assert_eq!(
        oversized.clamp_to(surface),
        Rect::new(90.0, 90.0, 10.0, 10.0),
        "sanity: 1000×1000 rect at (90,90) intersects surface in a 10×10 corner",
    );
    let region = DamageRegion::collapse_from(&[oversized], f32::INFINITY, surface);
    // Region stores the clipped rect, not the raw input.
    let stored: Vec<_> = region.iter_rects().collect();
    assert_eq!(
        stored,
        vec![Rect::new(90.0, 90.0, 10.0, 10.0)],
        "collapse_from must store the surface-clipped rect, not the raw input",
    );
    let damage = Damage::new(region);
    assert!(
        matches!(damage, Damage::Partial(_)),
        "off-surface inflation must not trip FULL_REPAINT_THRESHOLD; got {damage:?}",
    );
}

/// Sister to the above: a rect that *fully* covers the surface
/// (regardless of how much extends past) still trips Full. The intent
/// of the surface-clamp is "don't count pixels that can't be painted,"
/// not "don't ever Full" — when the visible portion is the whole
/// viewport, Full is still the right call.
#[test]
fn full_when_visible_portion_covers_surface_even_if_rect_overflows() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    let covers_all_plus_overflow = Rect::new(-50.0, -50.0, 1000.0, 1000.0);
    let region = DamageRegion::collapse_from(&[covers_all_plus_overflow], f32::INFINITY, surface);
    let damage = Damage::new(region);
    assert_eq!(
        damage,
        Damage::Full,
        "rect that covers entire surface (plus overflow) must still trip Full",
    );
}

/// A rect that lies entirely off the surface contributes nothing to
/// the region (zero-area after clipping, dropped). Pins the "early-out
/// on degenerate clip" branch in `collapse_from`.
#[test]
fn fully_off_surface_rect_is_dropped_from_region() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    let off_screen = Rect::new(500.0, 500.0, 50.0, 50.0);
    let region = DamageRegion::collapse_from(&[off_screen], f32::INFINITY, surface);
    assert!(
        region.rects.is_empty(),
        "wholly-off-surface rect must produce an empty region (no Damage::Skip vs Partial drift)",
    );
}

/// First-seen Vacant arm short-circuits when `curr_rect` lies entirely
/// off the surface. The hashmap insert and rect push would both be
/// wasted: the rect is dropped by `collapse_from`'s surface-clip
/// downstream, and the prev entry would just describe an invisible
/// snapshot that the next frame's diff would have to evict. Pins the
/// pan/zoom workload where a node panned past the viewport edge
/// contributes nothing useful to damage bookkeeping.
#[test]
fn off_surface_first_seen_node_skips_prev_insert() {
    let straddling = [
        Paint {
            screen: Rect::new(-20.0, 0.0, 10.0, 10.0),
            ..Default::default()
        },
        Paint {
            screen: Rect::new(110.0, 0.0, 10.0, 10.0),
            ..Default::default()
        },
    ];
    assert!(
        !straddling.any_on_surface(Rect::new(0.0, 0.0, 100.0, 100.0)),
        "the union can cross the surface even though no paint row does",
    );

    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        // Wrap in a transformed parent: `Panel::transform` applies to
        // the body (children), so the inner panel's chrome paint_rect
        // = parent_transform.apply_rect(inner.layout_rect). With a
        // (+500,+500) parent translate over a 200×200 surface, the
        // inner panel's chrome lands at (500,500,50,50) — wholly off.
        Panel::canvas()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::FILL, Sizing::FILL))
            .transform(TranslateScale::from_translation(Vec2::new(500.0, 500.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("off"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
            });
    });

    assert!(
        !h.ui
            .damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("off")),
        "Vacant + off-surface paint_rect must not seed a prev entry — \
         hashmap insert + raw_rects push are both wasted work for a \
         node that contributes nothing visible",
    );
    assert!(
        h.damage_region().rects.is_empty(),
        "no visible widgets means no damage rects on the second-frame \
         diff (first frame is Full and walks differently)",
    );
}
