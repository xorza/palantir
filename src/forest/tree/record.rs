//! Per-layer recording-only state, kept off [`Tree`](super::Tree) so
//! downstream passes holding `&Tree` are type-prevented from reaching
//! transient state — `Tree` itself is the finalized output. The
//! ancestor stack + pending layer anchor ([`RecordingScratch`]) are
//! cleared by `Forest::pre_record`; [`RootSlot`] / [`PendingAnchor`]
//! carry per-root placement minted during recording.

use glam::Vec2;

use crate::forest::tree::NodeId;
use crate::primitives::size::Size;

/// One entry on the recording ancestor stack
/// ([`RecordingScratch::open_frames`]). Carries the open node's
/// `NodeId` and a precomputed `disabled` cascade bit
/// (`parent.ancestor_or_self_disabled || new_node.disabled`) so
/// [`RecordingScratch::ancestor_disabled`] is an O(1) read. The node's
/// resolved `WidgetId` is read on demand via
/// `records.widget_id()[node.idx()]` at the one site that needs it
/// (`Ui::widget_id`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    pub(crate) ancestor_or_self_disabled: bool,
}

/// Per-layer recording-only state: the ancestor stack and the pending
/// layer anchor. Lives off `Tree` so every downstream pass holding
/// `&Tree` is type-prevented from reaching transient state — `Tree`
/// itself is the finalized output. Cleared by `Forest::pre_record`;
/// drained at every top-level `close_node`.
#[derive(Default)]
pub(crate) struct RecordingScratch {
    /// Ancestor stack for the currently-open scope. Empty outside the
    /// `pre_record` ↔ root `close_node` window. Capacity retained across
    /// frames.
    ///
    /// Each frame carries a precomputed `ancestor_or_self_disabled` bit:
    /// on push, OR the new node's `disabled` with the parent frame's
    /// bit. That makes [`Self::ancestor_disabled`] a one-element load
    /// (read from `last()`) instead of an O(depth) walk.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// Anchor + optional size cap for the active `Forest::push_layer`
    /// scope. `Some` between `push_layer` and `pop_layer`; root mints
    /// inside the scope read it (don't consume — multiple roots share
    /// the same anchor). `None` outside any scope and always on `Main`
    /// (its implicit root paints the full surface); in that case root
    /// mints fall through to `PendingAnchor::default()` =
    /// `(Vec2::ZERO, None)`. `Forest::push_layer` asserts no nesting,
    /// so a single slot suffices.
    pub(crate) pending_anchor: Option<PendingAnchor>,
}

impl RecordingScratch {
    pub(crate) fn clear(&mut self) {
        self.open_frames.clear();
        self.pending_anchor = None;
    }

    /// True when any currently-open ancestor in the active recording
    /// scope has `disabled=true`. Lets widgets see inherited-disabled
    /// at record time, in the *same* frame the ancestor was opened —
    /// `cascade.disabled` is one frame stale, so without this an
    /// inherited-disabled child paints alive on first appearance and
    /// then animates to disabled. O(1): the bit is propagated on
    /// `open_node` push, so `last()` already encodes the OR over the
    /// whole open chain.
    #[inline]
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.open_frames
            .last()
            .is_some_and(|f| f.ancestor_or_self_disabled)
    }
}

/// One root within a single layer's [`Tree`](super::Tree). Multiple
/// roots in the same tree happen for popups (eater + body recorded as
/// two top-level scopes) and any future `Ui::layer` scope that opens
/// non-contiguous top-level subtrees in the same layer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: NodeId,
    /// Top-left placement in screen space. `Vec2::ZERO` for `Main`;
    /// set by `Forest::push_layer` for side layers.
    pub(crate) anchor: Vec2,
    /// Caller-supplied size cap (side layers only). `None` means
    /// "fill from `anchor` to the surface bottom-right" — the dropdown /
    /// tooltip default. `Some(s)` is anchor-independent: `available =
    /// min(s, surface)`, so the body can measure against its full
    /// natural size regardless of where it'll paint. The caller takes
    /// responsibility for placement in that mode (typically via a
    /// popup's flip-then-clamp). Always `None` for `Main`.
    pub(crate) size: Option<Size>,
}

/// Pending anchor entry for `Tree::pending_anchor`. Populated by
/// `Forest::push_layer`, consumed by root mints inside the scope, and
/// cleared by `pop_layer`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PendingAnchor {
    pub(crate) anchor: Vec2,
    pub(crate) size: Option<Size>,
}
