//! Public element authoring data and the builder configuration surface.

pub(crate) mod columns;

use crate::forest::element::columns::{
    BoundsExtras, ElementColumns, Gaps, LayoutCore, NodeFlags, PanelExtras,
};
use crate::forest::visibility::Visibility;
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ModePayload, ScrollSpec};
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use std::hash::Hash;

/// Recipe for an [`Element`]'s `WidgetId`. Mirrors egui's
/// `Option<Id>` "raw `id_salt`, resolve at `Ui::widget_id`"
/// pattern: the builder stores the user's intent, resolution happens
/// at record time when the parent context is known. Three sources:
///
/// - [`Salt::Auto`] — `#[track_caller]`-derived. The captured
///   `(file, line, column)` encodes call-site identity, but a call
///   site reached from a loop or helper resolves to the *same* base id
///   for every iteration, so identity must also depend on **where in
///   the tree** the widget sits. So an auto id is **parent-scoped**
///   too: mixed with the most-recently-opened parent's resolved
///   `WidgetId`, exactly like [`Salt::Hash`]. This is what keeps two
///   nodes drawn from one `draw_one` helper — whose interior text /
///   shape leaves share an auto call site — from swapping ids when the
///   nodes' paint order flips: each leaf hangs off its own stable-id
///   node body, so a raise/reorder can't churn its identity (and thus
///   can't spuriously damage or re-key state for untouched nodes).
///   Same-parent collisions from a genuine sibling loop are still
///   disambiguated by `SeenIds`' occurrence counter.
///
/// - [`Salt::Hash`] — raw user-supplied hash from `.id_salt(key)`.
///   At resolve time the hash is **mixed with the most-recently-
///   opened parent's resolved `WidgetId`** in the current layer
///   (`Layer::Main`'s synthetic viewport counts as a parent — its
///   `Salt::Auto` id is stable across frames). Two `.id_salt("row")`
///   under different parents resolve to distinct ids, so per-widget
///   `StateMap` / focus / animation entries survive subtree moves
///   without manual `WidgetId::with` chaining. Matches egui.
///
/// - [`Salt::Verbatim`] — precomputed [`WidgetId`] from `.id(id)`,
///   used as-is. Escape hatch for ids derived elsewhere
///   (cross-layer popups, sibling pairs sharing a seed). The **only**
///   variant that skips parent-scoping.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Salt {
    Auto(WidgetId),
    Hash(WidgetId),
    Verbatim(WidgetId),
}

impl Salt {
    /// Mix `self` with `parent`'s already-resolved `WidgetId` to
    /// produce the id that will be recorded into the tree.
    /// [`Salt::Auto`] and [`Salt::Hash`] both consult `parent` (so a
    /// widget's identity tracks its position in the tree, not its
    /// global record order); only [`Salt::Verbatim`] passes through.
    /// `parent == None` covers the "no open node at all" case (the
    /// root of a side layer). `Layer::Main`'s synthetic viewport
    /// counts as a parent with a frame-stable id, so top-level widgets
    /// resolve to `VIEWPORT.with(salt)` like any other parent-scoped
    /// id.
    #[inline]
    pub(crate) fn resolve(self, parent: Option<WidgetId>) -> WidgetId {
        match self {
            Salt::Verbatim(id) => id,
            Salt::Auto(id) | Salt::Hash(id) => match parent {
                Some(p) => p.with(id.0),
                None => id,
            },
        }
    }

    /// `true` for [`Salt::Hash`] / [`Salt::Verbatim`] — caller-supplied
    /// ids. `SeenIds::record` uses this to flag explicit collisions
    /// (caller bugs) with the magenta debug overlay while leaving
    /// auto collisions silent.
    #[inline]
    pub(crate) fn is_explicit(self) -> bool {
        matches!(self, Salt::Hash(_) | Salt::Verbatim(_))
    }
}

