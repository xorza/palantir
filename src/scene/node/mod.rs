//! Public node authoring data and the builder configuration surface.

pub(crate) mod bounds_extras;
pub(crate) mod configure;
pub(crate) mod container_chrome;
pub(crate) mod gaps;
pub(crate) mod layout_core;
pub(crate) mod node_columns;
pub(crate) mod node_flags;
pub(crate) mod node_mode;
pub(crate) mod panel_extras;
pub(crate) mod salt;
pub(crate) mod theme_defaults;

use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{LayoutMode, ScrollSpec, ScrollbarsDefId};
use crate::layout::types::limits;
use crate::layout::types::sizing::Sizes;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::configure::{Configure, ConfigureNode};
use crate::scene::node::container_chrome::ContainerChrome;
use crate::scene::node::gaps::Gaps;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_columns::NodeColumns;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::node::salt::Salt;
use crate::scene::visibility::Visibility;
use glam::Vec2;

/// Per-node config: identity + spatial layout + interaction + paint flags.
/// Every widget builder owns one and records it via `Ui::widget` +
/// `Widget::record`. [`Configure`] gives chained setters for all fields by
/// implementing one method.
///
/// Fields are grouped by who reads them: identity, own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and paint.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Recipe for this node's `WidgetId`. Resolution happens inside
    /// [`crate::Ui::widget`] — `Node` itself never carries a
    /// resolved id. Mirrors egui's "builder stores raw `id_salt`,
    /// `Ui::widget` mixes in the parent's id at `.show()`" pattern.
    pub(crate) salt: Salt,
    pub(crate) mode: NodeMode,

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

