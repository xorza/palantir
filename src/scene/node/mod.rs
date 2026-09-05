//! The layout, interaction and paint record a [`Widget`] carries, in the
//! shape the tree reads it.
//!
//! [`Widget`]: crate::widgets::widget::Widget

pub(crate) mod authored_gaps;
pub(crate) mod bounds_extras;
pub(crate) mod container_chrome;
pub(crate) mod gaps;
pub(crate) mod ident;
pub(crate) mod layout_core;
pub(crate) mod node_columns;
pub(crate) mod node_flags;
pub(crate) mod node_mode;
pub(crate) mod panel_extras;

use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{LayoutMode, ScrollSpec};
use crate::layout::types::limits;
use crate::layout::types::sizing::Sizes;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::authored_gaps::AuthoredGaps;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::container_chrome::ContainerChrome;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_columns::NodeColumns;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::visibility::Visibility;
use glam::Vec2;

/// Per-node config: spatial layout + interaction + paint flags. Every
/// [`Widget`] owns one, and [`Widget::record`] hands it to the tree.
///
/// Fields are grouped by who reads them: own-size (every parent),
/// mode-specific (only certain parents read these), interaction, and
/// paint. Identity is the widget's, not the node's — a node is what a
/// widget records, and it never carries the id it records under.
///
/// [`Widget`]: crate::widgets::widget::Widget
/// [`Widget::record`]: crate::widgets::widget::Widget::record
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) mode: NodeMode,

    /// The themable fields are `None` until explicitly set, so
    /// widgets can layer theme defaults under user intent with a plain
    /// `get_or_insert` / `unwrap_or` — there is no separate provenance
    /// tracking. [`Self::columns`] resolves `None` to the layout
    /// defaults (`Sizes::default()`, `Size::ZERO`/`Size::INF` bounds,
    /// `Spacing::ZERO`).
    pub(crate) size: Option<Sizes>,
    pub(crate) min_size: Option<Size>,
    pub(crate) max_size: Option<Size>,
    pub(crate) padding: Option<Spacing>,
    pub(crate) margin: Option<Spacing>,
    /// Clip mode, `None` until set. Kept out of [`NodeFlags`] during
    /// authoring for the same theme-fallback reason; folded into the
    /// recorded flags by [`Self::columns`].
    pub(crate) clip: Option<ClipMode>,

    /// Within-line gap + between-line gap packed as two f16 lanes.
    /// `gaps.gap()` is the sibling spacing (HStack/VStack/WrapHStack/
    /// WrapVStack, and a Grid's columns); `gaps.line_gap()` is the
    /// between-line spacing (WrapHStack/WrapVStack, and a Grid's rows).
    /// Both ignored by Leaf/ZStack/Canvas.
    pub(crate) gaps: AuthoredGaps,

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

    /// Set the lower size bound, checking it against the upper one.
    ///
    /// The four `set_*` writers below own every check an authored field
    /// owes, and everything that writes one goes through them: the
    /// consuming [`Configure`] setter, the
    /// [`ThemeDefaults`](crate::widgets::configure::ThemeDefaults)
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
    /// [`ThemeDefaults`](crate::widgets::configure::ThemeDefaults)
    /// wrapper owns that name, and reads apart from it.
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
        if self.gaps.gap().is_none() {
            self.gaps.set_gap(gap);
        }
    }

    /// Fill each axis the caller left `Auto`, leaving the other alone.
    ///
    /// Per-axis rather than whole-value like the writers above, because
    /// `Auto` is what `Align` spells "unset" with and it spells it once
    /// per axis. A widget defaulting one axis must not silently take
    /// the other with it.
    #[inline]
    pub(crate) fn fill_align(&mut self, value: Align) {
        let h = match self.align.halign() {
            HAlign::Auto => value.halign(),
            set => set,
        };
        let v = match self.align.valign() {
            VAlign::Auto => value.valign(),
            set => set,
        };
        self.align = Align::new(h, v);
    }

    /// Take over `from`'s placement — where the node sits in its parent,
    /// and nothing about what it contains or how it behaves.
    ///
    /// For a widget that hands its slot to a second node partway through
    /// a gesture: [`crate::DragValue`] swaps its scrub chip for an inline
    /// [`crate::TextEdit`] on click, and without this the field visibly
    /// moves and resizes on the edit frame, because margin, alignment,
    /// grid placement and canvas position all go with the chip.
    ///
    /// And for a widget that records as two nodes rather than one:
    /// [`crate::Scroll`] splits the caller's node into an outer box and
    /// an inner viewport, and the placement is the outer one's.
    ///
    /// Margin is the one `Option`: `None` there means the caller stated
    /// no opinion, so the adopting node keeps its own themed default
    /// rather than taking a zero.
    ///
    /// The destructure is exhaustive on purpose. A new field has to be
    /// given a side here rather than silently vanishing across the swap,
    /// and an elided `..` would let that back in.
    pub(crate) fn adopt_placement(&mut self, from: Node) {
        let Node {
            // Layout mode is the adopting node's own: it is whatever
            // container or leaf it was built as.
            mode: _,
            // Box extent, not placement. The adopting node sizes itself:
            // the editor pins its width to the chip's last rect so a long
            // value scrolls instead of growing the row, and the scroll's
            // outer box takes the caller's sizing separately. Forwarding
            // these would undo both.
            size: _,
            min_size: _,
            max_size: _,
            padding: _,
            clip: _,
            // Interior configuration: what the node does with its own
            // children, and what it senses.
            gaps: _,
            justify: _,
            child_align: _,
            flags: _,
            // A render transform over the node's body, which is content
            // rather than placement.
            transform: _,
            // Everything below places the node inside its parent.
            margin,
            align,
            position,
            grid,
            visibility,
        } = from;

        if let Some(margin) = margin {
            self.set_margin(margin);
        }
        self.align = align;
        self.position = position;
        self.grid = grid;
        self.visibility = visibility;
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

    pub(crate) fn new(mode: NodeMode) -> Self {
        Self {
            mode,
            size: None,
            min_size: None,
            max_size: None,
            padding: None,
            margin: None,
            clip: None,
            gaps: AuthoredGaps::UNSET_PAIR,
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
    /// type and one in the routing block below. `widget_id` is the
    /// widget's, resolved before it recorded, so `Node` itself never
    /// carries one.
    ///
    /// Takes `&self` rather than the 100-byte `Node` by value, so the
    /// four-hop opener chain above it moves no bytes at any hop and does
    /// not lean on `#[inline]` to elide them. Named without a `to_` /
    /// `into_` prefix for that reason: both would read as a by-value
    /// receiver on a `Copy` type, which is what this avoids.
    #[inline(always)]
    pub(super) fn columns(&self, widget_id: WidgetId) -> NodeColumns {
        let mut attrs = self.flags;
        attrs.set_clip(self.clip.unwrap_or(ClipMode::None));
        NodeColumns {
            widget_id,
            layout: LayoutCore::from_node(self),
            attrs,
            bounds: BoundsExtras {
                position: self.position,
                grid: self.grid,
                min_size: self.min_size.unwrap_or(Size::ZERO),
                max_size: self.max_size.unwrap_or(Size::INF),
            },
            panel: PanelExtras {
                gaps: self.gaps.resolve(),
                justify: self.justify,
                child_align: self.child_align,
                transform: self.transform,
            },
        }
    }
}

#[cfg(test)]
mod tests;
