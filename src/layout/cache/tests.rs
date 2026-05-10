use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::tree::Layer;
use crate::forest::widget_id::WidgetId;
use crate::layout::cache::{ArenaSnapshot, AvailableKey};
use crate::primitives::{color::Color, size::Size};
use crate::support::testing::{begin, ui_at};
use crate::widgets::theme::Background;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

fn run_frame(ui: &mut Ui, build: impl FnOnce(&mut Ui)) {
    begin(ui, UVec2::new(200, 200));
    Panel::hstack().id_salt("root").show(ui, build);
    ui.end_frame();
}

/// Read the snapshot's live arena range for `wid`.
struct SnapView<'a> {
    snap: ArenaSnapshot,
    desired: &'a [Size],
    avail: AvailableKey,
}

fn snap_for(ui: &Ui, wid: WidgetId) -> Option<SnapView<'_>> {
    let cache = &ui.layout.cache;
    let snap = *cache.snapshots.get(&wid)?;
    let nodes = snap.nodes.range();
    Some(SnapView {
        snap,
        desired: &cache.nodes.desired[nodes],
        avail: snap.available_q,
    })
}

/// `NodeArenas` bundles two parallel columns and enforces
/// length-equality by construction. Drift would silently corrupt
/// snapshot lookups (one column reads the right index, another reads
/// past its end). Pin the invariant after every operation that touches
/// the cache.
#[track_caller]
fn assert_node_columns_aligned(ui: &Ui) {
    let n = &ui.layout.cache.nodes;
    let len = n.desired.len();
    assert_eq!(n.text_spans.len(), len, "text_spans length drift");
    assert!(n.live <= len, "live {} > total {}", n.live, len);
}

#[test]
fn leaf_snapshot_populated_after_first_frame() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    });
    let wid = WidgetId::from_hash("a");
    let SnapView { snap, desired, .. } =
        snap_for(&ui, wid).expect("leaf snapshot must be inserted");
    assert_eq!(snap.nodes.len, 1, "leaf subtree spans one node");
    assert_eq!(desired[0].w, 50.0);
    assert_eq!(desired[0].h, 50.0);
}

#[test]
fn unchanged_leaf_keeps_subtree_hash_across_frames() {
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let h1 = snap_for(&ui, wid).unwrap().snap.subtree_hash;
    run_frame(&mut ui, build);
    let h2 = snap_for(&ui, wid).unwrap().snap.subtree_hash;
    assert_eq!(h1, h2);
}

#[test]
fn changing_leaf_authoring_replaces_snapshot() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    });
    let wid = WidgetId::from_hash("a");
    let h1 = snap_for(&ui, wid).unwrap().snap.subtree_hash;
    run_frame(&mut ui, |ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.9, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    });
    let h2 = snap_for(&ui, wid).unwrap().snap.subtree_hash;
    assert_ne!(
        h1, h2,
        "changed authoring must update the leaf's snapshot hash",
    );
}

#[test]
fn removed_widget_is_evicted() {
    let mut ui = Ui::new();
    run_frame(&mut ui, |ui| {
        Frame::new().id_salt("gone").size(40.0).show(ui);
        Frame::new().id_salt("kept").size(40.0).show(ui);
    });
    let gone = WidgetId::from_hash("gone");
    let kept = WidgetId::from_hash("kept");
    assert!(ui.layout.cache.snapshots.contains_key(&gone));
    assert!(ui.layout.cache.snapshots.contains_key(&kept));

    run_frame(&mut ui, |ui| {
        Frame::new().id_salt("kept").size(40.0).show(ui);
    });
    assert!(
        !ui.layout.cache.snapshots.contains_key(&gone),
        "vanished widget must be evicted via SeenIds.removed",
    );
    assert!(ui.layout.cache.snapshots.contains_key(&kept));
}

#[test]
fn cache_hit_replays_same_desired_size() {
    // Two identical frames: the second must produce the same `desired`
    // as the first. Correctness contract for the short-circuit — a
    // hit must not perturb layout output.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(0.2, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    };
    run_frame(&mut ui, build);
    let wid = WidgetId::from_hash("a");
    let d1 = snap_for(&ui, wid).unwrap().desired[0];
    run_frame(&mut ui, build);
    let d2 = snap_for(&ui, wid).unwrap().desired[0];
    assert_eq!(d1, d2);
}

