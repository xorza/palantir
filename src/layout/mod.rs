use crate::element::LayoutMode;
use crate::primitives::{Rect, Size, Sizing};
use crate::shape::{Shape, TextWrap};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};
use grid::GridContext;
use support::{resolve_axis_size, zero_subtree};
use wrapstack::WrapScratch;

mod axis;
mod canvas;
mod grid;
mod intrinsic;
mod result;
mod stack;
mod support;
mod wrapstack;
mod zstack;

pub use axis::Axis;
pub use intrinsic::LenReq;
pub use result::{LayoutResult, ShapedText};

/// Persistent layout engine. Holds intermediate per-frame **scratch**
/// (valid only during `run`) and per-frame **output** (`LayoutResult`,
/// read by encoder/hit-index after `run` returns). Allocations retain
/// capacity across frames.
///
/// **Scratch** — internal to the layout pass. Drivers in this module
/// read/write directly. Module-internal tests (e.g.
/// `stack/tests.rs`) reach in via `pub(in crate::layout)` to pin
/// measure output independently of arrange's slot-clamping.
///
/// - `grid` — grid-driver scratch (per-depth track state, hug pool).
/// - `desired` — measure-pass output, read by arrange.
/// - `intrinsics` — intra-frame cache for `intrinsic(node, axis, req)`
///   queries (see `intrinsic.md`). Pure function of subtree; safe to
///   memoize within a frame. Flat `Vec` indexed by node, four slots
///   per node (one per `(axis, req)` combination). NaN means "not yet
///   computed". Cleared and resized to `node_count` in `run`.
///
/// **Output**:
///
/// - `result` — post-layout rects + text shapes. Public read-only via
///   [`LayoutEngine::result`].
#[derive(Default)]
pub struct LayoutEngine {
    pub(in crate::layout) grid: GridContext,
    pub(in crate::layout) wrap: WrapScratch,
    pub(in crate::layout) desired: Vec<Size>,
    intrinsics: Vec<[f32; 4]>,
    pub(in crate::layout) result: LayoutResult,
}

#[inline]
fn intrinsic_slot(axis: Axis, req: LenReq) -> usize {
    let a = match axis {
        Axis::X => 0,
        Axis::Y => 1,
    };
    let r = match req {
        LenReq::MinContent => 0,
        LenReq::MaxContent => 1,
    };
    a * 2 + r
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

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` under content-sizing `req`. See `intrinsic.md`.
    ///
    /// Pure function of the subtree at `node`: doesn't depend on the
    /// parent's available width or the arranged rect. Memoized via the
    /// intra-frame cache so repeated queries during the same `run` cost
    /// one array load. Consumed by `grid::measure` (Phase 1 column
    /// resolution) and `stack::measure` (Fill min-content floor).
    pub fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        text: &mut TextMeasurer,
    ) -> f32 {
        let slot = intrinsic_slot(axis, req);
        let cached = self.intrinsics[node.index()][slot];
        if !cached.is_nan() {
            return cached;
        }
        let v = intrinsic::compute(self, tree, node, axis, req, text);
        self.intrinsics[node.index()][slot] = v;
        v
    }

    /// Run measure + arrange for `root` given the surface rect. Reuses
    /// internal scratch — call this each frame for amortized zero-alloc
    /// layout (after warmup). Output lands in `self.result`.
    ///
    /// `text` carries the shaper (or the mono fallback inside it) and is
    /// borrowed for the duration of the call so wrapping leaves can reshape
    /// against the parent-committed width during measure.
    pub fn run(&mut self, tree: &Tree, root: NodeId, surface: Rect, text: &mut TextMeasurer) {
        assert_eq!(
            self.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.intrinsics.clear();
        self.intrinsics.resize(n, [f32::NAN; 4]);
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
    /// recurse. Stores the resolved size for each visited node in
    /// `self.desired` (read by `arrange`).
    pub(in crate::layout) fn measure(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available: Size,
        text: &mut TextMeasurer,
    ) -> Size {
        if tree.is_collapsed(node) {
            self.desired[node.index()] = Size::ZERO;
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
            LayoutMode::HStack => stack::measure(self, tree, node, inner_avail, Axis::X, text),
            LayoutMode::VStack => stack::measure(self, tree, node, inner_avail, Axis::Y, text),
            LayoutMode::WrapHStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::X, text)
            }
            LayoutMode::WrapVStack => {
                wrapstack::measure(self, tree, node, inner_avail, Axis::Y, text)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, inner_avail, text),
            LayoutMode::Canvas => canvas::measure(self, tree, node, inner_avail, text),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx, inner_avail, text),
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

        self.desired[node.index()] = desired;
        desired
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in `self.result`.
    pub(in crate::layout) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect) {
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
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, Axis::X),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, Axis::Y),
            LayoutMode::WrapHStack => wrapstack::arrange(self, tree, node, inner, Axis::X),
            LayoutMode::WrapVStack => wrapstack::arrange(self, tree, node, inner, Axis::Y),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner),
            LayoutMode::Grid(idx) => grid::arrange(self, tree, node, inner, idx),
        }
    }

    /// Walk a Leaf's recorded shapes and return the content size that drives
    /// its hugging. For `Shape::Text` runs, this is also where shaping
    /// happens: the shaped buffer + measured size land on
    /// `LayoutResult.text_shapes` so the encoder can pick them up later.
    /// `available_w` flows down from the parent and gates wrapping.
    fn leaf_content_size(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let mut s = Size::ZERO;
        for shape in tree.shapes_of(node) {
            if let Shape::Text {
                text: src,
                font_size_px,
                wrap,
                ..
            } = shape
            {
                let m = self.shape_text(node, src, *font_size_px, *wrap, available_w, text);
                s = s.max(m);
            }
        }
        s
    }

    fn shape_text(
        &mut self,
        node: NodeId,
        src: &str,
        font_size_px: f32,
        wrap: TextWrap,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let unbounded = text.measure(src, font_size_px, None);
        let (measured, key) = if matches!(wrap, TextWrap::Wrap)
            && available_w.is_finite()
            && available_w < unbounded.size.w
        {
            // Floor at the widest unbreakable run so we don't break inside a
            // word — the run overflows the slot instead.
            let target = available_w.max(unbounded.intrinsic_min);
            tracing::trace!(node = node.index(), target, "wrap reshape");
            let m = text.measure(src, font_size_px, Some(target));
            (m.size, m.key)
        } else {
            (unbounded.size, unbounded.key)
        };

        self.result
            .set_text_shape(node, ShapedText { measured, key });
        measured
    }
}
