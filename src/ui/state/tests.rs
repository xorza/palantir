use std::rc::Rc;

use crate::ui::state::*;

fn widget_id(n: u64) -> WidgetId {
    WidgetId::from_hash(n)
}

#[test]
fn value_persists_across_frames() {
    let mut map = StateMap::default();
    assert!(map.try_get::<u32>(widget_id(1)).is_none());
    assert!(map.take::<u32>(widget_id(1)).is_none());
    assert!(
        map.stores.is_empty(),
        "a missing probe must not create a typed store",
    );
    map.put(widget_id(1), 42u32);
    map.put(widget_id(1), 43u32);
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&43));
    assert!(map.try_get::<u32>(widget_id(2)).is_none());
}

/// A lent row reads empty until it is returned, and lending moves the value out: no copy and no
/// default is left behind.
#[test]
fn a_lent_row_is_empty_until_returned() {
    let mut map = StateMap::default();
    let shared = Rc::new(7u32);
    map.put(widget_id(1), Rc::clone(&shared));
    let lent = map
        .take::<Rc<u32>>(widget_id(1))
        .expect("the row holds a value");
    assert!(map.try_get::<Rc<u32>>(widget_id(1)).is_none());
    assert!(map.take::<Rc<u32>>(widget_id(1)).is_none(), "lent once");
    assert_eq!(Rc::strong_count(&shared), 2, "moved, not cloned");
    map.put(widget_id(1), lent);
    assert_eq!(map.try_get::<Rc<u32>>(widget_id(1)).map(|v| **v), Some(7));
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