/// Per-node config: identity + spatial layout + interaction + paint flags.
/// Every widget builder owns one and forwards it to `Ui::node`. `Configure` (the
/// trait below) gives chained setters for all fields by impl'ing one method.
///
/// Fields are grouped by who reads them: identity, own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and paint.
#[derive(Clone, Copy, Debug)]
pub struct Element {
    /// Recipe for this node's `WidgetId`. Resolution happens inside
    /// [`crate::Ui::node`] (and the parallel [`crate::Ui::widget_id`]
    /// callable by widgets that need the recorded id pre-`node` for
    /// theme picking / focus / animation slots) — `Element` itself
    /// never carries a resolved id. Mirrors egui's "builder stores
    /// raw `id_salt`, `Ui::widget_id` mixes in the parent's
    /// id at `.show()`" pattern.
    pub(crate) salt: Salt,
    pub(crate) mode: LayoutMode,
    /// `LayoutMode::Grid` arena idx. Set by `Grid::show` once the def is
    /// pushed; ignored for every other `mode`.
    pub(crate) mode_payload: ModePayload,

    pub(crate) size: Sizes,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,

    /// Within-line gap + between-line gap packed as two f16 lanes.
    /// `gaps.gap()` (HStack/VStack/WrapHStack/WrapVStack) is the
    /// sibling spacing; `gaps.line_gap()` (WrapHStack/WrapVStack only)
    /// is the row/column spacing. Both ignored by Leaf/ZStack/Canvas/
    /// Grid (Grid uses its own row_gap/col_gap).
    pub(crate) gaps: Gaps,

    /// Main-axis distribution of leftover space (HStack/VStack only).
    pub(crate) justify: Justify,
    /// Own alignment within the parent's inner rect.
    pub(crate) align: Align,
    /// Default alignment applied to children with `Auto` axis (panels only).
    pub(crate) child_align: Align,
    /// Absolute position inside a `Canvas` parent (parent-inner coordinates).
    /// Defaults to `Vec2::ZERO`. Ignored when the parent isn't a `Canvas`.
    pub(crate) position: Vec2,
    /// Cell + span inside a `Grid` parent. Defaults to `(0, 0)` placement and
    /// `(1, 1)` span. Ignored when the parent isn't a `Grid`.
    pub(crate) grid: GridCell,

    /// Packed paint/input flags (sense, disabled, focusable, clip).
    /// Two bytes, mirrors the `NodeFlags` column `into_columns` writes
    /// to — fan-out is a single `u16` copy.
    pub(crate) flags: NodeFlags,

    /// WPF-style three-state visibility. `Hidden` keeps the node's slot in
    /// layout but suppresses paint + input; `Collapsed` zeros the slot and
    /// skips the subtree everywhere. Lives on `LayoutCore` (not `NodeFlags`)
    /// because measure's fast-path reads it next to size/margin.
    pub(crate) visibility: Visibility,
    /// Pan/zoom applied to descendants (post-layout, like WPF's `RenderTransform`).
    /// `TranslateScale::IDENTITY` = no transform. The transform composes
    /// with any ancestor transform; descendants render and hit-test in
    /// the world coordinates the cumulative transform produces. Origin
    /// is the top-left of the panel's logical-rect — the caller
    /// composes its own pivot by pre/post-translation.
    pub(crate) transform: TranslateScale,
}
impl Element {
    pub(crate) fn set_grid_def(&mut self, id: GridDefId) {
        debug_assert_eq!(
            self.mode,
            LayoutMode::Grid,
            "grid payload set on {:?}",
            self.mode
        );
        self.mode_payload = ModePayload::grid(id);
    }

    pub(crate) fn grid_def_id(&self) -> GridDefId {
        self.mode_payload.grid_def_id(self.mode)
    }

    pub(crate) fn set_scroll_spec(&mut self, spec: ScrollSpec) {
        debug_assert_eq!(
            self.mode,
            LayoutMode::Scroll,
            "scroll payload set on {:?}",
            self.mode,
        );
        self.mode_payload = ModePayload::scroll(spec);
    }

    pub(crate) fn scroll_spec(&self) -> ScrollSpec {
        self.mode_payload.scroll_spec(self.mode)
    }

