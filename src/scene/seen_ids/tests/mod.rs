use crate::internals::panic_probe;
use crate::scene::layer::Layer;
use crate::scene::seen_ids::*;
use crate::scene::tree::node_id::NodeId;

mod reference;

fn ep(node: u32) -> Endpoint {
    Endpoint {
        layer: Layer::Main,
        node: NodeId(node),
    }
}

/// Stand-in for the production `resolve → record_endpoint`
/// pairing every widget does (`Widget::resolve` →
/// `scene::open_node`). The lazy-counter fast path in `resolve`
/// depends on `curr` being populated between consecutive resolves
/// of the same raw id, so tests interleave them the same way.
fn open(ids: &mut SeenIds, raw_id: WidgetId, is_explicit: bool, node: u32) -> WidgetId {
    let resolved = ids.resolve(raw_id, is_explicit);
    ids.record_endpoint(resolved, ep(node));
    resolved.id()
}

#[test]
fn resolve_returns_raw_id_on_first_call() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    assert_eq!(open(&mut ids, x, false, 1), x);
    // Fast path didn't touch `counters` — only collisions populate it.
    assert!(ids.counters.is_empty());
}

#[test]
fn resolve_disambiguates_collisions_by_occurrence() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    assert_eq!(open(&mut ids, x, false, 1), x);
    assert_eq!(open(&mut ids, x, false, 2), x.with(1));
    assert_eq!(open(&mut ids, x, false, 3), x.with(2));
}

#[test]
fn resolve_skips_occupied_occurrence_ids() {
    let x = WidgetId::from_hash("x");

    for occupied_slots in [1_u32, 2] {
        let mut ids = SeenIds::default();
        assert_eq!(open(&mut ids, x, true, 0), x);

        for slot in 1..=occupied_slots {
            let occupied = x.with(slot);
            assert_eq!(open(&mut ids, occupied, true, slot), occupied);
        }

        let node = occupied_slots + 1;
        let final_id = open(&mut ids, x, true, node);
        assert_eq!(final_id, x.with(occupied_slots + 1));
        assert_eq!(ids.curr.entries.len(), (occupied_slots + 2) as usize);
        assert_eq!(ids.endpoint(x), Some(ep(0)));
        for slot in 1..=occupied_slots {
            assert_eq!(ids.endpoint(x.with(slot)), Some(ep(slot)));
        }
        assert_eq!(ids.endpoint(final_id), Some(ep(node)));
        assert!(ids.pending.is_empty());
    }
}

#[test]
fn resolve_queues_pending_only_for_explicit_collisions() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    open(&mut ids, x, false, 1);
    open(&mut ids, x, false, 2); // auto collision — silent
    assert!(ids.pending.is_empty());

    let y = WidgetId::from_hash("y");
    // First explicit — fast path, no pending.
    let first = ids.resolve(y, true);
    ids.record_endpoint(first, ep(3));
    // Second explicit — collision, queued. record_endpoint will
    // drain it; check it was queued first.
    let second = ids.resolve(y, true);
    assert_eq!(ids.pending.len(), 1);
    assert_eq!(ids.pending[0].first_raw_id, y);
    assert_eq!(ids.pending[0].second_final_id, second.id());
}

#[test]
fn record_endpoint_emits_collision_pair_for_explicit_only() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    // First occurrence resolves + opens.
    let first = ids.resolve(x, true);
    assert!(ids.record_endpoint(first, ep(1)).is_none());
    // Second occurrence resolves + opens — should hand back the pair.
    let second = ids.resolve(x, true);
    let pair = ids
        .record_endpoint(second, ep(2))
        .expect("expected collision pair");
    assert_eq!(pair.first, ep(1));
    assert_eq!(pair.second, ep(2));
    // Pending drained.
    assert!(ids.pending.is_empty());
}

#[test]
fn record_endpoint_no_pair_for_auto_collisions() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    let first = ids.resolve(x, false);
    ids.record_endpoint(first, ep(1));
    let second = ids.resolve(x, false);
    assert!(ids.record_endpoint(second, ep(2)).is_none());
}

#[test]
fn record_endpoint_rejects_duplicate_without_overwriting() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    let resolved = ids.resolve(x, false);
    ids.record_endpoint(resolved, ep(1));

    panic_probe::assert_panics_with("record_endpoint called twice", || {
        ids.record_endpoint(resolved, ep(2))
    });
    assert_eq!(ids.endpoint(x), Some(ep(1)));

    // An id resolved in an earlier pass names an entry this pass may
    // have given to another widget, so recording it is refused.
    let mut ids = SeenIds::default();
    let stale = ids.resolve(x, false);
    ids.pre_record();
    let y = WidgetId::from_hash("y");
    let fresh = ids.resolve(y, false);
    panic_probe::assert_panics_with("which was not resolved this pass", || {
        ids.record_endpoint(stale, ep(1))
    });
    assert_eq!(
        ids.endpoint(y),
        None,
        "the entry the stale id named is untouched"
    );
    assert!(ids.record_endpoint(fresh, ep(2)).is_none());
}

