//! The authoring surface every widget builder forwards to: two traits of
//! layout, identity and paint setters, over a borrowed view of the
//! [`Widget`] behind them.

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

/// Emit one setter family twice from one definition: as borrowing
/// `&mut self -> &mut Self` methods on [`ConfigureWidget`], and as
/// consuming `self -> Self` forwarders on the trait declared in the
/// invocation.
///
/// Both forms are the same authoring surface reached from two places. A
/// chain on a value it owns takes the consuming form
/// (`Widget::leaf().sense(Sense::CLICK)`); a patch on a widget already
/// in place — one a `show` body has read a response through and now
/// gives its themed defaults — takes the borrowing one
/// (`widget.configure().gap(0.0)`) rather than reading the value out
/// and writing it back. Generating the pair is what keeps a setter from
/// drifting between them, and lets one doc comment answer for both.
///
/// The invocation reads as the trait it declares, so the two front arms
/// exist only to lift the visibility out: a `vis` fragment cannot be
/// followed by the `trait` keyword.
macro_rules! node_setters {
    (
        $(#[$trait_doc:meta])*
        pub trait $trait:ident: $sup:path { $($required:tt)* }
        $($setters:tt)*
    ) => {
        node_setters!(@emit pub, [$(#[$trait_doc])*] $trait $sup { $($required)* } $($setters)*);
    };
    (
        $(#[$trait_doc:meta])*
        pub(crate) trait $trait:ident: $sup:path { $($required:tt)* }
        $($setters:tt)*
    ) => {
        node_setters!(@emit pub(crate), [$(#[$trait_doc])*] $trait $sup { $($required)* } $($setters)*);
    };
    (
        @emit $vis:vis, [$(#[$trait_doc:meta])*] $trait:ident $sup:path { $($required:tt)* }
        $(
            $(#[$doc:meta])*
            fn $name:ident($view:ident $(, $arg:ident: $ty:ty)?) { $($stmt:tt)* }
        )*
    ) => {
        impl ConfigureWidget<'_> {
            $(
                $(#[$doc])*
                #[inline]
                $vis fn $name(&mut self $(, $arg: $ty)?) -> &mut Self {
                    let $view: &mut Self = self;
                    $($stmt)*
                    self
                }
            )*
        }

        $(#[$trait_doc])*
        $vis trait $trait: $sup {
            $($required)*
            $(
                $(#[$doc])*
                #[inline]
                fn $name(mut self $(, $arg: $ty)?) -> Self {
                    Configure::configure(&mut self).$name($($arg)?);
                    self
                }
            )*
        }
    };
}

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

node_setters! {
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
    }

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
    fn id_salt(view, key: impl Hash) {
        view.widget.ident = Ident::Hash(WidgetId::from_hash(key));
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
    fn id(view, id: WidgetId) {
        view.widget.ident = Ident::Verbatim(id);
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
    fn auto_id(view) {
        view.widget.ident = Ident::Auto(WidgetId::auto_stable());
    }

    fn size(view, s: impl Into<Sizes>) {
        view.widget.node.size = Some(s.into());
    }

    /// The size only where none was set: a widget's themed default,
    /// applied after the caller's chain ran so the caller's choice wins.
    fn default_size(view, s: impl Into<Sizes>) {
        view.widget.node.size.get_or_insert(s.into());
    }

    /// # Panics
    ///
    /// Panics if the bound is negative, non-finite, or above a maximum
    /// already set on this node.
    fn min_size(view, s: impl Into<Size>) {
        view.widget.node.set_min_size(s.into());
    }

    /// # Panics
    ///
    /// Panics if the bound is negative, NaN, or below a minimum already
    /// set on this node. Positive infinity is the unbounded maximum.
    fn max_size(view, s: impl Into<Size>) {
        view.widget.node.set_max_size(s.into());
    }

    fn padding(view, p: impl Into<Spacing>) {
        view.widget.node.set_padding(p.into());
    }

    fn margin(view, m: impl Into<Spacing>) {
        view.widget.node.set_margin(m.into());
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
    fn transform(view, t: TranslateScale) {
        view.widget.node.transform = t;
    }

    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(view, p: impl Into<Vec2>) {
        view.widget.node.position = p.into();
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
    fn grid_cell(view, cell: impl Into<GridCell>) {
        view.widget.node.grid = cell.into();
    }

    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack, the within-line direction of WrapHStack/
    /// WrapVStack, and a Grid's columns.
    fn gap(view, g: f32) {
        view.widget.node.gaps.set_gap(g);
    }

    /// Logical-px space between *lines*: the cross-axis spacing between
    /// a WrapHStack/WrapVStack's wrap rows, and between a Grid's rows.
    /// Inert in every other layout mode. Pair with `.gap(...)` for the
    /// within-line spacing.
    fn line_gap(view, g: f32) {
        view.widget.node.gaps.set_line_gap(g);
    }

    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has [`crate::Sizing::fill`] on the main axis.
    fn justify(view, j: Justify) {
        view.widget.node.justify = j;
    }

    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn align(view, a: Align) {
        view.widget.node.align = a;
    }

    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(view, a: Align) {
        view.widget.node.child_align = a;
    }

    fn sense(view, s: Sense) {
        view.widget.node.flags.set_sense(s);
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
    fn add_sense(view, s: Sense) {
        let sense = view.widget.node.flags.sense() | s;
        view.widget.node.flags.set_sense(sense);
    }

    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(view, d: bool) {
        view.widget.node.flags.set_disabled(d);
    }

    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(view, f: bool) {
        view.widget.node.flags.set_focusable(f);
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
    fn input_scope(view, takes: KeyFilter) {
        view.widget.node.flags.set_key_filter(takes);
    }

    /// Three-state visibility. See [`Visibility`].
    fn visibility(view, v: Visibility) {
        view.widget.node.visibility = v;
    }

    /// Shorthand for [`Visibility::Hidden`]: keeps the slot, hides paint + input.
    fn hidden(view) {
        view.visibility(Visibility::Hidden);
    }

    /// Shorthand for [`Visibility::Collapsed`]: skip the node entirely (zero slot).
    fn collapsed(view) {
        view.visibility(Visibility::Collapsed);
    }

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    fn clip(view, mode: ClipMode) {
        view.widget.node.clip = Some(mode);
    }

    /// Axis-aligned scissor clip on this node's rect.
    fn clip_rect(view) {
        view.clip(ClipMode::Rect);
    }

    /// Rounded-corner stencil clip — shape comes from the widget chrome's
    /// background radius. Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    fn clip_rounded(view) {
        view.clip(ClipMode::Rounded);
    }
}

node_setters! {
    /// The *theme* half of [`Configure`]: fill a field in only where the
    /// caller stayed silent.
    ///
    /// This is the contract every themed widget states in prose — *explicit
    /// wins, the theme fills in the rest*. `Configure`'s plain setters
    /// always overwrite, so a widget resolving its defaults has to know
    /// whether the caller already spoke, which those setters can't say.
    /// These can.
    ///
    /// **Deliberately `pub(crate)` and separate from `Configure`.** Theme
    /// resolution is the framework's job, not the caller's: an app chaining
    /// `.default_padding(…)` onto a `Button` would be overriding nothing and
    /// shadowing a decision the widget makes for it. Keeping the family off
    /// the public trait keeps it off every exported widget's method list.
    ///
    /// Blanket-implemented for everything `Configure`, so it reaches a bare
    /// [`Widget`] *and* a builder that wraps one — `ContextMenu` resolves
    /// the menu theme into the `Popup` it is built from, which an inherent
    /// `Widget` method could not do without one builder reaching into the
    /// other's widget.
    pub(crate) trait ThemeDefaults: Configure {}

    /// Identity to fall back on when the caller set none.
    ///
    /// "Set" means [`Configure::id`] / [`Configure::id_salt`] — a
    /// `#[track_caller]` auto id doesn't count, since every widget has
    /// one and counting it would make the fallback unreachable.
    fn default_id(view, id: WidgetId) {
        view.widget.fill_id(id);
    }

    /// Padding to fall back on when the caller set none.
    fn default_padding(view, p: impl Into<Spacing>) {
        view.widget.node.fill_padding(p.into());
    }

    /// Margin to fall back on when the caller set none.
    fn default_margin(view, m: impl Into<Spacing>) {
        view.widget.node.fill_margin(m.into());
    }

    /// Alignment to fall back on, one axis at a time — an axis the
    /// caller aligned keeps what they gave it.
    fn default_align(view, a: Align) {
        view.widget.node.fill_align(a);
    }

    /// Sibling spacing to fall back on when the caller set none.
    fn default_gap(view, g: f32) {
        view.widget.node.fill_gap(g);
    }

    /// Lower size bound to fall back on when the caller set none.
    fn default_min_size(view, s: impl Into<Size>) {
        view.widget.node.fill_min_size(s.into());
    }

    /// Upper size bound to fall back on when the caller set none.
    fn default_max_size(view, s: impl Into<Size>) {
        view.widget.node.fill_max_size(s.into());
    }
}

impl<T: Configure> ThemeDefaults for T {}
