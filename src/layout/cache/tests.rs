use crate::Ui;
use crate::element::Configure;
use crate::primitives::{Color, Display, Size, WidgetId};
use crate::widgets::{Frame, Panel, Styled};
use glam::UVec2;

fn run_frame(ui: &mut Ui, build: impl FnOnce(&mut Ui)) {
    ui.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    Panel::hstack_with_id("root").show(ui, build);
    ui.end_frame();
}

/// Read the snapshot's live arena range for `wid`. Returns
/// `(snapshot, desired_slice, _text_slice)`.
fn snap_for(ui: &Ui, wid: WidgetId) -> Option<(super::ArenaSnapshot, &[Size])> {
    let cache = &ui.layout_engine.cache;
    let snap = *cache.snapshots.get(&wid)?;
    let s = snap.start as usize;
    let e = s + snap.len as usize;
    Some((snap, &cache.desired_arena[s..e]))
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
    let (snap, desired) = snap_for(&ui, wid).expect("leaf snapshot must be inserted");
    assert_eq!(snap.len, 1, "leaf subtree spans one node");
    assert_eq!(desired[0].w, 50.0);
    assert_eq!(desired[0].h, 50.0);
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
    let h1 = snap_for(&ui, wid).unwrap().0.subtree_hash;
    run_frame(&mut ui, build);
    let h2 = snap_for(&ui, wid).unwrap().0.subtree_hash;
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
    let h1 = snap_for(&ui, wid).unwrap().0.subtree_hash;
    run_frame(&mut ui, |ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.9, 0.4, 0.8))
            .show(ui);
    });
    let h2 = snap_for(&ui, wid).unwrap().0.subtree_hash;
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
    assert!(ui.layout_engine.cache.snapshots.contains_key(&gone));
    assert!(ui.layout_engine.cache.snapshots.contains_key(&kept));

    run_frame(&mut ui, |ui| {
        Frame::with_id("kept").size(40.0).show(ui);
    });
    assert!(
        !ui.layout_engine.cache.snapshots.contains_key(&gone),
        "vanished widget must be evicted via SeenIds.removed",
    );
    assert!(ui.layout_engine.cache.snapshots.contains_key(&kept));
}

#[test]
fn cache_hit_replays_same_desired_size() {
    // Two identical frames: the second must produce the same `desired`
    // as the first. Correctness contract for the short-circuit — a
    // hit must not perturb layout output.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(0.2, 0.4, 0.8))
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let d1 = snap_for(&ui, wid).unwrap().1[0];
    run_frame(&mut ui, build);
    let d2 = snap_for(&ui, wid).unwrap().1[0];
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
    let avail1 =
        ui.layout_engine.cache.available_arena[snap_for(&ui, wid).unwrap().0.start as usize];
    let d1 = snap_for(&ui, wid).unwrap().1[0];

    ui.begin_frame(Display::from_physical(UVec2::new(80, 80), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, build);
    ui.end_frame();

    let avail2 =
        ui.layout_engine.cache.available_arena[snap_for(&ui, wid).unwrap().0.start as usize];
    let desired2 = snap_for(&ui, wid).unwrap().1[0];
    assert_ne!(
        avail1, avail2,
        "shrinking the surface must change the cache's available key",
    );
    assert_ne!(
        d1, desired2,
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
    let (snap, desired) = snap_for(&ui, group_wid).unwrap();
    // group itself + 3 children = 4 entries.
    assert_eq!(snap.len, 4);
    // Children are leaves — their own desired sizes are stored at
    // indices 1, 2, 3 in pre-order.
    assert_eq!(desired[1].w, 10.0);
    assert_eq!(desired[2].w, 20.0);
    assert_eq!(desired[3].w, 30.0);
}

#[test]
fn subtree_skip_restores_descendant_available_q() {
    // Contract for downstream consumers (e.g. the encode cache) that
    // read `LayoutResult.available_q` at every visited node:
    // descendants of a measure-cache hit must carry their correct
    // `available_q` even though `measure()` short-circuits at the
    // subtree root and never visits them. `resize_for` zeros the
    // column at frame start, so a missing restore would leave
    // descendants at `AvailableKey::ZERO`.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack_with_id("group").show(ui, |ui| {
            Frame::with_id("c1").size(10.0).show(ui);
            Frame::with_id("c2").size(20.0).show(ui);
        });
    };
    run_frame(&mut ui, build);
    let n = ui.tree().node_count();
    let cold: Vec<_> = (0..n)
        .map(|i| {
            ui.layout_engine
                .result()
                .available_q(crate::tree::NodeId(i as u32))
        })
        .collect();
    // Cold frame must have populated every descendant.
    assert!(
        cold.iter()
            .all(|q| *q != crate::layout::AvailableKey::default()),
        "cold frame must populate `available_q` for every node",
    );

    run_frame(&mut ui, build);
    let warm: Vec<_> = (0..n)
        .map(|i| {
            ui.layout_engine
                .result()
                .available_q(crate::tree::NodeId(i as u32))
        })
        .collect();
    assert_eq!(
        cold, warm,
        "subtree-skip must restore descendants' `available_q` from the snapshot",
    );
}

