use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Display, WidgetId};
use crate::widgets::{Frame, Panel, Styled};
use glam::UVec2;

fn run_frame(ui: &mut Ui, build: impl FnOnce(&mut Ui)) {
    ui.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    Panel::hstack_with_id("root").show(ui, build);
    ui.end_frame();
}

#[test]
fn leaf_snapshot_populated_after_first_frame() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    });
    let wid = WidgetId::from_hash("a");
    let snap = ui
        .layout_engine
        .cache
        .prev
        .get(&wid)
        .copied()
        .expect("leaf snapshot must be inserted on first frame");
    assert_eq!(snap.desired.w, 50.0);
    assert_eq!(snap.desired.h, 50.0);
}

#[test]
fn unchanged_leaf_keeps_subtree_hash_across_frames() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let h1 = ui.layout_engine.cache.prev.get(&wid).unwrap().subtree_hash;
    run_frame(&mut ui, build);
    let h2 = ui.layout_engine.cache.prev.get(&wid).unwrap().subtree_hash;
    assert_eq!(h1, h2);
}

#[test]
fn changing_leaf_authoring_replaces_snapshot() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    });
    let wid = WidgetId::from_hash("a");
    let h1 = ui.layout_engine.cache.prev.get(&wid).unwrap().subtree_hash;
    run_frame(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.9, 0.4, 0.8))
            .show(ui);
    });
    let h2 = ui.layout_engine.cache.prev.get(&wid).unwrap().subtree_hash;
    assert_ne!(
        h1, h2,
        "changed authoring must update the leaf's snapshot hash",
    );
}

#[test]
fn removed_widget_is_evicted() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::with_id("gone").size(40.0).show(ui);
        Frame::with_id("kept").size(40.0).show(ui);
    });
    let gone = WidgetId::from_hash("gone");
    let kept = WidgetId::from_hash("kept");
    assert!(ui.layout_engine.cache.prev.contains_key(&gone));
    assert!(ui.layout_engine.cache.prev.contains_key(&kept));

    run_frame(&mut ui, |ui| {
        Frame::with_id("kept").size(40.0).show(ui);
    });
    assert!(
        !ui.layout_engine.cache.prev.contains_key(&gone),
        "vanished widget must be evicted via SeenIds.removed",
    );
    assert!(ui.layout_engine.cache.prev.contains_key(&kept));
}

#[test]
fn cache_hit_replays_same_desired_size() {
    // Two identical frames: the second must produce the same
    // `desired` (read off `LayoutResult.rect`) as the first. This
    // is the correctness contract for the short-circuit — a hit
    // must not perturb the layout output.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let snap1 = ui.layout_engine.cache.prev.get(&wid).copied().unwrap();
    run_frame(&mut ui, build);
    let snap2 = ui.layout_engine.cache.prev.get(&wid).copied().unwrap();
    assert_eq!(snap1.desired, snap2.desired);
}

#[test]
fn changing_available_forces_miss_and_remeasure() {
    // Same authoring (Fill child) but the parent's available size
    // shrinks between frames → `available_q` arm of the cache key
    // diverges. The snapshot must be replaced, not stale.
    use crate::primitives::Sizing;
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::hstack_with_id("inner").show(ui, |ui| {
            Frame::with_id("fill")
                .size((Sizing::Fill(1.0), Sizing::Fill(1.0)))
                .show(ui);
        });
    };
    ui.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, build);
    ui.end_frame();

    let wid = WidgetId::from_hash("fill");
    let snap1 = ui.layout_engine.cache.prev.get(&wid).copied().unwrap();

    ui.begin_frame(Display::from_physical(UVec2::new(80, 80), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, build);
    ui.end_frame();

    let snap2 = ui.layout_engine.cache.prev.get(&wid).copied().unwrap();
    assert_ne!(
        snap1.available_q, snap2.available_q,
        "shrinking the surface must change the cache's available key",
    );
    assert_ne!(
        snap1.desired, snap2.desired,
        "remeasure must produce a different desired for a Fill child",
    );
}

#[test]
fn quantize_available_handles_infinity() {
    use super::quantize_available;
    use crate::primitives::Size;
    let q = quantize_available(Size::new(f32::INFINITY, 100.4));
    assert_eq!(q, (i32::MAX, 100));
}
