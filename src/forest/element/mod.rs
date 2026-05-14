//! Per-node element data: `Element` (the wide builder form) and the
//! columns `Tree` stores it in:
//!
//! - `widget_id` — identity. Hit-test, state map, damage diff.
//! - `LayoutCore` — mode, size, padding, margin, align, visibility.
//!   Read by every measure / arrange / alignment pass.
//! - `NodeFlags` — 1-byte packed sense / disabled / clip / focusable.
//!   Read densely by cascade / encoder / hit-test.
//! - `BoundsExtras` — sparse side table for transform / position /
//!   grid / min_size / max_size. Allocated only when one differs
//!   from `BoundsExtras::DEFAULT`.
//! - `PanelExtras` — sparse side table for gap / line_gap / justify /
//!   child_align. Allocated only when one differs from `DEFAULT`.
//! - `chrome` — sparse `Background` paint. Routed straight from
//!   `Element.chrome` (with `is_noop` filtered out in `Tree::open_node`).
//!
//! Fan-out happens once in `Element::into_columns`. Adding a field
//! is two local edits: append to the column type and route it in
//! `into_columns`. `Configure` (trait below) provides one chained
//! setter per field on `Element`.

use crate::forest::seen_ids::IdSource;
use crate::forest::visibility::Visibility;
use crate::input::sense::Sense;
use crate::layout::types::{
    align::Align, align::HAlign, align::VAlign, clip_mode::ClipMode, grid_cell::GridCell,
    justify::Justify, sizing::Sizes,
};
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{
    background::Background, size::Size, spacing::Spacing, transform::TranslateScale,
};
use glam::Vec2;

/// How a node arranges its children. Stored on `Element::mode` and read by
/// the layout pass; the tree itself treats it as an opaque tag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutMode {
    Leaf,
    HStack,
    VStack,
    /// HStack with overflow wrap: children flow left-to-right; when the
    /// next child wouldn't fit in the remaining main-axis space, wrap to
    /// a new row below. Each row's cross-axis size = max child cross
    /// in that row. `gap` spaces siblings within a row; `line_gap`
    /// spaces rows. Justify applies per row. `Sizing::Fill` on main is
    /// treated as `Hug` (no row-leftover distribution today).
    WrapHStack,
    /// VStack with overflow wrap: same model as `WrapHStack`, axes
    /// swapped (children flow top-to-bottom; wrap to a new column on
    /// the right).
    WrapVStack,
    /// Children all laid out at the same position (top-left of inner rect),
    /// each sized per its own `Sizing`. Used by `Panel`.
    ZStack,
    /// Children placed at their declared `position` (parent-inner coords).
    /// Each child sized per its desired (intrinsic) size. Canvas hugs to the
    /// bounding box of placed children.
    Canvas,
    /// WPF-style grid. Carries an index into `Tree::grid_defs` holding the row
    /// and column track definitions and per-axis gaps. Children declare cell +
    /// span via `grid`. Cap is 65 535 grids per frame (`grid_defs` is cleared
    /// each frame).
    Grid(u16),
    /// Scroll viewport. Pan + child layout determined by [`ScrollAxes`]:
    /// `Vertical`/`Horizontal` use a stack on that axis with the panned
    /// axis measured as `INF`; `Both` uses a `ZStack` with both axes
    /// unbounded. The widget builder sets a `transform` to pan and
    /// enables `clip` so children render within the viewport rect.
    Scroll(ScrollAxes),
}

/// Which axes a [`LayoutMode::Scroll`] viewport pans (and lays its
/// children out along). Single-axis variants stack children on the
/// panned axis; `Both` overlays them like a `ZStack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollAxes {
    Vertical,
    Horizontal,
    Both,
}

/// `Grid(idx)` collapses to a single tag — `idx` is a frame-local arena
/// slot that shifts with sibling order, while the def's actual content
/// is hashed at `NodeExit` via `GridDef::hash`. Hashing the idx would
/// invalidate the cache for cosmetic reorderings.
impl std::hash::Hash for LayoutMode {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let tag: u8 = match self {
            LayoutMode::Leaf => 0,
            LayoutMode::HStack => 1,
            LayoutMode::VStack => 2,
            LayoutMode::WrapHStack => 3,
            LayoutMode::WrapVStack => 4,
            LayoutMode::ZStack => 5,
            LayoutMode::Canvas => 6,
            LayoutMode::Grid(_) => 7,
            LayoutMode::Scroll(ScrollAxes::Vertical) => 8,
            LayoutMode::Scroll(ScrollAxes::Horizontal) => 9,
            LayoutMode::Scroll(ScrollAxes::Both) => 10,
        };
        h.write_u8(tag);
    }
}