impl Node {
    /// Resolve this container node's chrome + clip against the theme
    /// fallbacks, setting the clip mode in place. Shared by
    /// `Panel`/`Grid`/`Popup` (theme slot `panel_background` /
    /// `panel_clip`): an explicit `.background(...)` wins, otherwise the
    /// theme default fills in; the clip default only applies when the
    /// caller did not configure clipping. Returns the chrome to pass to
    /// [`Widget::record`].
    ///
    /// **Not every container wants this.** Tooltip, Modal and ContextMenu
    /// resolve their own with `Option::unwrap_or`, and `Frame` has no theme
    /// slot to fall back to at all — the three differ in what the consumer
    /// needs (a borrow, an owned value, nothing), and none of them has a
    /// clip default, which is the only thing this adds over `.or()`.
    ///
    /// [`Widget::record`]: crate::widgets::widget::Widget::record
    pub(crate) fn resolve_container_chrome<'a>(
        &mut self,
        explicit: Option<&'a Background>,
        theme: ContainerChrome<'a>,
    ) -> Option<&'a Background> {
        self.clip.get_or_insert(theme.clip);
        explicit.or(theme.background)
    }

    /// Paint/layout leaf for custom widget content.
    #[track_caller]
    pub fn leaf() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Leaf))
    }

    /// Horizontal stack container for custom widgets.
    #[track_caller]
    pub fn hstack() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Stack(Axis::X)))
    }

    /// Vertical stack container for custom widgets.
    #[track_caller]
    pub fn vstack() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Stack(Axis::Y)))
    }

    /// Wrapping horizontal stack container for custom widgets.
    #[track_caller]
    pub fn wrap_hstack() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::WrapStack(Axis::X)))
    }

    /// Wrapping vertical stack container for custom widgets.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::WrapStack(Axis::Y)))
    }

    /// Layered stack container for custom widgets.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::ZStack))
    }

    /// Absolute-positioned container for custom widgets.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Canvas))
    }

    #[track_caller]
    pub(crate) fn grid() -> Self {
        Self::new(NodeMode::PendingGrid)
    }

    /// Bar-overlay container for [`crate::widgets::scroll::Scroll`]. Its
    /// children are placed by `layout::scrollbars` after measure, which
    /// is the only point the content extent they size against exists.
    #[track_caller]
    pub(crate) fn scrollbars(id: ScrollbarsDefId) -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Scrollbars(id)))
    }

    #[track_caller]
    pub(crate) fn scroll(spec: ScrollSpec) -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Scroll(spec)))
    }

    /// Set the lower size bound, checking it against the upper one.
    ///
    /// The four `set_*` writers below own every check an authored field
    /// owes, and everything that writes one goes through them: the
    /// consuming [`Configure`](crate::Configure) setter, the
    /// [`ThemeDefaults`](crate::scene::node::theme_defaults::ThemeDefaults)
    /// fallback beside it, and the widgets that hold a `&mut Node` and
    /// cannot move it through a builder. A field written past them is a
    /// field whose bound or NaN screen did not run.
    ///
    /// # Panics
    ///
    /// Panics if the bound is negative, non-finite, or above a maximum
    /// already set on this node.
    #[inline]
    pub(crate) fn set_min_size(&mut self, value: Size) {
        limits::assert_valid_bounds(value, self.max_size.unwrap_or(Size::INF));
        self.min_size = Some(value);
    }

    /// Set the upper size bound, checking it against the lower one.
    ///
    /// # Panics
    ///
    /// Panics if the bound is negative, NaN, or below a minimum already
    /// set on this node. Positive infinity is the unbounded maximum.
    #[inline]
    pub(crate) fn set_max_size(&mut self, value: Size) {
        limits::assert_valid_bounds(self.min_size.unwrap_or(Size::ZERO), value);
        self.max_size = Some(value);
    }

    /// Set the padding, screening NaN.
    ///
    /// A NaN edge does not fail on its own — it poisons every extent
    /// derived from it and surfaces frames later as a widget that
    /// measured to nothing, with no way back to the call that set it.
    /// `Corners` is screened at shape lowering for the same reason; this
    /// is the equivalent gate for the two spacings, which reach layout
    /// instead of the record.
    #[inline]
    pub(crate) fn set_padding(&mut self, value: Spacing) {
        debug_assert!(!value.has_nan(), "NaN in padding: {value:?}");
        self.padding = Some(value);
    }

    #[inline]
    pub(crate) fn set_margin(&mut self, value: Spacing) {
        debug_assert!(!value.has_nan(), "NaN in margin: {value:?}");
        self.margin = Some(value);
    }

    /// Fill a field in only where the caller stayed silent — the theme
    /// half of authoring, in the same one place as the plain writes.
    ///
    /// A guard plus the writer above, rather than a raw `get_or_insert`:
    /// the guard is what makes an explicit value win, and the writer is
    /// what makes a themed value face the same checks an authored one
    /// does.
    ///
    /// `fill_`, not `default_`: the consuming
    /// [`ThemeDefaults`](crate::scene::node::theme_defaults::ThemeDefaults)
    /// wrapper owns that name, and a `Node` held by value would resolve
    /// the trait's by-value method ahead of these and silently drop the
    /// node it returns.
    #[inline]
    pub(crate) fn fill_min_size(&mut self, value: Size) {
        if self.min_size.is_none() {
            self.set_min_size(value);
        }
    }

    #[inline]
    pub(crate) fn fill_max_size(&mut self, value: Size) {
        if self.max_size.is_none() {
            self.set_max_size(value);
        }
    }

    #[inline]
    pub(crate) fn fill_padding(&mut self, value: Spacing) {
        if self.padding.is_none() {
            self.set_padding(value);
        }
    }

    #[inline]
    pub(crate) fn fill_margin(&mut self, value: Spacing) {
        if self.margin.is_none() {
            self.set_margin(value);
        }
    }

    #[inline]
    pub(crate) fn fill_gap(&mut self, gap: f32) {
        if !self.gaps.gap_is_set() {
            self.gaps.set_gap(gap);
        }
    }

    /// Install this node's layout mode, once the payload a builder chain
    /// could not carry exists.
    ///
    /// The one way a mode is bound after construction — see
    /// [`NodeMode::accepts`] for what a mode may be replaced with.
    ///
    /// # Panics
    ///
    /// Panics if `mode` is not a refinement of the one the node has.
    pub(crate) fn set_mode(&mut self, mode: LayoutMode) {
        assert!(
            self.mode.accepts(mode),
            "{mode:?} installed on a {:?} node",
            self.mode,
        );
        self.mode = NodeMode::Resolved(mode);
    }

    pub(crate) fn scroll_spec(&self) -> ScrollSpec {
        let NodeMode::Resolved(LayoutMode::Scroll(spec)) = self.mode else {
            panic!("scroll specification read from {:?} node", self.mode);
        };
        spec
    }

    #[track_caller]
    fn new(mode: NodeMode) -> Self {
        Self {
            salt: Salt::Auto(WidgetId::auto_stable()),
            mode,
            size: None,
            min_size: None,
            max_size: None,
            padding: None,
            margin: None,
            clip: None,
            gaps: Gaps::UNSET_PAIR,
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

    /// Fan this `Node` out into the per-`NodeId` columns `Tree` stores,
    /// resolving every still-`None` themable field to its layout
    /// default — the tail of the per-widget recording chain, after which
    /// the node is dead.
    ///
    /// Single routing point: adding a field is one edit in the column
    /// type and one in the routing block below. `widget_id` is supplied
    /// by the caller (resolved from `self.salt` upstream in
    /// `Forest::open_node`) so `Node` itself never carries a resolved id.
    ///
    /// Consumes `self` to say exactly that the node is spent: the caller
    /// has no reason to read it again, and handing it over lets the
    /// chain feed it forward rather than keep a live copy alongside the
    /// columns.
    #[inline(always)]
    pub(super) fn into_columns(self, widget_id: WidgetId) -> NodeColumns {
        let mut attrs = self.flags;
        attrs.set_clip(self.clip.unwrap_or(ClipMode::None));
        NodeColumns {
            widget_id,
            layout: LayoutCore::from_node(&self),
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

/// A bare `Node` is its own configurable builder, so widget authors
/// can chain the [`Configure`] setters on the child nodes they construct
/// inside their `show` body — e.g.
/// `Node::leaf().id(my_id).size(...).sense(Sense::CLICK)`.
impl Configure for Node {
    #[inline]
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        ConfigureNode { node: self }
    }
}

#[cfg(test)]
mod tests;
