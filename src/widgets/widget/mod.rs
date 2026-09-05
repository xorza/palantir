//! The one authoring entity: a layout record with an identity, which a
//! widget configures, reads its last-frame state through, and records.
//!
//! Identity resolves on the widget's first contact with [`Ui`] —
//! [`Widget::resolve`], [`Widget::response`], or [`Widget::record`],
//! whichever comes first — and stays put from then on, unless an
//! identity setter replaces it. A widget that records without reading
//! first never names its id at all.

use crate::input::response::response_state::ResponseState;
use crate::layout::axis::Axis;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::layout_mode::{LayoutMode, ScrollSpec, ScrollbarsDefId};
use crate::layout::types::sizing::Sizes;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::node::node_mode::NodeMode;
use crate::ui::Ui;
use crate::widgets::configure::{Configure, ConfigureWidget};
use crate::widgets::response::{InnerResponse, Response};

/// What a widget records: its identity, and the node the tree reads.
/// Every widget builder owns one, chains the [`Configure`] setters on it,
/// and hands it to [`Self::record`] in its `show`. A widget author builds
/// the children the same way — `Widget::leaf().id(id.with("knob"))` —
/// so one type carries the whole authoring surface.
///
/// Three steps, in order, and the middle one is optional:
///
/// 1. **Create and configure.** The constructors take the layout mode,
///    the setters take everything else, and the `#[track_caller]` on
///    the constructor is what gives the widget its default identity.
/// 2. **Read.** [`Self::resolve`] turns the identity recipe into the
///    id this frame records under, and keeps it. [`Self::response`] is
///    the read most widgets make with it. Neither is needed by a
///    widget that only records.
/// 3. **Record.** [`Self::record`] opens the node, runs the body, and
///    closes it. It takes `self`, so a second record is a use-after-move
///    the compiler rejects rather than a duplicate-endpoint panic at
///    frame time — which is why this is neither `Copy` nor `Clone`.
///
/// The parent context an identity resolves against is the
/// most-recently-opened node in the current layer, so a widget resolves
/// under the node whose body it is recorded in. Resolve and record in
/// the same body.
#[derive(Debug)]
#[must_use = "record the widget with Widget::record"]
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

    #[track_caller]
    fn new(mode: NodeMode) -> Self {
        Self {
            ident: Ident::Auto(WidgetId::auto_stable()),
            node: Node::new(mode),
        }
    }

    /// The id this widget records under, resolved on the first call and
    /// kept for every later one.
    ///
    /// The step a widget takes before `record` when it needs its id
    /// first: to read last frame's state through [`Ui::response_for`]
    /// or [`Ui::state_mut`], to key an animation slot, or to derive
    /// child ids with [`WidgetId::with`]. A widget that needs none of
    /// those never calls it — [`Self::record`] resolves on its own.
    ///
    /// An auto call-site id and an `id_salt` hash both
    /// resolve to `parent.with(id)`, so identity tracks tree position
    /// rather than record order; an explicit `.id(id)` resolves
    /// verbatim. A raw id a sibling already opened this frame is bumped
    /// to a fresh occurrence, so what this returns is what the tree, the
    /// cascade, and `response_for` will see. Kept rather than re-derived
    /// at record because that bump is not repeatable: a sibling opening
    /// the same raw id between this call and the record would move the
    /// record to a second occurrence, and the reads made here would have
    /// keyed the first.
    pub fn resolve(&mut self, ui: &mut Ui) -> WidgetId {
        match self.ident {
            Ident::Resolved(id) => id,
            recipe => {
                let id = ui.resolve_ident(recipe);
                self.ident = Ident::Resolved(id);
                id
            }
        }
    }

    /// This frame's interaction state, folding in the widget's own
    /// `disabled` bit so a widget disabled *this* frame reads and paints
    /// as disabled without waiting for the cascade to catch up.
    ///
    /// Resolves the identity if nothing did yet. The eager half of the
    /// API, for widgets that act on input: decorative ones never call
    /// this and never pay for the `response_for` lookup.
    ///
    /// Returns a plain owned [`ResponseState`], not a borrowing
    /// [`Response`]: everything a widget does after probing — theme
    /// resolution, [`Self::record`], the body closure — needs
    /// `&mut Ui`, and a `Response` holds `&Ui` for its lazy cache.
    /// Owned state is what lets the probe outlive all of it and become
    /// the widget's [`Response::eager`] at the end.
    pub fn response(&mut self, ui: &mut Ui) -> ResponseState {
        let id = self.resolve(ui);
        let mut state = ui.response_for(id);
        // The third and last source of `disabled`, and the only one that
        // needs the node — which is why the interaction half is dropped
        // by the fold rather than by the caller before it.
        state.merge_disabled(self.node.flags.is_disabled());
        state
    }

    /// Open this widget's node, run its body, and close it. Resolves the
    /// identity if nothing did yet.
    ///
    /// **The crate's one opener.** Every widget reaches the tree here,
    /// so the open/close pairing lives in a single place.
    ///
    /// `chrome` is `None` for the common layout-only / text-leaf /
    /// chrome-less path and `Some(bg)` when the widget paints a
    /// background — container widgets resolve an explicit-or-theme
    /// `Option<Background>` and pass `chrome.as_ref()`. Both it and the
    /// node travel by reference from here down `Ui::open_node` →
    /// `Forest::open_node` → `Tree::open_node` → `Node::columns`,
    /// so neither the `Background` nor the 100-byte `Node` is re-copied
    /// per hop — structurally, not by inlining.
    pub fn record<R>(
        mut self,
        ui: &mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let id = self.resolve(ui);
        ui.open_node(id, &self.node, chrome);
        let r = body(ui);
        ui.close_node();
        r
    }

    /// [`Self::record`] plus a lazy [`Response`] for the node just
    /// recorded — the whole tail of a decorative widget's `show()`.
    ///
    /// A convenience over the opener, not a second way to open: a widget
    /// that acts on input needs the state *before* it records — to pick
    /// chrome, to apply a click to a bound value — so it opens with
    /// [`Self::response`] and closes with [`Response::eager`], with the
    /// probe in hand throughout — including `ToggleChrome::record_row`,
    /// which takes it once on behalf of the three toggles. There is
    /// deliberately no helper for it: the response comes off the widget
    /// before `record` consumes it, so a packager would save one line
    /// and cost a type.
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

    /// The size the caller authored, or `None` where they stayed silent.
    ///
    /// The read half of authoring. A widget layers its themed default
    /// under the caller's choice with [`Configure::default_size`] and
    /// friends, but a widget whose default *depends* on whether the
    /// caller spoke has to ask, and this is how. `None` is the whole
    /// answer: it is what the themable fields mean by "unset".
    ///
    /// Named `authored_*` rather than after the field: an inherent
    /// `size(&self)` would shadow [`Configure::size`] and break every
    /// builder chain.
    #[inline]
    pub fn authored_size(&self) -> Option<Sizes> {
        self.node.size
    }

    /// The lower size bound the caller authored, or `None`. See
    /// [`Self::authored_size`].
    #[inline]
    pub fn authored_min_size(&self) -> Option<Size> {
        self.node.min_size
    }

    /// The upper size bound the caller authored, or `None`. See
    /// [`Self::authored_size`].
    #[inline]
    pub fn authored_max_size(&self) -> Option<Size> {
        self.node.max_size
    }

    /// The padding the caller authored, or `None`. See
    /// [`Self::authored_size`].
    #[inline]
    pub fn authored_padding(&self) -> Option<Spacing> {
        self.node.padding
    }

    /// The clip mode the caller authored, or `None`. See
    /// [`Self::authored_size`].
    #[inline]
    pub fn authored_clip(&self) -> Option<ClipMode> {
        self.node.clip
    }

    /// Take over `from`'s placement — where it sits in its parent, and
    /// nothing about what it contains, how it behaves, or who it is.
    ///
    /// For a widget that hands its slot to a second one partway through
    /// a gesture: [`crate::DragValue`] swaps its scrub chip for an inline
    /// [`crate::TextEdit`] on click, and without this the field visibly
    /// moves and resizes on the edit frame, because margin, alignment,
    /// grid placement and canvas position all go with the chip.
    ///
    /// And for a widget that records as two nodes rather than one:
    /// [`crate::Scroll`] splits the caller's widget into an outer box
    /// and an inner viewport, and the placement is the outer one's.
    ///
    /// Margin is the one `Option`: `None` there means the caller stated
    /// no opinion, so the adopting widget keeps its own themed default
    /// rather than taking a zero.
    pub fn adopt_placement(&mut self, from: &Widget) {
        self.node.adopt_placement(from.node);
    }

    /// Identity's half of "explicit wins, the theme fills in the rest".
    /// Its "caller stayed silent" test is [`Ident::is_explicit`] rather
    /// than an `Option`, because every widget carries a `#[track_caller]`
    /// auto id from the moment it is built — silence is an id the caller
    /// did not choose, not the absence of one.
    #[inline]
    pub(crate) fn fill_id(&mut self, id: WidgetId) {
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
