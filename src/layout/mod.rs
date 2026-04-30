use crate::primitives::{Rect, Size, Sizing};
use crate::tree::{LayoutKind, NodeId, Tree};
use glam::Vec2;

/// Run measure + arrange for `root` given the surface rect.
pub fn run(tree: &mut Tree, root: NodeId, surface: Rect) {
    measure(tree, root, Size::new(surface.width(), surface.height()));
    arrange(tree, root, surface);
}

/// Bottom-up. Returns the node's desired *slot* size (including its own margin)
/// and stores it on the node.
fn measure(tree: &mut Tree, node: NodeId, available: Size) -> Size {
    let style = tree.node(node).style;
    let layout = tree.node(node).layout;

    // Inner available = available minus margin minus padding.
    let inner_avail = Size::new(
        (available.w - style.margin.horiz() - style.padding.horiz()).max(0.0),
        (available.h - style.margin.vert() - style.padding.vert()).max(0.0),
    );

    let content = match layout {
        LayoutKind::Leaf => leaf_content_size(tree, node),
        LayoutKind::HStack => hstack_measure(tree, node, inner_avail),
        LayoutKind::VStack => vstack_measure(tree, node, inner_avail),
        LayoutKind::ZStack => zstack_measure(tree, node),
    };

    let hug_w = content.w + style.padding.horiz() + style.margin.horiz();
    let hug_h = content.h + style.padding.vert() + style.margin.vert();
    let desired = Size::new(
        resolve_axis(
            style.size.w,
            hug_w,
            available.w,
            style.margin.horiz(),
            style.min_size.w,
            style.max_size.w,
        ),
        resolve_axis(
            style.size.h,
            hug_h,
            available.h,
            style.margin.vert(),
            style.min_size.h,
            style.max_size.h,
        ),
    );

    tree.node_mut(node).desired = desired;
    desired
}

/// Top-down. `slot` is the rect the parent reserved (including this node's margin).
fn arrange(tree: &mut Tree, node: NodeId, slot: Rect) {
    let style = tree.node(node).style;
    let layout = tree.node(node).layout;

    let rendered = Rect {
        min: slot.min + Vec2::new(style.margin.left, style.margin.top),
        size: Size::new(
            (slot.width() - style.margin.horiz()).max(0.0),
            (slot.height() - style.margin.vert()).max(0.0),
        ),
    };
    tree.node_mut(node).rect = rendered;

    let inner = Rect {
        min: rendered.min + Vec2::new(style.padding.left, style.padding.top),
        size: Size::new(
            (rendered.width() - style.padding.horiz()).max(0.0),
            (rendered.height() - style.padding.vert()).max(0.0),
        ),
    };

    match layout {
        LayoutKind::Leaf => {}
        LayoutKind::HStack => arrange_stack(tree, node, inner, Axis::X),
        LayoutKind::VStack => arrange_stack(tree, node, inner, Axis::Y),
        LayoutKind::ZStack => arrange_zstack(tree, node, inner),
    }
}

fn resolve_axis(s: Sizing, hug_outer: f32, available: f32, margin: f32, min: f32, max: f32) -> f32 {
    let slot = match s {
        Sizing::Fixed(v) => v + margin,
        Sizing::Hug => hug_outer,
        Sizing::Fill => {
            if available.is_finite() {
                available
            } else {
                hug_outer
            }
        }
    };
    let rendered = (slot - margin).max(0.0).clamp(min, max);
    rendered + margin
}

fn leaf_content_size(tree: &Tree, node: NodeId) -> Size {
    // For a Leaf, content size = bounding box of any Text shapes' measured size,
    // or zero. Other shapes are owner-relative and don't drive size.
    let mut s = Size::ZERO;
    for sh in tree.shapes_of(node) {
        if let crate::shape::Shape::Text { measured, .. } = sh {
            s = s.max(*measured);
        }
    }
    s
}

fn hstack_measure(tree: &mut Tree, node: NodeId, inner: Size) -> Size {
    // Pass infinite width to children on the main axis (WPF trick).
    let child_avail = Size::new(f32::INFINITY, inner.h);
    let kids: Vec<NodeId> = tree.children(node).collect();
    let mut total_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in kids {
        let d = measure(tree, c, child_avail);
        total_w += d.w;
        max_h = max_h.max(d.h);
    }
    Size::new(total_w, max_h)
}

