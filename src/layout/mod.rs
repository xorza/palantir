use crate::primitives::{Align, Layout, Rect, Size, Sizes, Sizing, Visibility};
use crate::shape::Shape;
use crate::tree::{LayoutMode, NodeId, Tree};
use glam::Vec2;

/// Run measure + arrange for `root` given the surface rect.
pub fn run(tree: &mut Tree, root: NodeId, surface: Rect) {
    measure(tree, root, Size::new(surface.width(), surface.height()));
    arrange(tree, root, surface);
}

/// Bottom-up. Returns the node's desired *slot* size (including its own margin)
/// and stores it on the node.
fn measure(tree: &mut Tree, node: NodeId, available: Size) -> Size {
    if tree.node(node).element.visibility == Visibility::Collapsed {
        tree.node_mut(node).desired = Size::ZERO;
        return Size::ZERO;
    }
    let style = tree.node(node).element.layout;
    let mode = tree.node(node).element.mode;

    // Inner available = available minus margin minus padding.
    let inner_avail = Size::new(
        (available.w - style.margin.horiz() - style.padding.horiz()).max(0.0),
        (available.h - style.margin.vert() - style.padding.vert()).max(0.0),
    );

    let content = match mode {
        LayoutMode::Leaf => leaf_content_size(tree, node),
        LayoutMode::HStack => stack_measure(tree, node, inner_avail, Axis::X),
        LayoutMode::VStack => stack_measure(tree, node, inner_avail, Axis::Y),
        LayoutMode::ZStack => zstack_measure(tree, node),
        LayoutMode::Canvas => canvas_measure(tree, node),
    };

    let hug_w = content.w + style.padding.horiz() + style.margin.horiz();
    let hug_h = content.h + style.padding.vert() + style.margin.vert();
    let desired = Size::new(
        resolve_axis_size(
            style.size.w,
            hug_w,
            available.w,
            style.margin.horiz(),
            style.min_size.w,
            style.max_size.w,
        ),
        resolve_axis_size(
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
    if tree.node(node).element.visibility == Visibility::Collapsed {
        tree.node_mut(node).rect = Rect {
            min: slot.min,
            size: Size::ZERO,
        };
        return;
    }
    let style = tree.node(node).element.layout;
    let mode = tree.node(node).element.mode;

    let rendered = slot.deflated_by(style.margin);
    tree.node_mut(node).rect = rendered;
    let inner = rendered.deflated_by(style.padding);

    match mode {
        LayoutMode::Leaf => {}
        LayoutMode::HStack => arrange_stack(tree, node, inner, Axis::X),
        LayoutMode::VStack => arrange_stack(tree, node, inner, Axis::Y),
        LayoutMode::ZStack => arrange_zstack(tree, node, inner),
        LayoutMode::Canvas => arrange_canvas(tree, node, inner),
    }
}

/// Resolve a node's outer slot size on one axis, given its sizing policy,
/// hug-content size, parent-supplied available, own margin, and clamps.
fn resolve_axis_size(
    s: Sizing,
    hug_outer: f32,
    available: f32,
    margin: f32,
    min: f32,
    max: f32,
) -> f32 {
    let slot = match s {
        Sizing::Fixed(v) => v + margin,
        Sizing::Hug => hug_outer,
        Sizing::Fill(_) => {
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
        if let Shape::Text { measured, .. } = sh {
            s = s.max(*measured);
        }
    }
    s
}

#[derive(Copy, Clone, PartialEq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn main(self, s: Size) -> f32 {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross(self, s: Size) -> f32 {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    fn main_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.y,
            Axis::Y => v.x,
        }
    }
    fn main_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    /// HStack's cross axis is Y; VStack's is X.
    fn cross_align(self, l: &Layout) -> Align {
        match self {
            Axis::X => l.align_y,
            Axis::Y => l.align_x,
        }
    }
    /// Build a `Size` from main- and cross-axis lengths.
    fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::X => Size::new(main, cross),
            Axis::Y => Size::new(cross, main),
        }
    }
    /// Build a `Rect` from main- and cross-axis positions and lengths.
    fn compose_rect(self, main_pos: f32, cross_pos: f32, main: f32, cross: f32) -> Rect {
        match self {
            Axis::X => Rect::new(main_pos, cross_pos, main, cross),
            Axis::Y => Rect::new(cross_pos, main_pos, cross, main),
        }
    }
}

