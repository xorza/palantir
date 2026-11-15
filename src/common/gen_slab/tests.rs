use crate::common::gen_slab::{GenSlab, SlabKey};

/// A key resolves to its value until the value is removed, and never after, even once its slot
/// holds another: `a` and `b` fill slots 0 and 1; removing `a` frees slot 0 and moves its
/// generation to 1, so `c` takes slot 0 at generation 1 and `a` resolves to nothing.
#[test]
fn a_removed_key_never_resolves_again() {
    let mut slab = GenSlab::new();
    let a = slab.insert("a");
    let b = slab.insert("b");
    assert_eq!(
        (a, b),
        (
            SlabKey {
                idx: 0,
                generation: 0
            },
            SlabKey {
                idx: 1,
                generation: 0
            }
        )
    );
    assert_eq!(slab.remove(a), Some("a"));
    assert_eq!(slab.remove(a), None, "a removed key removes nothing");
    let c = slab.insert("c");
    assert_eq!(
        c,
        SlabKey {
            idx: 0,
            generation: 1
        },
        "the freed slot, a generation on"
    );
    assert_eq!(slab.get(a), None, "the old key misses the new value");
    assert_eq!(slab.get_mut(a), None);
    assert_eq!(slab.get(c), Some(&"c"));
    assert_eq!(slab.get(b), Some(&"b"));
    *slab.get_mut(b).unwrap() = "B";
    assert_eq!(slab.get(b), Some(&"B"));
    let past = SlabKey {
        idx: 9,
        generation: 0,
    };
    assert_eq!(slab.get(past), None, "a slot past the last");
    assert_eq!(slab.remove(past), None);
}

/// A walk by slot index sees the vacant slots as vacant: with slot 1 of three freed, the slots
/// count three, slot 1 has no key and no value, and the iterators skip it.
#[test]
fn a_walk_by_slot_skips_the_vacant_ones() {
    let mut slab = GenSlab::new();
    let keys = [slab.insert(10), slab.insert(11), slab.insert(12)];
    slab.remove(keys[1]);
    assert_eq!(slab.slot_count(), 3);
    assert_eq!(slab.key_at(0), Some(keys[0]));
    assert_eq!(slab.key_at(1), None);
    assert_eq!(slab.key_at(3), None);
    assert_eq!(slab.at(1), None);
    assert_eq!(slab.at(2), Some(&12));
    *slab.at_mut(2).unwrap() = 22;
    assert_eq!(slab.at_mut(1), None);
    let walked: Vec<_> = (&slab)
        .into_iter()
        .map(|(key, &value)| (key, value))
        .collect();
    assert_eq!(walked, [(keys[0], 10), (keys[2], 22)]);
    assert_eq!(slab.values().copied().collect::<Vec<_>>(), [10, 22]);
}

/// A value built from its key takes that key, the last slot freed first, and a build that fails
/// files nothing: with slot 0 freed, a failed build leaves it vacant, and the next takes slot 0 at
/// generation 1, its value holding that key; with none free, the next takes a new slot.
#[test]
fn a_value_built_from_its_key_takes_it() {
    let mut slab = GenSlab::new();
    let first = slab.insert(SlabKey {
        idx: 0,
        generation: 0,
    });
    slab.remove(first);
    assert_eq!(slab.try_insert_with_key(|_| Err("full")), Err("full"));
    assert_eq!(slab.key_at(0), None, "a failed build files nothing");
    let reused = slab.try_insert_with_key(Ok::<_, ()>).unwrap();
    assert_eq!(
        reused,
        SlabKey {
            idx: 0,
            generation: 1
        }
    );
    assert_eq!(slab.get(reused), Some(&reused));
    let fresh = slab.try_insert_with_key(Ok::<_, ()>).unwrap();
    assert_eq!(
        fresh,
        SlabKey {
            idx: 1,
            generation: 0
        }
    );
    assert_eq!(slab.get(fresh), Some(&fresh));
}
