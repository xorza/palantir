use super::region::DamageRegion;
use super::{Damage, DamageEngine};
use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::forest::tree::NodeId;
use crate::input::InputEvent;
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, transform::TranslateScale};
use crate::ui::FrameStamp;
use crate::ui::frame_report::RenderPlan;
use crate::widgets::popup::Popup;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};
use std::time::Duration;

#[allow(dead_code)]
const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 200.0);
const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    scale_factor: 1.0,
    pixel_snap: true,
};

/// Drive one frame through the real [`Ui::frame`] path, simulate a
/// successful `WgpuBackend::submit` so the next frame's auto-rewind
/// doesn't fire, and return the damage decision for the just-completed
/// frame. Test sites that care about the damage shape bind the return;
/// the rest ignore it.
fn frame(ui: &mut Ui, f: impl FnMut(&mut Ui)) -> Damage {
    let report = ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), f);
    ui.frame_state.mark_submitted();
    match report.plan {
        None => Damage::None,
        Some(RenderPlan::Full { .. }) => Damage::Full,
        Some(RenderPlan::Partial { region, .. }) => Damage::Partial(region),
    }
}

/// The standard "root with one 50×50 frame" tree used by most damage
/// tests. Color flips between frames to drive minimal authoring
/// changes.
const BLUE: Color = Color::rgb(0.2, 0.4, 0.8);
const RED: Color = Color::rgb(0.9, 0.4, 0.8);

fn one_frame(ui: &mut Ui, color: Color) {
    Panel::hstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: color.into(),
                    ..Default::default()
                })
                .show(ui);
        });
}

/// Pin: the very first frame has no `prev_frame` entries, so every
/// painting node is "added" → marked dirty and contributes its rect.
/// The root Panel records no chrome and no direct shapes, so it's
/// non-painting and stays out of `dirty`/`region`.
#[test]
fn first_frame_marks_every_painting_node_dirty() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    let painting = ui.layout.cascades.layers[Layer::Main]
        .paint_arena
        .node_spans
        .iter()
        .filter(|s| s.len > 0)
        .count();
    assert_eq!(ui.damage_engine.dirty.len(), painting);
    // First frame is `force_full`, so `compute` short-circuits to
    // `Damage::Full` after the structural diff and skips the
    // collapse — `region` stays empty by design. Check the pre-
    // collapse `raw_rects` buffer to confirm every painting node
    // actually pushed its rect.
    assert!(!ui.damage_engine.raw_rects.is_empty());
}

/// Pin: re-recording identical authoring → zero dirty nodes,
/// damage rect is `None`. The steady-state ideal: idle UI does
/// nothing.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut ui, build);
    frame(&mut ui, build);

    assert!(ui.damage_engine.dirty.is_empty());
    assert!(ui.damage_region().is_empty());
    assert_eq!(
        Damage::new(ui.display.logical_rect(), ui.damage_region()),
        Damage::None,
    );
}

/// Pin: when a subtree's `(paint_rect, node_hash, subtree_hash,
/// cascade_input)` all match the prev-frame snapshot at its painting
/// root, the damage diff jumps to `subtree_end` instead of walking every
/// descendant. The fast path's correctness is already covered by every
/// "unchanged → no damage" test in this file; this pin specifically
/// guards that the jump *fires* — without it the path silently degrades
/// to a per-node walk that still produces correct damage.
#[test]
fn stable_painting_subtree_triggers_skip_jump() {
    let mut ui = Ui::for_test();
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
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(60.0)))
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
    frame(&mut ui, build);
    assert_eq!(
        ui.damage_subtree_skips(),
        0,
        "first frame populates prev — no prior snapshots to skip against"
    );

    frame(&mut ui, build);
    assert!(
        ui.damage_subtree_skips() >= 1,
        "identical second frame must skip at least the painting_parent subtree, got {}",
        ui.damage_subtree_skips(),
    );
    assert!(ui.damage_engine.dirty.is_empty());
}

/// Pin: a widget that loses its background between frames flips from
/// painting to non-painting. The diff must (a) contribute its prev
/// rect to damage so the prior pixels get cleared, (b) drop the entry
/// from `prev` so the next frame sees it as truly absent, and (c)
/// contribute no curr rect.
#[test]
fn paints_to_non_paints_transition_evicts_and_clears() {
    let mut ui = Ui::for_test();
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
    frame(&mut ui, with_bg);
    let id = WidgetId::from_hash("a");
    assert!(ui.damage_engine.prev.contains_key(&id));

    frame(&mut ui, no_bg);
    assert!(
        !ui.damage_engine.prev.contains_key(&id),
        "paints→non-paints transition must evict the prev entry"
    );
    let rects: Vec<_> = ui.damage_region().iter_rects().collect();
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
    let mut ui = Ui::for_test();
    let anchor = glam::Vec2::new(40.0, 40.0);
    // Frame 1: popup open. Eater (full-surface) + body (small).
    frame(&mut ui, |ui| {
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
    let out = ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
        Frame::new()
            .id(WidgetId::from_hash("placeholder"))
            .size(10.0)
            .show(ui);
    });
    let Some(RenderPlan::Partial { region, .. }) = out.plan else {
        panic!(
            "popup dismissal escalated to {:?}; eater contributed full-surface \
             rect despite painting nothing",
            out.plan
        );
    };
    let surface_area = DISPLAY.logical_rect().area();
    assert!(
        region.total_area() / surface_area < 0.5,
        "damage region covers {:.1}% of surface — eater leaked into damage",
        100.0 * region.total_area() / surface_area
    );
}

/// Regression: a click on empty background (no widget hit, no
/// state change) must not force the next paint to `Full`. The
/// discarded pre-pass in `run_frame` (triggered by any pointer /
/// key event via `frame_had_action`) calls `pre_record` →
/// `reset_to_idle`, then never reaches `post_record`. Pass 2's
/// `pre_record` then sees `frame_state == IDLE` and treats it as
/// "host dropped the previous frame", invalidating prev_surface
/// and forcing `Damage::Full`.
#[test]
fn click_on_empty_bg_does_not_force_full() {
    use crate::input::pointer::PointerButton;
    use std::time::Duration;
    let mut ui = Ui::for_test();
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
    ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), build);
    ui.frame_state.mark_submitted();
    // Frame 1 (warm): nothing changed → Skip.
    let warm = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), build)
        .plan;
    assert!(warm.is_none(), "warm frame must Skip");
    ui.frame_state.mark_submitted();

    // Click on empty background (far from the 50×50 frame at origin).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(180.0, 180.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    let click_plan = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), build)
        .plan;
    assert!(
        !matches!(click_plan, Some(RenderPlan::Full { .. })),
        "click on empty bg escalated to Full repaint: {click_plan:?}",
    );
}

