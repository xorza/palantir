//! Pin: a panel's arranged rect is at least as large as the union of
//! its children's rects on each axis (modulo negative margins, not
//! tested here). The rule is: `Fill` and `Hug` parents grow to contain
//! their children even when the parent's available space is smaller
//! than the children's measured size; `Fixed` is a hard contract and
//! does *not* grow. The root rect grows past the surface for the same
//! reason.
//!
//! Anti-regression for "showcase window resized small → parent panel
//! ends up smaller than its child."

use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

fn assert_contains(parent: Rect, child: Rect, label: &str) {
    assert!(
        parent.size.w + 0.5 >= child.size.w,
        "{label}: parent.w={} < child.w={}",
        parent.size.w,
        child.size.w,
    );
    assert!(
        parent.size.h + 0.5 >= child.size.h,
        "{label}: parent.h={} < child.h={}",
        parent.size.h,
        child.size.h,
    );
    assert!(
        parent.min.x <= child.min.x + 0.5
            && child.min.x + child.size.w <= parent.min.x + parent.size.w + 0.5,
        "{label}: child x-span [{}, {}] outside parent [{}, {}]",
        child.min.x,
        child.min.x + child.size.w,
        parent.min.x,
        parent.min.x + parent.size.w,
    );
    assert!(
        parent.min.y <= child.min.y + 0.5
            && child.min.y + child.size.h <= parent.min.y + parent.size.h + 0.5,
        "{label}: child y-span [{}, {}] outside parent [{}, {}]",
        child.min.y,
        child.min.y + child.size.h,
        parent.min.y,
        parent.min.y + parent.size.h,
    );
}

/// Cramped surface, Fill parent with a Fixed child wider than the
/// surface: parent grows past the surface to contain the child.
#[test]
fn fill_hstack_grows_past_surface_to_contain_fixed_child() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let mut child_node = None;
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            child_node = Some(
                Frame::new()
                    .size((Sizing::Fixed(300.0), Sizing::Fixed(50.0)))
                    .show(ui)
                    .node,
            );
        })
        .node;
    ui.end_frame();

    let p = ui.layout_engine.result.rect[parent.index()];
    let c = ui.layout_engine.result.rect[child_node.unwrap().index()];

    assert_eq!(c.size.w, 300.0, "Fixed child stays Fixed");
    assert!(
        p.size.w >= 300.0,
        "Fill parent should grow to contain 300-wide child; got w={}",
        p.size.w,
    );
    assert_contains(p, c, "fill_hstack/fixed_child");
}

/// `Fixed` parent is a hard contract: it does *not* grow even when its
/// child is bigger. The child overflows. This documents the explicit
/// carve-out from the parent-≥-child rule. The Fixed panel here is a
/// *non-root* node so its slot follows from its declared size, not the
/// surface (the root special-case grows past the surface to contain
/// its measured content; see `root_grows_past_surface_*`).
#[test]
fn fixed_parent_does_not_grow_for_oversized_child() {
    let mut ui = ui_at(UVec2::new(800, 600));
    let mut fixed_panel = None;
    let mut child_node = None;
    Panel::vstack().show(&mut ui, |ui| {
        fixed_panel = Some(
            Panel::hstack()
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui, |ui| {
                    child_node = Some(
                        Frame::new()
                            .size((Sizing::Fixed(300.0), Sizing::Fixed(300.0)))
                            .show(ui)
                            .node,
                    );
                })
                .node,
        );
    });
    ui.end_frame();

    let p = ui.layout_engine.result.rect[fixed_panel.unwrap().index()];
    let c = ui.layout_engine.result.rect[child_node.unwrap().index()];

    assert_eq!(p.size.w, 50.0, "Fixed parent stays at its declared size");
    assert_eq!(p.size.h, 50.0);
    assert_eq!(c.size.w, 300.0, "Fixed child still measured at its size");
    assert_eq!(c.size.h, 300.0);
}

