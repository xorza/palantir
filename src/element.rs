use crate::primitives::{
    Align, GridCell, HAlign, Justify, Sense, Size, Sizes, Spacing, TranslateScale, VAlign,
    Visibility, WidgetId,
};
use crate::tree::NodeFlags;
use glam::Vec2;

/// How a node arranges its children. Stored on `UiElement::mode` and read by
/// the layout pass; the tree itself treats it as an opaque tag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutMode {
    Leaf,
    HStack,
    VStack,
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
}

/// Rarely-set fields lifted out of `UiElement` so they don't bloat every
/// stored `Node`. Builders write defaults inline; on `Tree::push_node` the
/// non-default values get stamped into `Tree::node_extras` and the `Node`
/// keeps just an `Option<u16>` slot. Two categories live here: per-node
/// overrides that most nodes don't set (`transform`, `position`, `grid`) and
/// panel-only knobs that leaves never read (`gap`, `justify`, `child_align`).
/// Leaves vastly outnumber panels, so paying ~36B once per panel beats
/// carrying these fields inline on every leaf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiElementExtras {
    pub transform: Option<TranslateScale>,
    pub position: Vec2,
    pub grid: GridCell,
    /// Lower clamp on the resolved outer size. Default `Size::ZERO`.
    pub min_size: Size,
    /// Upper clamp on the resolved outer size. Default `Size::INF`.
    pub max_size: Size,
    /// Logical-px space between children (panels only).
    pub gap: f32,
    /// Main-axis distribution of leftover space (HStack/VStack only).
    pub justify: Justify,
    /// Default alignment applied to children with `Auto` axis (panels only).
    pub child_align: Align,
}

impl UiElementExtras {
    /// All-defaults instance. Single source of truth — `Default` and
    /// `Tree::read_extras`'s "missing extras" fallback both go through this.
    pub const DEFAULT: Self = Self {
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
        gap: 0.0,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
    };
}

impl Default for UiElementExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl UiElementExtras {
    /// True when nothing has been customized — push_node skips the side-table
    /// allocation in this case. Compared exactly against `DEFAULT` so adding
    /// a field only requires updating `DEFAULT`; no separate predicate to
    /// keep in sync.
    pub fn is_default(&self) -> bool {
        self == &Self::DEFAULT
    }
}

/// Compact form of a node's recorded element, stored inline in `Node`. Built
/// from a `UiElement` at `Tree::push_node`: the rarely-set fields move to
/// `Tree::node_extras`, addressed via `extras: Option<u16>`. The wide
/// `UiElement` lives only on builders during recording.
#[derive(Clone, Copy, Debug)]
pub struct NodeElement {
    pub id: WidgetId,
    pub mode: LayoutMode,

    pub size: Sizes,
    pub padding: Spacing,
    pub margin: Spacing,

    /// Packed `sense` / `disabled` / `clip` / `visibility` / `align`. Read
    /// through the accessor methods on `NodeFlags`.
    pub flags: NodeFlags,

    /// Index into `Tree::node_extras`, or `None` when all extras are at
    /// default (the common case). Cap is 65 535 non-default elements per
    /// frame; `node_extras` is cleared per frame.
    pub extras: Option<u16>,
}

/// Per-node config: identity + spatial layout + interaction + paint flags.
/// Every widget builder owns one and forwards it to `Ui::node`. `Element` (the
/// trait below) gives chained setters for all fields by impl'ing one method.
///
/// Fields are grouped by who reads them: identity, own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and paint.
#[derive(Clone, Copy, Debug)]
pub struct UiElement {
    // ---- Identity + layout-algorithm selector --------------------------------
    pub id: WidgetId,
    pub mode: LayoutMode,

    // ---- Own size + alignment (read by every parent layout) ------------------
    pub size: Sizes,
    pub min_size: Size,
    pub max_size: Size,
    pub padding: Spacing,
    pub margin: Spacing,