#[test]
fn changing_available_forces_miss_and_remeasure() {
    // Same authoring (Fill child) but the parent's available size
    // shrinks between frames → `available_q` arm of the cache key
    // diverges. The snapshot must be replaced, not stale.
    use crate::layout::types::sizing::Sizing;
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::hstack().id_salt("inner").show(ui, |ui| {
            Frame::new()
                .id_salt("fill")
                .size((Sizing::Fill(1.0), Sizing::Fill(1.0)))
                .show(ui);
        });
    };
    begin(&mut ui, UVec2::new(200, 200));
    Panel::hstack().id_salt("root").show(&mut ui, build);
    ui.end_frame();

    let wid = WidgetId::from_hash("fill");
    let avail1 = snap_for(&ui, wid).unwrap().avail;
    let d1 = snap_for(&ui, wid).unwrap().desired[0];

    begin(&mut ui, UVec2::new(80, 80));
    Panel::hstack().id_salt("root").show(&mut ui, build);
    ui.end_frame();

    let avail2 = snap_for(&ui, wid).unwrap().avail;
    let desired2 = snap_for(&ui, wid).unwrap().desired[0];
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
        Panel::vstack().id_salt("group").show(ui, |ui| {
            Frame::new().id_salt("c1").size(10.0).show(ui);
            Frame::new().id_salt("c2").size(20.0).show(ui);
            Frame::new().id_salt("c3").size(30.0).show(ui);
        });
    });
    let group_wid = WidgetId::from_hash("group");
    let SnapView { snap, desired, .. } = snap_for(&ui, group_wid).unwrap();
    // group itself + 3 children = 4 entries.
    assert_eq!(snap.nodes.len, 4);
    // Children are leaves — their own desired sizes are stored at
    // indices 1, 2, 3 in pre-order.
    assert_eq!(desired[1].w, 10.0);
    assert_eq!(desired[2].w, 20.0);
    assert_eq!(desired[3].w, 30.0);
}

#[test]
fn subtree_skip_preserves_descendant_rects() {
    // Identical frames must produce identical arranged rects for
    // every node, even when the parent (and so the whole subtree) is
    // short-circuited.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui| {
        Panel::vstack().id_salt("group").show(ui, |ui| {
            Frame::new().id_salt("c1").size(10.0).show(ui);
            Frame::new().id_salt("c2").size(20.0).show(ui);
        });
    };
    run_frame(&mut ui, build);
    let n = ui.forest.tree(Layer::Main).records.len();
    let layout1 = &ui.layout.result[Layer::Main];
    let rects1: Vec<_> = (0..n).map(|i| layout1.rect[i]).collect();

    run_frame(&mut ui, build);
    let layout2 = &ui.layout.result[Layer::Main];
    let rects2: Vec<_> = (0..n).map(|i| layout2.rect[i]).collect();
    assert_eq!(
        rects1, rects2,
        "subtree-skip cache hit must not perturb any arranged rect",
    );
}

#[test]
fn quantize_available_axis_invariants() {
    // The `i32::MAX` sentinel for `INFINITY` is load-bearing for the
    // measure cache's `(subtree_hash, available_q)` key. Pin:
    // `INFINITY` quantizes to `i32::MAX` independently per axis, both
    // axes together also do.
    use super::quantize_available;
    let inf = f32::INFINITY;
    assert_eq!(
        quantize_available(Size::new(inf, 100.4)),
        glam::IVec2::new(i32::MAX, 100),
    );
    assert_eq!(
        quantize_available(Size::new(50.7, inf)),
        glam::IVec2::new(51, i32::MAX),
    );
    assert_eq!(
        quantize_available(Size::new(inf, inf)),
        glam::IVec2::splat(i32::MAX),
    );
    assert_eq!(quantize_available(Size::ZERO), glam::IVec2::ZERO);
}