/// Regression: a `Skip` frame that the host bypasses (no
/// `backend.submit` → no `mark_submitted`) must not force the next
/// frame to `Full`. `post_record` marks `Skip` as submitted directly so
/// the next `pre_record`'s auto-rewind doesn't kick in.
#[test]
fn skip_frame_does_not_force_next_to_full() {
    let mut ui = Ui::for_test();
    let first = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(matches!(first, Some(RenderPlan::Full { .. })));
    ui.frame_state.mark_submitted();

    // Identical content → Skip. Host::render confirms submitted on
    // the skip path too (copies the backbuffer onto the swapchain);
    // the test mirrors that ack.
    let skip = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(skip.is_none(), "identical content must Skip");
    ui.frame_state.mark_submitted();

    // Next frame: still no diff. Pre-fix this could regress to Full if
    // the skip wasn't acked — Host::render owns that ack now.
    let next = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(
        next.is_none(),
        "Skip frames must not poison the next frame into Full",
    );
}

/// Regression: a host that early-returns on `skip_render` (the natural
/// pattern — no swapchain acquire when there's nothing to paint, see
/// `examples/showcase/main.rs`) never calls `mark_submitted`. Without
/// `Ui::frame` self-acking skip frames, the next paint frame's
/// `classify_frame` saw `frame_skipped = true` and escalated
/// to `Full` — visible as a full-window red flash in the damage debug
/// overlay on every idle→input transition (e.g. mouse move).
#[test]
fn skip_frame_without_explicit_ack_does_not_force_next_to_full() {
    let mut ui = Ui::for_test();
    let first = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(matches!(first, Some(RenderPlan::Full { .. })));
    ui.frame_state.mark_submitted();

    // Identical content → Skip. Host bypasses `render()` entirely and
    // never acks; `Ui::frame` must self-ack the skip.
    let skip = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(skip.is_none(), "identical content must Skip");
    // NOTE: deliberately no `mark_submitted` here.

    // Authoring change → expect `Partial`, not `Full`.
    let next = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, RED)
        })
        .plan;
    assert!(
        matches!(next, Some(RenderPlan::Partial { .. })),
        "unacked skip poisoned next frame into Full: {next:?}",
    );
}

/// Pin: an authoring change on one leaf marks just that leaf
/// dirty; the parent (whose own fields didn't change and whose
/// rect is identical) stays clean.
#[test]
fn fill_change_marks_only_the_changed_leaf() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });

    assert_eq!(ui.damage_engine.dirty.len(), 1);
    let dirty_id = ui.damage_engine.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("a")
    );
    // DamageEngine rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage_region().iter_rects().next(),
        Some(ui.layout[Layer::Main].rect[dirty_id.idx()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::for_test();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::Fixed(30.0), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    frame(&mut ui, |ui| build(50.0, ui));
    frame(&mut ui, |ui| build(80.0, ui));

    // `a` changed authoring (size). `b`'s authoring is unchanged
    // but its arranged x shifts from 50 → 80. Both are dirty.
    let dirty_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("a")));
    assert!(dirty_ids.contains(&WidgetId::from_hash("b")));
}

/// Pin: a widget that disappears between frames contributes its
/// previous rect to damage — the renderer must repaint that
/// region to erase the leftover pixels.
#[test]
fn removed_widget_contributes_prev_rect_to_damage() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    let prev_button_rect = ui.damage_engine.prev[&WidgetId::from_hash("gone")].rect;

    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Button is gone; root Panel is non-painting (no chrome) so it
    // never entered prev. Only contribution is the Button's prev
    // rect, surfaced via the `removed` list.
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    assert_eq!(rects, vec![prev_button_rect]);
}

/// Pin: an added widget that wasn't in last frame contributes
/// its current rect to damage and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("new"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });

    let dirty_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(!ui.damage_region().is_empty());
}

// --- Ui::damage_filter ---------------------------------------------------

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns `Partial(rect)`, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_partial_when_small() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });
    let region = ui.damage_region();
    let r = region
        .iter_rects()
        .next()
        .expect("single-leaf change → some damage");
    assert_eq!(
        Damage::new(ui.display.logical_rect(), ui.damage_region()),
        Damage::Partial(r.into()),
    );
}

// --- transforms ---------------------------------------------------------
// DamageEngine rects must be in *screen space*. When an ancestor has a
// transform, the rendered position of a node differs from its layout
// rect; the damage rect, the prev_frame snapshot, and the encoder/
// backend scissor all need to track that screen-space position.

/// Pin: when a transformed parent's child changes authoring, the
/// damage rect covers the child's *screen* rect (post-transform),
/// not its layout rect. Without this, the backend scissor would
/// clip the actual paint position and leave the screen unchanged.
#[test]
fn child_under_transformed_parent_damage_in_screen_space() {
    let translate = Vec2::new(100.0, 0.0);
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let build = |fill: Color, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(translate))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: fill.into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut ui, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut ui, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = ui.layout[Layer::Main].rect[child_node.unwrap().idx()];
    let expected_screen_rect = Rect {
        min: child_layout_rect.min + translate,
        size: child_layout_rect.size,
    };
    let region = ui.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.min.x >= 100.0 - 0.5,
        "damage min.x must reflect parent translate; got {damage_rect:?}, expected near {expected_screen_rect:?}",
    );
    assert_eq!(damage_rect, expected_screen_rect);
}

/// Pin: animating a parent's transform shifts every child's screen
/// rect even though the children's authoring is unchanged. The
/// damage union must cover both prev and curr screen rects so the
/// backend repaints over the old positions too (otherwise the old
/// frame's pixels would streak through `LoadOp::Load`).
#[test]
fn animated_parent_transform_unions_old_and_new_positions() {
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    };

    build(0.0, &mut ui, &mut child_node);
    build(50.0, &mut ui, &mut child_node);

    // Child layout rect didn't change. Parent's transform shifted by
    // (50, 0). Prev screen rect = (0,0,40,40); curr = (50,0,40,40);
    // gap of 10 px between them. bbox = 90×40 = 3600, sum = 3200,
    // SAH cost = 400 ≪ default budget — the merge rule collapses
    // into one bbox. (A *much* larger distance would push cost over
    // the budget; pinned by
    // `transform_animation_keeps_far_positions_split`.)
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(50.0, 0.0, 40.0, 40.0);
    assert_eq!(
        rects,
        vec![prev.union(curr)],
        "near transform animation → one merged bbox",
    );
    // Only the child is dirty: its authoring is unchanged but its
    // screen rect moved (rect comparison catches this). The parent
    // panel's own paint is unaffected by its own transform — the
    // transform only composes into descendants — so the parent's
    // hash and screen rect are both stable, leaving it clean.
    let dirty_widget_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert_eq!(dirty_widget_ids, vec![WidgetId::from_hash("c")]);
}

