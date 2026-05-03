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
        .expect("leaf snapshot must be inserted on first frame");
    assert_eq!(snap.desired.len(), 1, "leaf subtree is one entry");
    assert_eq!(snap.desired[0].w, 50.0);
    assert_eq!(snap.desired[0].h, 50.0);
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
    // `desired` as the first. Correctness contract for the
    // short-circuit — a hit must not perturb layout output.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let d1 = ui.layout_engine.cache.prev.get(&wid).unwrap().desired[0];
    run_frame(&mut ui, build);
    let d2 = ui.layout_engine.cache.prev.get(&wid).unwrap().desired[0];
    assert_eq!(d1, d2);
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
    let snap1 = ui.layout_engine.cache.prev.get(&wid).unwrap();
    let avail1 = snap1.available_q;
    let desired1 = snap1.desired[0];

    ui.begin_frame(Display::from_physical(UVec2::new(80, 80), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, build);
    ui.end_frame();

    let snap2 = ui.layout_engine.cache.prev.get(&wid).unwrap();
    assert_ne!(
        avail1, snap2.available_q,
        "shrinking the surface must change the cache's available key",
    );
    assert_ne!(
        desired1, snap2.desired[0],
        "remeasure must produce a different desired for a Fill child",
    );
}

#[test]
fn subtree_snapshot_covers_every_descendant() {
    // Phase-2 contract: a parent's snapshot stores `desired` for
    // every node in its subtree, in pre-order, contiguous. Verifies
    // that the snapshot's length matches the tree's `subtree_end`.
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Panel::vstack_with_id("group").show(ui, |ui| {
            Frame::with_id("c1").size(10.0).show(ui);
            Frame::with_id("c2").size(20.0).show(ui);
            Frame::with_id("c3").size(30.0).show(ui);
        });
    });
    let group_wid = WidgetId::from_hash("group");
    let snap = ui.layout_engine.cache.prev.get(&group_wid).unwrap();
    // group itself + 3 children = 4 entries.
    assert_eq!(snap.desired.len(), 4);
    // Children are leaves — their own desired sizes are stored at
    // indices 1, 2, 3 in pre-order.
    assert_eq!(snap.desired[1].w, 10.0);
    assert_eq!(snap.desired[2].w, 20.0);
    assert_eq!(snap.desired[3].w, 30.0);
}

#[test]
fn subtree_skip_preserves_descendant_rects() {
    // Identical frames must produce identical arranged rects for
    // every node, even when the parent (and so the whole subtree)
    // is short-circuited. If the restore path skipped a descendant,
    // arrange would zero or stale-fill its rect on frame 2.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack_with_id("group").show(ui, |ui| {
            Frame::with_id("c1").size(10.0).show(ui);
            Frame::with_id("c2").size(20.0).show(ui);
        });
    };
    run_frame(&mut ui, build);
    let layout1 = ui.layout_engine.result();
    let group_node = ui.tree().root().unwrap(); // root is the outer "root" hstack
    // Snapshot every node's rect for frame 1.
    let n = ui.tree().node_count();
    let rects1: Vec<_> = (0..n)
        .map(|i| layout1.rect(crate::tree::NodeId(i as u32)))
        .collect();
    let _ = group_node;

    run_frame(&mut ui, build);
    let layout2 = ui.layout_engine.result();
    let rects2: Vec<_> = (0..n)
        .map(|i| layout2.rect(crate::tree::NodeId(i as u32)))
        .collect();
    assert_eq!(
        rects1, rects2,
        "subtree-skip cache hit must not perturb any arranged rect",
    );
}

#[test]
fn quantize_available_handles_infinity() {
    use super::quantize_available;
    use crate::primitives::Size;
    let q = quantize_available(Size::new(f32::INFINITY, 100.4));
    assert_eq!(q, (i32::MAX, 100));
}