fn vstack_measure(tree: &mut Tree, node: NodeId, inner: Size) -> Size {
    let child_avail = Size::new(inner.w, f32::INFINITY);
    let kids: Vec<NodeId> = tree.children(node).collect();
    let mut total_h = 0.0f32;
    let mut max_w = 0.0f32;
    for c in kids {
        let d = measure(tree, c, child_avail);
        total_h += d.h;
        max_w = max_w.max(d.w);
    }
    Size::new(max_w, total_h)
}

#[derive(Copy, Clone)]
enum Axis {
    X,
    Y,
}

fn arrange_stack(tree: &mut Tree, node: NodeId, inner: Rect, axis: Axis) {
    let kids: Vec<NodeId> = tree.children(node).collect();
    if kids.is_empty() {
        return;
    }

    // Sum desired along main axis; count Fill children for distribution.
    let mut sum_main_desired = 0.0f32;
    let mut fill_count = 0u32;
    for &c in &kids {
        let d = tree.node(c).desired;
        let main = match axis {
            Axis::X => d.w,
            Axis::Y => d.h,
        };
        sum_main_desired += main;
        let s = tree.node(c).style;
        let main_sizing = match axis {
            Axis::X => s.size.w,
            Axis::Y => s.size.h,
        };
        if matches!(main_sizing, Sizing::Fill) {
            fill_count += 1;
        }
    }

    let main_total = match axis {
        Axis::X => inner.size.w,
        Axis::Y => inner.size.h,
    };
    let cross = match axis {
        Axis::X => inner.size.h,
        Axis::Y => inner.size.w,
    };
    let leftover = (main_total - sum_main_desired).max(0.0);
    let fill_share = if fill_count > 0 {
        leftover / fill_count as f32
    } else {
        0.0
    };

    let mut cursor = match axis {
        Axis::X => inner.min.x,
        Axis::Y => inner.min.y,
    };
    for c in kids {
        let d = tree.node(c).desired;
        let s = tree.node(c).style;
        let (main_sizing, main_desired) = match axis {
            Axis::X => (s.size.w, d.w),
            Axis::Y => (s.size.h, d.h),
        };
        let main_size = main_desired
            + if matches!(main_sizing, Sizing::Fill) {
                fill_share
            } else {
                0.0
            };

        let cross_sizing = match axis {
            Axis::X => s.size.h,
            Axis::Y => s.size.w,
        };
        let cross_desired = match axis {
            Axis::X => d.h,
            Axis::Y => d.w,
        };
        let cross_size = match cross_sizing {
            Sizing::Fill => cross,
            _ => cross_desired,
        };

        let child_rect = match axis {
            Axis::X => Rect::new(cursor, inner.min.y, main_size, cross_size),
            Axis::Y => Rect::new(inner.min.x, cursor, cross_size, main_size),
        };
        arrange(tree, c, child_rect);
        cursor += main_size;
    }
}

/// ZStack: children all at the same position (top-left of inner rect).
/// Pass `INFINITY` on both axes during measure so `Fill` children fall back to
/// intrinsic — otherwise the `Hug` panel would size to its own `Fill` children
/// (recursive). Content size = `max(child desired)` per axis, so the panel
/// hugs the largest child.
fn zstack_measure(tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let kids: Vec<NodeId> = tree.children(node).collect();
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    for c in kids {
        let d = measure(tree, c, child_avail);
        max_w = max_w.max(d.w);
        max_h = max_h.max(d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets the full inner rect as its slot, sized per its own `Sizing`.
/// Default position is top-left; per-child alignment lands later.
fn arrange_zstack(tree: &mut Tree, node: NodeId, inner: Rect) {
    let kids: Vec<NodeId> = tree.children(node).collect();
    for c in kids {
        let d = tree.node(c).desired;
        let s = tree.node(c).style;

        let w = match s.size.w {
            Sizing::Fill => inner.size.w,
            _ => d.w,
        };
        let h = match s.size.h {
            Sizing::Fill => inner.size.h,
            _ => d.h,
        };

        let child_rect = Rect::new(inner.min.x, inner.min.y, w, h);
        arrange(tree, c, child_rect);
    }
}

#[cfg(test)]
mod tests;
