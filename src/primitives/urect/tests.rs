use super::URect;

// --- intersect ---------------------------------------------------------

#[test]
fn intersect_overlapping() {
    let a = URect::new(0, 0, 10, 10);
    let b = URect::new(5, 5, 10, 10);
    assert_eq!(a.intersect(b), Some(URect::new(5, 5, 5, 5)));
}

#[test]
fn intersect_disjoint_returns_none() {
    let a = URect::new(0, 0, 10, 10);
    let b = URect::new(20, 20, 5, 5);
    assert_eq!(a.intersect(b), None);
}

#[test]
fn intersect_touching_edges_returns_none() {
    // Strict overlap. Mirror of `Rect::intersects`.
    let a = URect::new(0, 0, 10, 10);
    let b = URect::new(10, 0, 10, 10);
    assert_eq!(a.intersect(b), None);
}

#[test]
fn intersect_contained_returns_inner() {
    let outer = URect::new(0, 0, 100, 100);
    let inner = URect::new(20, 30, 10, 10);
    assert_eq!(outer.intersect(inner), Some(inner));
    assert_eq!(inner.intersect(outer), Some(inner));
}

#[test]
fn intersect_self_with_self() {
    let r = URect::new(5, 7, 11, 13);
    assert_eq!(r.intersect(r), Some(r));
}

// --- clamp_to ---------------------------------------------------------

#[test]
fn clamp_to_inside_parent_returns_self() {
    let parent = URect::new(0, 0, 100, 100);
    let me = URect::new(20, 30, 10, 10);
    assert_eq!(me.clamp_to(parent), me);
}

#[test]
fn clamp_to_overlapping_parent_clips_to_overlap() {
    let parent = URect::new(0, 0, 50, 50);
    let me = URect::new(40, 40, 30, 30);
    assert_eq!(me.clamp_to(parent), URect::new(40, 40, 10, 10));
}

#[test]
fn clamp_to_disjoint_returns_zero_sized() {
    let parent = URect::new(0, 0, 10, 10);
    let me = URect::new(20, 20, 5, 5);
    let r = me.clamp_to(parent);
    assert!(r.is_empty());
}

// --- area / is_empty ---------------------------------------------------

#[test]
fn area_basic() {
    assert_eq!(URect::new(0, 0, 4, 5).area(), 20);
}

#[test]
fn is_empty_zero_dim() {
    assert!(URect::new(0, 0, 0, 5).is_empty());
    assert!(URect::new(0, 0, 5, 0).is_empty());
    assert!(!URect::new(0, 0, 1, 1).is_empty());
}

#[test]
fn max_x_max_y() {
    let r = URect::new(2, 3, 4, 5);
    assert_eq!(r.max_x(), 6);
    assert_eq!(r.max_y(), 8);
}

// --- const fn callable in const context --------------------------------

/// Compile-time check that `URect`'s `const fn`s can be evaluated in
/// const context. If any of these stop being `const`, the file stops
/// compiling — that's the assertion. No runtime checks needed.
#[test]
#[allow(dead_code)]
fn const_constructors_compile() {
    const _A: URect = URect::new(1, 2, 3, 4);
    const _ZERO: URect = URect::ZERO;
    const _AREA: u32 = _A.area();
    const _EMPTY: bool = _ZERO.is_empty();
    const _MAXX: u32 = _A.max_x();
    const _MAXY: u32 = _A.max_y();
    const _CLAMP: URect = _A.clamp_to(_ZERO);
    const _INTER: Option<URect> = _A.intersect(_ZERO);
}