/// Two widgets resolve the same raw auto id before either records — the
/// `.state(ui)` then `.show()` shape. The second is disambiguated against
/// the first's reservation, and both open in either order.
#[test]
fn resolving_twice_before_recording_disambiguates() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    let first = ids.resolve(x, false);
    let second = ids.resolve(x, false);
    assert_eq!(first.id(), x);
    assert_eq!(second.id(), x.with(1));
    assert!(ids.record_endpoint(second, ep(2)).is_none());
    assert!(ids.record_endpoint(first, ep(1)).is_none());

    // An explicit id that is only reserved is its owner claiming it —
    // a widget recording a wrapper under the id it resolved.
    let mut ids = SeenIds::default();
    let owner = ids.resolve(x, false);
    assert_eq!(owner.id(), x);
    assert_eq!(
        ids.resolve(x, true),
        owner,
        "the reservation's owner claims it, entry and all"
    );
    assert!(ids.record_endpoint(owner, ep(1)).is_none());
    // Once recorded, an explicit repeat is a collision as ever.
    assert_eq!(ids.resolve(x, true).id(), x.with(1));

    // A reservation lasts one pass.
    ids.pre_record();
    assert_eq!(ids.resolve(x, false).id(), x);

    // And it is no recording: an id resolved and never shown has no
    // endpoint, is in no frame's recording, and so is never reported
    // removed either.
    let mut ids = SeenIds::default();
    let y = WidgetId::from_hash("y");
    open(&mut ids, x, false, 1);
    assert_eq!(ids.resolve(y, false).id(), y);
    assert_eq!(ids.endpoint(y), None);
    assert!(ids.rollover().is_empty());
    assert_eq!(ids.last_frame().keys().copied().collect::<Vec<_>>(), [x]);
    ids.pre_record();
    open(&mut ids, x, false, 1);
    assert!(ids.rollover().is_empty(), "y was never recorded");
}

#[test]
fn rollover_sweeps_ids_seen_only_in_a_discarded_pass() {
    let mut ids = SeenIds::default();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    // Pass A records a + b, then is discarded by the next
    // pre_record (double-layout / warmup shape).
    open(&mut ids, a, false, 1);
    open(&mut ids, b, false, 2);
    ids.pre_record();
    // Final pass records only a.
    open(&mut ids, a, false, 1);
    let removed = ids.rollover();
    assert!(
        removed.contains(&b),
        "pass-A-only id must be swept or its state rows leak"
    );
    assert!(
        !removed.contains(&a),
        "id re-recorded in the final pass survives"
    );
    // The discarded set drained at rollover: the next frame's diff
    // doesn't resurrect b.
    ids.pre_record();
    open(&mut ids, a, false, 1);
    let removed = ids.rollover();
    assert!(removed.is_empty(), "got {removed:?}");

    // The other path: an id the *previous frame* also recorded, dropped
    // by the settling pass. `discarded` never has to carry it — the
    // prev-minus-curr diff reports exactly this case — so a settling
    // pass over steady widgets adds nothing to the set.
    let c = WidgetId::from_hash("c");
    open(&mut ids, a, false, 1);
    open(&mut ids, c, false, 2);
    ids.rollover();
    open(&mut ids, a, false, 1);
    open(&mut ids, c, false, 2);
    ids.pre_record();
    assert!(
        ids.discarded.is_empty(),
        "ids `prev` already holds must cost no entry, got {:?}",
        ids.discarded
    );
    open(&mut ids, a, false, 1);
    let removed = ids.rollover();
    assert!(
        removed.contains(&c) && !removed.contains(&a),
        "the diff still sweeps the dropped survivor, got {removed:?}"
    );
}

#[test]
fn pre_record_clears_per_frame_state_but_keeps_prev() {
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    // Force `counters` to be non-empty by opening the same id
    // twice (collision path populates it).
    open(&mut ids, x, false, 1);
    open(&mut ids, x, false, 2);
    assert!(!ids.counters.is_empty());

    ids.rollover();
    assert!(ids.curr.entries.is_empty());
    assert_eq!(ids.prev.entries.len(), 2);
    // Counters persist across rollover (rollover is the painted-
    // frame swap; `pre_record` clears per-frame disambiguation
    // state at the next record cycle).
    assert!(!ids.counters.is_empty());

    ids.pre_record();
    assert!(ids.counters.is_empty());
    assert!(ids.curr.entries.is_empty());
    assert_eq!(ids.prev.entries.len(), 2, "prev must survive pre_record");
}

/// One resolve of a frame's script.
#[derive(Clone, Copy, Debug)]
enum Resolve {
    /// [`SeenIds::resolve_auto`] at `SITES[site]` under `PARENTS[parent]`.
    Auto { site: usize, parent: usize },
    /// [`SeenIds::resolve`] of a raw id.
    Raw { raw: WidgetId, explicit: bool },
}

