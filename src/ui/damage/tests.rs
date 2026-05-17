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
    Panel::hstack().id_salt("root").show(ui, |ui| {
        Frame::new()
            .id_salt("a")
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
    let painting = ui.forest.tree(Layer::Main).rollups.paints.count_ones(..);
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
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Panel::hstack()
                .id_salt("painting_parent")
                .size((Sizing::Fixed(80.0), Sizing::Fixed(60.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("child_a")
                        .size(20.0)
                        .background(Background {
                            fill: RED.into(),
                            ..Default::default()
                        })
                        .show(ui);
                    Frame::new()
                        .id_salt("child_b")
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
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui);
        });
    };
    let no_bg = |ui: &mut Ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new().id_salt("a").size(50.0).show(ui);
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
            .id_salt("p")
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui, _popup| {
                Frame::new()
                    .id_salt("body-leaf")
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
        Frame::new().id_salt("placeholder").size(10.0).show(ui);
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
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("a")
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
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.index()],
        WidgetId::from_hash("a")
    );
    // DamageEngine rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage_region().iter_rects().next(),
        Some(ui.layout[Layer::Main].rect[dirty_id.index()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::for_test();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("a")
                .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8).into(),
                    ..Default::default()
                })
                .show(ui);
            Frame::new()
                .id_salt("b")
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
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.index()])
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
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Button::new().id_salt("gone").label("X").show(ui);
        });
    });
    let prev_button_rect = ui.damage_engine.prev[&WidgetId::from_hash("gone")].rect;

    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |_| {});
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
        Panel::hstack().id_salt("root").show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("new")
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
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.index()])
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
                .id_salt("outer")
                .transform(TranslateScale::from_translation(translate))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id_salt("c")
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
    let child_layout_rect = ui.layout[Layer::Main].rect[child_node.unwrap().index()];
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
                .id_salt("outer")
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id_salt("c")
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
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.index()])
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
                .id_salt("outer")
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id_salt("c")
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
            DamageRegion::collapse_from(&d.raw_rects, d.budget_px)
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
    // Root: Fixed-size HStack (3050×60) containing two Fixed children
    // — bigger than the 2000×2000 surface so children that overflow
    // surface drive damage union past the surface bounds. Root rect is
    // stable across surface changes (Fixed never reads `available`),
    // so any damage rect change must come from the descendant nudge,
    // not the root re-resolving. Frame "small" ends up at
    // (3000, 0, 50, 60).
    let mut scene = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("root")
            .size((Sizing::Fixed(3050.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("big")
                    .size((3000.0, 60.0))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("small")
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
            Panel::vstack().id_salt("root").show(ui, |ui| {
                *hot = Some(
                    Button::new()
                        .id_salt("hot")
                        .label("Hover me")
                        .show(ui)
                        .node(ui),
                );
                *cold = Some(
                    Button::new()
                        .id_salt("cold")
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

    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().index()];
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
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.index()],
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
            Panel::vstack().id_salt("root").show(ui, |ui| {
                *hot = Some(
                    Button::new()
                        .id_salt("hot")
                        .label("Hover me")
                        .show(ui)
                        .node(ui),
                );
                *cold = Some(
                    Button::new()
                        .id_salt("cold")
                        .label("Quiet")
                        .show(ui)
                        .node(ui),
                );
            });
        });
    };

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().index()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage_engine.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage_engine.dirty.len(), 1);
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[ui.damage_engine.dirty[0].index()],
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
            Panel::hstack().id_salt("clip-host").show(ui, |ui| {
                Panel::zstack()
                    .id_salt("clip-root")
                    .size((Sizing::Fixed(viewport_size), Sizing::Fixed(viewport_size)))
                    .clip_rect()
                    .show(ui, |ui| {
                        *child = Some(
                            Frame::new()
                                .id_salt("overflow")
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
                .id_salt("card")
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.add_shape(Shape::Shadow {
                        local_rect: None,
                        radius: Corners::all(0.0),
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
                .id_salt("card")
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
            Panel::hstack().id_salt("root").show(ui, build);
        });
        let prev_rect = ui.damage_engine.prev[&WidgetId::from_hash("card")].rect;
        assert!(
            prev_rect.size.w >= frame_size + 2.0 * expected_overhang - 0.5
                && prev_rect.size.h >= frame_size + 2.0 * expected_overhang - 0.5,
            "[{label}] snapshot rect must include drop-shadow overhang; got {prev_rect:?}",
        );

        frame(&mut ui, |ui| {
            Panel::hstack().id_salt("root").show(ui, |_| {});
        });
        let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
        assert_eq!(rects, vec![prev_rect], "[{label}] damage region");
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
            Panel::hstack().id_salt("host").show(ui, |ui| {
                Panel::zstack()
                    .id_salt("viewport")
                    .size((Sizing::Fixed(viewport), Sizing::Fixed(viewport)))
                    .clip_rect()
                    .show(ui, |ui| {
                        Panel::hstack()
                            .id_salt("card")
                            .size((Sizing::Fixed(card), Sizing::Fixed(card)))
                            .background(Background {
                                fill: fill.into(),
                                ..Default::default()
                            })
                            .show(ui, |ui| {
                                ui.add_shape(Shape::Shadow {
                                    local_rect: None,
                                    radius: Corners::all(0.0),
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
