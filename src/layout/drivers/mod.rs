//! The layout driver contract, and the one dispatch over [`LayoutMode`] that
//! reaches it: seven modules answer the same three questions about a subtree,
//! [`LayoutDriver`] is that agreement as a type and [`DriverOp::dispatch`] the
//! single match over it, so a new driver is one arm plus one impl.

mod canvas;
pub(crate) mod grid;
mod scroll;
pub(crate) mod scrollbars;
pub(crate) mod stack;
pub(crate) mod wrapstack;
mod zstack;

use crate::layout::drivers::canvas::Canvas;
use crate::layout::drivers::grid::Grid;
use crate::layout::drivers::scroll::Scroll;
use crate::layout::drivers::scrollbars::Scrollbars;
use crate::layout::drivers::stack::Stack;
use crate::layout::drivers::wrapstack::WrapStack;
use crate::layout::drivers::zstack::ZStack;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::scene::tree::node_id::NodeId;

/// One container node as its layout driver sees it: how it measures, arranges
/// and reports the intrinsic size of its children. Each driver module's value
/// carries the node and the per-instance config taken off its [`LayoutMode`]
/// variant (the pack axis of a stack, the def index of a grid or scrollbar
/// overlay, the axes of a scroll). **Every arm of [`DriverOp::dispatch`] calls
/// into one of these**, so a driver's policy lives in its own file;
/// [`Scrollbars`] contributes nothing to an intrinsic yet still answers.
pub(super) trait LayoutDriver: Copy {
    /// Whether [`Self::arrange`] is a pure function of the slot it is handed,
    /// reading nothing outside its own subtree. `LayoutPass::replay_arranged` rests
    /// on this: a measure hit proves the subtree's authoring is unchanged, so with
    /// an identical slot its rects are copied forward. A driver reading *outside*
    /// its subtree breaks that silently (stale rects, no panic). No default, so a
    /// new driver must answer; `false` opts its subtree out of replay.
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool;

    /// Bottom-up. Recurses through `pass.measure(..)` and returns the content size
    /// and floor, before padding, margin and clamping (which
    /// [`LayoutPass::measure`] folds in); see [`Measured`]. Called exactly once per
    /// measure: a `Fill` axis growing past `inner_avail` needs no re-measure
    /// (`NodeSlot::resolve` carries the reason).
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured;

    /// Top-down. Assigns each child a final rect and recurses through
    /// `pass.arrange(..)`. **Local coordinates:** `inner` is the node's inner box
    /// size, children are placed in that box's coordinates, and
    /// `LayoutPass::arrange` moves each onto the page with a single add, so a moved
    /// subtree is rebuilt exactly (see `LayoutPass::replay_arranged`).
    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size);

    /// Pure on-demand query on [`IntrinsicQuery::axis`], the one step taking an
    /// [`IntrinsicPass`]: it must not reach the frame's text shapes. Driven by
    /// `Grid`'s Phase-1 column resolution and `Stack`'s Fill min-content floor. A
    /// driver whose answer depends on its pack axis compares it with the query's.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange;
}

/// One operation applied to whichever driver a [`LayoutMode`] names; the match
/// lives once, in [`Self::dispatch`].
pub(super) trait DriverOp: Sized {
    /// What this pass answers with: a [`Measured`] for measure, nothing for
    /// arrange, an [`IntrinsicWalk`](crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk)
    /// for the query.
    type Output;

    fn run<D: LayoutDriver>(self, driver: D) -> Self::Output;

    fn leaf(self, node: NodeId) -> Self::Output;

    /// Pick the driver `mode` names for `node` and run; the exhaustive match
    /// flags a missing arm.
    fn dispatch(self, node: NodeId, mode: LayoutMode) -> Self::Output {
        match mode {
            LayoutMode::Leaf => self.leaf(node),
            LayoutMode::Stack(axis) => self.run(Stack::new(node, axis)),
            LayoutMode::WrapStack(axis) => self.run(WrapStack::new(node, axis)),
            LayoutMode::ZStack => self.run(ZStack::new(node)),
            LayoutMode::Canvas => self.run(Canvas::new(node)),
            LayoutMode::Grid(def) => self.run(Grid::new(node, def)),
            LayoutMode::Scroll(axes) => self.run(Scroll::new(node, axes)),
            LayoutMode::Scrollbars(def) => self.run(Scrollbars::new(node, def)),
        }
    }
}

/// Whether the driver `mode` names may replay its arranged rects.
#[derive(Debug)]
pub(super) struct ReplayOp;

impl DriverOp for ReplayOp {
    type Output = bool;

    fn run<D: LayoutDriver>(self, _driver: D) -> bool {
        D::ARRANGE_DEPENDS_ONLY_ON_SLOT
    }

    fn leaf(self, _node: NodeId) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::drivers::{DriverOp, ReplayOp};
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
    use crate::primitives::layout::scroll_axes::ScrollAxes;
    use crate::scene::tree::node_id::NodeId;

    /// `Scrollbars` is the sole driver reading outside its subtree, and this flag
    /// alone prevents stale rects; pinning both sides stops a `true` being added by
    /// reflex.
    #[test]
    fn only_scrollbars_opts_out_of_arrange_replay() {
        let slot_pure = [
            LayoutMode::Leaf,
            LayoutMode::Stack(Axis::X),
            LayoutMode::Stack(Axis::Y),
            LayoutMode::WrapStack(Axis::X),
            LayoutMode::WrapStack(Axis::Y),
            LayoutMode::ZStack,
            LayoutMode::Canvas,
            LayoutMode::Grid(GridDefId::from_idx(0)),
            LayoutMode::Scroll(ScrollAxes::BOTH),
        ];
        for mode in slot_pure {
            assert!(
                ReplayOp.dispatch(NodeId(0), mode),
                "{mode:?} arranges from its own subtree, so it may replay",
            );
        }
        assert!(
            !ReplayOp.dispatch(
                NodeId(0),
                LayoutMode::Scrollbars(ScrollbarsDefId::from_idx(0))
            ),
            "Scrollbars reads a sibling's scroll_content and must never replay",
        );
    }
}