#[test]
fn in_place_rewrite_preserves_arena_position() {
    // Steady-state hot path: same WidgetId, same subtree size → the
    // arena range must be reused in place, never appended. Verifies
    // the optimization that lets us amortize allocations.
    let mut ui = Ui::new();
    let build = |ui: &mut Ui, c: f32| {
        Frame::new()
            .id_salt("a")
            .size(50.0)
            .background(Background {
                fill: Color::rgb(c, 0.4, 0.8),
                ..Default::default()
            })
            .show(ui);
    };

    begin(&mut ui, UVec2::new(200, 200));
    Panel::hstack()
        .id_salt("root")
        .show(&mut ui, |ui| build(ui, 0.2));
    ui.end_frame();
    let start1 = snap_for(&ui, WidgetId::from_hash("a"))
        .unwrap()
        .snap
        .nodes
        .start;

    // Different fill → different hash, but same subtree size (still 1
    // leaf). In-place path should reuse the slot.
    begin(&mut ui, UVec2::new(200, 200));
    Panel::hstack()
        .id_salt("root")
        .show(&mut ui, |ui| build(ui, 0.9));
    ui.end_frame();
    let start2 = snap_for(&ui, WidgetId::from_hash("a"))
        .unwrap()
        .snap
        .nodes
        .start;

    assert_eq!(
        start1, start2,
        "same-len rewrite must stay at same arena offset"
    );
    assert_node_columns_aligned(&ui);
}

#[test]
fn arena_invariant_holds_under_fragmentation() {
    // Force fragmentation by inserting widgets, dropping most, then
    // appending a fresh subtree. After everything settles, the
    // arena's invariant must hold: `arena.len <= live * COMPACT_RATIO`
    // once we're past the floor. Compaction is triggered lazily
    // inside `write_subtree`; we don't assert *which* write fired
    // it, only that the invariant holds at the end.
    use crate::common::cache_arena::{COMPACT_FLOOR, COMPACT_RATIO};
    let mut ui = Ui::new();

    let n_first = (COMPACT_FLOOR) * 4;
    begin(&mut ui, UVec2::new(800, 800));
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        for i in 0..n_first {
            Frame::new().id_salt(("a", i)).size(10.0).show(ui);
        }
    });
    ui.end_frame();

    // Drop all but one and add a fresh subtree to force append-path
    // writes; expect compaction to trigger somewhere along the way.
    begin(&mut ui, UVec2::new(800, 800));
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        Frame::new().id_salt(("a", 0usize)).size(10.0).show(ui);
        Panel::vstack().id_salt("new-group").show(ui, |ui| {
            for j in 0..(COMPACT_FLOOR + 4) {
                Frame::new().id_salt(("inner", j)).size(5.0).show(ui);
            }
        });
    });
    ui.end_frame();

    let cache = &ui.layout.cache;
    if cache.nodes.live > COMPACT_FLOOR {
        assert!(
            cache.nodes.desired.len() <= cache.nodes.live.saturating_mul(COMPACT_RATIO),
            "arena {} > live {} × {}x",
            cache.nodes.desired.len(),
            cache.nodes.live,
            COMPACT_RATIO,
        );
    }
    assert_node_columns_aligned(&ui);
}

#[test]
fn cache_hits_remain_valid_after_compaction() {
    // Compaction rewrites snapshot `start` indices. Verify that a
    // widget which survives compaction still produces correct
    // `desired` data on subsequent cache hits — i.e. the snapshot's
    // new arena range still contains the right bytes.
    use crate::common::cache_arena::{COMPACT_FLOOR, COMPACT_RATIO};
    let mut ui = Ui::new();

    // Frame 1: enough widgets to clear the floor; remember one that
    // we'll keep across frames.
    let n_first = (COMPACT_FLOOR) * 4;
    begin(&mut ui, UVec2::new(800, 800));
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        for i in 0..n_first {
            Frame::new().id_salt(("a", i)).size(11.0).show(ui);
        }
    });
    ui.end_frame();
    let kept_wid = WidgetId::from_hash(("a", 0usize));
    let kept_desired_pre = snap_for(&ui, kept_wid).unwrap().desired[0];

    // Frame 2: drop most, add fresh subtree to drive compaction.
    begin(&mut ui, UVec2::new(800, 800));
    Panel::hstack().id_salt("root").show(&mut ui, |ui| {
        Frame::new().id_salt(("a", 0usize)).size(11.0).show(ui);
        Panel::vstack().id_salt("new-group").show(ui, |ui| {
            for j in 0..(COMPACT_FLOOR + 4) {
                Frame::new().id_salt(("inner", j)).size(5.0).show(ui);
            }
        });
    });
    ui.end_frame();

    // Whether or not compaction fired, the kept widget's snapshot
    // must still describe the right desired and arena range.
    let cache = &ui.layout.cache;
    let snap = cache
        .snapshots
        .get(&kept_wid)
        .expect("kept widget must still have a snapshot");
    let s = snap.nodes.start as usize;
    let kept_desired_post = cache.nodes.desired[s];
    assert_eq!(
        kept_desired_pre, kept_desired_post,
        "kept widget's `desired` must survive compaction unchanged",
    );

    // And the global invariant should still hold past the floor.
    if cache.nodes.live > COMPACT_FLOOR {
        assert!(cache.nodes.desired.len() <= cache.nodes.live.saturating_mul(COMPACT_RATIO),);
    }
    assert_node_columns_aligned(&ui);
}

