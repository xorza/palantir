use super::{Damage, DamagePaint};
use crate::Ui;
use crate::input::InputEvent;
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::{color::Color, rect::Rect, transform::TranslateScale};
use crate::support::testing::{begin, end_frame_acked};
use crate::tree::Layer;
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::popup::Popup;
use crate::widgets::theme::Background;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::{UVec2, Vec2};

#[allow(dead_code)]
const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 200.0);
const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    scale_factor: 1.0,
    pixel_snap: true,
};

/// Drive one frame with the given builder, then simulate a
/// successful `WgpuBackend::submit` so the next frame's
/// auto-rewind doesn't fire. Closure receives `ui` after
/// `begin_frame`.
fn frame(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
    ui.begin_frame(DISPLAY);
    f(ui);
    end_frame_acked(ui);
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
                fill: color,
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
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    let painting = ui.forest.tree(Layer::Main).rollups.paints.count_ones(..);
    assert_eq!(ui.damage.dirty.len(), painting);
    assert!(!ui.damage.region.is_empty());
}

/// Pin: re-recording identical authoring → zero dirty nodes,
/// damage rect is `None`. The steady-state ideal: idle UI does
/// nothing.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut ui, build);
    frame(&mut ui, build);

    assert!(ui.damage.dirty.is_empty());
    assert!(ui.damage.region.is_empty());
}

/// Pin: a widget that loses its background between frames flips from
/// painting to non-painting. The diff must (a) contribute its prev
/// rect to damage so the prior pixels get cleared, (b) drop the entry
/// from `prev` so the next frame sees it as truly absent, and (c)
/// contribute no curr rect.
#[test]
fn paints_to_non_paints_transition_evicts_and_clears() {
    let mut ui = Ui::new();
    let with_bg = |ui: &mut Ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("a")
                .size(50.0)
                .background(Background {
                    fill: BLUE,
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
    assert!(ui.damage.prev.contains_key(&id));

    frame(&mut ui, no_bg);
    assert!(
        !ui.damage.prev.contains_key(&id),
        "paints→non-paints transition must evict the prev entry"
    );
    let rects: Vec<_> = ui.damage.region.iter_rects().collect();
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
    let mut ui = Ui::new();
    let anchor = Rect::new(40.0, 40.0, 60.0, 30.0);
    // Frame 1: popup open. Eater (full-surface) + body (small).
    ui.begin_frame(DISPLAY);
    Popup::anchored_to(anchor)
        .id_salt("p")
        .background(Background {
            fill: BLUE,
            ..Default::default()
        })
        .show(&mut ui, |ui| {
            Frame::new()
                .id_salt("body-leaf")
                .size(60.0)
                .background(Background {
                    fill: RED,
                    ..Default::default()
                })
                .show(ui);
        });
    end_frame_acked(&mut ui);

    // Frame 2: popup gone. Body + eater both removed. Without the
    // paints-gate, the eater's full-surface prev rect would dominate
    // the region.
    ui.begin_frame(DISPLAY);
    Frame::new().id_salt("placeholder").size(10.0).show(&mut ui);
    let out = ui.end_frame();
    let DamagePaint::Partial(region) = out.damage else {
        panic!(
            "popup dismissal escalated to {:?}; eater contributed full-surface \
             rect despite painting nothing",
            out.damage
        );
    };
    let surface_area = DISPLAY.logical_rect().area();
    assert!(
        region.total_area() / surface_area < 0.5,
        "damage region covers {:.1}% of surface — eater leaked into damage",
        100.0 * region.total_area() / surface_area
    );
}

/// Regression: a `Skip` frame that the host bypasses (no
/// `backend.submit` → no `mark_submitted`) must not force the next
/// frame to `Full`. `end_frame` marks `Skip` as submitted directly so
/// the next `begin_frame`'s auto-rewind doesn't kick in.
#[test]
fn skip_frame_does_not_force_next_to_full() {
    let mut ui = Ui::new();
    ui.begin_frame(DISPLAY);
    one_frame(&mut ui, BLUE);
    let first = ui.end_frame();
    assert_eq!(first.damage, DamagePaint::Full);
    first.frame_state.mark_submitted();

    // Identical content → Skip. Drop the FrameOutput WITHOUT calling
    // mark_submitted (simulates the host taking the
    // `can_skip_rendering()` early-return path).
    ui.begin_frame(DISPLAY);
    one_frame(&mut ui, BLUE);
    let skip = ui.end_frame();
    assert_eq!(skip.damage, DamagePaint::Skip);
    drop(skip);

    // Next frame: still no diff. Pre-fix this returned Full because
    // the previous Skip never reached `mark_submitted`, so begin_frame
    // saw Pending and rewound prev.
    ui.begin_frame(DISPLAY);
    one_frame(&mut ui, BLUE);
    let next = ui.end_frame();
    assert_eq!(
        next.damage,
        DamagePaint::Skip,
        "Skip frames must not poison the next frame into Full"
    );
}

/// Pin: an authoring change on one leaf marks just that leaf
/// dirty; the parent (whose own fields didn't change and whose
/// rect is identical) stays clean.
#[test]
fn fill_change_marks_only_the_changed_leaf() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });

    assert_eq!(ui.damage.dirty.len(), 1);
    let dirty_id = ui.damage.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.index()],
        WidgetId::from_hash("a")
    );
    // Damage rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage.region.iter_rects().next(),
        Some(ui.layout.result[Layer::Main].rect[dirty_id.index()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::new();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("a")
                .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui);
            Frame::new()
                .id_salt("b")
                .size((Sizing::Fixed(30.0), Sizing::Fixed(20.0)))
                .background(Background {
                    fill: Color::rgb(0.5, 0.5, 0.5),
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
        .damage
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
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Button::new().id_salt("gone").label("X").show(ui);
        });
    });
    let prev_button_rect = ui.damage.prev[&WidgetId::from_hash("gone")].rect;

    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |_| {});
    });

    // Button is gone; root Panel is non-painting (no chrome) so it
    // never entered prev. Only contribution is the Button's prev
    // rect, surfaced via the `removed` list.
    let rects: Vec<Rect> = ui.damage.region.iter_rects().collect();
    assert_eq!(rects, vec![prev_button_rect]);
}

