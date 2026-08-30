use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::seen_ids::*;
use crate::scene::tree::node_id::NodeId;

fn ep(node: u32) -> Endpoint {
    Endpoint {
        layer: Layer::Main,
        node: NodeId(node),
    }
}

/// Stand-in for the production `resolve → record_endpoint`
/// pairing every widget does (`Ui::widget` →
/// `scene::open_node`). The lazy-counter fast path in `resolve`
/// depends on `curr` being populated between consecutive resolves
/// of the same raw id, so tests interleave them the same way.
fn open(ids: &mut SeenIds, raw_id: WidgetId, is_explicit: bool, node: u32) -> WidgetId {
    let final_id = ids.resolve(raw_id, is_explicit);
    ids.record_endpoint(final_id, ep(node));
    final_id
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
        assert_eq!(ids.curr.len(), (occupied_slots + 2) as usize);
        assert_eq!(ids.curr[&x], ep(0));
        for slot in 1..=occupied_slots {
            assert_eq!(ids.curr[&x.with(slot)], ep(slot));
        }
        assert_eq!(ids.curr[&final_id], ep(node));
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
    ids.resolve(y, true);
    ids.record_endpoint(y, ep(3));
    // Second explicit — collision, queued. record_endpoint will
    // drain it; check it was queued first.
    let second = ids.resolve(y, true);
    assert_eq!(ids.pending.len(), 1);
    assert_eq!(ids.pending[0].first_raw_id, y);
    assert_eq!(ids.pending[0].second_final_id, second);
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
    ids.record_endpoint(x, ep(1));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ids.record_endpoint(x, ep(2));
    }));

    assert!(result.is_err());
    assert_eq!(ids.curr[&x], ep(1));
}

#[test]
#[should_panic(expected = "recording order violated")]
fn record_endpoint_panics_if_first_endpoint_missing() {
    // Manually queue a pending collision whose first raw id was
    // never recorded — bypasses the production resolve+record
    // pairing to simulate the contract violation. The expect in
    // `record_endpoint` must fire — the alternative is a silent
    // miss that hides a recording-order bug from the magenta
    // collision overlay.
    let mut ids = SeenIds::default();
    let x = WidgetId::from_hash("x");
    let second = x.with(1);
    ids.pending.push(PendingExplicitCollision {
        first_raw_id: x,
        second_final_id: second,
    });
    ids.record_endpoint(second, ep(2));
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
    assert!(ids.curr.is_empty());
    assert_eq!(ids.prev.len(), 2);
    // Counters persist across rollover (rollover is the painted-
    // frame swap; `pre_record` clears per-frame disambiguation
    // state at the next record cycle).
    assert!(!ids.counters.is_empty());

    ids.pre_record();
    assert!(ids.counters.is_empty());
    assert!(ids.curr.is_empty());
    assert_eq!(ids.prev.len(), 2, "prev must survive pre_record");
}
