//! Public element authoring data and the builder configuration surface.

pub(crate) mod columns;

use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ScrollSpec};
use crate::layout::types::limits::{valid_lower_bound, valid_upper_bound};
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::columns::{
    BoundsExtras, ElementColumns, Gaps, LayoutCore, NodeFlags, PanelExtras,
};
use crate::scene::visibility::Visibility;
use glam::Vec2;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ElementMode {
    Resolved(LayoutMode),
    PendingGrid,
}

impl ElementMode {
    #[inline(always)]
    pub(crate) fn resolved(self) -> LayoutMode {
        match self {
            Self::Resolved(mode) => mode,
            Self::PendingGrid => {
                panic!("grid element recorded before its definition was installed")
            }
        }
    }
}

/// Recipe for an [`Element`]'s `WidgetId`. Mirrors egui's
/// `Option<Id>` "raw `id_salt`, resolve at `Ui::widget`"
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
/// Every widget builder owns one and records it via `Ui::widget` +
/// `Widget::node`. [`Configure`] gives chained setters for all fields by
/// implementing one method.
///
/// Fields are grouped by who reads them: identity, own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and paint.
#[derive(Clone, Copy, Debug)]
pub struct Element {
    /// Recipe for this node's `WidgetId`. Resolution happens inside
    /// [`crate::Ui::widget`] — `Element` itself never carries a
    /// resolved id. Mirrors egui's "builder stores raw `id_salt`,
    /// `Ui::widget` mixes in the parent's id at `.show()`" pattern.
    pub(crate) salt: Salt,
    pub(crate) mode: ElementMode,

    /// The five themable fields are `None` until explicitly set, so
    /// widgets can layer theme defaults under user intent with a plain
    /// `get_or_insert` / `unwrap_or` — there is no separate provenance
    /// tracking. [`Self::into_columns`] resolves `None` to the layout
    /// defaults (`Sizes::default()`, `Size::ZERO`/`Size::INF` bounds,
    /// `Spacing::ZERO`).
    pub(crate) size: Option<Sizes>,
    pub(crate) min_size: Option<Size>,
    pub(crate) max_size: Option<Size>,
    pub(crate) padding: Option<Spacing>,
    pub(crate) margin: Option<Spacing>,
    /// Clip mode, `None` until set. Kept out of [`NodeFlags`] during
    /// authoring for the same theme-fallback reason; folded into the
    /// recorded flags by [`Self::into_columns`].
    pub(crate) clip: Option<ClipMode>,

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

