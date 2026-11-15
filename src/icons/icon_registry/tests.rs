//! Loading, sharing and unloading a set: the slot lifecycle an [`IconSet`] drives.

use super::*;
use crate::common::span::Span;
use crate::icons::icon_table::IconDefinition;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::icon_id::IconId;
use std::borrow::Cow;
use std::collections;

const A_ICONS: &[IconDefinition] = &[IconDefinition {
    name: Cow::Borrowed("a"),
    view_box: Size::new(24.0, 24.0),
    svg: Span::new(0, 1),
    tintable: true,
}];
const B_ICONS: &[IconDefinition] = &[IconDefinition {
    name: Cow::Borrowed("b"),
    view_box: Size::new(16.0, 16.0),
    svg: Span::new(0, 1),
    tintable: false,
}];

fn a() -> IconTable {
    IconTable::baked(A_ICONS, b"a")
}
fn b() -> IconTable {
    IconTable::baked(B_ICONS, b"b")
}

fn drain(reg: &IconRegistry) -> Vec<IconSetId> {
    let mut freed = Vec::new();
    reg.drain_released(|ids| freed.extend_from_slice(ids));
    freed
}

/// Each load is a set of its own, as each image load is an image, identical tables included: two
/// slots, each resolving its own table.
#[test]
fn each_load_takes_a_set_of_its_own() {
    let reg = IconRegistry::default();
    let set_a = reg.register(a());
    let set_b = reg.register(b());
    let twin = reg.register(a());
    let ids = [&set_a, &set_b, &twin].map(|set| set.handle(IconId::new(0)).icon.set);
    assert_eq!(
        ids,
        [
            IconSetId::new(0, 0),
            IconSetId::new(1, 0),
            IconSetId::new(2, 0)
        ]
    );
    assert_eq!(reg.resident_ids(), ids);
    assert_eq!(reg.get(ids[0]).icons()[0].name, "a");
    assert_eq!(reg.get(ids[1]).icons()[0].name, "b");
    assert_eq!(reg.get(ids[2]).icons()[0].name, "a");
    drop((set_a, set_b, twin));
}

#[test]
fn clones_of_the_registry_share_one_table() {
    let reg = IconRegistry::default();
    let clone = reg.clone();
    let set = reg.register(a());
    let id = set.handle(IconId::new(0)).icon.set;
    assert_eq!(
        clone.resident_ids(),
        vec![id],
        "the backend's clone sees the load"
    );
    assert_eq!(clone.get(id).icons()[0].view_box, Size::new(24.0, 24.0));
}

#[test]
fn dropping_the_last_set_frees_its_slot_for_the_next_load() {
    let reg = IconRegistry::default();
    let set = reg.register(a());
    let id = set.handle(IconId::new(0)).icon.set;

    let clone = set.clone();
    drop(set);
    assert!(drain(&reg).is_empty());
    assert_eq!(reg.resident_ids(), vec![id]);

    drop(clone);
    assert!(
        reg.resident_ids().is_empty(),
        "a released set is not resident",
    );
    assert_eq!(
        reg.get(id).icons()[0].name,
        "a",
        "but the slot still holds the atlas until the drain frees it",
    );
    assert_eq!(drain(&reg), vec![id], "and the drain reports it once");
    assert!(reg.resident_ids().is_empty(), "the slot is empty now");
    assert!(drain(&reg).is_empty(), "a drained release does not repeat");

    let next = reg.register(b());
    let next_id = next.handle(IconId::new(0)).icon.set;
    assert_eq!(next_id, IconSetId::new(0, 1), "same slot, next generation");
    assert_ne!(next_id, id);
    assert_eq!(reg.get(next_id).icons()[0].name, "b");
}

/// A handle minted before a release must not resolve to whatever later took the slot.
#[test]
#[should_panic(expected = "is not loaded")]
fn a_handle_outliving_its_set_panics_instead_of_naming_the_slot_s_new_owner() {
    let reg = IconRegistry::default();
    let set = reg.register(a());
    let stale = set.handle(IconId::new(0));
    drop(set);
    drain(&reg);

    let replacement = reg.register(b());
    assert_eq!(
        replacement.handle(IconId::new(0)).icon.set,
        IconSetId::new(0, 1),
        "the replacement took the same slot, which is what makes this a trap",
    );
    reg.get(stale.icon.set);
}

/// A fresh atlas per frame cycles two slots instead of growing: the new set registers before the old one drops.
#[test]
fn loading_a_fresh_atlas_every_frame_cycles_a_fixed_pair_of_slots() {
    let reg = IconRegistry::default();
    let mut slots_used = collections::BTreeSet::new();
    let mut held: Option<IconSet> = None;
    let mut last = IconSetId::new(0, 0);
    for frame in 0..64u16 {
        let next = reg.register(a());
        last = next.handle(IconId::new(0)).icon.set;
        slots_used.insert(last.slot());
        held = Some(next);
        reg.drain_released(|_| {});
        assert_eq!(
            reg.resident_ids().len(),
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
    assert_eq!(last, IconSetId::new(1, 31));
}

/// Sets released between two drains are reported in one call, not one per id.
#[test]
fn one_drain_reports_every_set_released_since_the_last() {
    let reg = IconRegistry::default();
    let (first, second, held) = (reg.register(a()), reg.register(b()), reg.register(a()));
    let ids = [
        first.handle(IconId::new(0)).icon.set,
        second.handle(IconId::new(0)).icon.set,
    ];
    drop((first, second));

    let mut batches: Vec<Vec<IconSetId>> = Vec::new();
    reg.drain_released(|released| batches.push(released.to_vec()));
    assert_eq!(batches, vec![ids.to_vec()], "one call, both ids, in order");
    assert_eq!(reg.resident_ids().len(), 1, "the held set stayed");

    // Nothing released: the closure never runs, keeping the every-frame drain free.
    let mut ran = false;
    reg.drain_released(|_| ran = true);
    assert!(!ran, "an empty drain must not call back");
    drop(held);
}

/// `Debug` summarizes the set: a derived one would reach every loaded set's bytes via the registry.
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