/// Sister case to the test above: under a tight pass-budget, a
/// far-apart transform animation keeps prev and curr screen rects
/// split. Pinning both ends of the merge rule means a budget tweak
/// can't silently flip behaviour without breaking a test.
#[test]
fn transform_animation_keeps_far_positions_split() {
    let mut ui = Ui::for_test();
    // Drop the merge budget to strict-overlap-only so the prev/curr
    // pair (cost 6 400 < default budget) stays split. Pins both
    // ends of the merge rule against future budget tweaks.
    ui.damage_engine.budget_px = 0.0;
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    };

    build(0.0, &mut ui, &mut child_node);
    build(200.0, &mut ui, &mut child_node);

    // prev (0,0,40,40) area 1600; curr (200,0,40,40) area 1600.
    // bbox 240×40 = 9600. SAH cost = 6400 — under the default
    // 20 000 budget, this would merge; the guard above drops the
    // budget to 0 to pin the strict-overlap-only branch.
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(200.0, 0.0, 40.0, 40.0);
    assert_eq!(rects.len(), 2, "far transform animation → two rects");
    assert!(rects.contains(&prev) && rects.contains(&curr), "{rects:?}");
}

/// Soundness pin: when an ancestor's transform changes, a node whose
/// own `paint_rect` is **clipped invariant** (because its direct
/// shapes extend past the viewport / clip on every frame, so
/// `clip_to(...)` saturates to the same rect both passes) must still
/// contribute its `paint_rect` to damage. Otherwise the pixels of
/// those shapes — which DID move with the parent transform — get
/// stranded; the old positions never get cleared.
///
/// Repro of darkroom's "panning Scroll over a node-graph Canvas
/// leaves bezier trails": canvas's connection beziers are direct
/// shapes; canvas is wider than the viewport so its clipped paint
/// rect saturates; canvas's `node_hash` is stable but its
/// `cascade_input` shifts every pan frame.
#[test]
fn transform_shifted_direct_shape_with_invariant_clipped_paint_rect_contributes_damage() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            // Outermost clip pins descendants to the surface viewport
            // — without it, `parent_clip = None` and inner's paint
            // rect translates freely (the bug then doesn't manifest;
            // damage catches the rect change via the normal path).
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                                .show(ui, |ui| {
                                    // Shape wider than the surface so
                                    // the clipped paint rect
                                    // saturates and stays invariant
                                    // under small `dx` translates.
                                    ui.add_shape(Shape::RoundedRect {
                                        local_rect: Some(Rect::new(-200.0, 0.0, 500.0, 50.0)),
                                        corners: Corners::ZERO,
                                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                                        stroke: Stroke::default(),
                                    });
                                });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    build(5.0, &mut ui);
    let region = ui.damage_region();
    let covered = region.iter_rects().any(|r| {
        // Damage must cover the inner node's clipped paint area
        // (0..100 × 0..50) — that's where the shape's pixels live
        // both before and after the small pan.
        r.min.x <= 0.5 && r.min.y <= 0.5 && r.max().x >= 50.0 - 0.5 && r.max().y >= 50.0 - 0.5
    });
    assert!(
        covered,
        "ancestor-transform shift moves a direct-shape leaf's pixels; \
         damage must still cover the shape area even though the \
         clipped paint_rect is invariant. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Sister test to the soundness pin above: the new "cascade_input
/// shift on a direct-paint node → push `curr_rect`" branch in the
/// damage diff must not trip `FULL_REPAINT_THRESHOLD` for a pan of a
/// modestly-sized clip-saturated node. Same setup as that pin, but
/// repeated for several pan ticks; each step's damage stays
/// `Partial` and stays bounded to the inner clipped area.
#[test]
fn pan_with_invariant_clipped_paint_rect_stays_partial() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                                .show(ui, |ui| {
                                    ui.add_shape(Shape::RoundedRect {
                                        local_rect: Some(Rect::new(-200.0, 0.0, 500.0, 50.0)),
                                        corners: Corners::ZERO,
                                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                                        stroke: Stroke::default(),
                                    });
                                });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    for dx in [3.0, 6.0, 9.0, 12.0] {
        build(dx, &mut ui);
        let region = ui.damage_region();
        let damage = Damage::new(ui.display.logical_rect(), region);
        assert!(
            matches!(damage, Damage::Partial(_)),
            "pan with clip-saturated direct-paint node must stay Partial \
             (the new diff branch pushes one paint_rect per shifted node; \
             that must not blow past FULL_REPAINT_THRESHOLD on a single tick). \
             dx = {dx}, region = {:?}, damage = {damage:?}",
            region.iter_rects().collect::<Vec<_>>(),
        );
    }
}

