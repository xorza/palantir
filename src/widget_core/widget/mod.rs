//! The one authoring entity: a layout record with an identity, which a widget
//! configures, reads its last-frame state through, and records. Identity resolves
//! on first contact with [`Ui`] and stays put unless an identity setter replaces it.

use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::drivers::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::track::Track;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::background::Background;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::seen_ids::ResolvedId;
use crate::ui::Ui;
use crate::widget_core::configure::{Configure, ConfigureWidget};
use crate::widget_core::response::{InnerResponse, Response};
use glam::Vec2;
use std::panic::Location;

/// What a widget records: its identity, and the node the tree reads. Every widget
/// builder owns one, chains the [`Configure`] setters on it, and hands it to
/// [`Self::record`] in its `show`.
///
/// 1. **Create and configure.** The constructor's `#[track_caller]` gives the
///    default identity.
/// 2. **Read** (optional). [`Self::resolve`] fixes the id this frame records
///    under; [`Self::response`] is the usual read.
/// 3. **Record.** [`Self::record`] opens the node, runs the body and closes it. It
///    takes `self`, so a second record is a compile error, not a duplicate-endpoint
///    panic; hence no `Copy` or `Clone`.
///
/// Identity resolves against the most recently opened node in the current layer,
/// so resolve and record in the same body.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show` or `record`"]
pub struct Widget {
    pub(crate) ident: Ident,
    pub(crate) node: Node,
}

impl Widget {
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

