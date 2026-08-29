//! Loading, sharing and unloading a set — the slot lifecycle an
//! [`IconSet`] drives.

use super::*;
use crate::icons::icon_table::{IconDef, IconId};
use crate::primitives::span::Span;
use glam::Vec2;

const A_ICONS: &[IconDef] = &[IconDef {
    name: "a",
    view_box: Vec2::splat(24.0),
    svg: Span::new(0, 1),
    tintable: true,
    filtered: false,
}];
const B_ICONS: &[IconDef] = &[IconDef {
    name: "b",
    view_box: Vec2::splat(16.0),
    svg: Span::new(0, 1),
    tintable: false,
    filtered: true,
}];

fn a() -> Rc<IconTable> {
    Rc::new(IconTable::baked(A_ICONS, b"a"))
}
fn b() -> Rc<IconTable> {
    Rc::new(IconTable::baked(B_ICONS, b"b"))
}

/// Ids of the sets currently resident, in slot order.
fn resident(reg: &IconRegistry) -> Vec<IconSetId> {
    (0..reg.slot_count())
        .filter_map(|slot| Some(reg.resident(slot)?.id))
        .collect()
}

/// Drain with no backend to notify, answering which ids were freed.
fn drain(reg: &IconRegistry) -> Vec<IconSetId> {
    let mut freed = Vec::new();
    reg.drain_released(|ids| freed.extend_from_slice(ids));
    freed
}

#[test]
fn reregistering_a_held_set_shares_one_owner_rather_than_taking_a_slot() {
    let reg = IconRegistry::default();
    let (data_a, data_b) = (a(), b());
    let set_a = reg.register(Rc::clone(&data_a));
    let set_b = reg.register(Rc::clone(&data_b));
    assert_eq!(
        resident(&reg),
        vec![IconSetId::new(0, 0), IconSetId::new(1, 0)]
    );

    // The same allocation again: same id, no second entry. An
    // immediate-mode caller loading every frame must not grow the table.
    let again = reg.register(Rc::clone(&data_a));
    assert_eq!(again.handle(IconId(0)), set_a.handle(IconId(0)));
    assert_eq!(resident(&reg).len(), 2);

    // And it must share the *owner*, not just the id — two independent
    // owners of one slot would free it on the first drop, leaving the
    // second naming a slot the registry had already handed away.
    drop(again);
    assert!(drain(&reg).is_empty(), "a live clone still holds the set");
    assert_eq!(
        reg.get(set_a.handle(IconId(0)).icon.set).icons()[0].name,
        "a"
    );

    // A separate allocation over identical data is a separate set: the
    // registry keys on identity, not on contents.
    let twin = reg.register(a());
    assert_eq!(resident(&reg).len(), 3);
    drop((set_a, set_b, twin));
}

#[test]
fn clones_of_the_registry_share_one_table() {
    let reg = IconRegistry::default();
    let clone = reg.clone();
    let set = reg.register(a());
    let id = set.handle(IconId(0)).icon.set;
    assert_eq!(
        resident(&clone),
        vec![id],
        "the backend's clone sees the load"
    );
    assert_eq!(clone.get(id).icons()[0].view_box, Vec2::splat(24.0));
}

/// The whole point of the change: the last `IconSet` going away is what
/// unloads the set, and the slot it held comes back for the next one.
#[test]
fn dropping_the_last_set_frees_its_slot_for_the_next_load() {
    let reg = IconRegistry::default();
    let set = reg.register(a());
    let id = set.handle(IconId(0)).icon.set;
    let loaded_epoch = reg.epoch();

    // A live clone holds it: nothing is queued.
    let clone = set.clone();
    drop(set);
    assert!(drain(&reg).is_empty());
    assert_eq!(resident(&reg), vec![id]);

    // The last one is what releases.
    drop(clone);
    assert!(
        reg.epoch() > loaded_epoch,
        "a release moves the epoch as much as a load does",
    );
    assert_eq!(
        resident(&reg),
        vec![id],
        "the slot still holds the atlas until the drain frees it",
    );
    assert_eq!(drain(&reg), vec![id], "and the drain reports it once");
    assert!(resident(&reg).is_empty(), "the slot is empty now");
    assert!(drain(&reg).is_empty(), "a drained release does not repeat");

    // The freed slot is reused, with a generation that tells the two
    // occupants apart.
    let next = reg.register(b());
    let next_id = next.handle(IconId(0)).icon.set;
    assert_eq!(next_id, IconSetId::new(0, 1), "same slot, next generation");
    assert_ne!(next_id, id);
    assert_eq!(reg.get(next_id).icons()[0].name, "b");
}

