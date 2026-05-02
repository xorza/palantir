use super::{Damage, needs_full_repaint};
use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Rect, Sizing, WidgetId};
use crate::widgets::{Button, Frame, Panel, Styled};

/// Drive one frame with the given builder. Closure receives `ui`
/// after `begin_frame`.
fn frame(ui: &mut Ui, f: impl FnOnce(&mut Ui)) {
    ui.begin_frame();
    f(ui);
    ui.layout(Rect::new(0.0, 0.0, 200.0, 200.0));
    ui.end_frame();
}

/// Pin: the very first frame has no `prev_frame` entries, so every
/// node is "added" → all nodes dirty, damage covers their union.
#[test]
fn first_frame_marks_every_node_dirty() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
    });
    assert_eq!(ui.damage.dirty.len(), ui.tree().node_count());
    assert!(ui.damage.rect.is_some());
}

/// Pin: re-recording identical authoring → zero dirty nodes,
/// damage rect is `None`. The steady-state ideal: idle UI does
/// nothing.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
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
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
    });
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.9, 0.4, 0.8))
                .show(ui);
        });
    });

    assert_eq!(ui.damage.dirty.len(), 1);
    let dirty_id = ui.damage.dirty[0];
    assert_eq!(
        ui.tree().widget_ids()[dirty_id.index()],
        WidgetId::from_hash("a")
    );
    // Damage rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(ui.damage.rect, Some(ui.rect(dirty_id)));
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
        .map(|n| ui.tree().widget_ids()[n.index()])
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
    let prev_button_rect = ui.prev_frame[&WidgetId::from_hash("gone")].rect;

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
        .map(|n| ui.tree().widget_ids()[n.index()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(ui.damage.rect.is_some());
}

// --- needs_full_repaint --------------------------------------------------

#[test]
fn no_damage_means_no_full_repaint() {
    let d = Damage::default();
    assert!(!needs_full_repaint(&d, Rect::new(0.0, 0.0, 100.0, 100.0)));
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
    assert!(!needs_full_repaint(&d, Rect::new(0.0, 0.0, 100.0, 100.0)));
}

#[test]
fn large_damage_falls_back_to_full() {
    // 80x80 = 6400, surface 100x100 = 10000 → 64% > 50%.
    let d = damage_with(Rect::new(0.0, 0.0, 80.0, 80.0));
    assert!(needs_full_repaint(&d, Rect::new(0.0, 0.0, 100.0, 100.0)));
}

#[test]
fn at_threshold_stays_partial() {
    // Exactly 50%. The check is `>`, not `>=`, so 50% is partial.
    let d = damage_with(Rect::new(0.0, 0.0, 50.0, 100.0));
    assert!(!needs_full_repaint(&d, Rect::new(0.0, 0.0, 100.0, 100.0)));
}

#[test]
fn zero_area_surface_forces_full() {
    let d = damage_with(Rect::new(0.0, 0.0, 1.0, 1.0));
    assert!(needs_full_repaint(&d, Rect::ZERO));
}

/// Pin: `Damage::compute` (called by `Ui::end_frame`) sets
/// `full_repaint` on the first frame — every node is "added,"
/// damage = full surface, ratio = 1.0 > 0.5.
#[test]
fn first_frame_sets_full_repaint() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
    });
    assert!(ui.damage.full_repaint);
}

/// Pin: a small per-frame change (single leaf fill flip) stays in
/// the partial-repaint regime — `full_repaint` is false even though
/// the diff is non-empty.
#[test]
fn small_change_stays_partial_repaint() {
    let mut ui = Ui::new();
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.2, 0.4, 0.8))
                .show(ui);
        });
    });
    frame(&mut ui, |ui| {
        Panel::hstack_with_id("root").show(ui, |ui| {
            Frame::with_id("a")
                .size(50.0)
                .fill(Color::rgb(0.9, 0.4, 0.8))
                .show(ui);
        });
    });
    // 50x50 = 2500, surface 200x200 = 40000 → 6.25% < 50%.
    assert!(!ui.damage.full_repaint);
    assert!(ui.damage.rect.is_some());
}
