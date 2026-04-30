use crate::primitives::{
    Align, Justify, Layout, Sense, Size, Sizes, Spacing, TranslateScale, Visibility, WidgetId,
};
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
    /// Children placed at their declared `Layout.position` (parent-inner
    /// coords). Each child sized per its desired (intrinsic) size. Canvas
    /// hugs to the bounding box of placed children.
    Canvas,
    /// WPF-style grid. Carries an index into `Tree::grid_defs` holding the row
    /// and column track definitions and per-axis gaps. Children declare cell +
    /// span via `Layout::grid`.
    Grid(u32),
}

/// Per-node config bundle: identity + spatial layout + interaction. Every
/// widget builder owns one and forwards it to `Ui::node`. `Element` (the
/// trait below) gives chained setters for all fields by impl'ing one method.
#[derive(Clone, Copy, Debug)]
pub struct UiElement {
    pub id: WidgetId,
    pub layout: Layout,
    pub mode: LayoutMode,
    pub sense: Sense,
    pub disabled: bool,
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
        // Panels (HStack/VStack/ZStack/Canvas) clip descendants by default —
        // overflow is the unusual case. Leaf has no descendants, so defaulting
        // it to `false` saves a no-op `PushClip/PopClip` pair per leaf.
        let clip = !matches!(mode, LayoutMode::Leaf);
        Self {
            id,
            layout: Layout::default(),
            mode,
            sense: Sense::NONE,
            disabled: false,
            visibility: Visibility::Visible,
            clip,
            transform: None,
        }
    }
}

/// Mixin: any widget builder that holds a `UiElement` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Element: Sized {
    fn element_mut(&mut self) -> &mut UiElement;

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.element_mut().layout.size = s.into();
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.element_mut().layout.min_size = s.into();
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.element_mut().layout.max_size = s.into();
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.element_mut().layout.padding = p.into();
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.element_mut().layout.margin = m.into();
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.element_mut().layout.position = p.into();
        self
    }
    /// Cell `(row, col)` inside a `Grid` parent. Default `(0, 0)`. Ignored
    /// outside a Grid parent.
    fn grid_cell(mut self, (row, col): (u16, u16)) -> Self {
        let g = &mut self.element_mut().layout.grid;
        g.row = row;
        g.col = col;
        self
    }
    /// Span `(row_span, col_span)` inside a `Grid` parent. Default `(1, 1)`.
    /// Spans are clamped at layout time to the grid's bounds. Ignored outside
    /// a Grid parent.
    fn grid_span(mut self, (rs, cs): (u16, u16)) -> Self {
        let g = &mut self.element_mut().layout.grid;
        g.row_span = rs.max(1);
        g.col_span = cs.max(1);
        self
    }
    /// Space between children when this node is an `HStack` / `VStack`.
    fn gap(mut self, g: f32) -> Self {
        self.element_mut().layout.gap = g;
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has `Sizing::Fill` on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.element_mut().layout.justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors. See `Layout::align` for
    /// which parent layout modes honor each axis.
    fn align(mut self, a: Align) -> Self {
        self.element_mut().layout.align = a;
        self
    }
    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(mut self, a: Align) -> Self {
        self.element_mut().layout.child_align = a;
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
