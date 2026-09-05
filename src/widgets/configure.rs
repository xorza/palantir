//! The authoring surface every widget builder forwards to: two traits of
//! layout, identity and paint setters, over a borrowed view of the
//! [`Widget`] behind them.
//!
//! Every setter exists in two forms. The borrowing one on
//! [`ConfigureWidget`] holds the body: a widget already in place chains
//! it — `widget.configure().gap(0.0)` — rather than reading the value
//! out and writing it back. The consuming one on the trait forwards to
//! it, so a chain on a value it owns reads
//! `Widget::leaf().sense(Sense::CLICK)`.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::ident::Ident;
use crate::scene::visibility::Visibility;
use crate::widgets::widget::Widget;
use glam::Vec2;
use std::hash::Hash;

/// A widget borrowed for configuration: the same setters [`Configure`]
/// chains, in the form that writes where the widget already sits.
///
/// Opaque on purpose. It carries configuration without carrying the
/// widget's structural layout mode, so a builder reaches its own
/// placement and paint without reaching the tree's shape.
#[derive(Debug)]
#[must_use = "a bare configure() writes nothing; chain a setter onto it"]
pub struct ConfigureWidget<'a> {
    pub(crate) widget: &'a mut Widget,
}

impl ConfigureWidget<'_> {
    /// Borrowing form of [`Configure::id_salt`].
    #[inline]
    pub fn id_salt(&mut self, key: impl Hash) -> &mut Self {
        self.widget.ident = Ident::Hash(WidgetId::from_hash(key));
        self
    }

    /// Borrowing form of [`Configure::id`].
    #[inline]
    pub fn id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.ident = Ident::Verbatim(id);
        self
    }

    /// Borrowing form of [`Configure::auto_id`].
    #[track_caller]
    #[inline]
    pub fn auto_id(&mut self) -> &mut Self {
        self.widget.ident = Ident::Auto(WidgetId::auto_stable());
        self
    }

    /// Borrowing form of [`Configure::size`].
    #[inline]
    pub fn size(&mut self, s: impl Into<Sizes>) -> &mut Self {
        self.widget.node.size = Some(s.into());
        self
    }

    /// Borrowing form of [`Configure::default_size`].
    #[inline]
    pub fn default_size(&mut self, s: impl Into<Sizes>) -> &mut Self {
        self.widget.node.size.get_or_insert(s.into());
        self
    }

    /// Borrowing form of [`Configure::min_size`].
    #[inline]
    pub fn min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_min_size(s.into());
        self
    }

    /// Borrowing form of [`Configure::max_size`].
    #[inline]
    pub fn max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.set_max_size(s.into());
        self
    }

    /// Borrowing form of [`Configure::padding`].
    #[inline]
    pub fn padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_padding(p.into());
        self
    }

    /// Borrowing form of [`Configure::margin`].
    #[inline]
    pub fn margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.set_margin(m.into());
        self
    }

    /// Borrowing form of [`Configure::transform`].
    #[inline]
    pub fn transform(&mut self, t: TranslateScale) -> &mut Self {
        self.widget.node.transform = t;
        self
    }

    /// Borrowing form of [`Configure::position`].
    #[inline]
    pub fn position(&mut self, p: impl Into<Vec2>) -> &mut Self {
        self.widget.node.position = p.into();
        self
    }

    /// Borrowing form of [`Configure::grid_cell`].
    #[inline]
    pub fn grid_cell(&mut self, cell: impl Into<GridCell>) -> &mut Self {
        self.widget.node.grid = cell.into();
        self
    }

    /// Borrowing form of [`Configure::gap`].
    #[inline]
    pub fn gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_gap(g);
        self
    }

    /// Borrowing form of [`Configure::line_gap`].
    #[inline]
    pub fn line_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.gaps.set_line_gap(g);
        self
    }

    /// Borrowing form of [`Configure::justify`].
    #[inline]
    pub fn justify(&mut self, j: Justify) -> &mut Self {
        self.widget.node.justify = j;
        self
    }

    /// Borrowing form of [`Configure::align`].
    #[inline]
    pub fn align(&mut self, a: Align) -> &mut Self {
        self.widget.node.align = a;
        self
    }

    /// Borrowing form of [`Configure::child_align`].
    #[inline]
    pub fn child_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.child_align = a;
        self
    }

    /// Borrowing form of [`Configure::sense`].
    #[inline]
    pub fn sense(&mut self, s: Sense) -> &mut Self {
        self.widget.node.flags.set_sense(s);
        self
    }

    /// Borrowing form of [`Configure::add_sense`].
    #[inline]
    pub fn add_sense(&mut self, s: Sense) -> &mut Self {
        let sense = self.widget.node.flags.sense() | s;
        self.widget.node.flags.set_sense(sense);
        self
    }

    /// Borrowing form of [`Configure::disabled`].
    #[inline]
    pub fn disabled(&mut self, d: bool) -> &mut Self {
        self.widget.node.flags.set_disabled(d);
        self
    }

    /// Borrowing form of [`Configure::focusable`].
    #[inline]
    pub fn focusable(&mut self, f: bool) -> &mut Self {
        self.widget.node.flags.set_focusable(f);
        self
    }

    /// Borrowing form of [`Configure::input_scope`].
    #[inline]
    pub fn input_scope(&mut self, takes: KeyFilter) -> &mut Self {
        self.widget.node.flags.set_key_filter(takes);
        self
    }

    /// Borrowing form of [`Configure::visibility`].
    #[inline]
    pub fn visibility(&mut self, v: Visibility) -> &mut Self {
        self.widget.node.visibility = v;
        self
    }

    /// Borrowing form of [`Configure::hidden`].
    #[inline]
    pub fn hidden(&mut self) -> &mut Self {
        self.visibility(Visibility::Hidden);
        self
    }

    /// Borrowing form of [`Configure::collapsed`].
    #[inline]
    pub fn collapsed(&mut self) -> &mut Self {
        self.visibility(Visibility::Collapsed);
        self
    }

    /// Borrowing form of [`Configure::clip`].
    #[inline]
    pub fn clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip = Some(mode);
        self
    }

    /// Borrowing form of [`Configure::clip_rect`].
    #[inline]
    pub fn clip_rect(&mut self) -> &mut Self {
        self.clip(ClipMode::Rect);
        self
    }

    /// Borrowing form of [`Configure::clip_rounded`].
    #[inline]
    pub fn clip_rounded(&mut self) -> &mut Self {
        self.clip(ClipMode::Rounded);
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_id`].
    #[inline]
    pub fn default_id(&mut self, id: WidgetId) -> &mut Self {
        self.widget.fill_id(id);
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_padding`].
    #[inline]
    pub fn default_padding(&mut self, p: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_padding(p.into());
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_margin`].
    #[inline]
    pub fn default_margin(&mut self, m: impl Into<Spacing>) -> &mut Self {
        self.widget.node.fill_margin(m.into());
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_align`].
    #[inline]
    pub fn default_align(&mut self, a: Align) -> &mut Self {
        self.widget.node.fill_align(a);
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_gap`].
    #[inline]
    pub fn default_gap(&mut self, g: f32) -> &mut Self {
        self.widget.node.fill_gap(g);
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_min_size`].
    #[inline]
    pub fn default_min_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_min_size(s.into());
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_max_size`].
    #[inline]
    pub fn default_max_size(&mut self, s: impl Into<Size>) -> &mut Self {
        self.widget.node.fill_max_size(s.into());
        self
    }

    /// Borrowing form of [`ThemeDefaults::default_clip`].
    #[inline]
    pub fn default_clip(&mut self, mode: ClipMode) -> &mut Self {
        self.widget.node.clip.get_or_insert(mode);
        self
    }
}

/// Mixin: any widget builder that holds a [`Widget`] gets the setters
/// (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for free
/// by impl'ing just [`Self::configure`].
pub trait Configure: Sized {
    /// This builder's widget, borrowed for configuration.
    ///
    /// The one method an implementor writes: every setter below
    /// forwards onto it. It is also the chain head each of them has
    /// a borrowing twin for, so a widget already in place takes the
    /// same chain — `widget.configure().gap(0.0).line_gap(0.0)` —
    /// instead of being read out and written back.
    fn configure(&mut self) -> ConfigureWidget<'_>;

    /// Override this widget's id with a hash of `key`, scoped to the
    /// parent.
    ///
    /// # Which id a widget needs
    ///
    /// Every builder already has one: `*::new()` is `#[track_caller]`, so a
    /// widget's default id is its own call site, mixed with its parent's.
    /// **That is the answer unless one of the two below applies**, and a
    /// widget written out in source — including one inside a `show` closure,
    /// however deeply nested — is always the default case.
    ///
    /// - **The call site repeats.** A `for` loop, or one helper drawing many
    ///   widgets, gives every instance the same call site. Distinguish them
    ///   with `id_salt(key)`, keyed on whatever makes the instance itself
    ///   distinct — the item's own id, not its index or its label, unless
    ///   those are stable (an index re-keys every widget when a row is
    ///   inserted; a label re-keys when someone edits the caption).
    /// - **The call site is a helper's, and you wanted the caller's.**
    ///   `fn card(ui: &mut Ui)` gives all its callers one id. Mark the helper
    ///   `#[track_caller]` and chain [`Self::auto_id`] inside it — no keys to
    ///   invent and no keys to collide.
    ///
    /// [`Self::id`] is the third, for an id computed elsewhere that must
    /// match exactly.
    ///
    /// # Scoping
    ///
    /// The stored hash is mixed with the parent node's
    /// already-disambiguated [`WidgetId`] when the node opens, so
    /// `.id_salt("row")` resolves to distinct ids under
    /// different parents — same scoping rule egui uses. At the root
    /// (no parent) the salt hash is used as-is. Marks the id as a hash salt:
    /// same-parent sibling collisions are disambiguated
    /// (so state stays well-formed) but flagged with a magenta runtime
    /// outline because they're caller bugs.
    #[inline]
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.configure().id_salt(key);
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`] used
    /// verbatim — **not** mixed with the parent. Use when the id was
    /// derived elsewhere and must match exactly (parent → child via
    /// [`WidgetId::with`], a shared seed for sibling widgets across
    /// layers, cross-frame state lookups that key off a domain id).
    /// For the parent-scoped path, prefer [`Self::id_salt`] — see the
    /// "which id a widget needs" rule there.
    ///
    /// Set after [`Widget::resolve`], it replaces the resolved identity
    /// and the record uses the new one. That is how [`crate::Modal`]
    /// moves the configuration it was handed under a child of the id
    /// its backdrop took — the reads made before were the root's.
    #[inline]
    fn id(mut self, id: WidgetId) -> Self {
        self.configure().id(id);
        self
    }

    /// Re-derive this widget's auto id at the *current* call site.
    ///
    /// **Only useful inside a `#[track_caller]` helper**, where "the current
    /// call site" is the helper's caller rather than the helper. That is the
    /// whole of what it is for: a helper that records widgets gives them all
    /// one id, and this is how each caller gets its own instead —
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
    ///
    /// Chaining it onto a widget written out in source is a no-op with extra
    /// steps: `*::new()` is already `#[track_caller]`, so the widget's id is
    /// already its own call site's. See [`Self::id_salt`] for which of the
    /// three id mechanisms a given widget wants.
    #[track_caller]
    #[inline]
    fn auto_id(mut self) -> Self {
        self.configure().auto_id();
        self
    }

    #[inline]
    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.configure().size(s);
        self
    }

    /// The size only where none was set: a widget's themed default,
    /// applied after the caller's chain ran so the caller's choice wins.
    #[inline]
    fn default_size(mut self, s: impl Into<Sizes>) -> Self {
        self.configure().default_size(s);
        self
    }

    /// # Panics
    ///
    /// Panics if the bound is negative, non-finite, or above a maximum
    /// already set on this node.
    #[inline]
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().min_size(s);
        self
    }

    /// # Panics
    ///
    /// Panics if the bound is negative, NaN, or below a minimum already
    /// set on this node. Positive infinity is the unbounded maximum.
    #[inline]
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().max_size(s);
        self
    }

    #[inline]
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.configure().padding(p);
        self
    }

    #[inline]
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.configure().margin(m);
        self
    }

    /// Apply a pan/zoom transform to this node's body — both child
    /// subtrees AND shapes recorded directly on it via `Ui::add_shape`.
    /// Layout runs in untransformed space; the transform only affects
    /// paint and hit-test. Composes with any ancestor transform.
    ///
    /// **Scale anchors at the node's own origin** (its `layout_rect.min`),
    /// not at the cascade's (0, 0). The transform's translation component
    /// is then applied in post-scale, node-local space —
    /// `TranslateScale::new(pan, zoom)` means "scale my body 2× about my
    /// top-left, then shift by `pan`" regardless of where the node sits on
    /// the surface. See [`TranslateScale::anchored_at`] for the math.
    /// Translation is identity-preserving (when `scale == 1`, the anchor is
    /// a no-op).
    ///
    /// Widget chrome — [`Panel::background`](crate::Panel::background) and
    /// its siblings — is the one exception: it paints in the *parent's*
    /// space, anchored under any ancestor clip/transform. That's
    /// deliberate: a transformed container acts as a pan/zoom viewport over
    /// its body, and the background frames the viewport rather than panning
    /// with it. For a background that scales/pans *with* the body, nest one
    /// container deep — transform on the outer, chrome on its child.
    ///
    /// Inert on a leaf that records no shapes of its own.
    #[inline]
    fn transform(mut self, t: TranslateScale) -> Self {
        self.configure().transform(t);
        self
    }

    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    #[inline]
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.configure().position(p);
        self
    }

    /// Placement inside a `Grid` parent: a bare `(row, col)` for a
    /// single-track cell, or a [`GridCell`] for one that spans — see
    /// [`GridCell::at`] and [`GridCell::span`]. Default `(0, 0)`.
    ///
    /// One setter for one field, so the placement cannot arrive half
    /// written and no chain order can drop a span. Cell and span are
    /// validated against the parent's grid def at record time — an
    /// out-of-range placement panics (`Tree::check_grid_cell`). Ignored
    /// outside a Grid parent.
    #[inline]
    fn grid_cell(mut self, cell: impl Into<GridCell>) -> Self {
        self.configure().grid_cell(cell);
        self
    }

    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack, the within-line direction of WrapHStack/
    /// WrapVStack, and a Grid's columns.
    #[inline]
    fn gap(mut self, g: f32) -> Self {
        self.configure().gap(g);
        self
    }

    /// Logical-px space between *lines*: the cross-axis spacing between
    /// a WrapHStack/WrapVStack's wrap rows, and between a Grid's rows.
    /// Inert in every other layout mode. Pair with `.gap(...)` for the
    /// within-line spacing.
    #[inline]
    fn line_gap(mut self, g: f32) -> Self {
        self.configure().line_gap(g);
        self
    }

    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has [`crate::Sizing::fill`] on the main axis.
    #[inline]
    fn justify(mut self, j: Justify) -> Self {
        self.configure().justify(j);
        self
    }

    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    #[inline]
    fn align(mut self, a: Align) -> Self {
        self.configure().align(a);
        self
    }

    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    #[inline]
    fn child_align(mut self, a: Align) -> Self {
        self.configure().child_align(a);
        self
    }

    #[inline]
    fn sense(mut self, s: Sense) -> Self {
        self.configure().sense(s);
        self
    }

    /// Fold `s` into whatever this node already senses, instead of
    /// replacing it.
    ///
    /// What a widget with a non-negotiable gesture chains at `show`
    /// time. [`crate::Scroll`] with zoom on must take [`Sense::PINCH`]
    /// however the caller sensed the viewport, and
    /// [`crate::DragValue`] must take the drag it scrubs on — but
    /// [`Self::sense`] would drop the caller's choice, and the order the
    /// two were chained in would decide the answer. This makes the order
    /// stop mattering.
    #[inline]
    fn add_sense(mut self, s: Sense) -> Self {
        self.configure().add_sense(s);
        self
    }

    /// Suppress this node's interactions and cascade to all descendants.
    #[inline]
    fn disabled(mut self, d: bool) -> Self {
        self.configure().disabled(d);
        self
    }

    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    #[inline]
    fn focusable(mut self, f: bool) -> Self {
        self.configure().focusable(f);
        self
    }

    /// Make this node an **input scope** taking `takes` while it is
    /// active.
    ///
    /// Scopes nest. A key press walks the active path deepest-first and
    /// is granted to the first scope whose filter contains its
    /// [`KeyClass`](crate::KeyClass); scopes further out never see it.
    /// That is what lets a focused text field own `Ctrl+Z` while
    /// `Ctrl+S` walks past it to the application —
    /// [`KeyFilter::TEXT_FIELD`] deliberately omits `ACCEL`.
    ///
    /// The active path is rooted at the topmost *layer* declaring any
    /// scope, so an overlay declaring [`KeyFilter::ALL`] cuts the layers
    /// below it off entirely. A reader outside every scope resolves as
    /// the active layer's outermost one.
    ///
    /// Deliberately **not** focus: a scope is where input *belongs*,
    /// focus is where typing *goes*. Conflating them is what forces an
    /// app to reconstruct one from the other.
    ///
    /// [`KeyFilter::empty`] clears it — an empty filter is how "not a
    /// scope" is stored.
    #[inline]
    fn input_scope(mut self, takes: KeyFilter) -> Self {
        self.configure().input_scope(takes);
        self
    }

    /// Three-state visibility. See [`Visibility`].
    #[inline]
    fn visibility(mut self, v: Visibility) -> Self {
        self.configure().visibility(v);
        self
    }

    /// Shorthand for [`Visibility::Hidden`]: keeps the slot, hides paint + input.
    #[inline]
    fn hidden(mut self) -> Self {
        self.configure().hidden();
        self
    }

    /// Shorthand for [`Visibility::Collapsed`]: skip the node entirely (zero slot).
    #[inline]
    fn collapsed(mut self) -> Self {
        self.configure().collapsed();
        self
    }

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    #[inline]
    fn clip(mut self, mode: ClipMode) -> Self {
        self.configure().clip(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    #[inline]
    fn clip_rect(mut self) -> Self {
        self.configure().clip_rect();
        self
    }

    /// Rounded-corner stencil clip — shape comes from the widget chrome's
    /// background radius. Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    #[inline]
    fn clip_rounded(mut self) -> Self {
        self.configure().clip_rounded();
        self
    }
}

/// The *theme* half of [`Configure`]: fill a field in only where the
/// caller stayed silent.
///
/// This is the contract every themed widget states in prose — *explicit
/// wins, the theme fills in the rest*. `Configure`'s plain setters
/// always overwrite, so a widget resolving its defaults has to know
/// whether the caller already spoke, which those setters can't say.
/// These can.
///
/// **Separate from `Configure`, and for widget authors.** Theme
/// resolution is the widget's job, not its caller's: an app chaining
/// `.default_padding(..)` onto a `Button` overrides nothing, because the
/// button resolved its own default first. A widget written outside this
/// crate resolves its theme the same way, which is why the family is
/// public.
///
/// Blanket-implemented for everything `Configure`, so it reaches a bare
/// [`Widget`] *and* a builder that wraps one — `ContextMenu` resolves
/// the menu theme into the `Popup` it is built from, which an inherent
/// `Widget` method could not do without one builder reaching into the
/// other's widget.
pub trait ThemeDefaults: Configure {
    /// Identity to fall back on when the caller set none.
    ///
    /// "Set" means [`Configure::id`] / [`Configure::id_salt`] — a
    /// `#[track_caller]` auto id doesn't count, since every widget has
    /// one and counting it would make the fallback unreachable.
    #[inline]
    fn default_id(mut self, id: WidgetId) -> Self {
        self.configure().default_id(id);
        self
    }

    /// Padding to fall back on when the caller set none.
    #[inline]
    fn default_padding(mut self, p: impl Into<Spacing>) -> Self {
        self.configure().default_padding(p);
        self
    }

    /// Margin to fall back on when the caller set none.
    #[inline]
    fn default_margin(mut self, m: impl Into<Spacing>) -> Self {
        self.configure().default_margin(m);
        self
    }

    /// Alignment to fall back on, one axis at a time — an axis the
    /// caller aligned keeps what they gave it.
    #[inline]
    fn default_align(mut self, a: Align) -> Self {
        self.configure().default_align(a);
        self
    }

    /// Sibling spacing to fall back on when the caller set none.
    #[inline]
    fn default_gap(mut self, g: f32) -> Self {
        self.configure().default_gap(g);
        self
    }

    /// Lower size bound to fall back on when the caller set none.
    #[inline]
    fn default_min_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().default_min_size(s);
        self
    }

    /// Upper size bound to fall back on when the caller set none.
    #[inline]
    fn default_max_size(mut self, s: impl Into<Size>) -> Self {
        self.configure().default_max_size(s);
        self
    }

    /// Clip mode to fall back on when the caller set none.
    #[inline]
    fn default_clip(mut self, mode: ClipMode) -> Self {
        self.configure().default_clip(mode);
        self
    }
}

impl<T: Configure> ThemeDefaults for T {}
