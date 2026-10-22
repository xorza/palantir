//! [`Configurator`]: the borrowing form of every [`Configure`](crate::widget_core::configure::Configure) setter
//! (`widget.configure().gap(0.0)`).

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::math::domain::vec2;
use crate::scene::node::ident::Ident;
use crate::widget_core::widget::Widget;
use glam::Vec2;
use std::hash::Hash;
use std::panic::Location;

/// A widget borrowed for configuration. Opaque on purpose: it exposes configuration, not the
/// widget's structural layout mode.
#[derive(Debug)]
#[must_use = "a bare configure() writes nothing; chain a setter onto it"]
pub struct Configurator<'a> {
    pub(crate) widget: &'a mut Widget,
}

impl Configurator<'_> {
    /// See [`Configure::id_salt`](crate::widget_core::configure::Configure::id_salt).
    #[inline]
    pub fn id_salt(&mut self, key: impl Hash) -> &mut Self {
        self.widget.ident = Ident::Hash(WidgetId::from_hash(key));
        self
    }

    /// See [`Configure::id`](crate::widget_core::configure::Configure::id).
    #[inline]
    pub const fn id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.ident = Ident::Verbatim(id);
        self
    }

    /// See [`Configure::auto_id`](crate::widget_core::configure::Configure::auto_id).
    #[track_caller]
    #[inline]
    pub const fn auto_id(&mut self) -> &mut Self {
        self.widget.ident = Ident::Auto(Location::caller());
        self
    }

    /// See [`Configure::size`](crate::widget_core::configure::Configure::size).
    #[inline]
    #[track_caller]
    pub fn size(&mut self, s: impl Into<SizeSpec>) -> &mut Self {
        self.widget.node.size = Some(s.into());
        self
    }

    /// See [`ThemeDefaults::default_size`](crate::widget_core::configure::ThemeDefaults::default_size).
    #[inline]
    #[track_caller]
    pub fn default_size(&mut self, s: impl Into<SizeSpec>) -> &mut Self {
        self.widget.node.size.get_or_insert_with(|| s.into());
        self
    }

    /// See [`Configure::min_size`](crate::widget_core::configure::Configure::min_size).
    #[inline]
    #[track_caller]
    pub fn min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_min_size(s.into());
        self
    }

    /// See [`Configure::max_size`](crate::widget_core::configure::Configure::max_size).
    #[inline]
    #[track_caller]
    pub fn max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_max_size(s.into());
        self
    }

    /// See [`Configure::padding`](crate::widget_core::configure::Configure::padding).
    #[inline]
    #[track_caller]
    pub fn padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_padding(p.into());
        self
    }

    /// See [`Configure::margin`](crate::widget_core::configure::Configure::margin).
    #[inline]
    #[track_caller]
    pub fn margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_margin(m.into());
        self
    }

    /// See [`Configure::transform`](crate::widget_core::configure::Configure::transform).
    #[inline]
    #[track_caller]
    pub const fn transform(&mut self, t: TranslateScale) -> &mut Self {
        self.widget.node.transform = t;
        self
    }

    /// See [`Configure::pos`](crate::widget_core::configure::Configure::pos).
    #[inline]
    #[track_caller]
    pub fn pos(&mut self, p: impl Into<Vec2>) -> &mut Self {
        self.widget.node.pos = vec2::offset(p.into());
        self
    }

    /// See [`Configure::grid_cell`](crate::widget_core::configure::Configure::grid_cell).
    #[inline]
    #[track_caller]
    pub fn grid_cell(&mut self, cell: impl Into<GridCell>) -> &mut Self {
        self.widget.node.grid = cell.into();
        self
    }

    /// See [`Configure::adopt_placement`](crate::widget_core::configure::Configure::adopt_placement).
    #[inline]
    #[track_caller]
    pub fn adopt_placement(&mut self, from: &Widget) -> &mut Self {
        self.widget.node.adopt_placement(from.node);
        self
    }

    /// See [`Configure::gap`](crate::widget_core::configure::Configure::gap).
    #[inline]
    #[track_caller]
    pub fn gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_gap(g);
        self
    }

    /// See [`Configure::line_gap`](crate::widget_core::configure::Configure::line_gap).
    #[inline]
    #[track_caller]
    pub fn line_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_line_gap(g);
        self
    }

    /// See [`Configure::justify`](crate::widget_core::configure::Configure::justify).
    #[inline]
    pub const fn justify(&mut self, j: Justify) -> &mut Self {
        self.widget.node.justify = j;
        self
    }

    /// See [`Configure::align`](crate::widget_core::configure::Configure::align).
    #[inline]
    pub const fn align(&mut self, a: Align) -> &mut Self {
        self.widget.node.align = a;
        self
    }

    /// See [`Configure::child_align`](crate::widget_core::configure::Configure::child_align).
    #[inline]
    pub const fn child_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.child_align = a;
        self
    }

    /// See [`Configure::sense`](crate::widget_core::configure::Configure::sense).
    #[inline]
    pub const fn sense(&mut self, s: Sense) -> &mut Self {
        self.widget.node.flags.set_sense(s);
        self
    }

    /// See [`Configure::add_sense`](crate::widget_core::configure::Configure::add_sense).
    #[inline]
    pub fn add_sense(&mut self, s: Sense) -> &mut Self {
        let sense = self.widget.node.flags.sense() | s;
        self.widget.node.flags.set_sense(sense);
        self
    }

    /// See [`Configure::disabled`](crate::widget_core::configure::Configure::disabled).
    #[inline]
    pub const fn disabled(&mut self, d: bool) -> &mut Self {
        self.widget.node.flags.set_disabled(d);
        self
    }

    /// See [`Configure::focusable`](crate::widget_core::configure::Configure::focusable).
    #[inline]
    pub const fn focusable(&mut self, f: bool) -> &mut Self {
        self.widget.node.flags.set_focusable(f);
        self
    }

    /// See [`Configure::tab_stop`](crate::widget_core::configure::Configure::tab_stop).
    #[inline]
    pub const fn tab_stop(&mut self, stop: bool) -> &mut Self {
        self.widget.node.flags.set_tab_stop(stop);
        self
    }

    /// See [`Configure::arrow_focus`](crate::widget_core::configure::Configure::arrow_focus).
    #[inline]
    pub const fn arrow_focus(&mut self, axis: Axis) -> &mut Self {
        self.widget.node.flags.set_arrow_focus(Some(axis));
        self
    }

    /// See [`Configure::tab_idx`](crate::widget_core::configure::Configure::tab_idx).
    #[inline]
    pub const fn tab_idx(&mut self, idx: i16) -> &mut Self {
        self.widget.node.tab_idx = idx;
        self
    }

    /// See [`Configure::input_scope`](crate::widget_core::configure::Configure::input_scope).
    #[inline]
    pub const fn input_scope(&mut self, takes: KeyFilter) -> &mut Self {
        self.widget.node.flags.set_key_filter(takes);
        self
    }

    /// See [`Configure::visibility`](crate::widget_core::configure::Configure::visibility).
    #[inline]
    pub const fn visibility(&mut self, v: Visibility) -> &mut Self {
        self.widget.node.visibility = v;
        self
    }

    /// See [`Configure::hidden`](crate::widget_core::configure::Configure::hidden).
    #[inline]
    pub const fn hidden(&mut self) -> &mut Self {
        self.visibility(Visibility::Hidden);
        self
    }

    /// See [`Configure::collapsed`](crate::widget_core::configure::Configure::collapsed).
    #[inline]
    pub const fn collapsed(&mut self) -> &mut Self {
        self.visibility(Visibility::Collapsed);
        self
    }

    /// See [`Configure::clip`](crate::widget_core::configure::Configure::clip).
    #[inline]
    pub const fn clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip = Some(mode);
        self
    }

    /// See [`Configure::clip_rect`](crate::widget_core::configure::Configure::clip_rect).
    #[inline]
    pub const fn clip_rect(&mut self) -> &mut Self {
        self.clip(ClipMode::Rect);
        self
    }

    /// See [`Configure::clip_rounded`](crate::widget_core::configure::Configure::clip_rounded).
    #[inline]
    pub const fn clip_rounded(&mut self) -> &mut Self {
        self.clip(ClipMode::Rounded);
        self
    }

    /// See [`ThemeDefaults::default_id`](crate::widget_core::configure::ThemeDefaults::default_id).
    #[inline]
    pub const fn default_id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.fill_id(id);
        self
    }

    /// See [`ThemeDefaults::default_padding`](crate::widget_core::configure::ThemeDefaults::default_padding).
    #[inline]
    #[track_caller]
    pub fn default_padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_padding(p.into());
        self
    }

    /// See [`ThemeDefaults::default_margin`](crate::widget_core::configure::ThemeDefaults::default_margin).
    #[inline]
    #[track_caller]
    pub fn default_margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_margin(m.into());
        self
    }

    /// See [`ThemeDefaults::default_align`](crate::widget_core::configure::ThemeDefaults::default_align).
    #[inline]
    pub const fn default_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.fill_align(a);
        self
    }

    /// See [`ThemeDefaults::default_gap`](crate::widget_core::configure::ThemeDefaults::default_gap).
    #[inline]
    #[track_caller]
    pub fn default_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.fill_gap(g);
        self
    }

    /// See [`ThemeDefaults::default_min_size`](crate::widget_core::configure::ThemeDefaults::default_min_size).
    #[inline]
    #[track_caller]
    pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_min_size(s.into());
        self
    }

    /// See [`ThemeDefaults::default_max_size`](crate::widget_core::configure::ThemeDefaults::default_max_size).
    #[inline]
    #[track_caller]
    pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_max_size(s.into());
        self
    }

    /// See [`ThemeDefaults::default_clip`](crate::widget_core::configure::ThemeDefaults::default_clip).
    #[inline]
    pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip.get_or_insert(mode);
        self
    }
}