impl ScrollAxes {
    /// Mask of axes that consume scroll deltas. `Both` ⇒ `(true, true)`,
    /// `Vertical` ⇒ `(false, true)`, `Horizontal` ⇒ `(true, false)`.
    #[inline]
    pub(crate) fn pan_mask(self) -> glam::BVec2 {
        match self {
            Self::Vertical => glam::BVec2::new(false, true),
            Self::Horizontal => glam::BVec2::new(true, false),
            Self::Both => glam::BVec2::TRUE,
        }
    }
}

/// Per-node bounds + transform + parent-relative placement. Set on any
/// `Element` (leaf or panel) whose builder customizes one of these fields.
/// Lifted into a sparse side-table so leaves that touch none of these stay
/// at zero per-node bytes here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) transform: Option<TranslateScale>,
    pub(crate) position: Vec2,
    pub(crate) grid: GridCell,
    /// Lower clamp on the resolved outer size. Default `Size::ZERO`.
    pub(crate) min_size: Size,
    /// Upper clamp on the resolved outer size. Default `Size::INF`.
    pub(crate) max_size: Size,
}

/// Panel-only knobs. Read by stack/wrap/grid/zstack drivers on the parent
/// node — leaves never touch them. Sparse so leaves don't allocate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelExtras {
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack (single line) and WrapHStack/WrapVStack (within
    /// each wrap row/column).
    pub(crate) gap: f32,
    /// Logical-px space between lines for WrapHStack/WrapVStack only.
    /// Inert in HStack/VStack/ZStack/Canvas/Grid.
    pub(crate) line_gap: f32,
    /// Main-axis distribution of leftover space (HStack/VStack only).
    pub(crate) justify: Justify,
    /// Default alignment applied to children with `Auto` axis (panels only).
    pub(crate) child_align: Align,
}

/// `transform` is intentionally omitted: it doesn't affect this node's own
/// paint (the encoder draws the node at its layout rect *before*
/// `PushTransform`; the transform composes into descendants' screen rects via
/// `CascadesEngine`). A parent transform change shows up as descendant screen-rect
/// diffs in `DamageEngine::compute`, the right granularity. Transform IS folded
/// into `subtree_hash` separately (in the tree's rollup loop) so the encode
/// cache invalidates on transform-only changes.
impl std::hash::Hash for BoundsExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write(bytemuck::bytes_of(&self.position));
        self.grid.hash(h);
        self.min_size.hash(h);
        self.max_size.hash(h);
    }
}

impl std::hash::Hash for PanelExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_u32(self.gap.to_bits());
        h.write_u32(self.line_gap.to_bits());
        self.child_align.hash(h);
        self.justify.hash(h);
    }
}

impl BoundsExtras {
    pub(crate) const DEFAULT: Self = Self {
        transform: None,
        position: Vec2::ZERO,
        grid: GridCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        },
        min_size: Size::ZERO,
        max_size: Size::INF,
    };

    /// True when nothing has been customized — push_node skips the side-table
    /// allocation in this case. Exact equality against `DEFAULT` so adding a
    /// field only requires updating `DEFAULT`; no separate predicate to keep
    /// in sync.
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::DEFAULT
    }
}

impl PanelExtras {
    pub(crate) const DEFAULT: Self = Self {
        gap: 0.0,
        line_gap: 0.0,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
    };

    pub(crate) fn is_default(&self) -> bool {
        self == &Self::DEFAULT
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Default for PanelExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Per-node layout column, stored in `Tree::layout`. Read by every
/// pass that runs measure/arrange/alignment math. Held tight so the
/// layout pass pulls only what it reads. Visibility lives here so
/// `is_collapsed` short-circuits in the layout fast-path. Packed
/// paint/input flags (sense / disabled / clip / focusable) live in
/// `Tree::attrs` — a separate 1-byte/node column read by cascade /
/// encoder / hit-test.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutCore {
    pub(crate) mode: LayoutMode,
    pub(crate) size: Sizes,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) align: Align,
    pub(crate) visibility: Visibility,
}

