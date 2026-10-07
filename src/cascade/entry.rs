//! The row tables the cascade walk produces and `input` consumes: per-node
//! [`EntryRow`]s for response lookup, the interactive-only [`HitRow`] table,
//! and the declared [`ScopeRow`]s a key press resolves against.

use crate::input::key_class::KeyFilter;
use crate::input::scroll_targets::ScrollTargets;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;

/// One per-node cascade row, in `Vec<EntryRow>` on
/// [`Cascade::entries`](super::Cascade::entries).
///
/// Array-of-structs on purpose: every consumer is a single-index gather
/// ([`crate::input::input_state::InputState::response_for`] reads all three
/// fields once per widget per frame), so interleaving them puts a lookup on
/// one cache line, where `soa_rs` columns used three. The opposite call from
/// `Tree.records: Soa<NodeRecord>`, whose columns the measure/arrange passes
/// walk. Hit testing reads [`HitRow`], not this table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EntryRow {
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    pub(crate) rect: Rect,
    /// The cumulative ancestor transform mapping this node's layout rect into
    /// unclipped surface space, `IDENTITY` when untransformed. Surfaced via
    /// `ResponseState::transform` to localize surface-space vectors.
    pub(crate) transform: TranslateScale,
    /// Effective disabled (self or any ancestor). Stored, not acted on:
    /// `ResponseState::merge_disabled` empties a disabled widget's response.
    pub(crate) disabled: bool,
}

/// One interactive row: a node whose effective `sense` is nonempty or which is
/// focusable. Pushed in paint order, so a reverse scan yields topmost-first.
///
/// Self-sufficient on purpose: carrying geometry and gates makes a hit test a
/// dense sequential scan over interactive rows, instead of a random access into
/// the all-node [`Cascade::entries`](super::Cascade::entries) per candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HitRow {
    /// Visible screen rect, the same as `EntryRow::rect`; duplicated to keep
    /// the scan sequential.
    pub(crate) rect: Rect,
    pub(crate) widget_id: WidgetId,
    /// Pointer interactions this row participates in (`HOVER` / `CLICK` /
    /// `DRAG` / `SCROLL`), already cascaded: `Sense::NONE` for an invisible
    /// subtree, as declared for a disabled one, which keeps absorbing what it
    /// covers.
    pub(crate) sense: Sense,
    /// Focus eligibility, checked by the focusable hit-test only; always
    /// `false` in a disabled or invisible subtree.
    pub(crate) focusable: bool,
    /// Disabled (this widget or any ancestor), as [`EntryRow::disabled`]. The
    /// pointer walks ignore it, since `ResponseState::merge_disabled` empties
    /// what a disabled row reads. Focus reads it:
    /// [`Cascade::hit_test_press`](super::Cascade::hit_test_press) ends its walk
    /// here, so a press this row absorbed focuses nothing beneath it.
    pub(crate) disabled: bool,
}

/// Where one widget's per-frame rows live: the flat
/// [`Cascade::entries`](super::Cascade::entries) index plus the `(layer, node)`
/// [`Endpoint`] the layout columns are keyed by. From
/// [`Cascade::locate`](super::Cascade::locate).
#[derive(Clone, Copy, Debug)]
pub(crate) struct WidgetLocation {
    pub(crate) entry_idx: u32,
    pub(crate) endpoint: Endpoint,
}

/// One declared input scope, in record order across every layer; the table
/// [`crate::input::input_state::InputState`] resolves a key press against.
///
/// Not a column on [`EntryRow`]: scopes are a handful per frame, and
/// containment is answered by [`Cascade::is_within`](super::Cascade::is_within).
/// Same lifecycle as [`HitRow`]: cleared on a full rebuild, repopulated by the
/// non-incremental walk, retained across incremental runs with matching
/// subtree hashes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScopeRow {
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
    pub(crate) filter: KeyFilter,
}

/// One Tab stop, in record order across every layer. A node is a stop when it
/// is focusable, kept its [`Configure::tab_stop`](crate::Configure::tab_stop),
/// and is neither disabled nor invisible (the rule [`HitRow::focusable`]
/// applies to a press). Same lifecycle as [`ScopeRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabStopRow {
    pub(crate) layer: Layer,
    /// The root of the tree the stop was recorded under; what a trap domain is named by.
    pub(crate) root: WidgetId,
    pub(crate) id: WidgetId,
    pub(crate) index: i16,
}

/// Which way a Tab press moves focus: Tab forward, Shift+Tab back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabDirection {
    Next,
    Previous,
}

/// The stops one Tab press may reach; see
/// [`Cascade::next_tab_stop`](super::Cascade::next_tab_stop).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabDomain {
    /// The stops recorded under one overlay root.
    Root(WidgetId),
    /// Every stop of one layer.
    Layer(Layer),
    /// The stops recorded under one arrow group.
    Group(WidgetId),
}

/// One arrow group (see [`Configure::arrow_focus`](crate::Configure::arrow_focus))
/// in record order across every layer. A node in a disabled or invisible
/// subtree is no group, as it is no stop. Same lifecycle as [`ScopeRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrowGroupRow {
    pub(crate) id: WidgetId,
    pub(crate) axis: Axis,
}

/// One root a layer recorded, in record order across every layer, for Tab
/// traversal to find the topmost open modal when it holds no stop. Same
/// lifecycle as [`ScopeRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootRow {
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
}

/// What a press lands on: the topmost clickable row under the point and the
/// topmost focusable one, from a single reverse scan. The answers are
/// independent (clicking a `Button` must not steal focus from a `TextEdit`)
/// except that a disabled row absorbs the press, so `focus` is whatever sat
/// above it, `None` if nothing did.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct PressTargets {
    pub(crate) click: Option<WidgetId>,
    pub(crate) focus: Option<WidgetId>,
}

/// Topmost interactive row under a point for each independent sense filter
/// (hover, each wheel axis, pinch), from one reverse scan.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitTargets {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: ScrollTargets,
    pub(crate) pinch: Option<WidgetId>,
}
