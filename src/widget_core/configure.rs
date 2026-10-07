//! The authoring surface every widget builder forwards to: [`Configure`] and
//! [`ThemeDefaults`] over a borrowed view of the [`Widget`]. Each setter has a
//! borrowing form on [`ConfigureWidget`] (`widget.configure().gap(0.0)`) and a
//! consuming form on the trait that forwards to it.

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
pub struct ConfigureWidget<'a> {
    pub(crate) widget: &'a mut Widget,
}

impl ConfigureWidget<'_> {
    /// See [`Configure::id_salt`].
    #[inline]
    pub fn id_salt(&mut self, key: impl Hash) -> &mut Self {
        self.widget.ident = Ident::Hash(WidgetId::from_hash(key));
        self
    }

    /// See [`Configure::id`].
    #[inline]
    pub const fn id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.ident = Ident::Verbatim(id);
        self
    }

    /// See [`Configure::auto_id`].
    #[track_caller]
    #[inline]
    pub const fn auto_id(&mut self) -> &mut Self {
        self.widget.ident = Ident::Auto(Location::caller());
        self
    }

    /// See [`Configure::size`].
    #[inline]
    #[track_caller]
    pub fn size(&mut self, s: impl Into<SizeSpec>) -> &mut Self {
        self.widget.node.size = Some(s.into());
        self
    }

    /// See [`ThemeDefaults::default_size`].
    #[inline]
    #[track_caller]
    pub fn default_size(&mut self, s: impl Into<SizeSpec>) -> &mut Self {
        self.widget.node.size.get_or_insert(s.into());
        self
    }

    /// See [`Configure::min_size`].
    #[inline]
    #[track_caller]
    pub fn min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_min_size(s.into());
        self
    }

    /// See [`Configure::max_size`].
    #[inline]
    #[track_caller]
    pub fn max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_max_size(s.into());
        self
    }

    /// See [`Configure::padding`].
    #[inline]
    #[track_caller]
    pub fn padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_padding(p.into());
        self
    }

    /// See [`Configure::margin`].
    #[inline]
    #[track_caller]
    pub fn margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_margin(m.into());
        self
    }

    /// See [`Configure::transform`].
    #[inline]
    #[track_caller]
    pub const fn transform(&mut self, t: TranslateScale) -> &mut Self {
        self.widget.node.transform = t;
        self
    }

    /// See [`Configure::position`].
    #[inline]
    #[track_caller]
    pub fn position(&mut self, p: impl Into<Vec2>) -> &mut Self {
        self.widget.node.position = vec2::offset(p.into());
        self
    }

    /// See [`Configure::grid_cell`].
    #[inline]
    #[track_caller]
    pub fn grid_cell(&mut self, cell: impl Into<GridCell>) -> &mut Self {
        self.widget.node.grid = cell.into();
        self
    }

    /// See [`Configure::adopt_placement`].
    #[inline]
    #[track_caller]
    pub fn adopt_placement(&mut self, from: &Widget) -> &mut Self {
        self.widget.node.adopt_placement(from.node);
        self
    }

    /// See [`Configure::gap`].
    #[inline]
    #[track_caller]
    pub fn gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_gap(g);
        self
    }

    /// See [`Configure::line_gap`].
    #[inline]
    #[track_caller]
    pub fn line_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_line_gap(g);
        self
    }

    /// See [`Configure::justify`].
    #[inline]
    pub const fn justify(&mut self, j: Justify) -> &mut Self {
        self.widget.node.justify = j;
        self
    }

    /// See [`Configure::align`].
    #[inline]
    pub const fn align(&mut self, a: Align) -> &mut Self {
        self.widget.node.align = a;
        self
    }

    /// See [`Configure::child_align`].
    #[inline]
    pub const fn child_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.child_align = a;
        self
    }

    /// See [`Configure::sense`].
    #[inline]
    pub const fn sense(&mut self, s: Sense) -> &mut Self {
        self.widget.node.flags.set_sense(s);
        self
    }

    /// See [`Configure::add_sense`].
    #[inline]
    pub fn add_sense(&mut self, s: Sense) -> &mut Self {
        let sense = self.widget.node.flags.sense() | s;
        self.widget.node.flags.set_sense(sense);
        self
    }

    /// See [`Configure::disabled`].
    #[inline]
    pub const fn disabled(&mut self, d: bool) -> &mut Self {
        self.widget.node.flags.set_disabled(d);
        self
    }

    /// See [`Configure::focusable`].
    #[inline]
    pub const fn focusable(&mut self, f: bool) -> &mut Self {
        self.widget.node.flags.set_focusable(f);
        self
    }

    /// See [`Configure::tab_stop`].
    #[inline]
    pub const fn tab_stop(&mut self, stop: bool) -> &mut Self {
        self.widget.node.flags.set_tab_stop(stop);
        self
    }

    /// See [`Configure::arrow_focus`].
    #[inline]
    pub const fn arrow_focus(&mut self, axis: Axis) -> &mut Self {
        self.widget.node.flags.set_arrow_focus(Some(axis));
        self
    }

    /// See [`Configure::tab_index`].
    #[inline]
    pub const fn tab_index(&mut self, index: i16) -> &mut Self {
        self.widget.node.tab_index = index;
        self
    }

    /// See [`Configure::input_scope`].
    #[inline]
    pub const fn input_scope(&mut self, takes: KeyFilter) -> &mut Self {
        self.widget.node.flags.set_key_filter(takes);
        self
    }

    /// See [`Configure::visibility`].
    #[inline]
    pub const fn visibility(&mut self, v: Visibility) -> &mut Self {
        self.widget.node.visibility = v;
        self
    }

    /// See [`Configure::hidden`].
    #[inline]
    pub const fn hidden(&mut self) -> &mut Self {
        self.visibility(Visibility::Hidden);
        self
    }

    /// See [`Configure::collapsed`].
    #[inline]
    pub const fn collapsed(&mut self) -> &mut Self {
        self.visibility(Visibility::Collapsed);
        self
    }

    /// See [`Configure::clip`].
    #[inline]
    pub const fn clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip = Some(mode);
        self
    }

    /// See [`Configure::clip_rect`].
    #[inline]
    pub const fn clip_rect(&mut self) -> &mut Self {
        self.clip(ClipMode::Rect);
        self
    }

    /// See [`Configure::clip_rounded`].
    #[inline]
    pub const fn clip_rounded(&mut self) -> &mut Self {
        self.clip(ClipMode::Rounded);
        self
    }

    /// See [`ThemeDefaults::default_id`].
    #[inline]
    pub const fn default_id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.fill_id(id);
        self
    }

    /// See [`ThemeDefaults::default_padding`].
    #[inline]
    #[track_caller]
    pub fn default_padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_padding(p.into());
        self
    }

    /// See [`ThemeDefaults::default_margin`].
    #[inline]
    #[track_caller]
    pub fn default_margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_margin(m.into());
        self
    }

    /// See [`ThemeDefaults::default_align`].
    #[inline]
    pub const fn default_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.fill_align(a);
        self
    }

    /// See [`ThemeDefaults::default_gap`].
    #[inline]
    #[track_caller]
    pub fn default_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.fill_gap(g);
        self
    }

    /// See [`ThemeDefaults::default_min_size`].
    #[inline]
    #[track_caller]
    pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_min_size(s.into());
        self
    }

    /// See [`ThemeDefaults::default_max_size`].
    #[inline]
    #[track_caller]
    pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_max_size(s.into());
        self
    }

    /// See [`ThemeDefaults::default_clip`].
    #[inline]
    pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip.get_or_insert(mode);
        self
    }
}