/// The reason the slot carries a generation at all: an `IconHandle` is
/// `Copy` and owns nothing, so one minted before a release outlives its
/// set. It must not resolve to whatever took the slot.
#[test]
#[should_panic(expected = "is not loaded")]
fn a_handle_outliving_its_set_panics_instead_of_naming_the_slot_s_new_owner() {
    let reg = IconRegistry::default();
    let set = reg.register(a());
    let stale = set.handle(IconId(0));
    drop(set);
    drain(&reg);

    let replacement = reg.register(b());
    assert_eq!(
        replacement.handle(IconId(0)).icon.set,
        IconSetId::new(0, 1),
        "the replacement took the same slot, which is what makes this a trap",
    );
    reg.get(stale.icon.set);
}

/// The leak this whole mechanism exists for: a caller that builds a fresh
/// atlas inside its frame closure loads a set per frame. Each is released
/// as the previous `IconSet` drops, so the table cycles a fixed pair of
/// slots instead of growing until the slot index overflows.
///
/// A *pair* rather than one, and that is the frame order rather than a
/// slack allowance: the new set is registered before the closure's local
/// goes out of scope, so the two overlap for the length of the
/// assignment. The peak is the live set plus one frame's release, which is
/// the same bound the paint-snapshot arena settles at for the same reason.
#[test]
fn loading_a_fresh_atlas_every_frame_cycles_a_fixed_pair_of_slots() {
    let reg = IconRegistry::default();
    let mut slots_used = std::collections::BTreeSet::new();
    let mut held: Option<IconSet> = None;
    let mut last = IconSetId::new(0, 0);
    for frame in 0..64u16 {
        let next = reg.register(a());
        last = next.handle(IconId(0)).icon.set;
        slots_used.insert(last.slot);
        // Assigning is what drops the previous frame's set.
        held = Some(next);
        reg.drain_released(|_| {});
        assert_eq!(
            resident(&reg).len(),
            1,
            "frame {frame} left more than one set resident",
        );
    }
    drop(held);
    assert_eq!(
        slots_used.into_iter().collect::<Vec<_>>(),
        vec![0, 1],
        "64 frames of loading must not consume 64 slot indices",
    );
    // Slots alternate, so each took 32 turns and the last is the 32nd of
    // slot 1 — generations 0..=31.
    assert_eq!(last, IconSetId::new(1, 31));
}

/// Several sets released between two drains are reported in one call,
/// not one call each. The backend's stores are keyed on the set, so
/// finding a doomed family in either means walking all of it — a
/// per-id callback made unloading N sets cost N walks of each.
#[test]
fn one_drain_reports_every_set_released_since_the_last() {
    let reg = IconRegistry::default();
    let (first, second, held) = (reg.register(a()), reg.register(b()), reg.register(a()));
    let ids = [
        first.handle(IconId(0)).icon.set,
        second.handle(IconId(0)).icon.set,
    ];
    drop((first, second));

    let mut batches: Vec<Vec<IconSetId>> = Vec::new();
    reg.drain_released(|released| batches.push(released.to_vec()));
    assert_eq!(batches, vec![ids.to_vec()], "one call, both ids, in order");
    assert_eq!(resident(&reg).len(), 1, "the held set stayed");

    // Nothing released: the closure is not run at all, which is what
    // keeps the every-frame drain free.
    let mut ran = false;
    reg.drain_released(|_| ran = true);
    assert!(!ran, "an empty drain must not call back");
    drop(held);
}

/// A public handle must not print the host's whole icon table. The token
/// holds the registry so it can queue its own release, which makes a
/// derived `Debug` reach every other loaded set's bytes.
#[test]
fn debug_summarizes_the_set_rather_than_dumping_the_registry() {
    let reg = IconRegistry::default();
    let set = reg.register(a());
    let held = set.clone();
    let _other = reg.register(b());

    let shown = format!("{set:?}");
    assert_eq!(
        shown,
        "IconSet { id: IconSetId { slot: 0, generation: 0 }, icons: 1, owners: 2 }",
    );
    drop(held);
    assert!(format!("{set:?}").contains("owners: 1"), "owners is live");
}