/// Partial-invalidation contract: changing one leaf must bust the
/// `subtree_hash` for that leaf and every ancestor (so they miss
/// the cache and re-measure), but a sibling subtree must keep its
/// hash AND its arena slot — no spurious replace, no spurious
/// rewrite. Catches regressions where the rollup over-invalidates
/// (siblings re-measure for free, perf cliff invisible to rect
/// tests) or under-invalidates (ancestors hit with stale data).
#[test]
fn partial_invalidation_busts_ancestors_preserves_siblings() {
    let build = |ui: &mut Ui, leaf_color: Color| {
        Panel::vstack().id_salt("root").show(ui, |ui| {
            Panel::vstack().id_salt("changing-branch").show(ui, |ui| {
                Frame::new()
                    .id_salt("changing-leaf")
                    .size(50.0)
                    .background(Background {
                        fill: leaf_color,
                        ..Default::default()
                    })
                    .show(ui);
            });
            Panel::vstack().id_salt("stable-sibling").show(ui, |ui| {
                Frame::new()
                    .id_salt("stable-leaf")
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8),
                        ..Default::default()
                    })
                    .show(ui);
            });
        });
    };

    let mut ui = ui_at(UVec2::new(400, 400));
    build(&mut ui, Color::rgb(1.0, 0.0, 0.0));
    ui.end_frame();

    let snap = |ui: &Ui, key: &str| {
        ui.layout
            .cache
            .snapshots
            .get(&WidgetId::from_hash(key))
            .copied()
            .unwrap_or_else(|| panic!("missing snapshot for {key}"))
    };

    let root_1 = snap(&ui, "root");
    let branch_1 = snap(&ui, "changing-branch");
    let leaf_1 = snap(&ui, "changing-leaf");
    let sib_branch_1 = snap(&ui, "stable-sibling");
    let sib_leaf_1 = snap(&ui, "stable-leaf");

    // Frame 2: only the changing leaf's color flips. Hash rollup
    // must propagate the change all the way to `root`; the stable
    // sibling subtree must be untouched.
    begin(&mut ui, UVec2::new(400, 400));
    build(&mut ui, Color::rgb(0.0, 1.0, 0.0));
    ui.end_frame();

    let root_2 = snap(&ui, "root");
    let branch_2 = snap(&ui, "changing-branch");
    let leaf_2 = snap(&ui, "changing-leaf");
    let sib_branch_2 = snap(&ui, "stable-sibling");
    let sib_leaf_2 = snap(&ui, "stable-leaf");

    // Changed path: hashes must differ (caches missed and rewrote).
    assert_ne!(
        leaf_1.subtree_hash, leaf_2.subtree_hash,
        "changed leaf must bust its own subtree_hash",
    );
    assert_ne!(
        branch_1.subtree_hash, branch_2.subtree_hash,
        "ancestor of changed leaf must bust its subtree_hash via rollup",
    );
    assert_ne!(
        root_1.subtree_hash, root_2.subtree_hash,
        "root must bust its subtree_hash via rollup",
    );

    // Stable sibling: hash unchanged AND arena position unchanged.
    // The position check rules out a spurious in-place rewrite.
    assert_eq!(
        sib_branch_1.subtree_hash, sib_branch_2.subtree_hash,
        "sibling subtree hash must not change when an unrelated leaf changes",
    );
    assert_eq!(
        sib_leaf_1.subtree_hash, sib_leaf_2.subtree_hash,
        "sibling leaf hash must not change",
    );
    assert_eq!(
        sib_branch_1.nodes.start, sib_branch_2.nodes.start,
        "sibling's arena slot must be untouched (no replace, no rewrite)",
    );
    assert_eq!(
        sib_leaf_1.nodes.start, sib_leaf_2.nodes.start,
        "sibling leaf's arena slot must be untouched",
    );
}

