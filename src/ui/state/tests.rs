use std::rc::Rc;

use crate::internals::panic_probe;
use crate::ui::state::*;

fn widget_id(n: u64) -> WidgetId {
    WidgetId::from_hash(n)
}

#[test]
fn value_persists_across_frames() {
    let mut map = StateMap::default();
    assert!(map.try_get::<u32>(widget_id(1)).is_none());
    assert!(
        map.stores.is_empty(),
        "a missing probe must not create a typed store",
    );
    map.put(widget_id(1), 42u32);
    map.put(widget_id(1), 43u32);
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&43));
    assert!(map.try_get::<u32>(widget_id(2)).is_none());
}

/// Lending moves the value out, with no copy and no default left behind; while it is out, a read
/// and a second lend of the row panic, and another id of the same type stays readable. A first
/// lend makes the row, lent, so a second lend of a new row panics too.
#[test]
fn a_lent_row_refuses_a_read_and_a_second_lend_until_returned() {
    let mut map = StateMap::default();
    let shared = Rc::new(7u32);
    map.put(widget_id(1), Rc::clone(&shared));
    map.put(widget_id(2), Rc::new(9u32));
    let lent = map
        .lend::<Rc<u32>>(widget_id(1))
        .expect("the row holds a value");
    assert_eq!(Rc::strong_count(&shared), 2, "moved, not cloned");
    assert_eq!(map.try_get::<Rc<u32>>(widget_id(2)).map(|v| **v), Some(9));
    panic_probe::assert_panics_with(LENT_ROW, || map.try_get::<Rc<u32>>(widget_id(1)).is_some());
    panic_probe::assert_panics_with(LENT_ROW, || map.lend::<Rc<u32>>(widget_id(1)).is_some());
    map.put(widget_id(1), lent);
    assert_eq!(map.try_get::<Rc<u32>>(widget_id(1)).map(|v| **v), Some(7));

    assert!(map.lend::<Rc<u32>>(widget_id(3)).is_none(), "a new row");
    panic_probe::assert_panics_with(LENT_ROW, || map.try_get::<Rc<u32>>(widget_id(3)).is_some());
    panic_probe::assert_panics_with(LENT_ROW, || map.lend::<Rc<u32>>(widget_id(3)).is_some());
    map.put(widget_id(3), Rc::new(5u32));
    assert_eq!(map.try_get::<Rc<u32>>(widget_id(3)).map(|v| **v), Some(5));
}

#[test]
fn distinct_ids_in_same_store_dont_alias() {
    let mut map = StateMap::default();
    map.put(widget_id(1), 11u32);
    map.put(widget_id(2), 22u32);
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&11));
    assert_eq!(map.try_get::<u32>(widget_id(2)), Some(&22));
}

#[test]
fn distinct_types_at_distinct_ids_coexist() {
    let mut map = StateMap::default();
    map.put(widget_id(1), 11u32);
    map.put(widget_id(2), String::from("hi"));
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&11));
    assert_eq!(
        map.try_get::<String>(widget_id(2)).map(String::as_str),
        Some("hi")
    );
}

#[test]
fn sweep_removed_drops_rows() {
    let mut map = StateMap::default();
    map.put(widget_id(1), 99u32);
    map.sweep_removed(&WidgetIdSet::from_iter([widget_id(1)]));
    assert!(map.try_get::<u32>(widget_id(1)).is_none());
}

#[test]
fn sweep_patches_swapped_index() {
    let mut map = StateMap::default();
    for n in 1..=3 {
        map.put(widget_id(n), n as u32);
    }
    // Drop the middle one: `widget_id(3)` moves from idx 2 to 1 and still reads back as 3.
    map.sweep_removed(&WidgetIdSet::from_iter([widget_id(2)]));
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&1));
    assert_eq!(map.try_get::<u32>(widget_id(3)), Some(&3));
}

/// `set` replaces a row's value or makes the row, and `remove` hands the value back and drops the
/// row, patching the neighbour it swaps in, as a sweep does; a removed row reads `None`, and a
/// remove of no row is `None`. Both refuse a lent row, as its lend's `put` would undo them.
#[test]
fn set_and_remove_replace_and_drop_a_row() {
    let mut map = StateMap::default();
    for n in 1..=3 {
        map.set(widget_id(n), n as u32);
    }
    map.set(widget_id(1), 10u32);
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&10));
    assert_eq!(map.remove::<u32>(widget_id(2)), Some(2));
    assert_eq!(map.try_get::<u32>(widget_id(2)), None);
    assert_eq!(
        map.try_get::<u32>(widget_id(3)),
        Some(&3),
        "the swapped row"
    );
    assert_eq!(map.remove::<u32>(widget_id(2)), None);
    assert_eq!(
        map.remove::<String>(widget_id(1)),
        None,
        "no store of the type"
    );

    let lent = map.lend::<u32>(widget_id(1));
    panic_probe::assert_panics_with(LENT_ROW, || map.set(widget_id(1), 0u32));
    panic_probe::assert_panics_with(LENT_ROW, || map.remove::<u32>(widget_id(1)).is_some());
    map.put(widget_id(1), lent.expect("the row held a value"));
}