/// Reproduces the darkroom graph-canvas regression: a panel with
/// `Panel::transform` and direct shapes (bezier connections) shifts
/// its own transform every pan frame. Under the `Panel::transform`
/// contract those shapes paint *inside* the self-transform, so their
/// tessellated pixels move — but `cascade_input` only tracks
/// ancestor state and stays put. The fix is at the source: own
/// transform now folds into `node_hash`, so the diff's
/// `e.get().hash == curr_node_hash` guard fails and the generic
/// Occupied arm pushes both prev and curr rects, sweeping where the
/// shapes were and are.
#[test]
fn self_transform_shift_damages_direct_shapes() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id(WidgetId::from_hash("xpanel"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .show(ui, |ui| {
                            // Direct shape on the transformed panel —
                            // mirrors how darkroom adds connection
                            // beziers on the inner canvas.
                            ui.add_shape(Shape::RoundedRect {
                                local_rect: Some(Rect::new(40.0, 40.0, 30.0, 30.0)),
                                corners: Corners::ZERO,
                                fill: Color::rgb(0.2, 0.6, 0.9).into(),
                                stroke: Stroke::default(),
                            });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    build(20.0, &mut ui);
    let region = ui.damage_region();

    // After translating self by dx=20, the shape's prev pixels lived
    // at [40, 70] × [40, 70] (translation 0) and the new pixels live
    // at [60, 90] × [40, 70]. Damage must cover both — i.e. at least
    // [40, 90] × [40, 70].
    let covered = region.iter_rects().any(|r| {
        r.min.x <= 40.5 && r.min.y <= 40.5 && r.max().x >= 90.0 - 0.5 && r.max().y >= 70.0 - 0.5
    });
    assert!(
        covered,
        "self-transform shift on a panel with direct shapes must \
         damage both old and new shape positions. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

// --- DamageEngine::filter heuristic ---------------------------------------------

const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

#[test]
fn no_damage_means_skip() {
    let d = DamageEngine::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what coverage above
    // [`FULL_REPAINT_THRESHOLD`] produces.
    assert_eq!(
        Damage::new(
            TEST_SURFACE,
            DamageRegion::collapse_from(&d.raw_rects, d.budget_px, TEST_SURFACE)
        ),
        Damage::None,
    );
}

/// Heuristic: total coverage = `sum(rect.area()) / surface_area`;
/// strictly above `FULL_REPAINT_THRESHOLD` (0.7) ⇒ Full, otherwise
/// Partial. The check is `>`, not `>=`, so coverage exactly at the
/// threshold stays Partial. A zero-area surface forces Full
/// (divide-by-zero guard). `total_area` sums per-rect areas of the
/// post-merge region, so adjacent rects that the proximity-merge
/// rule collapses contribute their merged-bbox area (which here
/// equals the input sum since they tile cleanly).
#[test]
fn damage_filter_threshold_cases() {
    use super::region::DamageRegion;
    fn region(rects: &[Rect]) -> DamageRegion {
        let mut r = DamageRegion::default();
        for rect in rects {
            r.add(*rect);
        }
        r
    }
    // Adjacent halves on the 100×100 surface — the proximity-merge
    // policy collapses each pair into one rect whose area equals the
    // input sum, so the region's `total_area` lands exactly at the
    // threshold (or just above) and the strict `>` decision logic
    // is what's under test. Non-mergeable split-pair geometry at the
    // 0.7 threshold is mathematically impossible under
    // `MERGE_AREA_RATIO = 1.6` (would need `bbox > 1.12 × surface`),
    // so threshold-edge two-rect cases all collapse here first.
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
        // Zero-area-surface case dropped: `Damage::new` now asserts
        // `surface_area > 0.0` (host filters resize-to-zero before
        // we ever reach this layer), so the prior `Damage::Full`
        // fallback became unreachable.
    ];
    for (label, rects, surface, want) in cases {
        let region = region(rects);
        assert_eq!(Damage::new(*surface, region), *want, "case: {label}");
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
    ];
    for (label, mutated) in cases {
        let mut ui = Ui::for_test();
        let mut build = |ui: &mut Ui| {
            one_frame(ui, BLUE);
        };

        // Steady-state: Full first frame, then Skip on identical re-record.
        let f1 = ui
            .frame(FrameStamp::new(DISPLAY, Duration::ZERO), &mut build)
            .plan;
        assert!(
            matches!(f1, Some(RenderPlan::Full { .. })),
            "case: {label} f1"
        );
        ui.frame_state.mark_submitted();
        let f2 = ui
            .frame(FrameStamp::new(DISPLAY, Duration::ZERO), &mut build)
            .plan;
        assert!(f2.is_none(), "case: {label} f2 must Skip");
        assert!(ui.damage_engine.dirty.is_empty(), "case: {label} steady");
        ui.frame_state.mark_submitted();

        // Mutate Display; identical authoring; must short-circuit to Full.
        let mutated_plan = ui
            .frame(FrameStamp::new(*mutated, Duration::ZERO), &mut build)
            .plan;
        assert!(
            matches!(mutated_plan, Some(RenderPlan::Full { .. })),
            "case: {label} display change"
        );
        ui.frame_state.mark_submitted();
        assert!(
            !ui.damage_engine.dirty.is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        // Stable surface at the new size, identical authoring → back to Skip.
        let stable = ui
            .frame(FrameStamp::new(*mutated, Duration::ZERO), &mut build)
            .plan;
        assert!(
            stable.is_none(),
            "case: {label} post-mutation steady must Skip",
        );
        assert!(
            ui.damage_engine.dirty.is_empty(),
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
    let mut ui = Ui::for_test();
    let big = Display {
        physical: UVec2::new(2000, 2000),
        ..DISPLAY
    };
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
            .size((Sizing::Fixed(60.0), Sizing::Fixed(120.0)))
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

    ui.frame(FrameStamp::new(big, Duration::ZERO), &mut scene);
    ui.frame_state.mark_submitted();
    ui.frame(FrameStamp::new(big, Duration::ZERO), &mut scene);
    ui.frame_state.mark_submitted();
    assert!(ui.damage_engine.dirty.is_empty());

    // Inject: nudge widget "a"'s prev rect so the next diff sees a
    // small change. Tiny rect (3×50 = 150 area) inside a 2000×2000
    // surface (4M area) — ratio ≈ 0.004%, well below the 50%
    // threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap = ui
        .damage_engine
        .prev
        .get_mut(&target_wid)
        .expect("small in prev");
    snap.rect.min.x += 3.0;

    let smaller = Display {
        physical: UVec2::new(1999, 2000),
        ..big
    };
    let resize_plan = ui
        .frame(FrameStamp::new(smaller, Duration::ZERO), &mut scene)
        .plan;

    assert!(
        matches!(resize_plan, Some(RenderPlan::Full { .. })),
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
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui, color: Color| {
        one_frame(ui, color);
    };

    // Warm up: two identical frames bring damage to steady state.
    ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
        build(ui, BLUE)
    });
    ui.frame_state.mark_submitted();
    let warm = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            build(ui, BLUE)
        })
        .plan;
    assert!(warm.is_none(), "warm steady-state must Skip");
    assert!(ui.damage_engine.dirty.is_empty());
    ui.frame_state.mark_submitted();

    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    let changed = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            build(ui, RED)
        })
        .plan;
    let Some(RenderPlan::Partial { region, .. }) = changed else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {changed:?} — surface-change short-circuit fired incorrectly",
        );
    };
    // DamageEngine rect = the 50×50 frame's rect. Well below 50% of 200×200.
    let total_area = region.total_area();
    assert!(
        total_area / DISPLAY.logical_rect().area() < 0.5,
        "damage region should be small (partial repaint range), got {region:?}",
    );
}