/// The tracker against plain per-pass hash tables ([`reference`]), over
/// random frames: collisions of auto and explicit ids, auto ids from a few
/// call sites under a few parents, ids resolved and recorded out of order
/// or never recorded, and discarded passes. Most
/// frames replay the last one's resolves, some with one change at a
/// random position, so passes run in step, leave it at every position,
/// and never enter it. Every resolve, every collision pair, every pass's
/// recording and endpoints, and every frame's removed set must agree.
#[test]
fn matches_the_per_pass_tables_over_random_frames() {
    let (mut in_step, mut out_of_step) = (0, 0);
    for seed in 1..=8_u64 {
        let mut rng = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let mut next = move |n: usize| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng % n as u64) as usize
        };
        let sites: [&'static Location<'static>; 3] =
            [Location::caller(), Location::caller(), Location::caller()];
        let parents = [
            None,
            Some(WidgetId::from_hash("p")),
            Some(WidgetId::from_hash("q")),
        ];
        let auto_raw = |site: usize, parent: usize| {
            WidgetId::from_location(sites[site]).scoped(parents[parent])
        };
        // The auto ids' raw ids join the universe, so explicit ids collide
        // with auto ones too.
        let universe: Vec<_> = (0..10)
            .map(WidgetId::from_hash)
            .chain((0..3).map(|site| auto_raw(site, site % 3)))
            .collect();
        let probes: Vec<_> = universe
            .iter()
            .flat_map(|&id| [id, id.with(1), id.with(2), id.with(3)])
            .collect();
        let mut ids = SeenIds::default();
        let mut model = reference::Reference::default();
        let mut node = 0;
        let mut script: Vec<Resolve> = Vec::new();
        for frame in 0..300 {
            let resolve_at = |next: &mut dyn FnMut(usize) -> usize| {
                if next(3) == 0 {
                    Resolve::Auto {
                        site: next(sites.len()),
                        parent: next(parents.len()),
                    }
                } else {
                    Resolve::Raw {
                        raw: universe[next(universe.len())],
                        explicit: next(4) == 0,
                    }
                }
            };
            match next(6) {
                0 => script = (0..next(16)).map(|_| resolve_at(&mut next)).collect(),
                1 if !script.is_empty() => {
                    let at = next(script.len());
                    match next(3) {
                        0 => script[at] = resolve_at(&mut next),
                        1 => drop(script.remove(at)),
                        _ => script.insert(at, resolve_at(&mut next)),
                    }
                }
                _ => {}
            }
            let passes = if next(4) == 0 { 2 } else { 1 };
            for pass in 0..passes {
                if frame > 0 || pass > 0 {
                    ids.pre_record();
                    model.pre_record();
                }
                let at = format!("seed {seed} frame {frame} pass {pass}");
                let mut open: Vec<ResolvedId> = Vec::new();
                for &step in &script {
                    let (got, want) = match step {
                        Resolve::Auto { site, parent } => (
                            ids.resolve_auto(sites[site], parents[parent]),
                            model.resolve(auto_raw(site, parent), false),
                        ),
                        Resolve::Raw { raw, explicit } => {
                            (ids.resolve(raw, explicit), model.resolve(raw, explicit))
                        }
                    };
                    assert_eq!(got, want, "resolve, {at}");
                    if !open.contains(&got) {
                        open.push(got);
                    }
                    // Record a random open one, so records come out of order.
                    if !open.is_empty() && next(5) != 0 {
                        let resolved = open.swap_remove(next(open.len()));
                        node += 1;
                        let got = ids.record_endpoint(resolved, ep(node));
                        let want = model.record_endpoint(resolved, ep(node));
                        assert_eq!(
                            got.map(|pair| [pair.first, pair.second]),
                            want.map(|pair| [pair.first, pair.second]),
                            "collision pair, {at}",
                        );
                    }
                }
                // Whatever is still open stays only reserved.
                assert_eq!(ids.recorded().collect::<Vec<_>>(), model.recorded(), "{at}");
                for &id in &probes {
                    assert_eq!(ids.endpoint(id), model.endpoint(id), "endpoint, {at}");
                }
            }
            if ids.split.is_none() {
                in_step += 1;
            } else {
                out_of_step += 1;
            }
            let removed = ids.rollover().clone();
            assert_eq!(
                removed,
                model.rollover(),
                "removed, seed {seed} frame {frame}"
            );
            assert_eq!(
                ids.prev.index.len(),
                ids.prev.entries.len(),
                "`prev`'s index covers its entries and no more, seed {seed} frame {frame}",
            );
        }
    }
    assert!(
        in_step > 500 && out_of_step > 500,
        "both paths run: {in_step} frames in step, {out_of_step} out",
    );
}
