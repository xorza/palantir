//! Per-layer recording-only state, kept off [`Tree`](super::Tree) so
//! downstream passes holding `&Tree` are type-prevented from reaching
//! transient state — `Tree` itself is the finalized output. The
//! ancestor stack + pending root placement ([`RecordingScratch`]) are
//! cleared by `Forest::pre_record`; [`RootSlot`] / [`Placement`]
//! carry per-root placement minted during recording.

use glam::Vec2;

use crate::forest::tree::node::NodeId;
use crate::layout::types::overlay::OverlayPosition;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;

/// One entry on the recording ancestor stack
/// ([`RecordingScratch::open_frames`]). Carries the open node's
/// `NodeId` and a precomputed `disabled` cascade bit
/// (`parent.ancestor_or_self_disabled || new_node.disabled`) so
/// [`RecordingScratch::ancestor_disabled`] is an O(1) read. The node's
/// resolved `WidgetId` is read on demand via
/// `records.id[node.idx()]` at the one site that needs it
/// (`Ui::widget_id`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    pub(crate) ancestor_or_self_disabled: bool,
    /// Paint-arena rows this node's span holds so far: the chrome row
    /// (seeded to 1 when a `ChromeRow` was allocated at open) plus one
    /// per direct shape / immediate child, bumped in record order.
    /// Mirrors the row stream `cascade::compute_paint_rect` emits, so
    /// an animated shape can record its own row index at add time
    /// (`PaintAnimEntry::row`) instead of damage re-deriving it from a
    /// `TreeItems` walk every frame.
    pub(crate) paint_rows: u32,
}

/// Per-layer recording-only state: the ancestor stack and pending root
/// placement. Lives off `Tree` so every downstream pass holding
/// `&Tree` is type-prevented from reaching transient state — `Tree`
/// itself is the finalized output. Cleared by `Forest::pre_record`;
/// drained at every top-level `close_node`.
#[derive(Debug, Default)]
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

    /// Placement for the active `Forest::push_layer` scope. Root mints
    /// inside the scope read it without consuming it because multiple
    /// roots can share the policy. `Main` falls through to
    /// `Placement::default()`.
    pub(crate) pending_placement: Option<Placement>,
}

impl RecordingScratch {
    pub(crate) fn clear(&mut self) {
        self.open_frames.clear();
        self.pending_placement = None;
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
    pub(crate) placement: Placement,
}

/// Measurement and post-measure placement policy for one layer root.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Placement {
    Fixed { anchor: Vec2, size: Option<Size> },
    Overlay(OverlayPosition),
}

impl Placement {
    pub(crate) const fn fixed(anchor: Vec2, size: Option<Size>) -> Self {
        Self::Fixed { anchor, size }
    }

    pub(crate) const fn overlay(position: OverlayPosition) -> Self {
        Self::Overlay(position)
    }

    pub(crate) fn available(self, surface: Rect) -> Size {
        match self {
            Self::Fixed { anchor, size: None } => {
                let remaining = (surface.max() - anchor).max(Vec2::ZERO);
                Size::new(remaining.x, remaining.y)
            }
            Self::Fixed {
                size: Some(size), ..
            } => Size::new(size.w.min(surface.size.w), size.h.min(surface.size.h)),
            Self::Overlay(_) => surface.size,
        }
    }

    pub(crate) fn origin(self, measured: Size, surface: Rect) -> Vec2 {
        match self {
            Self::Fixed { anchor, .. } => anchor,
            Self::Overlay(position) => position.resolve(measured, surface),
        }
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::fixed(Vec2::ZERO, None)
    }
}
