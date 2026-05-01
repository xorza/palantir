use crate::element::{LayoutMode, NodeElement};
use crate::primitives::{Align, AxisAlign, Rect, Size, Sizing};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree};
use glam::Vec2;

mod canvas;
mod grid;
mod result;
mod stack;
mod zstack;

pub use result::LayoutResult;

/// Persistent layout engine: holds per-layout-kind scratch + the per-frame
/// `LayoutResult`. Owned by `Ui` (`Ui::layout(surface)`); construct directly
/// only when laying out a `Tree` outside the `Ui` flow.
#[derive(Default)]
pub struct LayoutEngine {
    pub(super) grid: grid::GridLayout,
    result: LayoutResult,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn result(&self) -> &LayoutResult {
        &self.result
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.result.rect(id)
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.result.desired(id)
    }

    /// Run measure + arrange for `root` given the surface rect. Reuses
    /// internal scratch — call this each frame for amortized zero-alloc
    /// layout (after warmup). `Tree` is read-only here; output lands in
    /// `self.result`.
    pub fn run(&mut self, tree: &Tree, root: NodeId, surface: Rect) {
        debug_assert_eq!(
            self.grid.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        self.result.resize_for(tree);
        self.measure(tree, root, Size::new(surface.width(), surface.height()));
        self.arrange(tree, root, surface);
        debug_assert_eq!(
            self.grid.depth(),
            0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores `desired` for each visited node in `self.result`.
    pub(super) fn measure(&mut self, tree: &Tree, node: NodeId, available: Size) -> Size {
        if tree.node(node).is_collapsed() {
            self.result.set_desired(node, Size::ZERO);
            return Size::ZERO;
        }
        let style = tree.node(node).element;
        let mode = style.mode;
        let extras = tree.read_extras(node);
        let (min_size, max_size) = (extras.min_size, extras.max_size);

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
                min_size.w,
                max_size.w,
            ),
            resolve_axis_size(
                style.size.h,
                hug_h,
                available.h,
                style.margin.vert(),
                min_size.h,
                max_size.h,
            ),
        );

        self.result.set_desired(node, desired);
        desired
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in `self.result`.
    pub(super) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect) {
        if tree.node(node).is_collapsed() {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
        let style = tree.node(node).element;
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        self.result.set_rect(node, rendered);
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
pub(super) fn zero_subtree(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, anchor: Vec2) {
    layout.result.set_rect(
        node,
        Rect {
            min: anchor,
            size: Size::ZERO,
        },
    );
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        zero_subtree(layout, tree, c, anchor);
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

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(super) fn resolved_axis_align(
    child: &NodeElement,
    parent_child_align: Align,
) -> (AxisAlign, AxisAlign) {
    let a = child.flags.align();
    (
        a.halign().or(parent_child_align.halign()).to_axis(),
        a.valign().or(parent_child_align.valign()).to_axis(),
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
