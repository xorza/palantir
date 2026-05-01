use crate::element::{LayoutMode, NodeElement};
use crate::primitives::{AxisAlign, Rect, Size, Sizing};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree};
use glam::Vec2;

mod canvas;
mod grid;
mod stack;
mod zstack;

/// Persistent layout engine: holds per-layout-kind scratch that survives
/// across frames for amortized zero-alloc layout. Owned by `Ui`
/// (`Ui::layout(surface)`); construct directly only when laying out a `Tree`
/// outside the `Ui` flow.
///
/// Today only `grid` carries scratch — stack/zstack/canvas are single-pass and
/// keep their state on the call stack. Add a sibling field here if that ever
/// changes.
#[derive(Default)]
pub struct LayoutEngine {
    pub(super) grid: grid::GridLayout,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run measure + arrange for `root` given the surface rect. Reuses
    /// internal scratch — call this each frame for amortized zero-alloc
    /// layout (after warmup).
    pub fn run(&mut self, tree: &mut Tree, root: NodeId, surface: Rect) {
        debug_assert_eq!(
            self.grid.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        self.measure(tree, root, Size::new(surface.width(), surface.height()));
        self.arrange(tree, root, surface);
        debug_assert_eq!(
            self.grid.depth(),
            0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores `desired` on each visited node.
    pub(super) fn measure(&mut self, tree: &mut Tree, node: NodeId, available: Size) -> Size {
        if tree.node(node).is_collapsed() {
            tree.node_mut(node).desired = Size::ZERO;
            return Size::ZERO;
        }
        let style = tree.node(node).element;
        let mode = style.mode;

        let inner_avail = Size::new(
            (available.w - style.margin.horiz() - style.padding.horiz()).max(0.0),
            (available.h - style.margin.vert() - style.padding.vert()).max(0.0),
        );

        let content = match mode {
            LayoutMode::Leaf => leaf_content_size(tree, node),
            LayoutMode::HStack => stack::measure(self, tree, node, inner_avail, stack::Axis::X),
            LayoutMode::VStack => stack::measure(self, tree, node, inner_avail, stack::Axis::Y),
            LayoutMode::ZStack => zstack::measure(self, tree, node),
            LayoutMode::Canvas => canvas::measure(self, tree, node),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx),
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

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` on each visited node.
    pub(super) fn arrange(&mut self, tree: &mut Tree, node: NodeId, slot: Rect) {
        if tree.node(node).is_collapsed() {
            zero_subtree(tree, node, slot.min);
            return;
        }
        let style = tree.node(node).element;
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        tree.node_mut(node).rect = rendered;
        let inner = rendered.deflated_by(style.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, stack::Axis::X),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, stack::Axis::Y),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner),
            LayoutMode::Grid(idx) => grid::arrange(self, tree, node, inner, idx),
        }
    }
}

/// Resolve a node's outer slot size on one axis, given its sizing policy,
/// hug-content size (margin-inclusive), parent-supplied available, own margin,
/// and clamps. Each branch produces *rendered* size (margin-exclusive); we
/// clamp once and add margin once at the end.
fn resolve_axis_size(
    s: Sizing,
    hug_outer: f32,
    available: f32,
    margin: f32,
    min: f32,
    max: f32,
) -> f32 {
    let rendered = match s {
        Sizing::Fixed(v) => v,
        Sizing::Hug => hug_outer - margin,
        Sizing::Fill(_) => {
            let outer = if available.is_finite() {
                available
            } else {
                hug_outer
            };
            outer - margin
        }
    };
    rendered.max(0.0).clamp(min, max) + margin
}

/// Set this node and every descendant to a zero-size rect anchored at
/// `anchor`. Bypasses layout dispatch so a collapsed subtree pays only one
/// pre-order walk regardless of what its children would have been.
pub(super) fn zero_subtree(tree: &mut Tree, node: NodeId, anchor: Vec2) {
    tree.node_mut(node).rect = Rect {
        min: anchor,
        size: Size::ZERO,
    };
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        zero_subtree(tree, c, anchor);
    }
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

/// Cross/placement alignment for a child, falling back to the parent's
/// `child_align` on whichever axis the child left as `Auto`. Returned as a
/// `(horizontal, vertical)` `AxisAlign` pair so layout math stays
/// axis-symmetric. Used by ZStack and Grid; HStack/VStack only need the
/// cross axis and resolve it inline.
pub(super) fn resolved_axis_align(
    child: &NodeElement,
    parent: &NodeElement,
) -> (AxisAlign, AxisAlign) {
    (
        child.align.h.or(parent.child_align.h).to_axis(),
        child.align.v.or(parent.child_align.v).to_axis(),
    )
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for stack cross-axis, ZStack per-axis, and Grid per-cell placement.
///
/// `auto_stretches` controls how `AxisAlign::Auto` is interpreted: stack and
/// ZStack pass `false` (Auto stretches only when the child is `Sizing::Fill`);
/// Grid passes `true` (Auto stretches unconditionally — WPF cell default).
pub(super) fn place_axis(
    align: AxisAlign,
    sizing: Sizing,
    desired: f32,
    inner: f32,
    auto_stretches: bool,
) -> (f32, f32) {
    let stretch = matches!(align, AxisAlign::Stretch)
        || matches!(align, AxisAlign::Auto)
            && (auto_stretches || matches!(sizing, Sizing::Fill(_)));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        AxisAlign::Center => ((inner - size) * 0.5).max(0.0),
        AxisAlign::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    (size, offset)
}
