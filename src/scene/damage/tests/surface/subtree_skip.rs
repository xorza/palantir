//! Subtrees the diff can jump over, and the transitions that end that.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::popup::Popup;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::Vec2;

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
        h.engines.damage.counters.subtree_skips(),
        0,
        "first frame populates prev — no prior snapshots to skip against"
    );

    frame(&mut h, build);
    assert!(
        h.engines.damage.counters.subtree_skips() >= 1,
        "identical second frame must skip at least the painting_parent subtree, got {}",
        h.engines.damage.counters.subtree_skips(),
    );
    assert!(h.engines.damage.counters.dirty().is_empty());
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
    assert!(h.engines.damage.prev.contains_key(&id));

    frame(&mut h, no_bg);
    assert!(
        !h.engines.damage.prev.contains_key(&id),
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
        damage: Damage::Partial(damage),
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
        damage.coverage < 0.5,
        "damage region covers {:.1}% of surface — eater leaked into damage",
        100.0 * damage.coverage
    );
}

/// Regression: a click on empty background has no route, so it must not set
/// `frame_had_action`, run a discarded pre-pass, and force the next paint
/// to
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
                damage: Damage::Full,
                ..
            })
        ),
        "click on empty bg escalated to Full repaint: {click_plan:?}",
    );
}
