use super::Damage;
use crate::Ui;
use crate::input::InputEvent;
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::{color::Color, rect::Rect, transform::TranslateScale};
use crate::test_support::begin;
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::{button::Button, frame::Frame, panel::Panel, styled::Styled};
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
    Panel::hstack_with_id("root").show(ui, |ui| {
        Frame::with_id("a").size(50.0).fill(color).show(ui);
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
    assert_eq!(ui.damage.dirty.len(), ui.tree.node_count());
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
        ui.tree.widget_ids[dirty_id.index()],
        WidgetId::from_hash("a")
    );
    // Damage rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage.rect,
        Some(ui.layout_engine.result.rect[dirty_id.index()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::new();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
            Frame::with_id("b")
                .size((Sizing::Fixed(30.0), Sizing::Fixed(20.0)))
                .fill(Color::rgb(0.5, 0.5, 0.5))
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
        .map(|n| ui.tree.widget_ids[n.index()])
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
        Panel::hstack_with_id("root").show(ui, |ui| {
            Button::with_id("gone").label("X").show(ui);
        });
    });
    let prev_button_rect = ui.damage.prev[&WidgetId::from_hash("gone")].rect;

    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |_| {});
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
        Panel::hstack_with_id("root").show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("new")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
    });

    let dirty_ids: Vec<WidgetId> = ui
        .damage
        .dirty
        .iter()
        .map(|n| ui.tree.widget_ids[n.index()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(ui.damage.rect.is_some());
}

// --- Ui::damage_filter ---------------------------------------------------

/// Pin: `damage_filter()` returns `None` when the damage rect covers
/// most of the surface — the encoder + backend treat `None` as
/// "paint everything" so they don't pay per-node filter cost on what
/// would be a full repaint anyway.
#[test]
fn damage_filter_returns_none_on_full_repaint() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    // First frame: every node is "added" → damage rect is the union
    // of every screen rect → ratio > 0.5 → filter returns None.
    assert!(ui.damage.rect.is_some());
    assert!(ui.damage.filter(ui.display.logical_rect()).is_none());
}

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns the damage rect, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_rect_when_partial() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });
    assert!(ui.damage.rect.is_some());
    assert_eq!(ui.damage.filter(ui.display.logical_rect()), ui.damage.rect);
    assert!(ui.damage.filter(ui.display.logical_rect()).is_some());
}

/// Pin: `damage_filter()` returns `None` when nothing changed at all
/// (no rect). Matches the steady-state idle case.
#[test]
fn damage_filter_returns_none_when_nothing_dirty() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut ui, build);
    frame(&mut ui, build);
    assert!(ui.damage.dirty.is_empty());
    assert!(ui.damage.filter(ui.display.logical_rect()).is_none());
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
        Panel::hstack_with_id("outer")
            .transform(TranslateScale::from_translation(translate))
            .show(ui, |ui| {
                *child = Some(Frame::with_id("c").size(40.0).fill(fill).show(ui).node);
            });
        ui.end_frame();
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut ui, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut ui, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = ui.layout_engine.result.rect[child_node.unwrap().index()];
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
        Panel::hstack_with_id("outer")
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                *child = Some(
                    Frame::with_id("c")
                        .size(40.0)
                        .fill(Color::rgb(0.2, 0.4, 0.8))
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
        .map(|n| ui.tree.widget_ids[n.index()])
        .collect();
    assert_eq!(dirty_widget_ids, vec![WidgetId::from_hash("c")]);
}

// --- Damage::filter heuristic ---------------------------------------------

const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

#[test]
fn no_damage_means_partial_repaint() {
    let d = Damage::default();
    // No damage rect → `filter` returns None for the "nothing to do"
    // reason, not for "force full repaint." Both reasons collapse to
    // the same return value; the host is gated by `should_repaint`.
    assert!(d.filter(TEST_SURFACE).is_none());
}

fn damage_with(r: Rect) -> Damage {
    Damage {
        rect: Some(r),
        ..Damage::default()
    }
}

#[test]
fn small_damage_stays_partial() {
    // 100 / 10000 = 1% — well below 50%.
    let d = damage_with(Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(
        d.filter(TEST_SURFACE),
        Some(Rect::new(0.0, 0.0, 10.0, 10.0))
    );
}

#[test]
fn large_damage_falls_back_to_full() {
    // 80x80 = 6400, surface 100x100 = 10000 → 64% > 50%.
    let d = damage_with(Rect::new(0.0, 0.0, 80.0, 80.0));
    assert!(d.filter(TEST_SURFACE).is_none());
}

#[test]
fn at_threshold_stays_partial() {
    // Exactly 50%. The check is `>`, not `>=`, so 50% is partial.
    let d = damage_with(Rect::new(0.0, 0.0, 50.0, 100.0));
    assert_eq!(
        d.filter(TEST_SURFACE),
        Some(Rect::new(0.0, 0.0, 50.0, 100.0))
    );
}

#[test]
fn zero_area_surface_forces_full() {
    let d = damage_with(Rect::new(0.0, 0.0, 1.0, 1.0));
    assert!(d.filter(Rect::ZERO).is_none());
}

/// Pin: on the first frame `Damage::filter` returns `None` — every
/// node is "added," damage = full surface, ratio = 1.0 > 0.5.
#[test]
fn first_frame_filter_is_none() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    assert!(ui.damage.filter(ui.display.logical_rect()).is_none());
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
        Panel::vstack_with_id("root").show(ui, |ui| {
            *hot = Some(Button::with_id("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::with_id("cold").label("Quiet").show(ui).node);
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

    let hot_rect = ui.layout_engine.result.rect[hot_node.unwrap().index()];
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
        ui.tree.widget_ids[dirty_id.index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.rect, Some(hot_rect));
    assert_eq!(
        ui.damage.filter(ui.display.logical_rect()),
        Some(hot_rect),
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
        Panel::vstack_with_id("root").show(ui, |ui| {
            *hot = Some(Button::with_id("hot").label("Hover me").show(ui).node);
            *cold = Some(Button::with_id("cold").label("Quiet").show(ui).node);
        });
        ui.end_frame();
    };

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.layout_engine.result.rect[hot_node.unwrap().index()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage.dirty.len(), 1);
    assert_eq!(
        ui.tree.widget_ids[ui.damage.dirty[0].index()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage.rect, Some(hot_rect));
    assert_eq!(ui.damage.filter(ui.display.logical_rect()), Some(hot_rect),);
}
