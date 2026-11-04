//! The layout driver contract, and the one dispatch over [`LayoutMode`] that
//! reaches it: seven modules answer the same three questions about a subtree,
//! [`LayoutDriver`] is that agreement as a type and [`DriverOp::dispatch`] the
//! single match over it, so a new driver is one arm plus one impl.

mod canvas;
pub(crate) mod grid;
mod per_axis_hug;
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
use crate::layout::measured::ContentMeasure;
use crate::layout::pass::LayoutPass;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// One container node as its layout driver sees it: how it measures, arranges
/// and reports the intrinsic size of its children. Each driver module's value
/// carries the node and the per-instance config taken off its [`LayoutMode`]
/// variant (the pack axis of a stack, the def index of a grid or scrollbar
/// overlay, the axes of a scroll). **Every arm of [`DriverOp::dispatch`] calls
/// into one of these**, so a driver's policy lives in its own file;
/// [`Scrollbars`] contributes nothing to an intrinsic yet still answers.
pub(super) trait LayoutDriver: Copy {
    /// Bottom-up. Recurses through `pass.measure(..)` and returns the content size
    /// and floor, before padding, margin and clamping (which
    /// [`LayoutPass::measure`] folds in); see [`ContentMeasure`]. Called exactly once per
    /// measure: a `Fill` axis growing past `inner_avail` needs no re-measure
    /// (`NodeSlot::resolve` carries the reason).
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> ContentMeasure;

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

    /// The extent [`Self::arrange`] read from outside its subtree, on which its rects depend
    /// beside its slot; `None` for a driver whose arrange is a pure function of its slot.
    ///
    /// Two readers rest on it. The cascade folds it into the node's input, so damage sees a
    /// subtree whose rects moved with no change of its own. `LayoutPass::replay_arranged` copies
    /// the rects of a measure-hit subtree forward only for `None`: the hit proves its authoring
    /// unchanged, so with an identical slot its rects are too, and a driver that reads outside
    /// its subtree would replay stale rects with no panic. No default, so a new driver answers.
    fn outside_input(self, tree: &Tree, layout: &PlaneLayout) -> Option<Size>;
}

/// One operation applied to whichever driver a [`LayoutMode`] names; the match
/// lives once, in [`Self::dispatch`].
pub(super) trait DriverOp: Sized {
    /// What this pass answers with: a [`ContentMeasure`] for measure, nothing for
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

/// What the driver `mode` names read from outside its subtree to arrange it
/// ([`LayoutDriver::outside_input`]).
#[derive(Debug)]
pub(super) struct OutsideInputOp<'a> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a PlaneLayout,
}

impl DriverOp for OutsideInputOp<'_> {
    type Output = Option<Size>;

    fn run<D: LayoutDriver>(self, driver: D) -> Option<Size> {
        driver.outside_input(self.tree, self.layout)
    }

    fn leaf(self, _node: NodeId) -> Option<Size> {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::drivers::{DriverOp, OutsideInputOp};
    use crate::layout::plane_layout::PlaneLayout;
    use crate::primitives::geometry::size::Size;
    use crate::primitives::geometry::spacing::Spacing;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode};
    use crate::primitives::layout::scroll_axes::ScrollAxes;
    use crate::scene::scrollbars_def::{ResolvedScrollbarsDef, ScrollbarsDef};
    use crate::scene::tree::Tree;
    use crate::scene::tree::node_id::NodeId;
    use glam::Vec2;

    /// `Scrollbars` is the sole driver reading outside its subtree: it answers its viewport's
    /// content, node 1's here, and so never replays its rects. Every other driver answers `None`,
    /// and may replay; pinning both sides stops an outside read being added by reflex.
    #[test]
    fn only_scrollbars_reads_outside_its_subtree() {
        let mut tree = Tree::default();
        let def = tree.push_scrollbars_def(ResolvedScrollbarsDef {
            def: ScrollbarsDef {
                content: WidgetId::from_hash("viewport"),
                offset: Vec2::ZERO,
                zoom: 1.0,
                axes: ScrollAxes::BOTH,
                reserve: Spacing::ZERO,
                padding: Spacing::ZERO,
                bar_thickness: 4.0,
                min_thumb: 8.0,
            },
            content: NodeId(1),
        });
        let content = Size::new(30.0, 70.0);
        let mut layout = PlaneLayout::default();
        layout.scroll_content = vec![Size::ZERO, content];
        let outside = |mode| {
            OutsideInputOp {
                tree: &tree,
                layout: &layout,
            }
            .dispatch(NodeId(0), mode)
        };
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
            assert_eq!(
                outside(mode),
                None,
                "{mode:?} arranges from its own subtree, so it may replay",
            );
        }
        assert_eq!(
            outside(LayoutMode::Scrollbars(def)),
            Some(content),
            "Scrollbars reads its viewport's scroll_content and must never replay",
        );
    }
}