/// Pin (motivating workload): hovering a button causes exactly one
/// node — the button — to be dirty, with damage rect == button's
/// rect. This is the bread-and-butter case Stage 3 is designed for:
/// pointer hover changes a small region; partial repaint should win.
///
/// Hit-test response lags by one frame (recording reads last frame's
/// state), so we run enough frames at each pointer position to let
/// the damage stream settle, then assert on the *transition* frame.
#[test]
fn button_hover_damage_covers_only_the_button() {
    let mut ui = Ui::for_test();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(ui),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    };

    // Pointer parked off-button. Settle for two frames so hit-test +
    // damage are at steady state (no diff).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "off-button pointer should reach a no-diff steady state"
    );

    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    let target = hot_rect.min + Vec2::new(5.0, 5.0);

    // Move pointer onto the hot button. The *next* post_record computes
    // hover=true. The frame *after* that records the button as
    // hovered → its fill differs → it lands in the dirty set alone.
    // `on_input` recomputes hover against the existing hit_index
    // immediately, so the *next* recording sees `hovered=true` and
    // emits the hovered fill. DamageEngine = button rect only.
    ui.on_input(InputEvent::PointerMoved(target));
    build(&mut ui, &mut hot_node, &mut cold_node);

    assert_eq!(
        ui.damage_engine.dirty.len(),
        1,
        "only the hovered button should be dirty"
    );
    let dirty_id = ui.damage_engine.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(ui.display.logical_rect(), ui.damage_region()),
        Damage::Partial(hot_rect.into()),
        "small per-button damage must not trip the full-repaint heuristic",
    );

    // Next frame at same cursor → no diff (settled).
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "settled hover should produce no further damage"
    );
}

/// Pin: leaving the button (un-hover) is symmetric — the only diff
/// is the button's fill flipping back, damage = button rect.
#[test]
fn button_unhover_damage_covers_only_the_button() {
    let mut ui = Ui::for_test();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(ui),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(ui),
                    );
                });
        });
    };

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage_engine.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage_engine.dirty.len(), 1);
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[ui.damage_engine.dirty[0].idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(ui.display.logical_rect(), ui.damage_region()),
        Damage::Partial(hot_rect.into()),
    );
}

/// Pin: a child whose layout rect overflows a clipped panel (e.g. a
/// scrolled-offscreen row inside a `Scroll` viewport) contributes
/// only its *visible* portion to the damage region. The fix replaces
/// `Cascade.screen_rect` with `Cascade.visible_rect` (raw screen rect
/// intersected with the active ancestor clip) as the damage rect
/// source — without it, panning a long list under a small viewport
/// would inflate the damage union to the full content extent and
/// trip `FULL_REPAINT_THRESHOLD` every frame.
#[test]
fn child_overflowing_clipped_parent_damage_clipped_to_viewport() {
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let viewport_size = 100.0;
    let child_size = 200.0;
    let build = |fill: Color, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            // Root hstack so the inner zstack honors its `Fixed` size
            // (root nodes get stretched to the surface anchor by the
            // layout engine, which would defeat the clip).
            Panel::hstack()
                .id(WidgetId::from_hash("clip-host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("clip-root"))
                        .size((Sizing::Fixed(viewport_size), Sizing::Fixed(viewport_size)))
                        .clip_rect()
                        .show(ui, |ui| {
                            *child = Some(
                                Frame::new()
                                    .id(WidgetId::from_hash("overflow"))
                                    .size(child_size)
                                    .background(Background {
                                        fill: fill.into(),
                                        ..Default::default()
                                    })
                                    .show(ui)
                                    .node(ui),
                            );
                        });
                });
        });
    };

    build(BLUE, &mut ui, &mut child_node);
    // Authoring change on the child only — fill flips. The child's
    // layout rect is `child_size × child_size` (way past the clip),
    // but the damage rect must stay inside the parent's clip.
    build(RED, &mut ui, &mut child_node);

    let region = ui.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.size.w <= viewport_size + 0.5 && damage_rect.size.h <= viewport_size + 0.5,
        "damage rect must be clipped to the {viewport_size}px viewport; got {damage_rect:?}",
    );
}

/// Pin: a node that paints a drop shadow contributes its **inflated**
/// paint bounds (rect + `|offset| + 3σ + spread` on each side) to the
/// damage region, not just the arranged rect. Both routes — direct
/// `Shape::Shadow` push and `Background::shadow` chrome — must reach
/// the same `paint_rect` so a tab swap clears the full halo, not just
/// the layout rect.
#[test]
fn drop_shadow_overhang_contributes_to_damage_on_remove() {
    use crate::Shadow;
    use crate::primitives::corners::Corners;
    use crate::shape::Shape;

    let frame_size = 50.0;
    let expected_overhang = 3.0 * 8.0 + 2.0;

    type Build = fn(&mut Ui);
    let cases: &[(&str, Build)] = &[
        ("shape", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.add_shape(Shape::Shadow {
                        local_rect: None,
                        corners: Corners::all(0.0),
                        shadow: Shadow {
                            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                            offset: Vec2::ZERO,
                            blur: 8.0,
                            spread: 2.0,
                            inset: false,
                        },
                    });
                });
        }),
        ("chrome", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    shadow: Shadow {
                        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                        offset: Vec2::ZERO,
                        blur: 8.0,
                        spread: 2.0,
                        inset: false,
                    },
                    ..Default::default()
                })
                .show(ui, |_| {});
        }),
    ];
    for (label, build) in cases {
        let mut ui = Ui::for_test();
        frame(&mut ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, build);
        });
        let prev_rect = ui.damage_engine.prev[&WidgetId::from_hash("card")].rect;
        assert!(
            prev_rect.size.w >= frame_size + 2.0 * expected_overhang - 0.5
                && prev_rect.size.h >= frame_size + 2.0 * expected_overhang - 0.5,
            "[{label}] snapshot rect must include drop-shadow overhang; got {prev_rect:?}",
        );

        frame(&mut ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        });
        let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
        // `damage_engine.prev` stores the raw paint_rect including
        // the shadow halo, which extends off the top-left of the
        // 200×200 surface for a 50×50 frame at origin. The damage
        // region, however, clips each rect to the surface in
        // `collapse_from` (off-surface pixels can never be painted
        // and would bias the Full-repaint threshold), so the emitted
        // damage is the visible portion of `prev_rect`.
        assert_eq!(
            rects,
            vec![prev_rect.intersect(TEST_SURFACE)],
            "[{label}] damage region",
        );
    }
}