impl std::hash::Hash for LayoutCore {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.mode.hash(h);
        self.size.hash(h);
        self.padding.hash(h);
        self.margin.hash(h);
        self.align.hash(h);
        self.visibility.hash(h);
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
    // ---- Identity + layout-algorithm selector --------------------------------
    pub(crate) id: WidgetId,
    /// How `id` was produced: [`IdSource::Auto`] when synthesized by
    /// [`WidgetId::auto_stable`] (caller used `Foo::new()` + `.auto_id()`),
    /// [`IdSource::Explicit`] when set via [`Configure::id_salt`] /
    /// [`Configure::id`]. Both sources are disambiguated by mixing in
    /// a per-id occurrence counter on collision; explicit collisions
    /// additionally get a magenta debug outline so caller bugs surface
    /// at runtime instead of corrupting per-id stores.
    pub(crate) id_source: IdSource,
    pub(crate) mode: LayoutMode,

    // ---- Own size + alignment (read by every parent layout) ------------------
    pub(crate) size: Sizes,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,

    // ---- Mode-specific: only read when the parent or self has the right mode.
    // Inert otherwise.
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack (single line) and WrapHStack/WrapVStack (within
    /// each wrap row/column). Ignored by `Leaf` / `ZStack` / `Canvas` /
    /// `Grid` (Grid uses its own row_gap/col_gap).
    pub(crate) gap: f32,
    /// Logical-px space between lines for WrapHStack/WrapVStack only.
    /// Inert otherwise.
    pub(crate) line_gap: f32,
    /// Main-axis distribution of leftover space in `HStack`/`VStack` (this
    /// node's children). No effect when any child is `Sizing::Fill` on the
    /// main axis. Ignored by `Leaf` / `ZStack` / `Canvas` / `Grid`.
    pub(crate) justify: Justify,
    /// Alignment of this node inside its parent's inner rect. Each axis is
    /// honored only by parent layout modes that own that axis as a cross or
    /// placement axis: HStack reads `align.v` (cross), VStack reads `align.h`
    /// (cross), ZStack and Grid read both, HStack/VStack ignore their main
    /// axis, Canvas ignores both (absolute placement).
    pub(crate) align: Align,
    /// Default `align` applied to children when the child's own axis is
    /// `Auto`. Mirrors CSS `align-items` (parent) + `align-self` (child).
    /// Read only by parents that honor `align` (HStack/VStack/ZStack/Grid).
    pub(crate) child_align: Align,
    /// Absolute position inside a `Canvas` parent (parent-inner coordinates).
    /// Defaults to `Vec2::ZERO`. Ignored when the parent isn't a `Canvas`.
    pub(crate) position: Vec2,
    /// Cell + span inside a `Grid` parent. Defaults to `(0, 0)` placement and
    /// `(1, 1)` span. Ignored when the parent isn't a `Grid`.
    pub(crate) grid: GridCell,

    // ---- Interaction ---------------------------------------------------------
    pub(crate) sense: Sense,
    pub(crate) disabled: bool,
    /// Eligible to capture keyboard focus on press. Disabled / invisible
    /// nodes don't take focus regardless of this flag — the cascade pass
    /// applies the same exclusion `Sense` gets. Default `false`; only
    /// editable widgets (TextEdit) flip it on. Distinct from `Sense::CLICK`
    /// because clicking a Button shouldn't steal focus from a TextEdit.
    pub(crate) focusable: bool,

    // ---- Paint + cascade -----------------------------------------------------
    /// WPF-style three-state visibility. `Hidden` keeps the node's slot in
    /// layout but suppresses paint + input; `Collapsed` zeros the slot and
    /// skips the subtree everywhere. CascadesEngine implicitly (paint and input
    /// early-return at non-`Visible` nodes).
    pub(crate) visibility: Visibility,
    /// Storage for the clip flag — written by `Configure::clip*`
    /// methods or set directly by framework-internal widgets like
    /// `Scroll`. `Rect` = scissor; `Rounded` = scissor + stencil mask
    /// (radius derived from `chrome.radius` in `Tree::open_node`).
    /// `None` = no clip. No effect on layout.
    pub(crate) clip: ClipMode,
    /// Optional paint chrome (fill, stroke, corner radius). Authored
    /// via `Configure::background`. `Tree::open_node` filters
    /// invisible paint to `None` and stashes the radius into a
    /// dedicated `clip_radius` column when `clip` is `Rounded` so
    /// the encoder reads paint and mask info as independent plumbing.
    pub(crate) chrome: Option<Background>,
    /// Pan/zoom applied to descendants (post-layout, like WPF's `RenderTransform`).
    /// `None` = identity = no transform. The transform composes with any
    /// ancestor transform; descendants render and hit-test in the world
    /// coordinates the cumulative transform produces. Origin is the top-left
    /// of the panel's logical-rect — the caller composes its own pivot by
    /// pre/post-translation.
    pub(crate) transform: Option<TranslateScale>,
}

/// Per-node columns derived from one `Element`. Single fan-out point —
/// `Tree::open_node` calls `Element::into_columns` once and moves each
/// field into its column. Adding an `Element` field is a one-line edit
/// here (one in the new column type, one routing line in `into_columns`).
pub(crate) struct ElementColumns {
    pub(crate) widget_id: WidgetId,
    pub(crate) layout: LayoutCore,
    pub(crate) attrs: NodeFlags,
    pub(crate) bounds: BoundsExtras,
    pub(crate) panel: PanelExtras,
    pub(crate) chrome: Option<Background>,
}

impl Element {
    /// Build an `Element` with a call-site-derived auto id. `#[track_caller]`
    /// propagates through every widget constructor that's also marked
    /// `#[track_caller]`, so `Foo::new()` at the user's source line yields a
    /// distinct id per call site. Override with [`Configure::id_salt`] /
    /// [`Configure::id`] when call order isn't stable across frames.
    #[track_caller]
    pub(crate) fn new(mode: LayoutMode) -> Self {
        Self {
            id: WidgetId::auto_stable(),
            mode,
            id_source: IdSource::Auto,
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            align: Align::default(),
            gap: 0.0,
            line_gap: 0.0,
            justify: Justify::default(),
            child_align: Align::default(),
            position: Vec2::ZERO,
            grid: GridCell::default(),
            sense: Sense::NONE,
            disabled: false,
            focusable: false,
            visibility: Visibility::Visible,
            clip: ClipMode::None,
            chrome: None,
            transform: None,
        }
    }