#[test]
fn subtree_skip_preserves_descendant_rects() {
    // Identical frames must produce identical arranged rects for
    // every node, even when the parent (and so the whole subtree) is
    // short-circuited.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack_with_id("group").show(ui, |ui| {
            Frame::with_id("c1").size(10.0).show(ui);
            Frame::with_id("c2").size(20.0).show(ui);
        });
    };
    run_frame(&mut ui, build);
    let n = ui.tree().node_count();
    let layout1 = ui.layout_engine.result();
    let rects1: Vec<_> = (0..n)
        .map(|i| layout1.rect(crate::tree::NodeId(i as u32)))
        .collect();

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
    let q = quantize_available(Size::new(f32::INFINITY, 100.4));
    assert_eq!(
        q,
        super::AvailableKey {
            w: i32::MAX,
            h: 100
        }
    );
}

#[test]
fn in_place_rewrite_preserves_arena_position() {
    // Steady-state hot path: same WidgetId, same subtree size → the
    // arena range must be reused in place, never appended. Verifies
    // the optimization that lets us amortize allocations.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui, c: f32| {
        Frame::with_id("a")
            .size(50.0)
            .fill(Color::rgb(c, 0.4, 0.8))
            .show(ui);
    };

    ui.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| build(ui, 0.2));
    ui.end_frame();
    let start1 = snap_for(&ui, WidgetId::from_hash("a")).unwrap().0.start;

    // Different fill → different hash, but same subtree size (still 1
    // leaf). In-place path should reuse the slot.
    ui.begin_frame(Display::from_physical(UVec2::new(200, 200), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| build(ui, 0.9));
    ui.end_frame();
    let start2 = snap_for(&ui, WidgetId::from_hash("a")).unwrap().0.start;

    assert_eq!(
        start1, start2,
        "same-len rewrite must stay at same arena offset"
    );
}

#[test]
fn arena_invariant_holds_under_fragmentation() {
    // Force fragmentation by inserting widgets, dropping most, then
    // appending a fresh subtree. After everything settles, the
    // arena's invariant must hold: `arena.len <= live * COMPACT_RATIO`
    // once we're past the floor. Compaction is triggered lazily
    // inside `write_subtree`; we don't assert *which* write fired
    // it, only that the invariant holds at the end.
    use super::{COMPACT_FLOOR, COMPACT_RATIO};
    let mut ui = Ui::new();

    let n_first = (COMPACT_FLOOR) * 4;
    ui.begin_frame(Display::from_physical(UVec2::new(800, 800), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        for i in 0..n_first {
            Frame::with_id(("a", i)).size(10.0).show(ui);
        }
    });
    ui.end_frame();

    // Drop all but one and add a fresh subtree to force append-path
    // writes; expect compaction to trigger somewhere along the way.
    ui.begin_frame(Display::from_physical(UVec2::new(800, 800), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Frame::with_id(("a", 0usize)).size(10.0).show(ui);
        Panel::vstack_with_id("new-group").show(ui, |ui| {
            for j in 0..(COMPACT_FLOOR + 4) {
                Frame::with_id(("inner", j)).size(5.0).show(ui);
            }
        });
    });
    ui.end_frame();

    let cache = &ui.layout_engine.cache;
    if cache.live_entries > COMPACT_FLOOR {
        assert!(
            cache.desired_arena.len() <= cache.live_entries.saturating_mul(COMPACT_RATIO),
            "arena {} > live {} × {}x",
            cache.desired_arena.len(),
            cache.live_entries,
            COMPACT_RATIO,
        );
    }
}

#[test]
fn cache_hits_remain_valid_after_compaction() {
    // Compaction rewrites snapshot `start` indices. Verify that a
    // widget which survives compaction still produces correct
    // `desired` data on subsequent cache hits — i.e. the snapshot's
    // new arena range still contains the right bytes.
    use super::{COMPACT_FLOOR, COMPACT_RATIO};
    let mut ui = Ui::new();

    // Frame 1: enough widgets to clear the floor; remember one that
    // we'll keep across frames.
    let n_first = (COMPACT_FLOOR) * 4;
    ui.begin_frame(Display::from_physical(UVec2::new(800, 800), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        for i in 0..n_first {
            Frame::with_id(("a", i)).size(11.0).show(ui);
        }
    });
    ui.end_frame();
    let kept_wid = WidgetId::from_hash(("a", 0usize));
    let kept_desired_pre = snap_for(&ui, kept_wid).unwrap().1[0];

    // Frame 2: drop most, add fresh subtree to drive compaction.
    ui.begin_frame(Display::from_physical(UVec2::new(800, 800), 1.0));
    Panel::hstack_with_id("root").show(&mut ui, |ui| {
        Frame::with_id(("a", 0usize)).size(11.0).show(ui);
        Panel::vstack_with_id("new-group").show(ui, |ui| {
            for j in 0..(COMPACT_FLOOR + 4) {
                Frame::with_id(("inner", j)).size(5.0).show(ui);
            }
        });
    });
    ui.end_frame();

    // Whether or not compaction fired, the kept widget's snapshot
    // must still describe the right desired and arena range.
    let cache = &ui.layout_engine.cache;
    let snap = cache
        .snapshots
        .get(&kept_wid)
        .expect("kept widget must still have a snapshot");
    let s = snap.start as usize;
    let kept_desired_post = cache.desired_arena[s];
    assert_eq!(
        kept_desired_pre, kept_desired_post,
        "kept widget's `desired` must survive compaction unchanged",
    );

    // And the global invariant should still hold past the floor.
    if cache.live_entries > COMPACT_FLOOR {
        assert!(cache.desired_arena.len() <= cache.live_entries.saturating_mul(COMPACT_RATIO),);
    }
}