/// Two Fill children inside a Fill parent in a cramped surface: each
/// child's MinContent floor (Fixed siblings here) sums past the parent's
/// available width. Parent grows; arranged inner span fits both
/// children.
#[test]
fn fill_hstack_grows_for_oversized_fixed_siblings() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let mut a_node = None;
    let mut b_node = None;
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            a_node = Some(
                Frame::new()
                    .with_id("a")
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
            b_node = Some(
                Frame::new()
                    .with_id("b")
                    .size((Sizing::Fixed(150.0), Sizing::Fixed(40.0)))
                    .show(ui)
                    .node,
            );
        })
        .node;
    ui.end_frame();

    let p = ui.layout_engine.result.rect[parent.index()];
    let a = ui.layout_engine.result.rect[a_node.unwrap().index()];
    let b = ui.layout_engine.result.rect[b_node.unwrap().index()];

    assert!(
        p.size.w >= 350.0,
        "Fill parent should grow to contain 200+150 children; got w={}",
        p.size.w,
    );
    assert_eq!(a.size.w, 200.0);
    assert_eq!(b.size.w, 150.0);
    assert_contains(p, a, "fill_hstack/sibling_a");
    assert_contains(p, b, "fill_hstack/sibling_b");
    assert!(
        b.min.x >= a.min.x + a.size.w - 0.5,
        "siblings should not overlap; a.end={} b.start={}",
        a.min.x + a.size.w,
        b.min.x,
    );
}

/// Nested case: Fill parent containing a Hug panel that contains a
/// Fixed wide child. Both wrappers grow; the parent's rect contains the
/// inner panel's rect, which contains the leaf.
#[test]
fn nested_fill_hug_grows_through_intermediate_panel() {
    let mut ui = ui_at(UVec2::new(80, 200));
    let mut inner_node = None;
    let mut leaf_node = None;
    let outer = Panel::vstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            let inner = Panel::hstack()
                .size((Sizing::Hug, Sizing::Hug))
                .show(ui, |ui| {
                    leaf_node = Some(
                        Frame::new()
                            .size((Sizing::Fixed(250.0), Sizing::Fixed(60.0)))
                            .show(ui)
                            .node,
                    );
                });
            inner_node = Some(inner.node);
        })
        .node;
    ui.end_frame();

    let o = ui.layout_engine.result.rect[outer.index()];
    let i = ui.layout_engine.result.rect[inner_node.unwrap().index()];
    let l = ui.layout_engine.result.rect[leaf_node.unwrap().index()];

    assert_eq!(l.size.w, 250.0);
    assert!(
        i.size.w >= 250.0,
        "Hug inner should hug to 250; got {}",
        i.size.w
    );
    assert!(o.size.w >= i.size.w, "Fill outer should contain Hug inner");
    assert_contains(i, l, "nested/inner_contains_leaf");
    assert_contains(o, i, "nested/outer_contains_inner");
}

/// Root rect grows past the surface when content exceeds it. The
/// audit confirmed the renderer/composer/cascade tolerate this; the
/// GPU scissor clips at the viewport.
#[test]
fn root_grows_past_surface_when_content_exceeds_it() {
    let mut ui = ui_at(UVec2::new(100, 100));
    let root = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .size((Sizing::Fixed(400.0), Sizing::Fixed(300.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let r = ui.layout_engine.result.rect[root.index()];
    assert!(
        r.size.w >= 400.0,
        "root should grow past 100 px surface to contain 400 px child; got w={}",
        r.size.w,
    );
    assert!(
        r.size.h >= 300.0,
        "root should grow past 100 px surface vertically; got h={}",
        r.size.h,
    );
}

/// Negative case: parent stays at the surface size when children fit.
/// The floor is `max(available, hug)`, so available wins when bigger.
#[test]
fn fill_parent_stays_at_available_when_children_fit() {
    let mut ui = ui_at(UVec2::new(800, 600));
    let parent = Panel::hstack()
        .size((Sizing::FILL, Sizing::FILL))
        .show(&mut ui, |ui| {
            Frame::new()
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .show(ui);
        })
        .node;
    ui.end_frame();

    let r = ui.layout_engine.result.rect[parent.index()];
    assert_eq!(r.size.w, 800.0);
    assert_eq!(r.size.h, 600.0);
}