/// Pin: a drop-shadow whose halo extends past a clipping ancestor
/// contributes only the **clipped** halo to damage. The shadow's
/// overhang is folded into `paint_rect` in owner-local space before
/// the ancestor clip is applied, so a `ClipMode::Clip` parent caps
/// the contribution at the parent's bounds — otherwise the halo
/// pretends to paint pixels the GPU's scissor will discard.
#[test]
fn shadow_overhang_inside_clipped_parent_is_clamped() {
    use crate::Shadow;
    use crate::primitives::corners::Corners;
    use crate::shape::Shape;

    let viewport = 60.0;
    let card = 40.0;
    let blur = 8.0;

    let mut ui = Ui::for_test();
    let build = |fill: Color, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("viewport"))
                        .size((Sizing::Fixed(viewport), Sizing::Fixed(viewport)))
                        .clip_rect()
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("card"))
                                .size((Sizing::Fixed(card), Sizing::Fixed(card)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui, |ui| {
                                    ui.add_shape(Shape::Shadow {
                                        local_rect: None,
                                        corners: Corners::all(0.0),
                                        shadow: Shadow {
                                            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                                            offset: Vec2::ZERO,
                                            blur,
                                            spread: 0.0,
                                            inset: false,
                                        },
                                    });
                                });
                        });
                });
        });
    };

    build(BLUE, &mut ui);
    build(RED, &mut ui);

    for r in ui.damage_region().iter_rects() {
        assert!(
            r.size.w <= viewport + 0.5 && r.size.h <= viewport + 0.5,
            "shadow halo damage must stay inside the {viewport}px clip; got {r:?}",
        );
    }
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
        oversized.intersect(surface),
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
    let damage = Damage::new(surface, region);
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
    let damage = Damage::new(surface, region);
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
        region.is_empty(),
        "wholly-off-surface rect must produce an empty region (no Damage::None vs Partial drift)",
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
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
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
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
            });
    });

    assert!(
        !ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("off")),
        "Vacant + off-surface paint_rect must not seed a prev entry — \
         hashmap insert + raw_rects push are both wasted work for a \
         node that contributes nothing visible",
    );
    assert!(
        ui.damage_region().is_empty(),
        "no visible widgets means no damage rects on the second-frame \
         diff (first frame is Full and walks differently)",
    );
}

/// `NodeSnapshot.paint_span` covers one entry per Paint row on the
/// node — chrome at row 0 when present, then each direct shape — with
/// matching rect and canonical hash. Mirrors `Cascades::paint_arenas`.
#[test]
fn node_snapshot_decomposition_matches_cascade() {
    use crate::Shape;
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: Vec2::new(0.0, 0.0),
                    b: Vec2::new(10.0, 10.0),
                    width: 1.0,
                    brush: Color::rgb(1.0, 0.0, 0.0).into(),
                    cap: crate::shape::LineCap::Butt,
                    join: crate::shape::LineJoin::Miter,
                });
                ui.add_shape(Shape::Line {
                    a: Vec2::new(20.0, 20.0),
                    b: Vec2::new(30.0, 30.0),
                    width: 1.0,
                    brush: Color::rgb(0.0, 1.0, 0.0).into(),
                    cap: crate::shape::LineCap::Butt,
                    join: crate::shape::LineJoin::Miter,
                });
            });
    });

    let snap = ui.damage_engine.prev[&WidgetId::from_hash("multi")];
    let layer = Layer::Main;
    let node_idx = ui.layout.cascades.by_id[&WidgetId::from_hash("multi")]
        .node
        .idx();
    let node_span = ui.layout.cascades.layers[layer].paint_arena.node_spans[node_idx];
    let layer_paints = &ui.layout.cascades.layers[layer].paint_arena.rows;

    // Chrome lands at row 0 of the node's paint span when present.
    let chrome_paint = layer_paints[node_span.start as usize];
    assert!(
        chrome_paint.screen.area() > 0.0,
        "chrome panel must have non-zero chrome rect",
    );

    // Snapshot mirrors the cascade arena slice.
    let snap_paints = &ui.damage_engine.arena.snaps[snap.paint_span.range()];
    assert_eq!(snap_paints.len(), 3, "chrome + 2 direct shapes ⇒ 3 rows");
    let cascade_paints = &layer_paints[node_span.range()];
    for (ord, p) in snap_paints.iter().enumerate() {
        assert_eq!(
            p.screen, cascade_paints[ord].screen,
            "paint #{ord} rect must match cascade column",
        );
        assert_eq!(
            p.hash, cascade_paints[ord].hash,
            "paint #{ord} hash must match cascade column",
        );
    }

    // Vacant pushes one rect per paint row (chrome + each shape).
    assert_eq!(
        ui.damage_engine.raw_rects.len(),
        3,
        "Vacant insert pushes one rect per paint row",
    );
    assert_eq!(ui.damage_engine.raw_rects[0], snap_paints[0].screen);
    assert_eq!(ui.damage_engine.raw_rects[1], snap_paints[1].screen);
    assert_eq!(ui.damage_engine.raw_rects[2], snap_paints[2].screen);
}

