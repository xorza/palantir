//! The layout, interaction and paint record a [`Widget`] carries, in the shape the tree reads it.
//!
//! [`Widget`]: crate::widget_core::widget::Widget

pub(crate) mod authored_gaps;
pub(crate) mod bounds_extras;
pub(crate) mod gaps;
pub(crate) mod ident;
pub(crate) mod layout_core;
pub(crate) mod node_columns;
pub(crate) mod node_flags;
pub(crate) mod node_mode;
pub(crate) mod panel_extras;

use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::math::domain;
use crate::scene::node::authored_gaps::AuthoredGaps;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_columns::NodeColumns;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::node::panel_extras::PanelExtras;
use glam::Vec2;

/// Per-node config: layout, interaction and paint flags. Every [`Widget`] owns one; identity is the widget's, never the node's.
///
/// [`Widget`]: crate::widget_core::widget::Widget
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) mode: NodeMode,

    /// Themable fields are `None` until set, so widgets layer theme defaults under user intent; [`Self::columns`] resolves `None` to layout defaults.
    pub(crate) size: Option<SizeSpec>,
    pub(crate) min_size: Option<Size>,
    pub(crate) max_size: Option<Size>,
    pub(crate) padding: Option<Spacing>,
    pub(crate) margin: Option<Spacing>,
    /// Clip mode, `None` until set; folded into the recorded flags by [`Self::columns`].
    pub(crate) clip: Option<ClipMode>,

    /// Within-line and between-line gap as two f16 lanes: `gap()` for siblings (and Grid columns), `line_gap()` for lines (and Grid rows). Ignored by Leaf/ZStack/Canvas.
    pub(crate) gaps: AuthoredGaps,

    pub(crate) justify: Justify,
    pub(crate) align: Align,
    pub(crate) child_align: Align,
    pub(crate) position: Vec2,
    /// Cell and span inside a `Grid` parent; ignored elsewhere.
    pub(crate) grid: GridCell,

    pub(crate) flags: NodeFlags,

    /// Three-state visibility: `Hidden` keeps the slot but suppresses paint and input; `Collapsed` zeros the slot and skips the subtree.
    pub(crate) visibility: Visibility,
    /// Pan/zoom applied to descendants post-layout, like WPF's `RenderTransform`; origin is the panel's top-left.
    pub(crate) transform: TranslateScale,
    pub(crate) tab_index: i16,
}

impl Node {
    /// Sets the lower size bound. A smaller maximum is raised to it, as in CSS and WPF.
    ///
    /// The four `set_*` writers own every check an authored field owes; the consuming setter, the theme fallback and widgets holding `&mut Node` all go through them.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are *lengths*.
    #[inline]
    #[track_caller]
    pub(crate) const fn set_min_size(&mut self, value: Size) {
        domain::length(value.w);
        domain::length(value.h);
        self.min_size = Some(value);
        if let Some(max) = self.max_size {
            self.max_size = Some(Size::new(max.w.max(value.w), max.h.max(value.h)));
        }
    }

    /// Sets the upper size bound; one below an already-set minimum is raised to it.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are *extents*; positive infinity is unbounded.
    #[inline]
    #[track_caller]
    pub(crate) const fn set_max_size(&mut self, value: Size) {
        domain::extent(value.w);
        domain::extent(value.h);
        let min = match self.min_size {
            Some(min) => min,
            None => Size::ZERO,
        };
        self.max_size = Some(Size::new(value.w.max(min.w), value.h.max(min.h)));
    }

    /// Sets the padding: every edge a *length*. Checked in release because a NaN edge poisons derived extents.
    ///
    /// # Panics
    ///
    /// Panics unless every edge is a length.
    #[inline]
    #[track_caller]
    pub(crate) fn set_padding(&mut self, value: Spacing) {
        for edge in value.as_array() {
            domain::length(edge);
        }
        self.padding = Some(value);
    }

