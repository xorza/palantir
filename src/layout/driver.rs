//! The layout driver contract, and the one dispatch over [`LayoutMode`]
//! that reaches it.
//!
//! Seven modules answer the same three questions about a subtree.
//! [`LayoutDriver`] is that agreement as a type rather than as a doc
//! comment over seven free functions, and [`DriverOp::dispatch`] is the
//! single match over it — so a new driver is one arm plus one impl, and
//! the compiler asks for both. Spread across free functions instead, an
//! argument can sit third in one match and fourth in the next, and
//! nothing says which files a new driver has to reach.

use crate::layout::axis::Axis;
use crate::layout::canvas::Canvas;
use crate::layout::engine::LayoutEngine;
use crate::layout::grid::Grid;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange};
use crate::layout::pass::LayoutPass;
use crate::layout::scroll::Scroll;
use crate::layout::scrollbars::Scrollbars;
use crate::layout::stack::Stack;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::wrapstack::WrapStack;
use crate::layout::zstack::ZStack;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// One layout driver: how a container measures, arranges and reports the
/// intrinsic size of its children.
///
/// Implemented on a unit marker per driver module, so the three passes
/// name a driver the same way and the payload sits in the same place in
/// all three signatures.
///
/// **Every arm of [`DriverOp::dispatch`] is a call into one of these**, so
/// a driver's policy lives in its own file and the dispatch stays
/// dispatch. [`Scrollbars`] contributes nothing to an intrinsic and still
/// answers rather than being written off as a `ZERO` inline, because
/// "what does this driver contribute" is the driver's answer to give.
pub(super) trait LayoutDriver {
    /// Per-instance config the driver takes off its [`LayoutMode`]
    /// variant: the pack axis for the two stack pairs, the def index for
    /// a grid or a scrollbar overlay, the spec for a scroll. `()` where
    /// the variant carries none.
    ///
    /// One function pair per pack orientation rather than one per
    /// variant, which is why `HStack` and `VStack` are the same driver
    /// with a different payload.
    type Payload: Copy;

    /// Whether this driver's [`Self::arrange`] is a pure function of the
    /// slot it is handed — reading nothing outside its own subtree and
    /// that rect.
    ///
    /// `LayoutPass::replay_arranged` rests on exactly this: a measure hit
    /// proves the subtree's authoring is unchanged, so given an identical
    /// slot its rects can be copied forward instead of re-derived. A
    /// driver that reads *outside* its subtree breaks the implication —
    /// its inputs can move while its own hash and slot sit still — and the
    /// damage is silent: stale rects, no panic, nothing that fails to
    /// compile. It surfaces only as a visual bug, which is how a scrollbar
    /// once survived the content that justified it.
    ///
    /// No default, so a new driver has to answer, in the file where the
    /// reason lives. One that answers `false` opts its whole subtree out
    /// of replay.
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool;

    /// Bottom-up. Recurses into children through `pass.measure(..)` and
    /// returns the driver's content size — before padding, margin and
    /// clamping, which [`LayoutPass::measure`] folds in.
    ///
    /// Called exactly once per measure. A `Fill` axis that grows past
    /// `inner_avail` needs no re-measure; `AxisSlot::resolve_node` carries
    /// the reason.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        payload: Self::Payload,
        inner_avail: Size,
    ) -> Size;

    /// Top-down. Assigns each child a final rect and recurses through
    /// `pass.arrange(..)`.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, payload: Self::Payload, inner: Rect);

    /// Pure on-demand query, and the one step that takes no pass: it must
    /// not reach the frame's text shapes. Driven by `Grid`'s Phase-1
    /// column resolution and `Stack`'s Fill min-content floor.
    ///
    /// `axis` is the axis being asked about. A driver whose answer also
    /// depends on the axis it packs along reads that off `payload` — "how
    /// tall given you pack across" is a different question from "how
    /// wide", and only the stacks have a pack axis to tell them apart.
    fn intrinsic(
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        payload: Self::Payload,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange;
}

/// One operation applied to whichever driver a [`LayoutMode`] names.
///
/// The three passes each dispatch over the same ten variants. Written out
/// three times, a new driver was three arms in three files, and the
/// compiler could only ask for the next one once the last was written.
/// Here the match lives once, in [`Self::dispatch`], and a pass is an
/// implementor.
pub(super) trait DriverOp: Sized {
    /// What this pass answers with: a content [`Size`] for measure,
    /// nothing for arrange, an [`IntrinsicRange`] for the query.
    type Output;

    /// Run against the driver `D` and the payload its variant carries.
    fn run<D: LayoutDriver>(self, payload: D::Payload) -> Self::Output;

    /// A leaf has no driver — the pass answers for it directly.
    fn leaf(self) -> Self::Output;

    /// Pick the driver `mode` names and run. The compiler flags a missing
    /// arm here because [`LayoutMode`] matches are exhaustive.
    fn dispatch(self, mode: LayoutMode) -> Self::Output {
        match mode {
            LayoutMode::Leaf => self.leaf(),
            LayoutMode::Stack(axis) => self.run::<Stack>(axis),
            LayoutMode::WrapStack(axis) => self.run::<WrapStack>(axis),
            LayoutMode::ZStack => self.run::<ZStack>(()),
            LayoutMode::Canvas => self.run::<Canvas>(()),
            LayoutMode::Grid(id) => self.run::<Grid>(id),
            LayoutMode::Scroll(spec) => self.run::<Scroll>(spec),
            LayoutMode::Scrollbars(id) => self.run::<Scrollbars>(id),
        }
    }
}

/// Whether the driver `mode` names may replay its arranged rects instead
/// of re-deriving them, as the [`DriverOp`] that asks.
#[derive(Debug)]
pub(super) struct ReplayOp;

impl DriverOp for ReplayOp {
    type Output = bool;

    fn run<D: LayoutDriver>(self, _payload: D::Payload) -> bool {
        D::ARRANGE_DEPENDS_ONLY_ON_SLOT
    }

    /// A leaf places no children, so copying its subtree forward is
    /// trivially sound.
    fn leaf(self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::axis::Axis;
    use crate::layout::driver::{DriverOp, ReplayOp};
    use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ScrollSpec, ScrollbarsDefId};

    /// `Scrollbars` is the sole driver that reads outside its own subtree,
    /// and the only thing standing between that and silently stale rects
    /// is this flag. Pinning both sides keeps a future `true` from being
    /// added by reflex — the failure mode is invisible at runtime.
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
            LayoutMode::Grid(GridDefId::from_index(0)),
            LayoutMode::Scroll(ScrollSpec::BOTH),
        ];
        for mode in slot_pure {
            assert!(
                ReplayOp.dispatch(mode),
                "{mode:?} arranges from its own subtree, so it may replay",
            );
        }
        assert!(
            !ReplayOp.dispatch(LayoutMode::Scrollbars(ScrollbarsDefId::from_index(0))),
            "Scrollbars reads a sibling's scroll_content and must never replay",
        );
    }
}