    /// Stack container along `axis`, for a direction picked at run time.
    #[track_caller]
    pub fn stack(axis: Axis) -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Stack(axis)))
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

    /// Grid container for custom widgets. Its tracks arrive through
    /// [`Self::grid_tracks`]; recording without them panics.
    #[track_caller]
    pub fn grid() -> Self {
        Self::new(NodeMode::PendingGrid)
    }

    /// Scroll viewport. Children measure unbounded on the axes `axes` pans, and
    /// [`Ui::scroll_content`] reads their extent back next frame. It moves nothing
    /// itself: pan with a [`transform`](ConfigureWidget::transform) of the negated
    /// offset and clip with [`clip_rect`](ConfigureWidget::clip_rect), as
    /// [`crate::Scroll`] does. [`Self::scrollbars`] records bars for it.
    #[track_caller]
    pub(crate) fn scroll(axes: ScrollAxes) -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::Scroll(axes)))
    }

    /// Bar-overlay container for a [`Self::scroll`] viewport. Its bars arrive through
    /// [`Self::scrollbar_def`]; recording without them panics.
    ///
    /// Record it after the viewport on the same layer, typically as its z-stack
    /// sibling. It places its children after measure, when the content extent exists,
    /// and reports no size. It takes exactly four leaves: vertical track, vertical
    /// thumb, horizontal track, horizontal thumb, recorded every frame; an axis with no
    /// bar arranges its two at zero size so leaf state survives.
    #[track_caller]
    pub(crate) fn scrollbars() -> Self {
        Self::new(NodeMode::PendingScrollbars)
    }

    #[track_caller]
    fn new(mode: NodeMode) -> Self {
        Self {
            ident: Ident::Auto(Location::caller()),
            node: Node::new(mode),
        }
    }

    /// The id this widget records under, resolved on the first call and kept. Call it
    /// before `record` when you need the id first: to read last frame's state
    /// ([`Ui::response_for`], [`Ui::with_state`]), key an animation slot, or derive
    /// child ids ([`WidgetId::with`]).
    ///
    /// An auto call-site id and an `id_salt` hash resolve to `parent.with(id)`, so
    /// identity tracks tree position; an explicit `.id(id)` resolves verbatim. A raw id
    /// a sibling already opened is bumped to a fresh occurrence; the result is kept
    /// because that bump isn't repeatable.
    pub fn resolve(&mut self, ui: &mut Ui) -> WidgetId {
        self.resolved(ui).id()
    }

    /// [`Self::resolve`], keeping the id table entry that [`Self::record`] hands to the open.
    fn resolved(&mut self, ui: &mut Ui) -> ResolvedId {
        match self.ident {
            Ident::Resolved(resolved) => resolved,
            recipe => {
                let resolved = ui.resolve_ident(recipe);
                self.ident = Ident::Resolved(resolved);
                resolved
            }
        }
    }

    /// This frame's interaction state, folding in the widget's own `disabled` bit so a
    /// widget disabled this frame paints as disabled before the cascade catches up.
    /// Resolves the identity if needed. Returns an owned [`ResponseState`], not a
    /// [`Response`], which holds `&Ui` while the body needs `&mut Ui`; it becomes the
    /// widget's [`Response::new`] at the end.
    pub fn response(&mut self, ui: &mut Ui) -> ResponseState {
        let id = self.resolve(ui);
        let mut state = ui.response_for(id);
        state.merge_disabled(self.node.flags.is_disabled());
        state
    }

    /// Whether `shortcut` was pressed and granted to this widget, for a widget that
    /// reads keys before opening its node. Unlike [`Ui::key_pressed`], which reads as
    /// the enclosing node, it sees keys granted by the widget's own
    /// [`input_scope`](Configure::input_scope) even when another scope (a popup)
    /// encloses it. Keeps the chord subscribed for the wake gate.
    pub fn key_pressed(&mut self, ui: &mut Ui, shortcut: Shortcut) -> bool {
        let id = self.resolve(ui);
        ui.key_pressed_as(id, shortcut)
    }

    /// Open this widget's node, run its body, and close it; the crate's one opener.
    /// `chrome` is `None` for chrome-less widgets, `Some(bg)` for a background; both it
    /// and the node pass by reference down to `Node::columns`.
    pub fn record<R>(
        mut self,
        ui: &mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let resolved = self.resolved(ui);
        ui.open_node(resolved, &self.node, chrome);
        let r = body(ui);
        ui.close_node();
        r
    }

    /// [`Self::record`] plus a lazy [`Response`] for the node just recorded, for
    /// decorative widgets. A widget that acts on input opens with [`Self::response`]
    /// and closes with [`Response::new`] instead.
    pub fn show<'a, R>(
        mut self,
        ui: &'a mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<'a, R> {
        let id = self.resolve(ui);
        let inner = self.record(ui, chrome, body);
        InnerResponse {
            response: Response::lazy(id, ui),
            inner,
        }
    }

    /// The size the caller authored, or `None` where they stayed silent. A widget whose
    /// default depends on whether the caller spoke must ask. Named `authored_*` because
    /// an inherent `size(&self)` would shadow [`Configure::size`].
    #[inline]
    pub const fn authored_size(&self) -> Option<SizeSpec> {
        self.node.size
    }

    /// The lower size bound the caller authored, or `None`.
    #[inline]
    pub const fn authored_min_size(&self) -> Option<Size> {
        self.node.min_size
    }

    /// The upper size bound the caller authored, or `None`.
    #[inline]
    pub const fn authored_max_size(&self) -> Option<Size> {
        self.node.max_size
    }

    /// The padding the caller authored, or `None`.
    #[inline]
    pub const fn authored_padding(&self) -> Option<Spacing> {
        self.node.padding
    }

    /// The margin the caller authored, or `None`.
    #[inline]
    pub const fn authored_margin(&self) -> Option<Spacing> {
        self.node.margin
    }

    /// The paint transform the caller authored; identity if none.
    #[inline]
    pub const fn authored_transform(&self) -> TranslateScale {
        self.node.transform
    }

    /// The `Canvas`-parent position the caller authored; `Vec2::ZERO` if none.
    #[inline]
    pub const fn authored_position(&self) -> Vec2 {
        self.node.position
    }

    /// The grid slot the caller named; a default [`GridCell`] means none.
    #[inline]
    pub const fn authored_grid_cell(&self) -> GridCell {
        self.node.grid
    }

    /// The sibling gap the caller authored, or `None`.
    #[inline]
    pub fn authored_gap(&self) -> Option<f32> {
        self.node.gaps.gap()
    }

    /// The line gap the caller authored, or `None`.
    #[inline]
    pub fn authored_line_gap(&self) -> Option<f32> {
        self.node.gaps.line_gap()
    }

    /// The main-axis distribution the caller authored; `Justify::Start` if silent.
    #[inline]
    pub const fn authored_justify(&self) -> Justify {
        self.node.justify
    }

    /// How the caller aligned this widget in its parent; `Auto` per axis if silent.
    #[inline]
    pub const fn authored_align(&self) -> Align {
        self.node.align
    }

    /// The child alignment the caller authored; `Auto` per axis if silent.
    #[inline]
    pub const fn authored_child_align(&self) -> Align {
        self.node.child_align
    }

    /// What the caller made this widget sense.
    #[inline]
    pub const fn authored_sense(&self) -> Sense {
        self.node.flags.sense()
    }

    /// Whether the caller disabled this widget.
    #[inline]
    pub const fn authored_disabled(&self) -> bool {
        self.node.flags.is_disabled()
    }

    /// Whether the caller made this widget focusable.
    #[inline]
    pub const fn authored_focusable(&self) -> bool {
        self.node.flags.is_focusable()
    }

    /// Whether the caller kept this widget a Tab stop ([`Configure::tab_stop`]).
    #[inline]
    pub const fn authored_tab_stop(&self) -> bool {
        self.node.flags.is_tab_stop()
    }

    /// The axis the caller made this widget an arrow group along, or `None`.
    #[inline]
    pub const fn authored_arrow_focus(&self) -> Option<Axis> {
        self.node.flags.arrow_focus()
    }

    /// The caller's Tab order key. See [`Configure::tab_index`].
    #[inline]
    pub const fn authored_tab_index(&self) -> i16 {
        self.node.tab_index
    }

    /// The input scope the caller declared; empty if none.
    #[inline]
    pub const fn authored_input_scope(&self) -> KeyFilter {
        self.node.flags.key_filter()
    }

    /// The visibility the caller set; [`Visibility::Visible`] if silent.
    #[inline]
    pub const fn authored_visibility(&self) -> Visibility {
        self.node.visibility
    }

    /// The clip mode the caller authored, or `None`.
    #[inline]
    pub const fn authored_clip(&self) -> Option<ClipMode> {
        self.node.clip
    }

    /// Install this grid's tracks, interned into the current layer's tree. They live
    /// there because a layout mode packs into 16 bits, so this needs the `Ui` and
    /// can't be a [`Configure`] setter.
    ///
    /// # Panics
    ///
    /// Panics on a widget that is not a [`Self::grid`].
    pub fn grid_tracks(&mut self, ui: &mut Ui, rows: &[Track], cols: &[Track]) {
        let id = ui.push_grid_def(rows, cols);
        self.node.set_mode(LayoutMode::Grid(id));
    }

    /// Install this bar overlay's definition, interned into the current layer's tree;
    /// the overlay places its bars from it once the viewport `def.content` names has
    /// measured.
    ///
    /// # Panics
    ///
    /// Panics on a widget that is not a [`Self::scrollbars`], or when the viewport
    /// `def.content` names was not recorded earlier this frame.
    pub(crate) fn scrollbar_def(&mut self, ui: &mut Ui, def: ScrollbarsDef) {
        let id = ui.push_scrollbars_def(def);
        self.node.set_mode(LayoutMode::Scrollbars(id));
    }

    /// Identity's half of "explicit wins, the theme fills in the rest". Silence is
    /// [`Ident::is_explicit`], not an `Option`: every widget carries an auto id from
    /// construction.
    #[inline]
    pub(crate) const fn fill_id(&mut self, id: WidgetId) {
        if !self.ident.is_explicit() {
            self.ident = Ident::Verbatim(id);
        }
    }
}

impl Configure for Widget {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        ConfigureWidget { widget: self }
    }
}

#[cfg(test)]
mod tests;