/// Mixin: a builder holding a [`Widget`] gets the setters by implementing [`Self::configure`].
pub trait Configure: Sized {
    /// This builder's widget, borrowed for configuration. The one method an implementor writes.
    fn configure(&mut self) -> ConfigureWidget<'_>;

    /// Override this widget's id with a hash of `key`, scoped to the parent.
    ///
    /// `*::new()` is `#[track_caller]`, so the default id is the widget's own call site.
    /// Use `id_salt(key)` when the call site repeats (a loop, a helper drawing many
    /// widgets), keyed on the instance's own id rather than an unstable index or label.
    /// For a `#[track_caller]` helper that should take its caller's site, chain
    /// [`Self::auto_id`]. [`Self::id`] is for an id computed elsewhere.
    ///
    /// Same-parent sibling collisions are disambiguated but flagged with a magenta
    /// outline, since they are caller bugs.
    #[inline]
    #[must_use]
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.configure().id_salt(key);
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`], used verbatim and
    /// not mixed with the parent. Otherwise prefer [`Self::id_salt`].
    ///
    /// Set after [`Widget::resolve`], it replaces the resolved identity; [`crate::Modal`]
    /// uses this to move its configuration under a child of its backdrop's id.
    #[inline]
    #[must_use]
    fn id(mut self, id: WidgetId) -> Self {
        self.configure().id(id);
        self
    }

    /// Re-derive this widget's auto id at the current call site. Only useful inside
    /// a `#[track_caller]` helper, where that site is the helper's caller:
    ///
    /// ```
    /// # use palantir::{Configure, Panel, Sizing, Text, Ui};
    /// /// One section per caller, each with its own id.
    /// #[track_caller]
    /// fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ///     Panel::vstack()
    ///         .auto_id()                 // ← the caller's location, not this one
    ///         .size((Sizing::FILL, Sizing::HUG))
    ///         .show(ui, |ui| {
    ///             Text::new(title).show(ui);
    ///             body(ui);
    ///         });
    /// }
    /// ```
    #[track_caller]
    #[inline]
    #[must_use]
    fn auto_id(mut self) -> Self {
        self.configure().auto_id();
        self
    }