    /// Build an `Element` with a call-site-derived auto id. `#[track_caller]`
    /// propagates through every widget constructor that's also marked
    /// `#[track_caller]`, so `Foo::new()` at the user's source line yields a
    /// distinct id per call site. Override with [`Configure::id_salt`] /
    /// [`Configure::id`] when call order isn't stable across frames.
    ///
    /// Public so library users can author their own widgets: build an
    /// `Element`, chain [`Configure`] setters on it (`Element` itself
    /// implements `Configure`), resolve its id with [`crate::Ui::widget_id`],
    /// and open it with [`crate::Ui::node`].
    #[track_caller]
    pub fn new(mode: LayoutMode) -> Self {
        Self {
            salt: Salt::Auto(WidgetId::auto_stable()),
            mode,
            mode_payload: ModePayload::NONE,
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            gaps: Gaps::ZERO,
            justify: Justify::Start,
            align: Align::new(HAlign::Auto, VAlign::Auto),
            child_align: Align::new(HAlign::Auto, VAlign::Auto),
            position: Vec2::ZERO,
            grid: GridCell::default(),
            flags: NodeFlags::default(),
            visibility: Visibility::Visible,
            transform: TranslateScale::IDENTITY,
        }
    }

    /// Fan this `Element` out into the per-NodeId columns `Tree` stores.
    /// Single routing point — adding a field is one edit in the column
    /// type and one in the routing block. `widget_id` is supplied by
    /// the caller (resolved from `self.salt` upstream in
    /// `Forest::open_node`) so `Element` itself never carries a
    /// resolved id.
    #[inline(always)]
    pub(crate) fn into_columns(self, widget_id: WidgetId) -> ElementColumns {
        ElementColumns {
            widget_id,
            layout: LayoutCore::from_element(&self),
            attrs: self.flags,
            bounds: BoundsExtras {
                position: self.position,
                grid: self.grid,
                min_size: self.min_size,
                max_size: self.max_size,
            },
            panel: PanelExtras {
                gaps: self.gaps,
                justify: self.justify,
                child_align: self.child_align,
                transform: self.transform,
            },
        }
    }
}

#[inline]
fn debug_assert_valid_bounds(min_size: Size, max_size: Size) {
    // Builder setters run per widget per frame, so validation compiles out in release.
    debug_assert!(
        min_size.w >= 0.0
            && min_size.h >= 0.0
            && max_size.w >= 0.0
            && max_size.h >= 0.0
            && min_size.w <= max_size.w
            && min_size.h <= max_size.h,
        "element bounds must be non-negative and ordered on both axes, got min_size \
         {min_size:?}, max_size {max_size:?}",
    );
}

