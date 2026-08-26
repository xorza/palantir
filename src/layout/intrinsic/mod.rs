//! Intrinsic-dimensions queries — the on-demand `LenReq` API.
//!
//! This module owns:
//! - The query type `LenReq`.
//! - The central `compute()` dispatch that handles `Sizing` overrides,
//!   padding/margin, and `min_size`/`max_size` clamps before delegating to
//!   each driver's `intrinsic()` for content-driven sizes.
//! - Leaf intrinsics (no driver module owns leaves).
//!
//! Per-driver intrinsic logic (`stack`, `zstack`, `canvas`, `grid`) lives
//! alongside that driver's `measure`/`arrange` in its own module — same
//! per-driver-file convention as the rest of layout.

use crate::layout::axis::Axis;
use crate::layout::axis_ctx::AxisCtx;
use crate::layout::engine::LayoutEngine;
use crate::layout::text_shape_input::TextShapeInput;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::{canvas, grid, scroll, scrollbars, stack, wrapstack, zstack};
use crate::primitives::interned_text::InternedText;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::system::TextRunSlot;

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum LenReq {
    /// Smallest size the node can occupy without breaking. Text: longest
    /// unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural unbroken
    /// width.
    MaxContent,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct IntrinsicRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl IntrinsicRange {
    pub(crate) const ZERO: Self = Self { min: 0.0, max: 0.0 };

    /// The half `req` names.
    #[inline]
    pub(crate) fn get(self, req: LenReq) -> f32 {
        match req {
            LenReq::MinContent => self.min,
            LenReq::MaxContent => self.max,
        }
    }

    /// The `(kind, slot)` pairs `query` asks for, as mutable handles into
    /// this accumulator.
    ///
    /// Every driver's `intrinsic` folds children into a range under the
    /// same gate. Spelling that per driver as two near-identical
    /// `if query.includes(..)` blocks would put a third `LenReq` behind
    /// six call-site edits, any one of which is silent to forget.
    /// Iterating the requested halves puts the gate here and leaves each
    /// driver one loop body.
    #[inline]
    pub(crate) fn requested(
        &mut self,
        query: IntrinsicQuery,
    ) -> impl Iterator<Item = (LenReq, &mut f32)> {
        [
            (LenReq::MinContent, &mut self.min),
            (LenReq::MaxContent, &mut self.max),
        ]
        .into_iter()
        .filter(move |(req, _)| query.includes(*req))
    }
}

/// Which content sizes one query asks for: a single [`LenReq`], or both
/// in one recursion (`None`).
///
/// **A runtime field on purpose.** This was a `const RANGE: bool`
/// threaded through eleven items across six modules, on the theory that
/// specializing kept the recursive path free of per-node mode branches.
/// Measured on `caches`' intrinsic arms, that theory is backwards: the
/// mode is constant for the whole of one query tree, so the branch is
/// predicted every time, while the second monomorphization doubles the
/// code the recursion walks through. Removing it made
/// `grid/intrinsic/forced_miss` ~9% faster (33.7 → 30.7 µs measure, mean
/// of seven interleaved rounds, distributions non-overlapping), with
/// `measure`/`heavy`/`broad` forced-miss arms moving the same direction.
/// Re-specializing needs a measurement that says otherwise.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery {
    single_req: Option<LenReq>,
}

impl IntrinsicQuery {
    /// Max over non-collapsed children's outer intrinsic on `axis`, each
    /// child's contribution shifted by `offset`.
    ///
    /// Drivers whose own size on an axis is "the largest child wants this
    /// much" (ZStack, Stack cross-axis, WrapStack) call
    /// [`Self::children_max_at_origin`] — Canvas is the one that folds in
    /// each child's declared position. Same closure-parameter shape the measure side uses for the
    /// identical split (`LayoutPass::measure_per_axis_hug`, shared by
    /// `zstack::measure` and `canvas::measure`).
    pub(crate) fn children_max(
        self,
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
        mut offset: impl FnMut(&Tree, NodeId) -> f32,
    ) -> IntrinsicRange {
        let mut range = IntrinsicRange::ZERO;
        for c in tree.active_children(node) {
            let child = self.child(layout, tree, c, axis, interned_text);
            let child_offset = offset(tree, c);
            for (req, slot) in range.requested(self) {
                *slot = slot.max(child.get(req) + child_offset);
            }
        }
        range
    }
    /// [`Self::children_max`] for drivers that place children at the
    /// container origin — every one except Canvas.
    #[inline]
    pub(crate) fn children_max_at_origin(
        self,
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        self.children_max(layout, tree, node, axis, interned_text, |_, _| 0.0)
    }

    pub(crate) const fn single(req: LenReq) -> Self {
        Self {
            single_req: Some(req),
        }
    }

    pub(crate) const fn range() -> Self {
        Self { single_req: None }
    }

    /// The query covering exactly the requested halves, or `None` when
    /// neither survives. Lets a caller that discards one half — a scroll
    /// on a panned axis — narrow the recursion instead of computing a
    /// value it will throw away.
    pub(crate) const fn of(min: bool, max: bool) -> Option<Self> {
        match (min, max) {
            (true, true) => Some(Self::range()),
            (true, false) => Some(Self::single(LenReq::MinContent)),
            (false, true) => Some(Self::single(LenReq::MaxContent)),
            (false, false) => None,
        }
    }

