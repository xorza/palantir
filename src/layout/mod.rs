use crate::element::{LayoutCore, LayoutMode};
use crate::primitives::{Align, AxisAlign, Rect, Size, Sizing};
use crate::shape::{Shape, TextWrap};
use crate::text::CosmicMeasure;
use crate::tree::{NodeId, Tree};
use glam::Vec2;
use grid::GridContext;

mod canvas;
mod grid;
mod result;
mod stack;
mod zstack;

pub use result::{LayoutResult, ReshapedText};

/// Persistent layout engine. Holds two kinds of per-frame state, both with
/// capacity reused across frames:
///
/// - [`GridContext`] — transient scratch (grid track sizes etc.), discarded
///   conceptually once each pass returns.
/// - [`LayoutResult`] — output (desired sizes, rects, text reshapes), read
///   by the encoder + hit-index after the layout pass.
#[derive(Default)]
pub struct LayoutEngine {
    pub(super) grid: GridContext,
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
    /// layout (after warmup). Output lands in `self.result`.
    ///
    /// `text` is borrowed for the duration of the call so a wrapping leaf
    /// (`Shape::Text` with `TextWrap::Wrap`) can reshape against the parent-
    /// committed width *during* measure — without it (or in tests with
    /// `mono_measure`) wrapping shapes are left at their unbounded size.
    pub fn run(
        &mut self,
        tree: &Tree,
        root: NodeId,
        surface: Rect,
        text: Option<&mut CosmicMeasure>,
    ) {
        assert_eq!(
            self.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        self.result.resize_for(tree);
        self.grid.hugs.reset_for(tree);
        self.measure(
            tree,
            root,
            Size::new(surface.width(), surface.height()),
            text,
        );
        self.arrange(tree, root, surface);
        assert_eq!(
            self.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores `desired` for each visited node in `self.result`.
    pub(super) fn measure(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available: Size,
        text: Option<&mut CosmicMeasure>,
    ) -> Size {
        if tree.is_collapsed(node) {
            self.result.set_desired(node, Size::ZERO);
            return Size::ZERO;
        }
        let style = *tree.layout(node);
        let mode = style.mode;
        let extras = tree.read_extras(node);
        let (min_size, max_size) = (extras.min_size, extras.max_size);

        // For each axis: if this node has a declared `Fixed` size, that's the
        // outer width children see — `inner = fixed - padding`. Otherwise
        // (Hug / Fill) we propagate whatever the parent gave us. Without
        // this, a fixed-width parent above a wrapping child wouldn't
        // constrain the child's available width during measure, so wrapping
        // text would never reshape.
        let outer_w = match style.size.w {
            Sizing::Fixed(v) => v,
            _ => (available.w - style.margin.horiz()).max(0.0),
        };
        let outer_h = match style.size.h {
            Sizing::Fixed(v) => v,
            _ => (available.h - style.margin.vert()).max(0.0),
        };
        let inner_avail = Size::new(
            (outer_w - style.padding.horiz()).max(0.0),
            (outer_h - style.padding.vert()).max(0.0),
        );

        let content = match mode {
            LayoutMode::Leaf => self.leaf_content_size(tree, node, inner_avail.w, text),
            LayoutMode::HStack => {
                stack::measure(self, tree, node, inner_avail, stack::Axis::X, text)
            }
            LayoutMode::VStack => {
                stack::measure(self, tree, node, inner_avail, stack::Axis::Y, text)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, text),
            LayoutMode::Canvas => canvas::measure(self, tree, node, text),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx, text),
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
        if tree.is_collapsed(node) {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
        let style = *tree.layout(node);
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

impl LayoutEngine {
    fn leaf_content_size(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        text: Option<&mut CosmicMeasure>,
    ) -> Size {
        // For a Leaf, content size = bounding box of any Text shapes'
        // measured size (other shapes are owner-relative and don't drive
        // size). For `TextWrap::Wrap` shapes we reshape against
        // `available_w` here — single-pass, the parent-committed width flows
        // down through the recursive measure call so desired size + arranged
        // height already reflect wrapping. Falls back to the recorded
        // unbounded measure when no shaper is available (mono path / tests).
        if let Some(t) = text {
            self.maybe_reshape_text(tree, node, available_w, t);
        }
        let mut s = Size::ZERO;
        for sh in tree.shapes_of(node) {
            if let Shape::Text { measured, .. } = sh {
                let m = self
                    .result
                    .text_reshape(node)
                    .map(|r| r.measured)
                    .unwrap_or(*measured);
                s = s.max(m);
            }
        }
        s
    }

    fn maybe_reshape_text(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        text: &mut CosmicMeasure,
    ) {
        if !available_w.is_finite() {
            return;
        }
        for sh in tree.shapes_of(node) {
            let Shape::Text {
                text: src,
                font_size_px,
                measured,
                wrap,
                ..
            } = sh
            else {
                continue;
            };
            let TextWrap::Wrap { intrinsic_min } = *wrap else {
                continue;
            };

            let target = available_w.max(intrinsic_min);
            // Slot wider than the natural unbroken width — no reshape
            // needed; the recorded shape is already the answer.
            if target >= measured.w {
                continue;
            }

            tracing::trace!(
                node = node.index(),
                target,
                prev_measured = ?*measured,
                "reshape wrap text"
            );
            let m = text.measure(src, *font_size_px, Some(target));
            self.result.set_text_reshape(
                node,
                ReshapedText {
                    measured: m.size,
                    key: m.key,
                    max_width_px: target,
                },
            );
            // One Shape::Text per node — no need to keep walking.
            return;
        }
    }
}

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(super) fn resolved_axis_align(
    child: &LayoutCore,
    parent_child_align: Align,
) -> (AxisAlign, AxisAlign) {
    let a = child.align;
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