    /// Sets the margin: every edge an *offset*, so a negative margin pulls a sibling in.
    ///
    /// # Panics
    ///
    /// Panics unless every edge is an offset.
    #[inline]
    #[track_caller]
    pub(crate) fn set_margin(&mut self, value: Spacing) {
        for edge in value.as_array() {
            domain::offset(edge);
        }
        self.margin = Some(value);
    }

    /// Fills a field only where the caller stayed silent, through the checked writer above. Named `fill_`, not `default_`, which [`ThemeDefaults`](crate::widget_core::configure::ThemeDefaults) owns. A default also yields to the *other* bound the caller set: a themed minimum above an authored maximum is clamped down.
    #[inline]
    #[track_caller]
    pub(crate) const fn fill_min_size(&mut self, value: Size) {
        if self.min_size.is_none() {
            let value = match self.max_size {
                Some(max) => Size::new(at_most(value.w, max.w), at_most(value.h, max.h)),
                None => value,
            };
            self.set_min_size(value);
        }
    }

    /// Mirror of [`Self::fill_min_size`]: a themed maximum below an authored minimum is raised to it.
    #[inline]
    #[track_caller]
    pub(crate) const fn fill_max_size(&mut self, value: Size) {
        if self.max_size.is_none() {
            self.set_max_size(value);
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn fill_padding(&mut self, value: Spacing) {
        if self.padding.is_none() {
            self.set_padding(value);
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn fill_margin(&mut self, value: Spacing) {
        if self.margin.is_none() {
            self.set_margin(value);
        }
    }

    #[inline]
    #[track_caller]
    pub(crate) fn fill_gap(&mut self, gap: f32) {
        if self.gaps.gap().is_none() {
            self.gaps.set_gap(gap);
        }
    }

    /// Fills each axis the caller left `Auto`, leaving the other alone.
    #[inline]
    pub(crate) const fn fill_align(&mut self, value: Align) {
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

    /// Takes over `from`'s placement (position in the parent and Tab order), nothing about contents or behavior; for a widget handing its slot to a second node ([`crate::DragValue`]'s chip to [`crate::TextEdit`]) or recording as two nodes ([`crate::Scroll`]). Margin is the one `Option`: `None` keeps the adopting node's themed default.
    ///
    /// The destructure is exhaustive so a new field must be given a side.
    pub(crate) fn adopt_placement(&mut self, from: Node) {
        let Node {
            mode: _,
            // Box extent, not placement: the adopting node sizes itself.
            size: _,
            min_size: _,
            max_size: _,
            padding: _,
            clip: _,
            gaps: _,
            justify: _,
            child_align: _,
            flags,
            transform: _,
            margin,
            align,
            position,
            grid,
            visibility,
            tab_index,
        } = from;

        if let Some(margin) = margin {
            self.set_margin(margin);
        }
        self.align = align;
        self.position = position;
        self.grid = grid;
        self.visibility = visibility;
        self.flags.set_tab_stop(flags.is_tab_stop());
        self.tab_index = tab_index;
    }

    /// Installs this node's layout mode once the payload a builder chain couldn't carry exists; see [`NodeMode::accepts`].
    ///
    /// # Panics
    ///
    /// Panics if `mode` is not a refinement of the node's current one.
    pub(crate) fn set_mode(&mut self, mode: LayoutMode) {
        assert!(
            self.mode.accepts(mode),
            "{mode:?} installed on a {:?} node",
            self.mode,
        );
        self.mode = NodeMode::Resolved(mode);
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
            tab_index: 0,
        }
    }

    /// Fans this `Node` out into the per-`NodeId` columns `Tree` stores, resolving `None` themable fields to layout defaults. Takes `&self` so the opener chain moves no 100-byte `Node` per hop; named without `to_`/`into_`, which would read as by-value.
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
                tab_index: self.tab_index,
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

/// `value` capped at `cap`, keeping a NaN `value` for the bound check that follows (`f32::min` would drop it).
const fn at_most(value: f32, cap: f32) -> f32 {
    if value > cap { cap } else { value }
}

#[cfg(test)]
mod tests;
