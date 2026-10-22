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
    *map.get_or_insert_with(widget_id(1), || 0u32) = 42;
    *map.get_or_insert_with(widget_id(1), || 0u32) = 43;
    assert_eq!(map.try_get::<u32>(widget_id(1)), Some(&43));
    assert!(map.try_get::<u32>(widget_id(2)).is_none());
}

#[test]
fn init_only_runs_on_first_insert() {
    let mut map = StateMap::default();
    let mut init_calls = 0u32;
    for _ in 0..3 {
        let _ = map.get_or_insert_with(widget_id(1), || {
            init_calls += 1;
            7u32
        });
    }
    assert_eq!(init_calls, 1);
}

#[test]
fn distinct_ids_in_same_store_dont_alias() {
    let mut map = StateMap::default();
    *map.get_or_insert_with(widget_id(1), || 0u32) = 11;
    *map.get_or_insert_with(widget_id(2), || 0u32) = 22;
    assert_eq!(*map.get_or_insert_with(widget_id(1), || 0u32), 11);
    assert_eq!(*map.get_or_insert_with(widget_id(2), || 0u32), 22);
}

#[test]
fn distinct_types_at_distinct_ids_coexist() {
    let mut map = StateMap::default();
    *map.get_or_insert_with(widget_id(1), || 0u32) = 11;
    *map.get_or_insert_with(widget_id(2), String::new) = "hi".into();
    assert_eq!(*map.get_or_insert_with(widget_id(1), || 0u32), 11);
    assert_eq!(map.get_or_insert_with(widget_id(2), String::new), "hi");
}

#[test]
fn sweep_removed_drops_rows() {
    let mut map = StateMap::default();
    *map.get_or_insert_with(widget_id(1), || 0u32) = 99;
    map.sweep_removed(&WidgetIdSet::from_iter([widget_id(1)]));
    assert_eq!(*map.get_or_insert_with(widget_id(1), || 0u32), 0);
}

#[test]
fn sweep_patches_swapped_index() {
    let mut map = StateMap::default();
    *map.get_or_insert_with(widget_id(1), || 0u32) = 1;
    *map.get_or_insert_with(widget_id(2), || 0u32) = 2;
    *map.get_or_insert_with(widget_id(3), || 0u32) = 3;
    // Drop the middle one: `widget_id(3)` moves from idx 2 to 1 and still reads back as 3.
    map.sweep_removed(&WidgetIdSet::from_iter([widget_id(2)]));
    assert_eq!(*map.get_or_insert_with(widget_id(1), || 0u32), 1);
    assert_eq!(*map.get_or_insert_with(widget_id(3), || 0u32), 3);
}