/// Pin: an added widget that wasn't in last frame contributes
/// its current rect to damage and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack().id_salt("root").show(ui, |ui| {
            Frame::new()
                .id_salt("new")
                .size(50.0)
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui);
        });
    });

    let dirty_ids: Vec<WidgetId> = ui
        .damage
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.index()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(!ui.damage.region.is_empty());
}

// --- Ui::damage_filter ---------------------------------------------------

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns `Partial(rect)`, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_partial_when_small() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });
    let r = ui
        .damage
        .region
        .iter_rects()
        .next()
        .expect("single-leaf change → some damage");
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(r.into())
    );
}

/// Pin: `filter()` returns `Skip` when nothing changed at all (no
/// damage rect). The steady-state idle case must opt out of the GPU
/// pass entirely so the backbuffer's existing pixels carry forward
/// untouched.
#[test]
fn damage_filter_returns_skip_when_nothing_dirty() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut ui, build);
    frame(&mut ui, build);
    assert!(ui.damage.dirty.is_empty());
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Skip
    );
}

// --- transforms ---------------------------------------------------------
// Damage rects must be in *screen space*. When an ancestor has a
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
    let mut ui = Ui::new();
    let mut child_node = None;
    let build = |fill: Color, ui: &mut Ui, child: &mut Option<NodeId>| {
        begin(ui, UVec2::new(400, 400));
        Panel::hstack()
            .id_salt("outer")
            .transform(TranslateScale::from_translation(translate))
            .show(ui, |ui| {
                *child = Some(
                    Frame::new()
                        .id_salt("c")
                        .size(40.0)
                        .background(Background {
                            fill,
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
        end_frame_acked(ui);
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut ui, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut ui, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = ui.layout.result[Layer::Main].rect[child_node.unwrap().index()];
    let expected_screen_rect = Rect {
        min: child_layout_rect.min + translate,
        size: child_layout_rect.size,
    };
    let damage_rect = ui
        .damage
        .region
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
    let mut ui = Ui::new();
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        begin(ui, UVec2::new(400, 400));
        Panel::hstack()
            .id_salt("outer")
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                *child = Some(
                    Frame::new()
                        .id_salt("c")
                        .size(40.0)
                        .background(Background {
                            fill: Color::rgb(0.2, 0.4, 0.8),
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
        end_frame_acked(ui);
    };

    build(0.0, &mut ui, &mut child_node);
    build(50.0, &mut ui, &mut child_node);

    // Child layout rect didn't change. Parent's transform shifted by
    // (50, 0). Prev screen rect = (0,0,40,40); curr = (50,0,40,40);
    // gap of 10 px between them. bbox = 90×40 = 3600, sum = 3200,
    // ratio 1.125 ≤ 1.3 — the proximity rule merges into one bbox.
    // (A larger animation distance would keep the two rects split;
    // pinned by `transform_animation_keeps_far_positions_split`.)
    let rects: Vec<Rect> = ui.damage.region.iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(50.0, 0.0, 40.0, 40.0);
    assert_eq!(
        rects,
        vec![prev.union(curr)],
        "transform animation under 1.3× ratio → one merged bbox",
    );
    // Only the child is dirty: its authoring is unchanged but its
    // screen rect moved (rect comparison catches this). The parent
    // panel's own paint is unaffected by its own transform — the
    // transform only composes into descendants — so the parent's
    // hash and screen rect are both stable, leaving it clean.
    let dirty_widget_ids: Vec<WidgetId> = ui
        .damage
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.index()])
        .collect();
    assert_eq!(dirty_widget_ids, vec![WidgetId::from_hash("c")]);
}

/// Sister case to the test above: a *large* animation distance keeps
/// the two screen rects split. Pinning both ends of the merge rule
/// means a tighter ratio constant won't silently flip behaviour
/// without breaking a test.
#[test]
fn transform_animation_keeps_far_positions_split() {
    let mut ui = Ui::new();
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        begin(ui, UVec2::new(400, 400));
        Panel::hstack()
            .id_salt("outer")
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                *child = Some(
                    Frame::new()
                        .id_salt("c")
                        .size(40.0)
                        .background(Background {
                            fill: Color::rgb(0.2, 0.4, 0.8),
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
        end_frame_acked(ui);
    };

    build(0.0, &mut ui, &mut child_node);
    build(200.0, &mut ui, &mut child_node);

    // prev (0,0,40,40) area 1600; curr (200,0,40,40) area 1600.
    // bbox 240×40 = 9600. ratio 9600/3200 = 3.0 ≫ 1.3 — split.
    let rects: Vec<Rect> = ui.damage.region.iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(200.0, 0.0, 40.0, 40.0);
    assert_eq!(rects.len(), 2, "far transform animation → two rects");
    assert!(rects.contains(&prev) && rects.contains(&curr), "{rects:?}");
}

// --- Damage::filter heuristic ---------------------------------------------

const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

#[test]
fn no_damage_means_skip() {
    let d = Damage::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what coverage above
    // [`FULL_REPAINT_THRESHOLD`] produces.
    assert_eq!(d.filter(TEST_SURFACE), DamagePaint::Skip);
}

/// Heuristic: total coverage = `sum(rect.area()) / surface_area`;
/// strictly above `FULL_REPAINT_THRESHOLD` (0.7) ⇒ Full, otherwise
/// Partial. The check is `>`, not `>=`, so coverage exactly at the
/// threshold stays Partial. A zero-area surface forces Full
/// (divide-by-zero guard). The `total_area` sum is over per-rect
/// areas, so two non-overlapping damage rects collectively crossing
/// the threshold escalate to Full even though neither rect alone
/// does.
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
    // Two distant rects on the 100×100 surface — bbox(A,B) = 10000
    // exceeds A.area() + B.area() in both pairs, so the LVGL merge
    // rule rejects and the region keeps both.
    const PAIR_BELOW: [Rect; 2] = [
        // total_area = 7000 / 10000 = 0.70 → stays Partial (`>` is strict).
        Rect::new(0.0, 0.0, 35.0, 100.0),
        Rect::new(65.0, 0.0, 35.0, 100.0),
    ];
    const PAIR_ABOVE: [Rect; 2] = [
        // total_area = 7200 / 10000 = 0.72 → escalates Full.
        Rect::new(0.0, 0.0, 36.0, 100.0),
        Rect::new(64.0, 0.0, 36.0, 100.0),
    ];
    let cases: &[(&str, &[Rect], Rect, DamagePaint)] = &[
        (
            "small_1pct",
            &[Rect::new(0.0, 0.0, 10.0, 10.0)],
            TEST_SURFACE,
            DamagePaint::Partial(Rect::new(0.0, 0.0, 10.0, 10.0).into()),
        ),
        (
            "large_81pct_above_threshold",
            &[Rect::new(0.0, 0.0, 90.0, 90.0)],
            TEST_SURFACE,
            DamagePaint::Full,
        ),
        (
            "below_threshold_64pct_stays_partial",
            &[Rect::new(0.0, 0.0, 80.0, 80.0)],
            TEST_SURFACE,
            DamagePaint::Partial(Rect::new(0.0, 0.0, 80.0, 80.0).into()),
        ),
        (
            "exact_70pct_stays_partial",
            &[Rect::new(0.0, 0.0, 70.0, 100.0)],
            TEST_SURFACE,
            DamagePaint::Partial(Rect::new(0.0, 0.0, 70.0, 100.0).into()),
        ),
        (
            "two_rect_sum_at_threshold_stays_partial",
            &PAIR_BELOW,
            TEST_SURFACE,
            DamagePaint::Partial(region(&PAIR_BELOW)),
        ),
        (
            "two_rect_sum_above_threshold_escalates_full",
            &PAIR_ABOVE,
            TEST_SURFACE,
            DamagePaint::Full,
        ),
        (
            "zero_area_surface",
            &[Rect::new(0.0, 0.0, 1.0, 1.0)],
            Rect::ZERO,
            DamagePaint::Full,
        ),
    ];
    for (label, rects, surface, want) in cases {
        let d = Damage {
            region: region(rects),
            ..Damage::default()
        };
        assert_eq!(d.filter(*surface), *want, "case: {label}");
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
        let mut ui = Ui::new();
        let build = |ui: &mut Ui| {
            one_frame(ui, BLUE);
        };

        // Steady-state: Full first frame, then Skip on identical re-record.
        ui.begin_frame(DISPLAY);
        build(&mut ui);
        let f1 = ui.end_frame();
        assert_eq!(f1.damage, DamagePaint::Full, "case: {label} f1");
        f1.frame_state.mark_submitted();
        ui.begin_frame(DISPLAY);
        build(&mut ui);
        let f2 = ui.end_frame();
        assert_eq!(f2.damage, DamagePaint::Skip, "case: {label} f2");
        f2.frame_state.mark_submitted();
        assert!(ui.damage.dirty.is_empty(), "case: {label} steady");

        // Mutate Display; identical authoring; must short-circuit to Full.
        ui.begin_frame(*mutated);
        build(&mut ui);
        let mutated_frame = ui.end_frame();
        assert_eq!(
            mutated_frame.damage,
            DamagePaint::Full,
            "case: {label} display change"
        );
        mutated_frame.frame_state.mark_submitted();
        assert!(
            !ui.damage.dirty.is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        // Stable surface at the new size, identical authoring → back to Skip.
        ui.begin_frame(*mutated);
        build(&mut ui);
        let settled = ui.end_frame().damage;
        assert_eq!(
            settled,
            DamagePaint::Skip,
            "case: {label} post-mutation steady"
        );
        assert!(
            ui.damage.dirty.is_empty(),
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
    let mut ui = Ui::new();
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
    let scene = |ui: &mut Ui| {
        Panel::hstack()
            .id_salt("root")
            .size((Sizing::Fixed(3050.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("big")
                    .size((3000.0, 60.0))
                    .background(Background {
                        fill: BLUE,
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id_salt("small")
                    .size((50.0, 60.0))
                    .background(Background {
                        fill: BLUE,
                        ..Default::default()
                    })
                    .show(ui);
            });
    };

    ui.begin_frame(big);
    scene(&mut ui);
    end_frame_acked(&mut ui);
    ui.begin_frame(big);
    scene(&mut ui);
    end_frame_acked(&mut ui);
    assert!(ui.damage.dirty.is_empty());

    // Inject: nudge widget "a"'s prev rect so the next diff sees a
    // small change. Tiny rect (3×50 = 150 area) inside a 2000×2000
    // surface (4M area) — ratio ≈ 0.004%, well below the 50%
    // threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap = ui.damage.prev.get_mut(&target_wid).expect("small in prev");
    snap.rect.min.x += 3.0;

    let smaller = Display {
        physical: UVec2::new(1999, 2000),
        ..big
    };
    ui.begin_frame(smaller);
    scene(&mut ui);
    let damage = ui.end_frame().damage;

    assert_eq!(
        damage,
        DamagePaint::Full,
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
    let mut ui = Ui::new();
    let build = |ui: &mut Ui, color: Color| {
        one_frame(ui, color);
    };

    // Warm up: two identical frames bring damage to steady state.
    ui.begin_frame(DISPLAY);
    build(&mut ui, BLUE);
    end_frame_acked(&mut ui);
    ui.begin_frame(DISPLAY);
    build(&mut ui, BLUE);
    let warm = ui.end_frame();
    assert_eq!(
        warm.damage,
        DamagePaint::Skip,
        "warm steady-state with no diff"
    );
    warm.frame_state.mark_submitted();
    assert!(ui.damage.dirty.is_empty());

    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    ui.begin_frame(DISPLAY);
    build(&mut ui, RED);
    let partial = ui.end_frame().damage;
    let DamagePaint::Partial(region) = partial else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {partial:?} — surface-change short-circuit fired incorrectly",
        );
    };
    // Damage rect = the 50×50 frame's rect. Well below 50% of 200×200.
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
    let mut ui = Ui::new();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        begin(ui, UVec2::new(400, 400));
        Panel::vstack().id_salt("root").show(ui, |ui| {
            *hot = Some(Button::new().id_salt("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::new().id_salt("cold").label("Quiet").show(ui).node);
        });
        end_frame_acked(ui);
    };

    // Pointer parked off-button. Settle for two frames so hit-test +
    // damage are at steady state (no diff).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage.dirty.is_empty(),
        "off-button pointer should reach a no-diff steady state"
    );

    let hot_rect = ui.layout.result[Layer::Main].rect[hot_node.unwrap().index()];
    let target = hot_rect.min + Vec2::new(5.0, 5.0);

    // Move pointer onto the hot button. The *next* end_frame computes
    // hover=true. The frame *after* that records the button as
    // hovered → its fill differs → it lands in the dirty set alone.
    // `on_input` recomputes hover against the existing hit_index
    // immediately, so the *next* recording sees `hovered=true` and
    // emits the hovered fill. Damage = button rect only.
    ui.on_input(InputEvent::PointerMoved(target));
    build(&mut ui, &mut hot_node, &mut cold_node);

    assert_eq!(
        ui.damage.dirty.len(),
        1,
        "only the hovered button should be dirty"
    );
    let dirty_id = ui.damage.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.region.iter_rects().next(), Some(hot_rect));
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(hot_rect.into()),
        "small per-button damage must not trip the full-repaint heuristic",
    );

    // Next frame at same cursor → no diff (settled).
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage.dirty.is_empty(),
        "settled hover should produce no further damage"
    );
}

/// Pin: leaving the button (un-hover) is symmetric — the only diff
/// is the button's fill flipping back, damage = button rect.
#[test]
fn button_unhover_damage_covers_only_the_button() {
    let mut ui = Ui::new();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        begin(ui, UVec2::new(400, 400));
        Panel::vstack().id_salt("root").show(ui, |ui| {
            *hot = Some(Button::new().id_salt("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::new().id_salt("cold").label("Quiet").show(ui).node);
        });
        end_frame_acked(ui);
    };

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.layout.result[Layer::Main].rect[hot_node.unwrap().index()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage.dirty.len(), 1);
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[ui.damage.dirty[0].index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.region.iter_rects().next(), Some(hot_rect));
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(hot_rect.into()),
    );
}