    // ---- Mode-specific: only read when the parent or self has the right mode.
    // Inert otherwise.
    /// Logical-px space between children when *this* node is `HStack`/`VStack`
    /// or `Grid`. Ignored by `Leaf` / `ZStack` / `Canvas`.
    pub gap: f32,
    /// Main-axis distribution of leftover space in `HStack`/`VStack` (this
    /// node's children). No effect when any child is `Sizing::Fill` on the
    /// main axis. Ignored by `Leaf` / `ZStack` / `Canvas` / `Grid`.
    pub justify: Justify,
    /// Alignment of this node inside its parent's inner rect. Each axis is
    /// honored only by parent layout modes that own that axis as a cross or
    /// placement axis: HStack reads `align.v` (cross), VStack reads `align.h`
    /// (cross), ZStack and Grid read both, HStack/VStack ignore their main
    /// axis, Canvas ignores both (absolute placement).
    pub align: Align,
    /// Default `align` applied to children when the child's own axis is
    /// `Auto`. Mirrors CSS `align-items` (parent) + `align-self` (child).
    /// Read only by parents that honor `align` (HStack/VStack/ZStack/Grid).
    pub child_align: Align,
    /// Absolute position inside a `Canvas` parent (parent-inner coordinates).
    /// Defaults to `Vec2::ZERO`. Ignored when the parent isn't a `Canvas`.
    pub position: Vec2,
    /// Cell + span inside a `Grid` parent. Defaults to `(0, 0)` placement and
    /// `(1, 1)` span. Ignored when the parent isn't a `Grid`.
    pub grid: GridCell,

    // ---- Interaction ---------------------------------------------------------
    pub sense: Sense,
    pub disabled: bool,

    // ---- Paint + cascade -----------------------------------------------------
    /// WPF-style three-state visibility. `Hidden` keeps the node's slot in
    /// layout but suppresses paint + input; `Collapsed` zeros the slot and
    /// skips the subtree everywhere. Cascades implicitly (paint and input
    /// early-return at non-`Visible` nodes).
    pub visibility: Visibility,
    /// Clip descendants' paint to this node's rendered rect (CSS `overflow:
    /// hidden`). The renderer applies a scissor while walking the subtree.
    /// Has no effect on layout — children may still measure beyond the rect;
    /// they're just visually clipped.
    pub clip: bool,
    /// Pan/zoom applied to descendants (post-layout, like WPF's `RenderTransform`).
    /// `None` = identity = no transform. The transform composes with any
    /// ancestor transform; descendants render and hit-test in the world
    /// coordinates the cumulative transform produces. Origin is the top-left
    /// of the panel's logical-rect — the caller composes its own pivot by
    /// pre/post-translation.
    pub transform: Option<TranslateScale>,
}

impl UiElement {
    pub fn new(id: WidgetId, mode: LayoutMode) -> Self {
        Self {
            id,
            mode,
            size: Sizes::default(),
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            align: Align::default(),
            gap: 0.0,
            justify: Justify::default(),
            child_align: Align::default(),
            position: Vec2::ZERO,
            grid: GridCell::default(),
            sense: Sense::NONE,
            disabled: false,
            visibility: Visibility::Visible,
            clip: false,
            transform: None,
        }
    }

    /// Split into the compact `NodeElement` (with `extras: None`) and the
    /// rarely-set bits. `Tree::push_node` stamps the side-table slot.
    pub fn split(self) -> (NodeElement, UiElementExtras) {
        let core = NodeElement {
            id: self.id,
            mode: self.mode,
            size: self.size,
            padding: self.padding,
            margin: self.margin,
            flags: NodeFlags::pack(
                self.sense,
                self.disabled,
                self.clip,
                self.visibility,
                self.align,
            ),
            extras: None,
        };
        let extras = UiElementExtras {
            transform: self.transform,
            position: self.position,
            grid: self.grid,
            min_size: self.min_size,
            max_size: self.max_size,
            gap: self.gap,
            justify: self.justify,
            child_align: self.child_align,
        };
        (core, extras)
    }
}

/// Mixin: any widget builder that holds a `UiElement` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Element: Sized {
    fn element_mut(&mut self) -> &mut UiElement;

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        let s = s.into();
        s.w.assert_non_negative();
        s.h.assert_non_negative();
        self.element_mut().size = s;
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let s = s.into();
        assert!(
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
    /// Space between children when this node is an `HStack` / `VStack`.
    fn gap(mut self, g: f32) -> Self {
        self.element_mut().gap = g;
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has `Sizing::Fill` on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors. See [`UiElement::align`] for
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
}