/// Mixin: any widget builder that holds an `Element` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Configure: Sized {
    fn element_mut(&mut self) -> &mut Element;

    /// Override this widget's id with a hash of `key`, scoped to the
    /// parent. The stored hash is mixed with the parent node's
    /// already-disambiguated [`WidgetId`] at [`crate::forest::Forest::open_node`]
    /// time, so `.id_salt("row")` resolves to distinct ids under
    /// different parents — same scoping rule egui uses. At the root
    /// (no parent) the salt hash is used as-is. Use whenever the
    /// default call-site-derived id wouldn't survive across frames or
    /// loop iterations — e.g. a `for` loop where each iteration must
    /// keep per-widget state separate. Marks the id as
    /// [`Salt::Hash`]: same-parent sibling collisions are disambiguated
    /// (so state stays well-formed) but flagged with a magenta runtime
    /// outline because they're caller bugs. For an unscoped "use this
    /// exact id" override, see [`Self::id`].
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.element_mut().salt = Salt::Hash(WidgetId::from_hash(key));
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`] used
    /// verbatim — **not** mixed with the parent. Use when the id was
    /// derived elsewhere and must match exactly (parent → child via
    /// [`WidgetId::with`], a shared seed for sibling widgets across
    /// layers, cross-frame state lookups that key off a domain id).
    /// For the parent-scoped path, prefer [`Self::id_salt`]. Stores
    /// the id as [`Salt::Verbatim`].
    fn id(mut self, id: WidgetId) -> Self {
        self.element_mut().salt = Salt::Verbatim(id);
        self
    }

    /// Re-derive an auto id at the *current* call site. Use when a builder
    /// helper constructs the widget (so `*::new()` resolved to the helper's
    /// source location) and you want each caller to get a distinct id —
    /// `helper().auto_id().show(ui)` reads the caller's `(file, line, col)`.
    #[track_caller]
    fn auto_id(mut self) -> Self {
        self.element_mut().salt = Salt::Auto(WidgetId::auto_stable());
        self
    }

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        let s = s.into();
        s.w().debug_assert_non_negative();
        s.h().debug_assert_non_negative();
        self.element_mut().size = s;
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        let element = self.element_mut();
        debug_assert_valid_bounds(s, element.max_size);
        element.min_size = s;
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        let element = self.element_mut();
        debug_assert_valid_bounds(element.min_size, s);
        element.max_size = s;
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.element_mut().padding = p.into();
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.element_mut().margin = m.into();
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.element_mut().position = p.into();
        self
    }
    /// Cell `(row, col)` inside a `Grid` parent. Default `(0, 0)`. Ignored
    /// outside a Grid parent.
    fn grid_cell(mut self, (row, col): (u16, u16)) -> Self {
        let g = &mut self.element_mut().grid;
        g.row = row;
        g.col = col;
        self
    }
    /// Span `(row_span, col_span)` inside a `Grid` parent. Default `(1, 1)`.
    /// Cell + span are validated against the parent's grid def at record
    /// time — an out-of-range placement panics (`Tree::check_grid_cell`).
    /// Ignored outside a Grid parent.
    fn grid_span(mut self, (rs, cs): (u16, u16)) -> Self {
        let g = &mut self.element_mut().grid;
        g.row_span = rs.max(1);
        g.col_span = cs.max(1);
        self
    }
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack and the within-line direction of WrapHStack/
    /// WrapVStack. Grid has its own `gap_xy` and ignores this field.
    fn gap(mut self, g: f32) -> Self {
        // Debug-only, matching `size` / `min_size` — builder setters
        // run per widget per frame.
        debug_assert!(g >= 0.0, "gap must be non-negative, got {g}");
        self.element_mut().gaps.set_gap(g);
        self
    }

    /// Logical-px space between *lines* for WrapHStack/WrapVStack —
    /// the cross-axis spacing between wrap rows/columns. Inert in
    /// every other layout mode. Pair with `.gap(...)` for the within-
    /// line spacing.
    fn line_gap(mut self, g: f32) -> Self {
        debug_assert!(g >= 0.0, "line_gap must be non-negative, got {g}");
        self.element_mut().gaps.set_line_gap(g);
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has [`crate::Sizing::Fill`] on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn align(mut self, a: Align) -> Self {
        self.element_mut().align = a;
        self
    }
    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(mut self, a: Align) -> Self {
        self.element_mut().child_align = a;
        self
    }
    fn sense(mut self, s: Sense) -> Self {
        self.element_mut().flags.set_sense(s);
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.element_mut().flags.set_disabled(d);
        self
    }
    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(mut self, f: bool) -> Self {
        self.element_mut().flags.set_focusable(f);
        self
    }
    /// Three-state visibility. See [`Visibility`].
    fn visibility(mut self, v: Visibility) -> Self {
        self.element_mut().visibility = v;
        self
    }
    /// Shorthand for [`Visibility::Hidden`]: keeps the slot, hides paint + input.
    fn hidden(self) -> Self {
        self.visibility(Visibility::Hidden)
    }
    /// Shorthand for [`Visibility::Collapsed`]: skip the node entirely (zero slot).
    fn collapsed(self) -> Self {
        self.visibility(Visibility::Collapsed)
    }

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    fn clip(mut self, mode: ClipMode) -> Self {
        self.element_mut().flags.set_clip(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    fn clip_rect(self) -> Self {
        self.clip(ClipMode::Rect)
    }

    /// Rounded-corner stencil clip — shape comes from the chrome's
    /// radius (set via [`Self::background`]). Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    fn clip_rounded(self) -> Self {
        self.clip(ClipMode::Rounded)
    }
}

/// A bare `Element` is its own configurable builder, so widget authors
/// can chain the [`Configure`] setters on the child nodes they construct
/// inside their `show` body — e.g.
/// `Element::new(LayoutMode::Leaf).id(my_id).size(...).sense(Sense::CLICK)`.
impl Configure for Element {
    #[inline]
    fn element_mut(&mut self) -> &mut Element {
        self
    }
}

#[cfg(test)]
mod tests;