    /// Overwrite the id with a precomputed [`WidgetId`], marking it
    /// [`IdSource::Explicit`] so `Ui::node` hard-asserts on collision.
    pub(crate) fn set_id(&mut self, id: WidgetId) {
        self.id = id;
        self.id_source = IdSource::Explicit;
    }

    /// Overwrite the id while inheriting `id_source` from another element.
    /// Use when forwarding an outer wrapper's identity to an internal
    /// sub-element built by the same widget (e.g. `Scroll`'s outer/inner
    /// split) — the caller's choice of auto vs. explicit propagates.
    pub(crate) fn set_id_from(&mut self, other: &Element) {
        self.id = other.id;
        self.id_source = other.id_source;
    }

    /// Fan this `Element` out into the per-NodeId columns `Tree` stores.
    /// Single routing point — adding a field is one edit in the column
    /// type plus one line here.
    pub(crate) fn into_columns(self) -> ElementColumns {
        ElementColumns {
            widget_id: self.id,
            layout: LayoutCore {
                mode: self.mode,
                size: self.size,
                padding: self.padding,
                margin: self.margin,
                align: self.align,
                visibility: self.visibility,
            },
            attrs: NodeFlags::pack(self.sense, self.disabled, self.clip, self.focusable),
            bounds: BoundsExtras {
                transform: self.transform,
                position: self.position,
                grid: self.grid,
                min_size: self.min_size,
                max_size: self.max_size,
            },
            panel: PanelExtras {
                gap: self.gap,
                line_gap: self.line_gap,
                justify: self.justify,
                child_align: self.child_align,
            },
            chrome: self.chrome,
        }
    }
}

/// Mixin: any widget builder that holds an `Element` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Configure: Sized {
    fn element_mut(&mut self) -> &mut Element;