fn stack_measure(tree: &mut Tree, node: NodeId, inner: Size, axis: Axis) -> Size {
    // Pass infinite size on the main axis (WPF trick): children report intrinsic.
    let child_avail = axis.compose_size(f32::INFINITY, axis.cross(inner));
    let gap = tree.node(node).element.layout.gap;

    let mut total_main = 0.0f32;
    let mut max_cross = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        // Collapsed children still get measured (so `desired` is set to ZERO),
        // but don't contribute to the parent's content size or gap count.
        let collapsed = tree.node(c).element.visibility == Visibility::Collapsed;
        let d = measure(tree, c, child_avail);
        if collapsed {
            continue;
        }
        total_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
        count += 1;
    }
    total_main += gap * count.saturating_sub(1) as f32;
    axis.compose_size(total_main, max_cross)
}

fn arrange_stack(tree: &mut Tree, node: NodeId, inner: Rect, axis: Axis) {
    let gap = tree.node(node).element.layout.gap;

    // Sum desired along main axis for non-Fill children; collect Fill weights.
    // Fill siblings split the remaining space proportionally (WPF Star semantics)
    // independent of their intrinsic content size.
    let mut sum_main_desired = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let n = tree.node(c);
        if n.element.visibility == Visibility::Collapsed {
            continue;
        }
        if let Sizing::Fill(weight) = axis.main_sizing(n.element.layout.size) {
            total_weight += weight.max(0.0);
        } else {
            sum_main_desired += axis.main(n.desired);
        }
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover = (main_total - sum_main_desired - total_gap).max(0.0);

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min);
    let mut first = true;

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            // Give Collapsed children a zero rect at the cursor so they exist
            // in the tree but consume no space, no gap, no fill weight.
            arrange(
                tree,
                c,
                Rect {
                    min: axis.compose_rect(cursor, cross_min, 0.0, 0.0).min,
                    size: Size::ZERO,
                },
            );
            continue;
        }
        if !first {
            cursor += gap;
        }
        first = false;
        let d = tree.node(c).desired;
        let s = tree.node(c).element.layout;

        let main_sizing = axis.main_sizing(s.size);
        let main_size = match main_sizing {
            Sizing::Fill(weight) if total_weight > 0.0 => {
                leftover * (weight.max(0.0) / total_weight)
            }
            _ => axis.main(d),
        };

        let cross_align = axis.cross_align(&s);
        let cross_sizing = axis.cross_sizing(s.size);
        let cross_desired = axis.cross(d);
        let (cross_size, cross_offset) =
            place_axis(cross_align, cross_sizing, cross_desired, cross);

        let child_rect = axis.compose_rect(cursor, cross_min + cross_offset, main_size, cross_size);
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
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = measure(tree, c, child_avail);
        max_w = max_w.max(d.w);
        max_h = max_h.max(d.h);
    }
    Size::new(max_w, max_h)
}

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Pass `INFINITY` on both axes during measure
/// so `Fill` children fall back to intrinsic — "fill the rest" is meaningless
/// when children can overlap. Content size = `max(child_pos + child_desired)`
/// per axis, so a `Hug` Canvas grows to the union of placed rects.
fn canvas_measure(tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let pos = tree.node(c).element.layout.position.unwrap_or(Vec2::ZERO);
        let d = measure(tree, c, child_avail);
        max_w = max_w.max(pos.x + d.w);
        max_h = max_h.max(pos.y + d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `canvas_measure`.
fn arrange_canvas(tree: &mut Tree, node: NodeId, inner: Rect) {
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = tree.node(c).desired;
        let pos = tree.node(c).element.layout.position.unwrap_or(Vec2::ZERO);
        let child_rect = Rect {
            min: inner.min + pos,
            size: d,
        };
        arrange(tree, c, child_rect);
    }
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y`. Defaults pin to top-left
/// (matching the original behavior) unless the child has `Sizing::Fill` —
/// then `Auto` falls back to stretch on that axis.
fn arrange_zstack(tree: &mut Tree, node: NodeId, inner: Rect) {
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = tree.node(c).desired;
        let s = tree.node(c).element.layout;

        let (w, x_off) = place_axis(s.align_x, s.size.w, d.w, inner.size.w);
        let (h, y_off) = place_axis(s.align_y, s.size.h, d.h, inner.size.h);

        let child_rect = Rect::new(inner.min.x + x_off, inner.min.y + y_off, w, h);
        arrange(tree, c, child_rect);
    }
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for both stack cross-axis placement and ZStack per-axis placement.
fn place_axis(align: Align, sizing: Sizing, desired: f32, inner: f32) -> (f32, f32) {
    let stretch = matches!(align, Align::Stretch)
        || (matches!(align, Align::Auto) && matches!(sizing, Sizing::Fill(_)));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        Align::Center => ((inner - size) * 0.5).max(0.0),
        Align::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    (size, offset)
}

#[cfg(test)]
mod tests;
