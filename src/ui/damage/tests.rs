use super::{Damage, DamagePaint};
use crate::Ui;
use crate::input::InputEvent;
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::{color::Color, rect::Rect, transform::TranslateScale};
use crate::support::testing::begin;
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
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

/// Drive one frame with the given builder. Closure receives `ui`
/// after `begin_frame`.
fn frame(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
    ui.begin_frame(DISPLAY);
    f(ui);
    ui.end_frame();
}

/// The standard "root with one 50×50 frame" tree used by most damage
/// tests. Color flips between frames to drive minimal authoring
/// changes.
const BLUE: Color = Color::rgb(0.2, 0.4, 0.8);
const RED: Color = Color::rgb(0.9, 0.4, 0.8);

fn one_frame(ui: &mut Ui, color: Color) {
    Panel::hstack().with_id("root").show(ui, |ui| {
        Frame::new()
            .with_id("a")
            .size(50.0)
            .background(Background {
                fill: color,
                ..Default::default()
            })
            .show(ui);
    });
}

/// Pin: the very first frame has no `prev_frame` entries, so every
/// node is "added" → all nodes dirty, damage covers their union.
#[test]
fn first_frame_marks_every_node_dirty() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    assert_eq!(ui.damage.dirty.len(), ui.tree.records.len());
    assert!(ui.damage.rect.is_some());
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
    assert!(ui.damage.rect.is_none());
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
        ui.tree.records.widget_id()[dirty_id.index()],
        WidgetId::from_hash("a")
    );
    // Damage rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage.rect,
        Some(ui.pipeline.layout.result.rect[dirty_id.index()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::new();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack().with_id("root").show(ui, |ui| {
            Frame::new()
                .with_id("a")
                .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                .background(Background {
                    fill: Color::rgb(0.2, 0.4, 0.8),
                    ..Default::default()
                })
                .show(ui);
            Frame::new()
                .with_id("b")
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
        .map(|n| ui.tree.records.widget_id()[n.index()])
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
        Panel::hstack().with_id("root").show(ui, |ui| {
            Button::new().with_id("gone").label("X").show(ui);
        });
    });
    let prev_button_rect = ui.damage.prev[&WidgetId::from_hash("gone")].rect;

    frame(&mut ui, |ui| {
        Panel::hstack().with_id("root").show(ui, |_| {});
    });

    // The button no longer exists in the tree, so it's not in
    // `dirty` — but its prev rect must still influence damage.
    // The root is dirty (its own arranged rect collapsed since
    // the only child is gone), so damage = union(root rect,
    // prev button rect).
    let damage = ui.damage.rect.expect("removed widget must produce damage");
    assert!(damage.size.w >= prev_button_rect.size.w);
    assert!(damage.size.h >= prev_button_rect.size.h);
}

/// Pin: an added widget that wasn't in last frame contributes
/// its current rect to damage and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack().with_id("root").show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack().with_id("root").show(ui, |ui| {
            Frame::new()
                .with_id("new")
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
        .map(|n| ui.tree.records.widget_id()[n.index()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(ui.damage.rect.is_some());
}

// --- Ui::damage_filter ---------------------------------------------------

/// Pin: `filter()` returns `Full` when the damage rect covers
/// most of the surface — the encoder + backend treat that as
/// "paint everything" so they don't pay per-node filter cost on what
/// would be a full repaint anyway.
#[test]
fn damage_filter_returns_full_on_first_frame() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    // First frame: every node is "added" → damage rect is the union
    // of every screen rect → ratio > 0.5 → filter returns Full.
    assert!(ui.damage.rect.is_some());
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Full
    );
}

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
    let r = ui.damage.rect.expect("single-leaf change → some damage");
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(r)
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
            .with_id("outer")
            .transform(TranslateScale::from_translation(translate))
            .show(ui, |ui| {
                *child = Some(
                    Frame::new()
                        .with_id("c")
                        .size(40.0)
                        .background(Background {
                            fill,
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
        ui.end_frame();
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut ui, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut ui, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = ui.pipeline.layout.result.rect[child_node.unwrap().index()];
    let expected_screen_rect = Rect {
        min: child_layout_rect.min + translate,
        size: child_layout_rect.size,
    };
    let damage_rect = ui.damage.rect.expect("child changed → some damage");
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
            .with_id("outer")
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                *child = Some(
                    Frame::new()
                        .with_id("c")
                        .size(40.0)
                        .background(Background {
                            fill: Color::rgb(0.2, 0.4, 0.8),
                            ..Default::default()
                        })
                        .show(ui)
                        .node,
                );
            });
        ui.end_frame();
    };

    build(0.0, &mut ui, &mut child_node);
    build(50.0, &mut ui, &mut child_node);

    // Child layout rect didn't change. Parent's transform shifted by
    // (50, 0). Prev screen rect = (0,0,40,40); curr = (50,0,40,40);
    // damage union = (0,0,90,40).
    let damage = ui.damage.rect.expect("transform animation → damage");
    assert_eq!(
        damage,
        Rect::new(0.0, 0.0, 90.0, 40.0),
        "damage must union old (0,0)-(40,40) and new (50,0)-(90,40)",
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
        .map(|n| ui.tree.records.widget_id()[n.index()])
        .collect();
    assert_eq!(dirty_widget_ids, vec![WidgetId::from_hash("c")]);
}

// --- Damage::filter heuristic ---------------------------------------------

const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

#[test]
fn no_damage_means_skip() {
    let d = Damage::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what `>50%` produces.
    assert_eq!(d.filter(TEST_SURFACE), DamagePaint::Skip);
}

fn damage_with(r: Rect) -> Damage {
    Damage {
        rect: Some(r),
        ..Damage::default()
    }
}

/// Heuristic: ratio = damage_area / surface_area; > 50% ⇒ Full,
/// otherwise Partial. The check is `>`, not `>=`, so exactly 50%
/// stays Partial. A zero-area surface forces Full (divide-by-zero
/// guard).
#[test]
fn damage_filter_threshold_cases() {
    let cases: &[(&str, Rect, Rect, DamagePaint)] = &[
        (
            "small_1pct",
            Rect::new(0.0, 0.0, 10.0, 10.0),
            TEST_SURFACE,
            DamagePaint::Partial(Rect::new(0.0, 0.0, 10.0, 10.0)),
        ),
        (
            "large_64pct",
            Rect::new(0.0, 0.0, 80.0, 80.0),
            TEST_SURFACE,
            DamagePaint::Full,
        ),
        (
            "exact_50pct_stays_partial",
            Rect::new(0.0, 0.0, 50.0, 100.0),
            TEST_SURFACE,
            DamagePaint::Partial(Rect::new(0.0, 0.0, 50.0, 100.0)),
        ),
        (
            "zero_area_surface",
            Rect::new(0.0, 0.0, 1.0, 1.0),
            Rect::ZERO,
            DamagePaint::Full,
        ),
    ];
    for (label, dmg, surface, want) in cases {
        let d = damage_with(*dmg);
        assert_eq!(d.filter(*surface), *want, "case: {label}");
    }
}

/// Pin: on the first frame `Damage::filter` returns `Full` — every
/// node is "added," damage = full surface, ratio = 1.0 > 0.5.
#[test]
fn first_frame_filter_is_full() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Full
    );
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
        assert_eq!(ui.end_frame().damage, DamagePaint::Full, "case: {label} f1");
        ui.begin_frame(DISPLAY);
        build(&mut ui);
        assert_eq!(ui.end_frame().damage, DamagePaint::Skip, "case: {label} f2");
        assert!(ui.damage.dirty.is_empty(), "case: {label} steady");

        // Mutate Display; identical authoring; must short-circuit to Full.
        ui.begin_frame(*mutated);
        build(&mut ui);
        let after = ui.end_frame().damage;
        assert_eq!(after, DamagePaint::Full, "case: {label} display change");
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

/// Pin: `Ui::invalidate_prev_frame` rewinds damage so the next
/// `end_frame` returns `Full` even when widgets are unchanged. This
/// is the host's escape hatch for "I called `end_frame` but never
/// presented" — failed surface acquire (Occluded / Timeout /
/// Validation), surface reconfigure, etc. Without it, the next
/// `compute` would produce `Skip` against an unpainted backbuffer
/// and the window stays black until something forces a real change.
#[test]
fn invalidate_prev_frame_forces_next_frame_to_full() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };

    // Two warm frames: first is `Full`, second is `Skip` (steady state).
    ui.begin_frame(DISPLAY);
    build(&mut ui);
    assert_eq!(ui.end_frame().damage, DamagePaint::Full);
    ui.begin_frame(DISPLAY);
    build(&mut ui);
    assert_eq!(ui.end_frame().damage, DamagePaint::Skip);

    // Host says "last `end_frame`'s output didn't actually paint."
    ui.invalidate_prev_frame();

    // Next frame must be `Full` even though authoring is identical
    // and the surface didn't move — damage has no valid prev to diff
    // against, so it falls back to a clear+repaint.
    ui.begin_frame(DISPLAY);
    build(&mut ui);
    let d = ui.end_frame().damage;
    assert_eq!(
        d,
        DamagePaint::Full,
        "invalidate_prev_frame must force the next compute to Full",
    );

    // And once a real frame paints, steady-state `Skip` resumes.
    ui.begin_frame(DISPLAY);
    build(&mut ui);
    assert_eq!(ui.end_frame().damage, DamagePaint::Skip);
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
/// The test uses a Fixed-size root so its rect stays constant across
/// surface changes (otherwise the FILL/Hug root's rect contribution
/// would dominate damage and the area threshold alone would short-
/// circuit). Then injects a small `prev` mutation so the diff produces
/// a tiny damage rect at a different surface, which would otherwise
/// pass the threshold filter.
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
            .with_id("root")
            .size((Sizing::Fixed(3050.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                Frame::new()
                    .with_id("big")
                    .size((3000.0, 60.0))
                    .background(Background {
                        fill: BLUE,
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .with_id("small")
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
    ui.end_frame();
    ui.begin_frame(big);
    scene(&mut ui);
    ui.end_frame();
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

    assert!(
        !ui.damage.dirty.is_empty(),
        "test setup invalid — injected nudge should mark widget a dirty",
    );
    let r = ui.damage.rect.expect("damage rect should be Some");
    assert!(
        r.area() / smaller.logical_rect().area() < 0.5,
        "test setup invalid — damage area ratio should be <50% (would \
         otherwise pass the threshold without exercising the surface check), \
         got rect={r:?}",
    );
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
    ui.end_frame();
    ui.begin_frame(DISPLAY);
    build(&mut ui, BLUE);
    let warm = ui.end_frame().damage;
    assert_eq!(warm, DamagePaint::Skip, "warm steady-state with no diff");
    assert!(ui.damage.dirty.is_empty());

    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    ui.begin_frame(DISPLAY);
    build(&mut ui, RED);
    let partial = ui.end_frame().damage;
    let DamagePaint::Partial(r) = partial else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {partial:?} — surface-change short-circuit fired incorrectly",
        );
    };
    // Damage rect = the 50×50 frame's rect. Well below 50% of 200×200.
    assert!(
        r.area() / DISPLAY.logical_rect().area() < 0.5,
        "damage rect should be small (partial repaint range), got {r:?}",
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
        Panel::vstack().with_id("root").show(ui, |ui| {
            *hot = Some(Button::new().with_id("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::new().with_id("cold").label("Quiet").show(ui).node);
        });
        ui.end_frame();
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

    let hot_rect = ui.pipeline.layout.result.rect[hot_node.unwrap().index()];
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
        ui.tree.records.widget_id()[dirty_id.index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.rect, Some(hot_rect));
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(hot_rect),
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
        Panel::vstack().with_id("root").show(ui, |ui| {
            *hot = Some(Button::new().with_id("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::new().with_id("cold").label("Quiet").show(ui).node);
        });
        ui.end_frame();
    };

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.pipeline.layout.result.rect[hot_node.unwrap().index()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage.dirty.len(), 1);
    assert_eq!(
        ui.tree.records.widget_id()[ui.damage.dirty[0].index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.rect, Some(hot_rect));
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        DamagePaint::Partial(hot_rect),
    );
}
