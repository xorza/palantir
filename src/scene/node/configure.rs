use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::justify::Justify;
use crate::layout::types::limits;
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::salt::Salt;
use crate::scene::visibility::Visibility;
use glam::Vec2;
use std::hash::Hash;

/// Opaque mutable view used only to implement [`Configure`] for a widget.
/// Delegating through an owned [`Node`] exposes configuration without
/// exposing the node's structural layout mode.
#[derive(Debug)]
pub struct ConfigureNode<'a> {
    pub(super) node: &'a mut Node,
}

/// Mixin: any widget builder that holds a [`Node`] gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `node_mut`.
pub trait Configure: Sized {
    fn node_mut(&mut self) -> ConfigureNode<'_>;

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
    fn id_salt(mut self, key: impl Hash) -> Self {
        self.node_mut().node.salt = Salt::Hash(WidgetId::from_hash(key));
        self
    }

    /// Override this widget's id with a precomputed [`WidgetId`] used
    /// verbatim — **not** mixed with the parent. Use when the id was
    /// derived elsewhere and must match exactly (parent → child via
    /// [`WidgetId::with`], a shared seed for sibling widgets across
    /// layers, cross-frame state lookups that key off a domain id).
    /// For the parent-scoped path, prefer [`Self::id_salt`] — see the
    /// "which id a widget needs" rule there.
    fn id(mut self, id: WidgetId) -> Self {
        self.node_mut().node.salt = Salt::Verbatim(id);
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
    fn auto_id(mut self) -> Self {
        self.node_mut().node.salt = Salt::Auto(WidgetId::auto_stable());
        self
    }

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.node_mut().node.size = Some(s.into());
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        let node = self.node_mut().node;
        let value = s.into();
        limits::debug_assert_valid_bounds(value, node.max_size.unwrap_or(Size::INF));
        node.min_size = Some(value);
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        let node = self.node_mut().node;
        let value = s.into();
        limits::debug_assert_valid_bounds(node.min_size.unwrap_or(Size::ZERO), value);
        node.max_size = Some(value);
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.node_mut().node.padding = Some(checked_spacing(p, "padding"));
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.node_mut().node.margin = Some(checked_spacing(m, "margin"));
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
    fn transform(mut self, t: TranslateScale) -> Self {
        self.node_mut().node.transform = t;
        self
    }

    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.node_mut().node.position = p.into();
        self
    }
    /// Cell `(row, col)` inside a `Grid` parent. Default `(0, 0)`. Ignored
    /// outside a Grid parent.
    fn grid_cell(mut self, (row, col): (u16, u16)) -> Self {
        let node = self.node_mut().node;
        node.grid.row = row;
        node.grid.col = col;
        self
    }
    /// Span `(row_span, col_span)` inside a `Grid` parent. Default `(1, 1)`.
    /// Cell + span are validated against the parent's grid def at record
    /// time — an out-of-range placement panics (`Tree::check_grid_cell`).
    /// Ignored outside a Grid parent.
    fn grid_span(mut self, (rs, cs): (u16, u16)) -> Self {
        let node = self.node_mut().node;
        node.grid.row_span = rs.max(1);
        node.grid.col_span = cs.max(1);
        self
    }
    /// Logical-px space between siblings within a line. Read by
    /// HStack/VStack and the within-line direction of WrapHStack/
    /// WrapVStack. Grid has its own `gap_xy` and ignores this field.
    fn gap(mut self, g: f32) -> Self {
        self.node_mut().node.gaps.set_gap(g);
        self
    }

    /// Logical-px space between *lines* for WrapHStack/WrapVStack —
    /// the cross-axis spacing between wrap rows/columns. Inert in
    /// every other layout mode. Pair with `.gap(...)` for the within-
    /// line spacing.
    fn line_gap(mut self, g: f32) -> Self {
        self.node_mut().node.gaps.set_line_gap(g);
        self
    }
    /// Main-axis distribution of leftover space for `HStack`/`VStack`.
    /// Ignored when any child has [`crate::Sizing::fill`] on the main axis.
    fn justify(mut self, j: Justify) -> Self {
        self.node_mut().node.justify = j;
        self
    }
    /// Alignment inside the parent's inner rect. For single-axis use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn align(mut self, a: Align) -> Self {
        self.node_mut().node.align = a;
        self
    }
    /// Default alignment applied to children when their own axis is `Auto`.
    /// Mirrors CSS `align-items`. For single-axis defaults use the
    /// [`Align::h`] / [`Align::v`] constructors.
    fn child_align(mut self, a: Align) -> Self {
        self.node_mut().node.child_align = a;
        self
    }
    fn sense(mut self, s: Sense) -> Self {
        self.node_mut().node.flags.set_sense(s);
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.node_mut().node.flags.set_disabled(d);
        self
    }
    /// Mark this node as eligible to take keyboard focus on press.
    /// Default `false`. Only editable widgets (TextEdit) opt in. Disabled
    /// or invisible nodes are excluded from focus regardless of this
    /// flag — same cascade rule as `Sense`.
    fn focusable(mut self, f: bool) -> Self {
        self.node_mut().node.flags.set_focusable(f);
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
    fn input_scope(mut self, takes: KeyFilter) -> Self {
        self.node_mut().node.flags.set_key_filter(takes);
        self
    }
    /// Three-state visibility. See [`Visibility`].
    fn visibility(mut self, v: Visibility) -> Self {
        self.node_mut().node.visibility = v;
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

    /// Generic clip setter. Most callers use the [`Self::clip_rect`]
    /// / [`Self::clip_rounded`] sugars instead.
    fn clip(mut self, mode: ClipMode) -> Self {
        self.node_mut().node.clip = Some(mode);
        self
    }

    /// Axis-aligned scissor clip on this node's rect.
    fn clip_rect(self) -> Self {
        self.clip(ClipMode::Rect)
    }

    /// Rounded-corner stencil clip — shape comes from the widget chrome's
    /// background radius. Calling this without
    /// a chrome leaves the radius at zero, equivalent to
    /// [`Self::clip_rect`].
    fn clip_rounded(self) -> Self {
        self.clip(ClipMode::Rounded)
    }
}

/// Screen one spacing on the way in, and say which knob it came from.
///
/// A NaN edge does not fail here — it poisons every extent derived from
/// it and surfaces frames later as a widget that measured to nothing,
/// with no way back to the call that set it. `Corners` is screened at
/// shape lowering for the same reason; this is the equivalent gate for
/// the two spacings, which reach layout instead of the record.
///
/// `debug_assert!` because it is per widget per frame, and because a NaN
/// here is a caller's arithmetic rather than untrusted data — the theme's
/// own spacing is checked where the theme is built.
fn checked_spacing(value: impl Into<Spacing>, knob: &str) -> Spacing {
    let value = value.into();
    debug_assert!(!value.has_nan(), "NaN in {knob}: {value:?}");
    value
}