    /// Both axes at once: a [`Sizing`](crate::Sizing), a bare number (fixed), a `(w, h)` pair or a
    /// [`Size`].
    #[inline]
    #[must_use]
    #[track_caller]
    fn size(mut self, s: impl Into<SizeSpec>) -> Self {
        self.configure().size(s);
        self
    }

    /// The smallest size layout gives this node, each axis a *length*; a smaller
    /// maximum is raised to it.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [lengths](crate::widget::domain::length).
    #[inline]
    #[must_use]
    #[track_caller]
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().min_size(s);
        self
    }

    /// The largest size layout gives this node, each axis an *extent* (`+inf` is
    /// unbounded).
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [extents](crate::widget::domain::extent).
    #[inline]
    #[must_use]
    #[track_caller]
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().max_size(s);
        self
    }

    /// Space between this node's edge and its children, each edge a *length*.
    ///
    /// # Panics
    ///
    /// Panics unless every edge is a [length](crate::widget::domain::length).
    #[inline]
    #[must_use]
    #[track_caller]
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.configure().padding(p);
        self
    }

    /// Space between this node's edge and its siblings, each edge an *offset* (a
    /// negative margin pulls a sibling in).
    ///
    /// # Panics
    ///
    /// Panics unless every edge is an [offset](crate::widget::domain::offset).
    #[inline]
    #[must_use]
    #[track_caller]
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.configure().margin(m);
        self
    }

    /// Apply a pan/zoom transform to this node's body (children and shapes recorded
    /// on it); layout runs untransformed. Scale anchors at the node's own origin; see
    /// [`TranslateScale::anchored_at`].
    ///
    /// Widget chrome ([`Panel::background`](crate::Panel::background) and siblings)
    /// paints in the parent's space, so the background frames the viewport; to pan it
    /// with the body, nest a container with the chrome on the child.
    #[inline]
    #[must_use]
    #[track_caller]
    fn transform(mut self, t: TranslateScale) -> Self {
        self.configure().transform(t);
        self
    }

    /// Absolute position inside a `Canvas` parent, each axis an *offset*. Ignored
    /// elsewhere.
    ///
    /// # Panics
    ///
    /// Panics unless both axes are [offsets](crate::widget::domain::offset).
    #[inline]
    #[must_use]
    #[track_caller]
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.configure().position(p);
        self
    }

    /// Placement inside a `Grid` parent: a bare `(row, col)` or a [`GridCell`]. Default
    /// `(0, 0)`.
    ///
    /// # Panics
    ///
    /// An out-of-range cell or span panics: debug builds check at record time
    /// (`Tree::check_grid_cell`), release builds in layout.
    #[inline]
    #[must_use]
    #[track_caller]
    fn grid_cell(mut self, cell: impl Into<GridCell>) -> Self {
        self.configure().grid_cell(cell);
        self
    }

    /// Take over `from`'s placement (where it sits in its parent), not its contents or
    /// behavior. For a widget handing its slot to another mid-gesture
    /// ([`DragValue`](crate::DragValue) to [`TextEdit`](crate::TextEdit)) or recorded as
    /// two nodes ([`Scroll`](crate::Scroll)). Size, padding and transform stay the
    /// adopter's; a `None` margin keeps its themed default.
    #[inline]
    #[must_use]
    #[track_caller]
    fn adopt_placement(mut self, from: &Widget) -> Self {
        self.configure().adopt_placement(from);
        self
    }

    /// Space between siblings within a line, a *gap*, read by stacks, wrap stacks and
    /// Grid columns.
    ///
    /// # Panics
    ///
    /// Panics unless `g` is a [gap](crate::widget::domain::gap).
    #[inline]
    #[must_use]
    #[track_caller]
    fn gap(mut self, g: f32) -> Self {
        self.configure().gap(g);
        self
    }

    /// Space between lines: wrap rows and Grid rows. Inert elsewhere. `g`: a *gap*.
    ///
    /// # Panics
    ///
    /// Panics unless `g` is a [gap](crate::widget::domain::gap).
    #[inline]
    #[must_use]
    #[track_caller]
    fn line_gap(mut self, g: f32) -> Self {
        self.configure().line_gap(g);
        self
    }

    /// Main-axis distribution of leftover space for `HStack`/`VStack`. [`crate::Sizing::fill`]
    /// children take leftover first, so this distributes only what a max size leaves.
    #[inline]
    #[must_use]
    fn justify(mut self, j: Justify) -> Self {
        self.configure().justify(j);
        self
    }

    /// Alignment inside the parent's inner rect. For one axis use [`Align::h`] / [`Align::v`].
    #[inline]
    #[must_use]
    fn align(mut self, a: Align) -> Self {
        self.configure().align(a);
        self
    }

    /// Default alignment for children whose own axis is `Auto` (CSS `align-items`). For one axis
    /// use [`Align::h`] / [`Align::v`].
    #[inline]
    #[must_use]
    fn child_align(mut self, a: Align) -> Self {
        self.configure().child_align(a);
        self
    }

    /// Replace what this node senses. [`Self::add_sense`] folds instead.
    #[inline]
    #[must_use]
    fn sense(mut self, s: Sense) -> Self {
        self.configure().sense(s);
        self
    }

    /// Fold `s` into what this node already senses, for widgets with a non-negotiable
    /// gesture ([`crate::Scroll`] zoom needs [`Sense::PINCH`]). [`Self::sense`] would
    /// drop the caller's choice and chain order would decide.
    #[inline]
    #[must_use]
    fn add_sense(mut self, s: Sense) -> Self {
        self.configure().add_sense(s);
        self
    }

    /// Suppress this node's interactions and cascade to descendants. It keeps its [`Sense`] and
    /// still takes hover, press and wheel from what it covers, but answers none.
    #[inline]
    #[must_use]
    fn disabled(mut self, d: bool) -> Self {
        self.configure().disabled(d);
        self
    }

    /// Make this node eligible for keyboard focus, on a press and as a Tab stop. Default `false`.
    /// Disabled or invisible nodes are excluded regardless.
    #[inline]
    #[must_use]
    fn focusable(mut self, f: bool) -> Self {
        self.configure().focusable(f);
        self
    }

    /// Whether Tab stops here when [`focusable`](Self::focusable). Default `true`; `false` keeps
    /// press focus but leaves the Tab order, as WPF's `IsTabStop`.
    #[inline]
    #[must_use]
    fn tab_stop(mut self, stop: bool) -> Self {
        self.configure().tab_stop(stop);
        self
    }

    /// Make this node a group whose stops the arrow keys along `axis` move focus
    /// between (menus, radio groups, toolbars), wrapping. Arrows no scope inside the
    /// group claims are the framework's, so a text field in a toolbar keeps its caret
    /// keys.
    #[inline]
    #[must_use]
    fn arrow_focus(mut self, axis: Axis) -> Self {
        self.configure().arrow_focus(axis);
        self
    }

    /// This node's key in the Tab order. Default `0`; ascending, equal indices in record order, as
    /// WPF's `TabIndex`.
    #[inline]
    #[must_use]
    fn tab_index(mut self, index: i16) -> Self {
        self.configure().tab_index(index);
        self
    }

    /// Make this node an input scope taking `takes` while active. A key press walks
    /// the active path deepest-first and goes to the first scope whose filter contains
    /// its [`KeyClass`](crate::KeyClass), so a focused text field owns `Ctrl+Z` while
    /// `Ctrl+S` passes on ([`KeyFilter::TEXT_FIELD`] omits `ACCEL`). An overlay declaring
    /// [`KeyFilter::ALL`] cuts the layers below off. Not focus: a scope is where input
    /// belongs, focus is where typing goes. [`KeyFilter::NONE`] clears it.
    #[inline]
    #[must_use]
    fn input_scope(mut self, takes: KeyFilter) -> Self {
        self.configure().input_scope(takes);
        self
    }

    /// Three-state visibility. See [`Visibility`].
    #[inline]
    #[must_use]
    fn visibility(mut self, v: Visibility) -> Self {
        self.configure().visibility(v);
        self
    }

    /// Shorthand for [`Visibility::Hidden`]: keeps the slot, hides paint and input.
    #[inline]
    #[must_use]
    fn hidden(mut self) -> Self {
        self.configure().hidden();
        self
    }

    /// Shorthand for [`Visibility::Collapsed`]: zero slot.
    #[inline]
    #[must_use]
    fn collapsed(mut self) -> Self {
        self.configure().collapsed();
        self
    }

    /// Generic clip setter; see [`Self::clip_rect`] / [`Self::clip_rounded`].
    #[inline]
    #[must_use]
    fn clip(mut self, mode: ClipMode) -> Self {
        self.configure().clip(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    #[inline]
    #[must_use]
    fn clip_rect(mut self) -> Self {
        self.configure().clip_rect();
        self
    }

    /// Rounded-corner stencil clip; the radius comes from the widget chrome's background (zero
    /// without chrome).
    #[inline]
    #[must_use]
    fn clip_rounded(mut self) -> Self {
        self.configure().clip_rounded();
        self
    }
}

/// The theme half of [`Configure`]: fill a field only where the caller stayed
/// silent. Public so widgets outside this crate resolve the same way; an app
/// chaining `.default_padding(..)` onto a `Button` overrides nothing.
/// Blanket-implemented for every `Configure`.
pub trait ThemeDefaults: Configure {
    /// Identity to fall back on when the caller set none; an auto id doesn't count.
    #[inline]
    #[must_use]
    fn default_id(mut self, id: WidgetId) -> Self {
        self.configure().default_id(id);
        self
    }

    /// The size to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_size(mut self, s: impl Into<SizeSpec>) -> Self {
        self.configure().default_size(s);
        self
    }

    /// Padding to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_padding(mut self, p: impl Into<Spacing>) -> Self {
        self.configure().default_padding(p);
        self
    }

    /// Margin to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_margin(mut self, m: impl Into<Spacing>) -> Self {
        self.configure().default_margin(m);
        self
    }

    /// Alignment to fall back on, per axis.
    #[inline]
    #[must_use]
    fn default_align(mut self, a: Align) -> Self {
        self.configure().default_align(a);
        self
    }

    /// Sibling spacing to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_gap(mut self, g: f32) -> Self {
        self.configure().default_gap(g);
        self
    }

    /// Lower size bound to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_min_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().default_min_size(s);
        self
    }

    /// Upper size bound to fall back on when the caller set none.
    #[inline]
    #[must_use]
    #[track_caller]
    fn default_max_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().default_max_size(s);
        self
    }

    /// Clip mode to fall back on when the caller set none.
    #[inline]
    #[must_use]
    fn default_clip(mut self, mode: ClipMode) -> Self {
        self.configure().default_clip(mode);
        self
    }
}

impl<T: Configure> ThemeDefaults for T {}
