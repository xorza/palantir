use super::Rect;

// --- intersects ---------------------------------------------------------

#[test]
fn intersects_overlapping() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    assert!(a.intersects(b));
    assert!(b.intersects(a));
}

#[test]
fn intersects_disjoint() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(20.0, 20.0, 5.0, 5.0);
    assert!(!a.intersects(b));
}

#[test]
fn intersects_touching_edges_does_not_count() {
    // Strict overlap — touching edges (a's right == b's left) is not
    // an intersection. Damage filter leans on this so a node whose
    // left edge sits exactly on the damage rect's right edge gets
    // skipped.
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(10.0, 0.0, 10.0, 10.0);
    assert!(!a.intersects(b));
}

#[test]
fn intersects_self_with_self() {
    let r = Rect::new(2.0, 3.0, 4.0, 5.0);
    assert!(r.intersects(r));
}

#[test]
fn intersects_zero_sized_never_overlaps() {
    let a = Rect::new(0.0, 0.0, 0.0, 0.0);
    let b = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert!(!a.intersects(b));
}

// --- union --------------------------------------------------------------

#[test]
fn union_overlapping_rects_returns_bounding_box() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 10.0, 10.0);
    let u = a.union(b);
    assert_eq!(u, Rect::new(0.0, 0.0, 15.0, 15.0));
}

#[test]
fn union_disjoint_rects_returns_enclosing_rect() {
    let a = Rect::new(0.0, 0.0, 5.0, 5.0);
    let b = Rect::new(10.0, 10.0, 5.0, 5.0);
    let u = a.union(b);
    assert_eq!(u, Rect::new(0.0, 0.0, 15.0, 15.0));
}

#[test]
fn union_with_self_is_self() {
    let r = Rect::new(2.0, 3.0, 4.0, 5.0);
    assert_eq!(r.union(r), r);
}

#[test]
fn union_is_commutative() {
    let a = Rect::new(1.0, 2.0, 3.0, 4.0);
    let b = Rect::new(7.0, 8.0, 5.0, 6.0);
    assert_eq!(a.union(b), b.union(a));
}

// --- area ---------------------------------------------------------------

#[test]
fn area_basic() {
    assert_eq!(Rect::new(0.0, 0.0, 4.0, 5.0).area(), 20.0);
}

#[test]
fn area_zero_rect() {
    assert_eq!(Rect::ZERO.area(), 0.0);
}