    /// Packed paint/input flags copied directly into the recorded tree.
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
    /// Paint/layout leaf for custom widget content.
    #[track_caller]
    pub fn leaf() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::Leaf))
    }

    /// Horizontal stack container for custom widgets.
    #[track_caller]
    pub fn hstack() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::HStack))
    }

    /// Vertical stack container for custom widgets.
    #[track_caller]
    pub fn vstack() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::VStack))
    }

    /// Wrapping horizontal stack container for custom widgets.
    #[track_caller]
    pub fn wrap_hstack() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::WrapHStack))
    }

    /// Wrapping vertical stack container for custom widgets.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::WrapVStack))
    }

    /// Layered stack container for custom widgets.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::ZStack))
    }

    /// Absolute-positioned container for custom widgets.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::Canvas))
    }

    #[track_caller]
    pub(crate) fn grid() -> Self {
        Self::new(ElementMode::PendingGrid)
    }

    #[track_caller]
    pub(crate) fn scroll(spec: ScrollSpec) -> Self {
        Self::new(ElementMode::Resolved(LayoutMode::Scroll(spec)))
    }

    pub(crate) fn set_grid_def(&mut self, id: GridDefId) {
        let ElementMode::PendingGrid = self.mode else {
            panic!("grid definition installed on {:?} element", self.mode);
        };
        self.mode = ElementMode::Resolved(LayoutMode::Grid(id));
    }

    pub(crate) fn set_scroll_spec(&mut self, spec: ScrollSpec) {
        let ElementMode::Resolved(LayoutMode::Scroll(current)) = &mut self.mode else {
            panic!("scroll specification installed on {:?} element", self.mode);
        };
        *current = spec;
    }

    pub(crate) fn scroll_spec(&self) -> ScrollSpec {
        let ElementMode::Resolved(LayoutMode::Scroll(spec)) = self.mode else {
            panic!("scroll specification read from {:?} element", self.mode);
        };
        spec
    }

    #[track_caller]
    fn new(mode: ElementMode) -> Self {
        Self {
            salt: Salt::Auto(WidgetId::auto_stable()),
            mode,
            size: None,
            min_size: None,
            max_size: None,
            padding: None,
            margin: None,
            clip: None,
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

    /// Fan this `Element` out into the per-NodeId columns `Tree` stores,
    /// resolving every still-`None` themable field to its layout
    /// default. Single routing point — adding a field is one edit in
    /// the column type and one in the routing block. `widget_id` is
    /// supplied by the caller (resolved from `self.salt` upstream in
    /// `Forest::open_node`) so `Element` itself never carries a
    /// resolved id.
    #[inline(always)]
    pub(crate) fn into_columns(self, widget_id: WidgetId) -> ElementColumns {
        let mut attrs = self.flags;
        attrs.set_clip(self.clip.unwrap_or(ClipMode::None));
        ElementColumns {
            widget_id,
            layout: LayoutCore::from_element(&self),
            attrs,
            bounds: BoundsExtras {
                position: self.position,
                grid: self.grid,
                min_size: self.min_size.unwrap_or(Size::ZERO),
                max_size: self.max_size.unwrap_or(Size::INF),
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

/// Opaque mutable view used only to implement [`Configure`] for a widget.
/// Delegating through an owned [`Element`] exposes configuration without
/// exposing the element's structural layout mode.
#[derive(Debug)]
pub struct ConfigureElement<'a> {
    element: &'a mut Element,
}

#[inline]
fn debug_assert_valid_bounds(min_size: Size, max_size: Size) {
    // Builder setters run per widget per frame, so validation compiles out in release.
    debug_assert!(
        valid_lower_bound(min_size.w)
            && valid_lower_bound(min_size.h)
            && valid_upper_bound(max_size.w)
            && valid_upper_bound(max_size.h)
            && min_size.w <= max_size.w
            && min_size.h <= max_size.h,
        "element minimums must be finite, bounds must be non-negative and ordered, and only \
         maximums may be infinite; got min_size {min_size:?}, max_size {max_size:?}",
    );
}

/// Mixin: any widget builder that holds an `Element` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Configure: Sized {
    fn element_mut(&mut self) -> ConfigureElement<'_>;

    /// Override this widget's id with a hash of `key`, scoped to the
    /// parent. The stored hash is mixed with the parent node's
    /// already-disambiguated [`WidgetId`] when the node opens, so
    /// `.id_salt("row")` resolves to distinct ids under
    /// different parents — same scoping rule egui uses. At the root
    /// (no parent) the salt hash is used as-is. Use whenever the
    /// default call-site-derived id wouldn't survive across frames or
    /// loop iterations — e.g. a `for` loop where each iteration must
    /// keep per-widget state separate. Marks the id as a hash salt:
    /// same-parent sibling collisions are disambiguated
    /// (so state stays well-formed) but flagged with a magenta runtime
    /// outline because they're caller bugs. For an unscoped "use this
    /// exact id" override, see [`Self::id`].
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.element_mut().element.salt = Salt::Hash(WidgetId::from_hash(key));
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`] used
    /// verbatim — **not** mixed with the parent. Use when the id was
    /// derived elsewhere and must match exactly (parent → child via
    /// [`WidgetId::with`], a shared seed for sibling widgets across
    /// layers, cross-frame state lookups that key off a domain id).
    /// For the parent-scoped path, prefer [`Self::id_salt`]. Stores
    /// the id verbatim.
    fn id(mut self, id: WidgetId) -> Self {
        self.element_mut().element.salt = Salt::Verbatim(id);
        self
    }

    /// Re-derive an auto id at the *current* call site. Use when a builder
    /// helper constructs the widget (so `*::new()` resolved to the helper's
    /// source location) and you want each caller to get a distinct id —
    /// `helper().auto_id().show(ui)` reads the caller's `(file, line, col)`.
    #[track_caller]
    fn auto_id(mut self) -> Self {
        self.element_mut().element.salt = Salt::Auto(WidgetId::auto_stable());
        self
    }

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.element_mut().element.size = Some(s.into());
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let element = self.element_mut().element;
        let value = s.into();
        debug_assert_valid_bounds(value, element.max_size.unwrap_or(Size::INF));
        element.min_size = Some(value);
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        let element = self.element_mut().element;
        let value = s.into();
        debug_assert_valid_bounds(element.min_size.unwrap_or(Size::ZERO), value);
        element.max_size = Some(value);
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.element_mut().element.padding = Some(p.into());
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.element_mut().element.margin = Some(m.into());
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.element_mut().element.position = p.into();
        self
    }
    /// Cell `(row, col)` inside a `Grid` parent. Default `(0, 0)`. Ignored
    /// outside a Grid parent.
    fn grid_cell(mut self, (row, col): (u16, u16)) -> Self {
        let element = self.element_mut().element;
        element.grid.row = row;
        element.grid.col = col;
        self
    }
    /// Span `(row_span, col_span)` inside a `Grid` parent. Default `(1, 1)`.
    /// Cell + span are validated against the parent's grid def at record
    /// time — an out-of-range placement panics (`Tree::check_grid_cell`).
    /// Ignored outside a Grid parent.
    fn grid_span(mut self, (rs, cs): (u16, u16)) -> Self {
        let element = self.element_mut().element;
        element.grid.row_span = rs.max(1);
        element.grid.col_span = cs.max(1);
        self
    }
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack and the within-line direction of WrapHStack/
    /// WrapVStack. Grid has its own `gap_xy` and ignores this field.
    fn gap(mut self, g: f32) -> Self {
        self.element_mut().element.gaps.set_gap(g);
        self
    }

    /// Logical-px space between *lines* for WrapHStack/WrapVStack —
    /// the cross-axis spacing between wrap rows/columns. Inert in
    /// every other layout mode. Pair with `.gap(...)` for the within-
    /// line spacing.
    fn line_gap(mut self, g: f32) -> Self {
        self.element_mut().element.gaps.set_line_gap(g);
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has [`crate::Sizing::fill`] on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().element.justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn align(mut self, a: Align) -> Self {
        self.element_mut().element.align = a;
        self
    }
    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(mut self, a: Align) -> Self {
        self.element_mut().element.child_align = a;
        self
    }
    fn sense(mut self, s: Sense) -> Self {
        self.element_mut().element.flags.set_sense(s);
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.element_mut().element.flags.set_disabled(d);
        self
    }
    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(mut self, f: bool) -> Self {
        self.element_mut().element.flags.set_focusable(f);
        self
    }
    /// Three-state visibility. See [`Visibility`].
    fn visibility(mut self, v: Visibility) -> Self {
        self.element_mut().element.visibility = v;
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
        self.element_mut().element.clip = Some(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    fn clip_rect(self) -> Self {
        self.clip(ClipMode::Rect)
    }

    /// Rounded-corner stencil clip — shape comes from the widget chrome's
    /// background radius. Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    fn clip_rounded(self) -> Self {
        self.clip(ClipMode::Rounded)
    }
}

/// A bare `Element` is its own configurable builder, so widget authors
/// can chain the [`Configure`] setters on the child nodes they construct
/// inside their `show` body — e.g.
/// `Element::leaf().id(my_id).size(...).sense(Sense::CLICK)`.
impl Configure for Element {
    #[inline]
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        ConfigureElement { element: self }
    }
}

#[cfg(test)]
mod tests;