    #[inline]
    pub(crate) fn includes(self, req: LenReq) -> bool {
        match self.single_req {
            Some(single) => single == req,
            None => true,
        }
    }

    /// This child's intrinsic under the same query.
    ///
    /// Halves the query didn't ask for come back as `0.0`. Read the
    /// result through [`IntrinsicRange::get`] inside an
    /// [`IntrinsicRange::requested`] loop and that can't bite — the two
    /// iterate the same set.
    #[inline]
    pub(crate) fn child(
        self,
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        engine.intrinsic_query(tree, node, axis, self, interned_text)
    }
}

/// Width of the `[f32; SLOT_COUNT]` array on `LayoutScratch.intrinsics`.
/// Equals `LenReq` variants × `Axis` variants. Adding a third variant
/// to either enum must update this constant and `LenReq::slot`; the
/// `const _:` below catches the array overflow at compile time.
pub(crate) const SLOT_COUNT: usize = 4;

impl LenReq {
    /// Index into `LayoutScratch.intrinsics[node]` for `(axis, self)`.
    /// Encoding lives next to the variant set so adding a `LenReq`
    /// surfaces here, not in `mod.rs`.
    #[inline]
    pub(crate) const fn slot(self, axis: Axis) -> usize {
        let a = match axis {
            Axis::X => 0,
            Axis::Y => 1,
        };
        let r = match self {
            LenReq::MinContent => 0,
            LenReq::MaxContent => 1,
        };
        a * 2 + r
    }
}

const _: () = {
    assert!(LenReq::MinContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MinContent.slot(Axis::Y) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::Y) < SLOT_COUNT);
};

/// Outer intrinsic on `axis`: content + padding + margin, respecting the
/// node's `Sizing` override and `min_size` / `max_size` clamps.
///
/// Pure function of the subtree at `node`. Engine caches the result; this
/// function is the cache miss path.
pub(crate) fn compute(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let layout = tree.records.layout()[node.idx()];
    if layout.meta.visibility().is_collapsed() {
        return IntrinsicRange::ZERO;
    }
    let bounds = tree.bounds(node);

    let sizing = axis.main_sizing(layout.size);
    let margin = axis.spacing(layout.margin);
    let min_clamp = axis.main(bounds.min_size);
    let max_clamp = axis.main(bounds.max_size);

    // Hug + Fill both report content-driven intrinsic: Fill in intrinsic
    // context returns its content's
    // intrinsic, ignoring weight — `AxisCtx::resolve` with `available =
    // INFINITY` enforces exactly that (Fill falls back to
    // `content_plus_padding`). Skip the content query and padding read
    // for Fixed: `AxisCtx::resolve` short-circuits Fixed and never
    // reads `content_plus_padding`.
    let mut content = if sizing.fixed_value().is_some() {
        IntrinsicRange::ZERO
    } else {
        let mut content = content_intrinsic(engine, tree, node, axis, query, interned_text, layout);
        let pad = axis.spacing(layout.padding);
        for (_, value) in content.requested(query) {
            *value += pad;
        }
        content
    };

    for (_, value) in content.requested(query) {
        *value = AxisCtx {
            sizing,
            content_plus_padding: *value,
            available: f32::INFINITY,
            intrinsic_min: 0.0,
            margin,
            min: min_clamp,
            max: max_clamp,
        }
        .resolve();
    }
    content
}

fn content_intrinsic(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
    layout: LayoutCore,
) -> IntrinsicRange {
    match LayoutMode::from(layout.meta) {
        LayoutMode::Leaf => leaf(engine, tree, node, axis, query, interned_text),
        LayoutMode::HStack => {
            stack::intrinsic(engine, tree, node, Axis::X, axis, query, interned_text)
        }
        LayoutMode::VStack => {
            stack::intrinsic(engine, tree, node, Axis::Y, axis, query, interned_text)
        }
        LayoutMode::WrapHStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::X, axis, query, interned_text)
        }
        LayoutMode::WrapVStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::Y, axis, query, interned_text)
        }
        LayoutMode::ZStack => zstack::intrinsic(engine, tree, node, axis, query, interned_text),
        LayoutMode::Canvas => canvas::intrinsic(engine, tree, node, axis, query, interned_text),
        LayoutMode::Grid(grid_def_id) => {
            grid::intrinsic(engine, tree, node, grid_def_id, axis, query, interned_text)
        }
        LayoutMode::Scrollbars(_) => scrollbars::intrinsic(),
        LayoutMode::Scroll(spec) => {
            scroll::intrinsic(engine, tree, node, spec, axis, query, interned_text)
        }
    }
}

/// Leaf: walk shapes and aggregate. Only `ShapeRecord::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
fn leaf(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let wid = tree.records.widget_id()[node.idx()];
    let mut range = IntrinsicRange::ZERO;
    for ts in TextShapeInput::on_leaf(tree, interned_text, node) {
        let unbounded = engine.text.root(
            TextRunSlot {
                widget_id: wid,
                ordinal: ts.ordinal,
            },
            ts.shape_request(),
            ts.wrap,
        );
        for (req, slot) in range.requested(query) {
            let run = match req {
                LenReq::MinContent => ts.wrap.min_content(&unbounded),
                LenReq::MaxContent => ts.wrap.max_content(&unbounded),
            };
            *slot = slot.max(axis.main(run));
        }
    }
    range
}

#[cfg(test)]
mod tests;
