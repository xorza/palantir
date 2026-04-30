use crate::primitives::{Rect, Size, Sizing};
use crate::tree::{LayoutKind, NodeId, Tree};
use glam::Vec2;

/// Run measure + arrange for `root` given the surface rect.
pub fn run(tree: &mut Tree, root: NodeId, surface: Rect) {
    measure(tree, root, Size::new(surface.width(), surface.height()));
    arrange(tree, root, surface);
}

/// Bottom-up. Returns the node's desired size and stores it on the node.
fn measure(tree: &mut Tree, node: NodeId, available: Size) -> Size {
    let style = tree.node(node).style;
    let layout = tree.node(node).layout;

    // Inner available = available minus padding.
    let inner_avail = Size::new(
        (available.w - style.padding.horiz()).max(0.0),
        (available.h - style.padding.vert()).max(0.0),
    );

    // Children-derived size (content size in inner coords).
    let content = match layout {
        LayoutKind::Leaf => leaf_content_size(tree, node),
        LayoutKind::HStack => hstack_measure(tree, node, inner_avail),
        LayoutKind::VStack => vstack_measure(tree, node, inner_avail),
    };

    // Apply style sizing on the outer (padded) box. Fixed/Fill specify outer size;
    // Hug returns content + padding.
    let desired = Size::new(
        resolve_axis(style.size.w, content.w + style.padding.horiz(), available.w),
        resolve_axis(style.size.h, content.h + style.padding.vert(), available.h),
    );

    tree.node_mut(node).desired = desired;
    desired
}

/// Top-down. Assigns final rect to `node`, recurses into children.
fn arrange(tree: &mut Tree, node: NodeId, final_rect: Rect) {
    tree.node_mut(node).rect = final_rect;
    let style = tree.node(node).style;
    let layout = tree.node(node).layout;

    // Inner rect after padding.
    let inner = Rect {
        min: final_rect.min + Vec2::new(style.padding.left, style.padding.top),
        size: Size::new(
            (final_rect.width() - style.padding.horiz()).max(0.0),
            (final_rect.height() - style.padding.vert()).max(0.0),
        ),
    };

    match layout {
        LayoutKind::Leaf => {}
        LayoutKind::HStack => arrange_stack(tree, node, inner, Axis::X),
        LayoutKind::VStack => arrange_stack(tree, node, inner, Axis::Y),
    }
}

fn resolve_axis(s: Sizing, hug_outer: f32, available: f32) -> f32 {
    match s {
        Sizing::Fixed(v) => v,
        Sizing::Hug => hug_outer,
        Sizing::Fill => {
            if available.is_finite() {
                available
            } else {
                hug_outer
            }
        }
    }
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