/// Slice 4 headline: a multi-shape owner whose shapes are spatially
/// disjoint pushes only the *changed* shape's rect pair on a frame
/// where one endpoint moved. Reproduces the darkroom graph pattern
/// (canvas owns N bezier connections; drag one node, only the
/// connections actually touching it should enter damage). Pre-slice-4
/// the Occupied-changed arm pushed `prev_rect ∪ curr_rect = union of
/// all shapes`; slice 4 pushes only the moved shape's prev + curr.
#[test]
fn per_shape_damage_only_pushes_changed_shapes() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    // Two stable shapes (drawn at fixed coords) + one shape whose
    // endpoint shifts between frames. Frame N records all three;
    // frame N+1 shifts only the third — the diff must push exactly
    // that shape's pair of rects.
    let mut ui = Ui::for_test();
    let build = |moving_y: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::Fixed(180.0), Sizing::Fixed(180.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(60.0, 0.0, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 1.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                // The moving shape, far from the other two, so its
                // bbox doesn't merge with theirs in the damage region.
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, moving_y, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    };

    // Frame 1 (cold) and frame 2 (steady — no diff).
    frame(&mut ui, |ui| build(120.0, ui));
    frame(&mut ui, |ui| build(120.0, ui));
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "steady frame must produce no diff"
    );

    // Frame 3 nudges shape 2's y endpoint. Slice 4 contract: only
    // shape 2's prev rect (at y=120) and curr rect (at y=140) enter
    // the damage region. Chrome (canvas background) is unchanged in
    // geometry AND authoring → no chrome push. Shapes 0 and 1 are
    // bit-identical → no push.
    let prev_snap = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    // paint_snaps row 0 is chrome; shapes follow at offset 1.
    let prev_shape2_rect = ui.damage_engine.arena.snaps[prev_snap.paint_span.range()][1 + 2].screen;
    frame(&mut ui, |ui| build(140.0, ui));

    let canvas_snap = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let curr_shape2_rect =
        ui.damage_engine.arena.snaps[canvas_snap.paint_span.range()][1 + 2].screen;

    // The damage region must intersect both old and new positions of
    // shape 2 (so the pixels-at-old-position get cleared and
    // pixels-at-new-position get painted). It must NOT intersect the
    // disjoint regions occupied by shapes 0 and 1 — those didn't move.
    let region = ui.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));
    assert!(
        intersects(prev_shape2_rect),
        "old position of moved shape must be in damage region; \
         prev_rect = {prev_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        intersects(curr_shape2_rect),
        "new position of moved shape must be in damage region; \
         curr_rect = {curr_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Sentinel: a rect on the chrome's top edge between shapes 0/1
    // (y < 120) must NOT be in the region — chrome didn't change,
    // shapes 0/1 are unchanged, only the moving shape's y-band gets
    // damaged. Pre-slice-4 the whole `paint_rect` union (covering
    // the entire 180×180 canvas) would have hit. This is the
    // tight-damage win.
    let stale_chrome_band = Rect::new(40.0, 40.0, 20.0, 20.0); // inside chrome, away from moved shape
    assert!(
        !intersects(stale_chrome_band),
        "unchanged chrome interior must not enter damage; \
         stale_band = {stale_chrome_band:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Chrome authoring change (hover fill flip, no rect change) must
/// push the chrome rect even though the geometric rect is identical.
/// Chrome is row 0 of the node's paint span and carries its own
/// authoring hash via `Paint.hash`; without that, a hover-color flip
/// would fall through the rect-only guard and emit no damage at all.
#[test]
fn chrome_authoring_change_pushes_chrome_paint_row() {
    let mut ui = Ui::for_test();
    let build = |fill: Color, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("c"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: fill.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut ui, |ui| build(BLUE, ui));
    frame(&mut ui, |ui| build(BLUE, ui)); // settle
    let snap = ui.damage_engine.prev[&WidgetId::from_hash("c")];
    let snap_rect = ui.damage_engine.arena.snaps[snap.paint_span.start as usize].screen;

    frame(&mut ui, |ui| build(RED, ui));
    let region = ui.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    assert!(
        rects.iter().any(|r| r.intersects(snap_rect)),
        "chrome authoring change must push chrome paint row even when \
         rect geometry is unchanged; region = {rects:?}",
    );
}

/// Ordinal shift: when the user removes a shape from the middle of a
/// widget's authoring (e.g., deletes a connection in the middle of a
/// connection list), the per-shape diff sees the trailing ordinals as
/// "different" because they now align with a *different* prev shape.
/// The contract: damage stays correct (the removed shape's pixels +
/// the shifted shapes' old+new positions all enter the region), and
/// the snapshot tail is trimmed via the `drain(ord..)` branch in the
/// Occupied-changed arm.
///
/// This is the degraded-coarsening behaviour mentioned in the design
/// doc — frame stays correct, one frame of over-paint, settles next.
#[test]
fn shape_removed_from_middle_evicts_trailing_ordinals() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;

    let mut ui = Ui::for_test();
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::Fixed(180.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 20.0, 20.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                if include_middle {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(60.0, 0.0, 20.0, 20.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        stroke: Stroke::ZERO,
                    });
                }
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(120.0, 0.0, 20.0, 20.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    };

    frame(&mut ui, |ui| build(true, ui));
    frame(&mut ui, |ui| build(true, ui)); // settle

    // Snapshot the prev rects for shapes 0/1/2 so we can verify the
    // post-delete damage region.
    let prev = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    // Chromeless canvas ⇒ paint_snaps maps 1:1 to direct shapes.
    let prev_shapes = &ui.damage_engine.arena.snaps[prev.paint_span.range()];
    assert_eq!(prev_shapes.len(), 3);
    let prev_middle_rect = prev_shapes[1].screen;
    let prev_blue_rect = prev_shapes[2].screen;

    // Delete the middle shape. Now ord 0 still maps to red, but
    // ord 1 — previously green — is now blue. The diff sees:
    //   ord 0: red == red          → no push.
    //   ord 1: green (prev) vs blue (curr) → push both.
    //   ord 2: blue (prev) vs none → push prev (trailing tail).
    // Net: union of all three "moved" rects in damage.
    frame(&mut ui, |ui| build(false, ui));

    let post = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(
        post.paint_span.len, 2,
        "snapshot tail must be trimmed to the new paint count",
    );

    let region = ui.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    let intersects = |r: Rect| rects.iter().any(|d| d.intersects(r));

    // The deleted shape's pixels must be in damage (cleared this frame).
    assert!(
        intersects(prev_middle_rect),
        "deleted shape's prev rect must enter damage; \
         prev_middle = {prev_middle_rect:?}, region = {rects:?}",
    );
    // The blue shape's old position must be in damage (mismatch push).
    assert!(
        intersects(prev_blue_rect),
        "shifted shape's prev rect must enter damage; \
         prev_blue = {prev_blue_rect:?}, region = {rects:?}",
    );
    // ord 0 (red) stayed put — its rect doesn't have to be in damage.
    // We don't assert absence (region merging may absorb it); we only
    // pin that the *deleted* and *shifted* rects are present.
}

/// Painting-only invariant: every `DamageEngine.prev` entry covers
/// at least one Paint row. A chrome-only owner used to land in `prev`
/// with `shape_span.len == 0` (chrome was tracked in a separate
/// column); under the unified `paint_arena`, chrome is row 0 of the
/// node's span, so the same owner now has `paint_span.len == 1`.
/// `compact_paint_snaps` asserts on zero-len entries — this test
/// pins the producer side of that contract.
#[test]
fn chrome_only_owner_has_nonzero_paint_span() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("chrome_only"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut ui, build);
    frame(&mut ui, build); // settle prev

    let wid = WidgetId::from_hash("chrome_only");
    let snap = ui.damage_engine.prev[&wid];
    assert_eq!(
        snap.paint_span.len, 1,
        "chrome-only owner must contribute exactly one Paint row (chrome)",
    );

    // Every entry in `prev` covers at least one row.
    for (k, s) in &ui.damage_engine.prev {
        assert!(
            s.paint_span.len > 0,
            "prev entry {k:?} has zero-len paint_span, violating painting-only invariant",
        );
    }

    // Compaction must accept the live state without tripping the
    // invariant assert.
    ui.damage_engine.compact_paint_snaps(&ui.forest);
}