/// Lifecycle: a widget can vanish (sweep_removed evicts its
/// snapshot, arena slot becomes garbage) and reappear with the same
/// id. Re-insertion exercises the append-on-no-prev branch of
/// `write_subtree`, distinct from the steady-state in-place
/// rewrite. The reappeared widget must measure correctly and the
/// cache's `live_entries` accounting must stay consistent.
#[test]
fn cache_handles_widget_reappearance_after_eviction() {
    let with_widget = |ui: &mut Ui| {
        Panel::vstack().id_salt("inner").show(ui, |ui| {
            Frame::new()
                .id_salt("blip")
                .size(40.0)
                .background(Background {
                    fill: Color::rgb(0.5, 0.2, 0.7),
                    ..Default::default()
                })
                .show(ui);
        });
    };
    let without_widget = |ui: &mut Ui| {
        Panel::vstack().id_salt("inner").show(ui, |_ui| {});
    };

    let mut ui = Ui::new();
    let blip = WidgetId::from_hash("blip");

    // Frame 1: present.
    run_frame(&mut ui, with_widget);
    let live_before = ui.layout.cache.nodes.live;
    assert!(
        ui.layout.cache.snapshots.contains_key(&blip),
        "widget must be cached after first frame",
    );

    // Frame 2: vanished — `SeenIds` flags it removed and
    // `Ui::end_frame` calls `MeasureCache::sweep_removed`.
    run_frame(&mut ui, without_widget);
    assert!(
        !ui.layout.cache.snapshots.contains_key(&blip),
        "vanished widget must be evicted via sweep_removed",
    );
    let live_after_evict = ui.layout.cache.nodes.live;
    assert!(
        live_after_evict < live_before,
        "live count must decrease after eviction",
    );

    // Frame 3: reappears with same id. Re-insertion runs the
    // `no-prev` arm of `write_subtree`. After the frame the
    // snapshot must exist and live_entries must match what we'd see
    // on a cold cache for the same build.
    run_frame(&mut ui, with_widget);
    assert!(
        ui.layout.cache.snapshots.contains_key(&blip),
        "reappeared widget must be re-cached",
    );

    // Cold oracle: clear and run again. live_entries and the
    // snapshot's payload must match the warm reappearance.
    let warm_snap = *ui.layout.cache.snapshots.get(&blip).unwrap();
    let warm_desired = ui.layout.cache.nodes.desired[warm_snap.nodes.range()].to_vec();
    let warm_live = ui.layout.cache.nodes.live;

    crate::support::internals::clear_measure_cache(&mut ui);
    run_frame(&mut ui, with_widget);

    let cold_snap = *ui.layout.cache.snapshots.get(&blip).unwrap();
    let cold_desired = ui.layout.cache.nodes.desired[cold_snap.nodes.range()].to_vec();
    let cold_live = ui.layout.cache.nodes.live;

    assert_eq!(
        warm_snap.subtree_hash, cold_snap.subtree_hash,
        "reappeared subtree_hash must equal cold-rebuild's",
    );
    assert_eq!(
        warm_snap.nodes.len, cold_snap.nodes.len,
        "reappeared snapshot len must equal cold-rebuild's",
    );
    assert_eq!(
        warm_desired, cold_desired,
        "reappeared snapshot's desired payload must equal cold-rebuild's",
    );
    assert_eq!(warm_live, cold_live, "live count must match cold rebuild",);
}
