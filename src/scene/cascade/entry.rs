//! The row tables the cascade walk produces and `input` consumes:
//! per-node [`EntryRow`]s for response lookup, the interactive-only
//! [`HitRow`] table the hit tests scan, and the declared [`ScopeRow`]s
//! a key press resolves against.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::seen_ids::Endpoint;

/// One per-node cascade row, in `Vec<EntryRow>` on
/// [`Cascade::entries`](super::Cascade::entries).
///
/// **Array-of-structs on purpose.** Every consumer of this table is a
/// single-index gather, never a column walk:
/// [`crate::input::input_state::InputState::response_for`] reads all three fields at
/// one index once per widget per frame, and `pointer_local_for` reads
/// two. Splitting them into `soa_rs` columns put those fields on three
/// separate cache lines per lookup; interleaved they share one 32-byte
/// row. This is the opposite call from `Tree.records: Soa<NodeRecord>`,
/// which *is* column-walked by the measure/arrange passes — the access
/// pattern, not the row count, is what picks the layout.
///
/// Hit testing does not read this table at all: [`HitRow`] carries its
/// own geometry so the hit scan stays sequential over interactive rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EntryRow {
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    pub(crate) rect: Rect,
    /// The cumulative ancestor transform mapping this node's layout rect
    /// into unclipped surface space. The visible `rect` may be smaller
    /// after ancestor clipping. Surfaced via `ResponseState::transform`
    /// for converting surface-space vectors into widget-local logical
    /// coordinates — `IDENTITY` when untransformed.
    pub(crate) transform: TranslateScale,
    /// Effective disabled (self OR any ancestor). Mirrors what
    /// `cascaded_off` already nulls `sense`/`focusable` from,
    /// kept here so per-widget responses can read it.
    pub(crate) disabled: bool,
}

/// One interactive row — a node whose effective `sense` is nonempty or
/// which is focusable. Pushed in paint order, so a reverse scan yields
/// topmost-first.
///
/// **Self-sufficient on purpose.** This table carries the geometry and
/// gates the hit tests need so a hit test is a dense sequential scan over
/// interactive rows alone, with no indirection into the all-node
/// [`Cascade::entries`](super::Cascade::entries). Holding only
/// `(entry_idx, widget_id)` and gathering `rect` / `sense` / `focusable`
/// from `entries` instead costs one random access per candidate row, over a
/// table sized by *every* node rather than the interactive subset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HitRow {
    /// Visible screen rect — the same value as `EntryRow::rect` for
    /// this node. Duplicated into this row deliberately: it is what
    /// makes the scan sequential, and interactive rows are a small
    /// fraction of all nodes.
    pub(crate) rect: Rect,
    pub(crate) widget_id: WidgetId,
    /// Pointer interactions this row participates in (`HOVER` / `CLICK`
    /// / `DRAG` / `SCROLL`). Already cascaded — `Sense::NONE` for a
    /// disabled or invisible subtree.
    pub(crate) sense: Sense,
    /// Focus eligibility — checked by the focusable hit-test only.
    pub(crate) focusable: bool,
}

/// Where one widget's per-frame rows live — the flat
/// [`Cascade::entries`](super::Cascade::entries) index plus the
/// `(layer, node)` [`Endpoint`] the layout columns are keyed by. Produced
/// by [`Cascade::locate`](super::Cascade::locate).
#[derive(Clone, Copy, Debug)]
pub(crate) struct WidgetLocation {
    pub(crate) entry_idx: u32,
    pub(crate) endpoint: Endpoint,
}

/// One declared input scope, in record order across every layer — the
/// table [`crate::input::input_state::InputState`] resolves a key press against.
///
/// Deliberately *not* a column on [`EntryRow`]: scopes are a handful per
/// frame while entries are one per node, and containment is answered by
/// [`Cascade::is_within`](super::Cascade::is_within) rather than by
/// anything stored here. Same lifecycle as [`HitRow`] — cleared on a full
/// rebuild, repopulated by the non-incremental walk, retained across
/// incremental runs whose subtree hashes matched.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeRow {
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
    pub(crate) filter: KeyFilter,
}

/// What a press lands on: the topmost clickable row under the point and
/// the topmost focusable one, from a single reverse scan.
///
/// Two slots rather than two calls because the answers are independent —
/// clicking a `Button` must not steal focus from a `TextEdit`, so the
/// press target and the focus target are routinely different rows — and
/// resolving them separately walked the same hit table twice for one
/// position.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PressTargets {
    pub(crate) click: Option<WidgetId>,
    pub(crate) focus: Option<WidgetId>,
}

/// Topmost interactive row under a point for each of three
/// independent sense filters — the result of one reverse scan.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitTargets {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: Option<WidgetId>,
    pub(crate) pinch: Option<WidgetId>,
}