/// Pin: changing the *content* of a `Shape::Text` with
/// `local_origin: Some(_)` damages the shaped-text bbox, not just the
/// origin point.
///
/// Before the fix, `paint_bbox_local` for `Text { local_origin: Some(_) }`
/// returned `{ min: origin, size: ZERO }` — a degenerate point, because
/// the glyph extent isn't known to the record. Cascade dutifully stored
/// that point in `Cascades::shape_rects[idx]`; the diff's
/// `diff_changed_shape_leg` then pushed two zero-size rects when text
/// changed → effectively no damage from the text shape. The
/// user-visible symptom: type a character in a `TextEdit`, and only the
/// caret-sized strip got repainted while the rest of the text went
/// stale.
///
/// Post-fix, cascade looks up the shaped extent from
/// `LayerLayout::text_shapes` (already computed by the measure pass)
/// and stores the tight `(origin, measured)` rect. The diff pushes
/// prev + curr extents, so the damage region covers the union of both
/// strings' bboxes.
#[test]
fn text_content_change_damages_shaped_extent_not_just_origin() {
    use crate::forest::element::{Element, LayoutMode, Salt};
    use crate::primitives::size::Size;
    use crate::shape::{Shape, TextWrap};
    use crate::text::FontFamily;

    let mut ui = Ui::for_test();
    // Mono fallback geometry: glyph width = font_size_px * 0.5, line
    // height = font_size_px. With font_size_px = 14, "abc" measures
    // 21×14 and "abcdef" measures 42×14.
    const FONT: f32 = 14.0;
    const ORIGIN: Vec2 = Vec2::new(10.0, 10.0);
    let leaf_id = WidgetId::from_hash("text-host");
    let build = |text: &'static str, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .show(ui, |ui| {
                let mut element = Element::new(LayoutMode::Leaf);
                element.salt = Salt::Verbatim(leaf_id);
                ui.node(leaf_id, element, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: Some(ORIGIN),
                        text: text.into(),
                        brush: Color::WHITE.into(),
                        font_size_px: FONT,
                        line_height_px: FONT,
                        wrap: TextWrap::Single,
                        align: Default::default(),
                        family: FontFamily::Sans,
                    });
                });
            });
    };

    frame(&mut ui, |ui| build("abc", ui));
    frame(&mut ui, |ui| build("abc", ui));
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "steady frame must produce no diff"
    );

    // Cache prev shaped rect (size of "abc") off the previous snapshot
    // so the assertion below can reason from the actual measured
    // values rather than hand-recomputing mono geometry.
    let prev_snap = ui.damage_engine.prev[&leaf_id];
    let prev_text_rect = ui.damage_engine.arena.snaps[prev_snap.paint_span.range()][0].screen;
    let prev_size_short: Size = Size::new(FONT * 0.5 * 3.0, FONT);
    assert!(
        (prev_text_rect.size.w - prev_size_short.w).abs() < 0.5
            && (prev_text_rect.size.h - prev_size_short.h).abs() < 0.5,
        "prev text rect should have shaped size ≈ {prev_size_short:?}, got {prev_text_rect:?}",
    );

    frame(&mut ui, |ui| build("abcdef", ui));

    let curr_snap = ui.damage_engine.prev[&leaf_id];
    let curr_text_rect = ui.damage_engine.arena.snaps[curr_snap.paint_span.range()][0].screen;
    let curr_size_long: Size = Size::new(FONT * 0.5 * 6.0, FONT);
    assert!(
        (curr_text_rect.size.w - curr_size_long.w).abs() < 0.5
            && (curr_text_rect.size.h - curr_size_long.h).abs() < 0.5,
        "curr text rect should have shaped size ≈ {curr_size_long:?}, got {curr_text_rect:?}",
    );

    let region = ui.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));

    // Probe deep inside the new "abcdef" rect but past where the old
    // "abc" rect ended (x = origin.x + 30 ≈ middle of "abcdef", past
    // the 21-px width of "abc"). Pre-fix this point is *not* in damage
    // (per-shape rect was a zero-size point at origin); post-fix it is
    // (curr rect spans origin..origin+42px).
    let inside_new_only = Rect::new(ORIGIN.x + 30.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_new_only),
        "probe inside new text but past old text must be in damage; \
         probe = {inside_new_only:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Also assert prev's middle gets damaged (so the old glyph
    // pixels actually clear).
    let inside_old = Rect::new(ORIGIN.x + 10.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_old),
        "probe inside old text must be in damage; \
         probe = {inside_old:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin: a direct shape on a clipped node has its per-shape rect (the
/// column the damage diff reads from) clipped to the node's own clip
/// mask — not just the ancestor clip.
///
/// Before the fix, `compute_paint_rect` clipped each shape's screen
/// rect to `parent_clip` only. A `Shape::Text` with `local_origin`
/// expressing a scroll offset reported its **full** shaped extent as
/// the per-shape rect (cosmic-text's measured `Size` for the whole
/// buffer). For a multi-line `TextEdit` taller than its visible rect,
/// scrolling produced damage rects spanning the entire text — way
/// past the editor's own `ClipMode::Rect`. The encoder's GPU scissor
/// clips the actual pixels, so the user *saw* tight repaints, but
/// the damage region driving the scissor pass was over-large,
/// inflating the partial-redraw quad to the unclipped text bbox.
///
/// This test fakes the scenario with a `RoundedRect` shape extending
/// past the host's clip on the right edge; pre-fix the per-shape rect
/// captures the full 400-px-wide shape, post-fix it's clipped to the
/// host's deflated mask.
#[test]
fn direct_shape_on_clipped_node_clips_to_own_mask() {
    use crate::Shape;
    use crate::forest::Layer;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    // Host panel: 80×40, padding 4 each side via background. The
    // direct shape extends to x=400 (well past 80). After the cascade
    // walk, `shape_rects[idx]` must be clipped to the host's deflated
    // mask, not span the full 400 px.
    let mut ui = Ui::for_test();
    let host_id = WidgetId::from_hash("clip-host");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(host_id)
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .clip_rect()
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(0.0, 0.0, 400.0, 20.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    };
    frame(&mut ui, build);
    frame(&mut ui, build);

    // Locate the host node by widget id and read its first shape's
    // cascaded screen rect. Pre-fix the rect spans the full 400 px;
    // post-fix it's clamped to (host_width − padding-fold).
    let cascades = &ui.layout.cascades;
    let host_ep = *cascades.by_id.get(&host_id).expect("host node recorded");
    let host_entry_idx = (cascades.layers[host_ep.layer].entries_base + host_ep.node.0) as usize;
    let host_rect = cascades.entries.rect()[host_entry_idx];
    let tree = ui.forest.tree(Layer::Main);
    let shape_span = tree.records.shape_span()[host_ep.node.idx()];
    assert!(shape_span.len >= 1, "host should have at least one shape");
    // First Paint row for the host node — chrome row 0 if present,
    // otherwise the first shape. The host here has no chrome, so
    // `node_spans[host_node].start` indexes the first shape's `Paint`.
    let paint_arena = &cascades.layers[Layer::Main].paint_arena;
    let paints_start = paint_arena.node_spans[host_ep.node.idx()].start as usize;
    let shape_rect = paint_arena.rows[paints_start].screen;
    assert!(
        shape_rect.size.w <= host_rect.size.w + 0.5,
        "direct shape rect must be clipped to the host's own mask; \
         host_rect = {host_rect:?}, shape_rect = {shape_rect:?}",
    );
}
