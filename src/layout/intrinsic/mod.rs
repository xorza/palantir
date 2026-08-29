//! Intrinsic-dimensions queries — the on-demand `LenReq` API.
//!
//! This module owns:
//! - The query type `LenReq`.
//! - The central `compute()` dispatch that handles `Sizing` overrides,
//!   padding/margin, and `min_size`/`max_size` clamps before delegating to
//!   each driver's `intrinsic()` for content-driven sizes.
//! - Leaf intrinsics (no driver module owns leaves).
//!
//! Per-driver intrinsic logic lives alongside that driver's
//! `measure`/`arrange`, in its
//! [`LayoutDriver`](crate::layout::driver::LayoutDriver) impl — same
//! per-driver-file convention as the rest of layout.

use crate::layout::axis::Axis;
use crate::layout::axis_slot::AxisSlot;
use crate::layout::driver::{DriverOp, LayoutDriver};
use crate::layout::engine::LayoutEngine;
use crate::layout::text_shape_input::TextShapeInput;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_text::InternedText;
use crate::primitives::size::Size;
use crate::scene::node::bounds_extras::BoundsExtras;
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

/// What one intrinsic walk answered, per axis.
///
/// A driver answers the axis it was asked about and nothing else — "how
/// tall given you pack across" is a different recursion from "how wide",
/// so the other axis costs a second walk. A leaf answers both at once:
/// its content is a shaped run's min-content and max-content, and those
/// are `Size`s, so the axis a query names only picks a lane of a value
/// the walk already holds.
///
/// [`LayoutPass::measure`](crate::layout::pass::LayoutPass) asks every
/// node for min-content on both axes. Carrying the free lane back is
/// what lets the engine record it, so the second of those two queries
/// reads the frame's slot array instead of shaping the leaf's runs
/// again.
#[derive(Copy, Clone, Debug)]
pub(crate) struct IntrinsicWalk {
    /// The axis the query named.
    pub(crate) answered: IntrinsicRange,
    /// The other axis, when the walk covered it for free.
    pub(crate) sibling: Option<IntrinsicRange>,
}

impl IntrinsicWalk {
    #[inline]
    const fn one_axis(answered: IntrinsicRange) -> Self {
        Self {
            answered,
            sibling: None,
        }
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
    /// `ZStack::measure` and `Canvas::measure`).
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
/// node's `Sizing` override and `min_size` / `max_size` clamps. The other
/// axis rides along whenever the walk covered it — see [`IntrinsicWalk`].
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
) -> IntrinsicWalk {
    let layout = tree.records.layout()[node.idx()];
    if layout.meta.visibility().is_collapsed() {
        return IntrinsicWalk::one_axis(IntrinsicRange::ZERO);
    }

    // Hug + Fill both report content-driven intrinsic: Fill in intrinsic
    // context returns its content's intrinsic, ignoring weight —
    // `AxisSlot::resolve` with `available = INFINITY` enforces exactly that
    // (Fill falls back to `content_plus_padding`). Skip the content query
    // for Fixed: `AxisSlot::resolve` short-circuits Fixed and never reads
    // `content_plus_padding`.
    let content = if axis.main_sizing(layout.size).fixed_value().is_some() {
        IntrinsicWalk::one_axis(IntrinsicRange::ZERO)
    } else {
        content_intrinsic(engine, tree, node, axis, query, interned_text, layout)
    };

    let bounds = tree.bounds(node);
    IntrinsicWalk {
        answered: outer(layout, bounds, axis, query, content.answered),
        sibling: content
            .sibling
            .map(|raw| outer(layout, bounds, axis.other(), query, raw)),
    }
}

/// Wrap a raw content range in the node's own box on `axis`: padding, the
/// `Sizing` override, margin, and the `min_size` / `max_size` clamps.
///
/// Padding is added unconditionally. A Fixed axis arrives with a zero
/// content range, and `AxisSlot::resolve` returns the declared value
/// without reading either — so the add cannot reach the result.
fn outer(
    layout: LayoutCore,
    bounds: &BoundsExtras,
    axis: Axis,
    query: IntrinsicQuery,
    mut content: IntrinsicRange,
) -> IntrinsicRange {
    let pad = axis.spacing(layout.padding);
    let slot = AxisSlot {
        sizing: axis.main_sizing(layout.size),
        available: f32::INFINITY,
        intrinsic_min: 0.0,
        margin: axis.spacing(layout.margin),
        min: axis.main(bounds.min_size),
        max: axis.main(bounds.max_size),
    };
    for (_, value) in content.requested(query) {
        *value = slot.resolve(*value + pad);
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
) -> IntrinsicWalk {
    IntrinsicOp {
        engine,
        tree,
        node,
        axis,
        query,
        interned_text,
    }
    .dispatch(LayoutMode::from(layout.meta))
}

/// The only [`DriverOp`] of the three that carries no `LayoutPass`. That is the
/// point: a pure query of a subtree must not be able to write the frame's
/// text shapes, and holding the engine and the tree separately is what
/// keeps a `LayerLayout` out of reach.
#[derive(Debug)]
struct IntrinsicOp<'op, 'text> {
    engine: &'op mut LayoutEngine,
    tree: &'op Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &'op InternedText<'text>,
}

impl DriverOp for IntrinsicOp<'_, '_> {
    type Output = IntrinsicWalk;

    fn run<D: LayoutDriver>(self, payload: D::Payload) -> IntrinsicWalk {
        let Self {
            engine,
            tree,
            node,
            axis,
            query,
            interned_text,
        } = self;
        IntrinsicWalk::one_axis(D::intrinsic(
            engine,
            tree,
            node,
            payload,
            axis,
            query,
            interned_text,
        ))
    }

    fn leaf(self) -> IntrinsicWalk {
        let Self {
            engine,
            tree,
            node,
            axis,
            query,
            interned_text,
        } = self;
        leaf(engine, tree, node, axis, query, interned_text)
    }
}

/// Leaf: walk shapes and aggregate. Only `ShapeRecord::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
///
/// A run's content demands are `Size`s, so the accumulators are too and
/// both axes fall out of the same pass. `axis` picks the answered lane
/// at the end; see [`IntrinsicWalk`] for what the other one buys.
fn leaf(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicWalk {
    let wid = tree.records.widget_id()[node.idx()];
    let mut min_content = Size::ZERO;
    let mut max_content = Size::ZERO;
    for ts in TextShapeInput::on_leaf(tree, interned_text, node) {
        let unbounded = engine.text.root(
            TextRunSlot {
                widget_id: wid,
                ordinal: ts.ordinal,
            },
            ts.shape_request(),
            ts.wrap,
        );
        if query.includes(LenReq::MinContent) {
            min_content = min_content.max(ts.wrap.min_content(&unbounded));
        }
        if query.includes(LenReq::MaxContent) {
            max_content = max_content.max(ts.wrap.max_content(&unbounded));
        }
    }
    IntrinsicWalk {
        answered: IntrinsicRange {
            min: axis.main(min_content),
            max: axis.main(max_content),
        },
        sibling: Some(IntrinsicRange {
            min: axis.cross(min_content),
            max: axis.cross(max_content),
        }),
    }
}

#[cfg(test)]
mod tests;