    /// Override this widget's id with a hash of `key`. Use whenever the
    /// default call-site-derived id wouldn't survive across frames or across
    /// loop iterations — e.g. a `for` loop where each iteration must keep
    /// per-widget state separate. Marks the id as `Explicit`: collisions
    /// are disambiguated (so state stays well-formed) but flagged with
    /// a magenta runtime outline because they're caller bugs.
    fn id_salt(mut self, key: impl std::hash::Hash) -> Self {
        let e = self.element_mut();
        e.id = WidgetId::from_hash(key);
        e.id_source = IdSource::Explicit;
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`]. Use when
    /// the id was derived elsewhere (parent → child via [`WidgetId::with`],
    /// shared seed for sibling widgets) so [`Self::id_salt`] would re-hash
    /// a value the caller already constructed. Marks the id as `Explicit`.
    fn id(mut self, id: WidgetId) -> Self {
        let e = self.element_mut();
        e.id = id;
        e.id_source = IdSource::Explicit;
        self
    }

    /// Re-derive an auto id at the *current* call site. Use when a builder
    /// helper constructs the widget (so `*::new()` resolved to the helper's
    /// source location) and you want each caller to get a distinct id —
    /// `helper().auto_id().show(ui)` reads the caller's `(file, line, col)`.
    #[track_caller]
    fn auto_id(mut self) -> Self {
        let e = self.element_mut();
        e.id = WidgetId::auto_stable();
        e.id_source = IdSource::Auto;
        self
    }

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        let s = s.into();
        s.w.debug_assert_non_negative();
        s.h.debug_assert_non_negative();
        self.element_mut().size = s;
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        debug_assert!(
            s.w >= 0.0 && s.h >= 0.0,
            "min_size must be non-negative on both axes, got {s:?}",
        );
        self.element_mut().min_size = s;
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        assert!(
            s.w >= 0.0 && s.h >= 0.0,
            "max_size must be non-negative on both axes, got {s:?}",
        );
        self.element_mut().max_size = s;
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
    /// Spans are clamped at layout time to the grid's bounds. Ignored outside
    /// a Grid parent.
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
        self.element_mut().gap = g;
        self
    }
    /// Logical-px space between *lines* for WrapHStack/WrapVStack —
    /// the cross-axis spacing between wrap rows/columns. Inert in
    /// every other layout mode. Pair with `.gap(...)` for the within-
    /// line spacing.
    fn line_gap(mut self, g: f32) -> Self {
        self.element_mut().line_gap = g;
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has `Sizing::Fill` on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors. See [`Element::align`] for
    /// which parent layout modes honor each axis.
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
        self.element_mut().sense = s;
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.element_mut().disabled = d;
        self
    }
    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(mut self, f: bool) -> Self {
        self.element_mut().focusable = f;
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

    /// Paint chrome (fill, stroke, corner radius). `Tree::open_node`
    /// drops invisible paint to `None` and stashes the radius into
    /// the encoder's `clip_radius` column when this node also calls
    /// [`Self::clip_rounded`], so the encoder doesn't run a noop
    /// guard at draw time.
    fn background(mut self, bg: Background) -> Self {
        self.element_mut().chrome = Some(bg);
        self
    }

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    fn clip(mut self, mode: ClipMode) -> Self {
        self.element_mut().clip = mode;
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

/// Packed paint/input flags. One byte.
///
/// `bits`: 0-3=sense bitflags (HOVER|CLICK|DRAG|SCROLL), 4=disabled,
/// 5-6=clip mode, 7=focusable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct NodeFlags {
    pub(crate) bits: u8,
}

impl NodeFlags {
    const SENSE_MASK: u8 = 0b1111;
    const DISABLED: u8 = 1 << 4;
    const CLIP_SHIFT: u8 = 5;
    const CLIP_MASK: u8 = 0b11 << Self::CLIP_SHIFT;
    const FOCUSABLE: u8 = 1 << 7;

    /// Pack the paint/input bits. Reads `clip` post-downgrade (rounded
    /// → rect when the chrome lacks a radius — that fix-up happens in
    /// `Tree::open_node` before this is called).
    pub(crate) fn pack(sense: Sense, disabled: bool, clip: ClipMode, focusable: bool) -> Self {
        let mut bits = sense.bits() & Self::SENSE_MASK;
        if disabled {
            bits |= Self::DISABLED;
        }
        bits |= (clip as u8) << Self::CLIP_SHIFT;
        if focusable {
            bits |= Self::FOCUSABLE;
        }
        Self { bits }
    }

    pub(crate) fn sense(self) -> Sense {
        Sense::from_bits_truncate(self.bits & Self::SENSE_MASK)
    }
    pub(crate) fn is_disabled(self) -> bool {
        self.bits & Self::DISABLED != 0
    }
    pub(crate) fn clip_mode(self) -> ClipMode {
        match (self.bits & Self::CLIP_MASK) >> Self::CLIP_SHIFT {
            0 => ClipMode::None,
            1 => ClipMode::Rect,
            2 => ClipMode::Rounded,
            _ => unreachable!(),
        }
    }
    pub(crate) fn is_focusable(self) -> bool {
        self.bits & Self::FOCUSABLE != 0
    }
}

// Compile-time width checks for the fields packed into `NodeFlags.bits`.
// Adding a `ClipMode` variant past 3 or a `Sense` flag past bit 3 would
// silently bleed into adjacent fields; these asserts fail the build
// instead.
const _: () = assert!(
    (ClipMode::Rounded as u8) <= (NodeFlags::CLIP_MASK >> NodeFlags::CLIP_SHIFT),
    "ClipMode discriminant exceeds 2 bits — would bleed into FOCUSABLE",
);
const _: () = assert!(
    Sense::all().bits() <= NodeFlags::SENSE_MASK,
    "Sense uses more than 4 bits — would bleed into DISABLED",
);

#[cfg(test)]
mod tests;
