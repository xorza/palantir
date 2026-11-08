//! The one authoring entity: a layout record with an identity, which a widget
//! configures, reads its last-frame state through, and records. Identity resolves
//! on first contact with [`Ui`] and stays put unless an identity setter replaces it.

use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::effect::press_input::PressInput;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::key_class::{KeyClass, KeyFilter};
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::Shortcut;
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
use crate::primitives::paint::chrome::Chrome;
use crate::scene::node::Node;
use crate::scene::node::id_recipe::{IdRecipe, ScopedRecipe};
use crate::scene::node::node_mode::NodeMode;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::scene::seen_ids::ResolvedId;
use crate::shape::Shape;
use crate::shape::paint_animation::PaintAnimation;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::look_plan::LookPlan;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::press_list::PressList;
use crate::widget_core::response::InnerResponse;
use glam::Vec2;
use std::panic::Location;

/// A widget's identity: the recipe it was given, until first contact with `Ui` resolves it, then
/// the id it resolved to, kept because resolution is not pure (`SeenIds` bumps a raw id another
/// widget opened).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Ident {
    Recipe(IdRecipe),
    Resolved(ResolvedId),
    /// Resolved by a [`Widget::wear`] that took a press effect from its slot, which
    /// [`Widget::record`] draws from the widget's [`PressList`]: a wear resolves the id first, so
    /// only a resolved widget wears one, and the flag costs the builder no byte.
    Pressed(ResolvedId),
}

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
    /// The slot [`Self::wear`] animates the look in, on the widget's id.
    pub(crate) const LOOK_SLOT: AnimationSlot = AnimationSlot::new("look");

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
        Self::wrap_stack(Axis::X)
    }

    /// Wrapping vertical stack container for custom widgets.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::wrap_stack(Axis::Y)
    }

    /// Wrapping stack container along `axis`, for a direction picked at run time.
    #[track_caller]
    pub fn wrap_stack(axis: Axis) -> Self {
        Self::new(NodeMode::Resolved(LayoutMode::WrapStack(axis)))
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
    /// itself: pan with a [`transform`](Configurator::transform) of the negated
    /// offset and clip with [`clip_rect`](Configurator::clip_rect), as
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
            ident: Ident::Recipe(IdRecipe::Scoped(ScopedRecipe::Auto(Location::caller()))),
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
            Ident::Resolved(resolved) | Ident::Pressed(resolved) => resolved,
            Ident::Recipe(recipe) => {
                let resolved = ui.resolve_ident(recipe);
                self.ident = Ident::Resolved(resolved);
                resolved
            }
        }
    }

    /// This frame's interaction state, folding in the widget's own `disabled` bit so a
    /// widget disabled this frame paints as disabled before the cascade catches up.
    /// Resolves the identity if needed, and probes the widget's [`Response`] with its own
    /// `disabled` folded in.
    pub fn response(&mut self, ui: &mut Ui) -> Response {
        let id = self.resolve(ui);
        let mut response = ui.response_for(id);
        response.state.merge_disabled(self.node.flags.is_disabled());
        response
    }

    /// [`Self::response`] with `activation`'s keys on a focused widget folded in as one left
    /// click, so `clicked()` reads the same from the pointer and the keyboard. Folds the classes
    /// of those keys alone, [`KeyFilter::SPACE`] and for a button [`KeyFilter::ENTER`], into the
    /// [`input_scope`](Configure::input_scope), so an enclosing scope cannot take them while
    /// every other key, a letter shortcut, reaches it, as a Qt or GTK button accepts only the keys
    /// it acts on.
    pub fn activation_response(&mut self, ui: &mut Ui, activation: Activation) -> Response {
        let keys = match activation {
            Activation::Button => KeyFilter::SPACE.union(KeyFilter::ENTER),
            Activation::Toggle => KeyFilter::SPACE,
        };
        let scope = self.authored().input_scope().union(keys);
        self.configure().input_scope(scope);
        let mut response = self.response(ui);
        if !response.disabled && ui.is_focus_within(response.id) {
            // By class, the classes the scope takes: a chord read would need exact modifiers,
            // and the scope would swallow a Shift+Space that acts on nothing.
            let space = ui.class_pressed_as(response.id, KeyClass::Space);
            let enter = activation == Activation::Button
                && ui.class_pressed_as(response.id, KeyClass::Enter);
            if space || enter {
                response.state.activate();
            }
        }
        response
    }

    /// Fills the padding and margin the builder left unset from `plan`, follows the presses its
    /// slot's [`PressEffect`](crate::PressEffect) draws, which [`Self::record`] draws over the
    /// chrome, then animates toward its look. The look is not stashed on the widget: a widget that
    /// wears it whole passes `Some(&look.chrome)` to [`Self::record`], and one that paints a part
    /// in it, as a checkbox paints its box, records that part with the chrome.
    // Forced inline across the theme/widget codegen-unit boundary: left outlined, the resolver cost
    // 3.9% self-time in the frame bench.
    #[inline(always)]
    pub fn wear(&mut self, ui: &mut Ui, plan: LookPlan) -> AnimatedLook {
        let LookPlan {
            target,
            key,
            defaults:
                SlotDefaults {
                    padding,
                    margin,
                    transition,
                    press,
                },
            press: edge,
        } = plan;
        self.configure()
            .default_padding(padding)
            .default_margin(margin);
        let resolved = self.resolved(ui);
        let id = resolved.id();
        if let Some(press) = press {
            let list = match ui.remove_state::<PressList>(id) {
                Some(list) => Some(list),
                None if edge.starts() => Some(PressList::new(press)),
                None => None,
            };
            if let Some(mut list) = list {
                list.note(edge, ui.now(), press);
                if !list.is_empty() {
                    ui.set_state(id, list);
                    self.ident = Ident::Pressed(resolved);
                }
            }
        }
        ui.animate(
            AnimationRequest::new(id, Self::LOOK_SLOT, target).with_key(key),
            transition,
        )
        .value
    }

    /// Whether `shortcut` was pressed and granted to this widget, for a widget that
    /// reads keys before opening its node. Unlike [`Ui::shortcut_pressed`], which reads as
    /// the enclosing node, it sees keys granted by the widget's own
    /// [`input_scope`](Configure::input_scope) even when another scope (a popup)
    /// encloses it. Keeps the chord subscribed for the wake gate.
    pub fn shortcut_pressed(&mut self, ui: &mut Ui, shortcut: impl Into<Shortcut>) -> bool {
        let id = self.resolve(ui);
        ui.shortcut_pressed_as(id, shortcut)
    }

    /// [`Self::shortcut_pressed`] for `key` whatever the modifiers, as [`Ui::key_pressed`]
    /// reads it.
    pub fn key_pressed(&mut self, ui: &mut Ui, key: Key) -> bool {
        let id = self.resolve(ui);
        ui.key_pressed_as(id, key)
    }

    /// Open this widget's node, run its body, and close it; the crate's one opener.
    /// `chrome` is `None` for chrome-less widgets, `Some(chrome)` for a painted one; both it
    /// and the node pass by reference down to `Node::cols`.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` is valid, as [`Chrome`]'s setters check it: the door every chrome
    /// passes, so a widget built outside the crate draws none quietly wrong.
    #[track_caller]
    pub fn record<R>(
        mut self,
        ui: &mut Ui,
        chrome: Option<&Chrome>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        if let Some(chrome) = chrome {
            chrome.validate();
        }
        let resolved = self.resolved(ui);
        ui.open_node(resolved, &self.node, chrome);
        if matches!(self.ident, Ident::Pressed(_)) {
            draw_presses(ui, resolved.id(), chrome);
        }
        let r = body(ui);
        ui.close_node();
        r
    }

    /// [`Self::record`] plus the [`Response`] of the node just recorded, for decorative
    /// widgets. A widget that acts on input opens with [`Self::response`] and hands that back
    /// instead.
    ///
    /// # Panics
    ///
    /// As [`Self::record`].
    #[track_caller]
    pub fn show<R>(
        mut self,
        ui: &mut Ui,
        chrome: Option<&Chrome>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        let id = self.resolve(ui);
        let inner = self.record(ui, chrome, body);
        InnerResponse {
            response: ui.response_for(id),
            inner,
        }
    }

    /// What the caller authored on this widget: each setting as the caller left it, for a widget
    /// whose default depends on whether the caller spoke. A view of its own, so its getters
    /// shadow no [`Configure`] setter of the same name.
    #[inline]
    pub const fn authored(&self) -> Authored<'_> {
        Authored { node: &self.node }
    }

    /// Install this grid's tracks, interned into the tree this widget records in. They live
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

    /// Install this bar overlay's definition, interned into the current plane's tree;
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

    /// Size this viewport's `Hug` panned axes to their content, as a `Hug`
    /// [`Scroll`](crate::Scroll) does ([`ScrollAxes::fit_content`]): an authored `Fixed`,
    /// `Fill` or max size still wins, and the content past it scrolls. Call it once the size is
    /// authored.
    ///
    /// # Panics
    ///
    /// Panics on a widget that is not a [`Self::scroll`].
    pub(crate) fn fit_scroll_content(&mut self) {
        let NodeMode::Resolved(LayoutMode::Scroll(axes)) = self.node.mode else {
            panic!("fit_scroll_content on a {:?} node", self.node.mode);
        };
        let size = self.authored().size().unwrap_or_default();
        self.node
            .set_mode(LayoutMode::Scroll(axes.fit_content(size)));
    }

    /// Identity's half of "explicit wins, the theme fills in the rest". Silence is an auto
    /// recipe ([`ScopedRecipe::Auto`]), not an `Option`: every widget carries one from
    /// construction.
    #[inline]
    pub(crate) const fn fill_id(&mut self, id: WidgetId) {
        if matches!(
            self.ident,
            Ident::Recipe(IdRecipe::Scoped(ScopedRecipe::Auto(_)))
        ) {
            self.ident = Ident::Recipe(IdRecipe::Verbatim(id));
        }
    }
}

impl Configure for Widget {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        Configurator { widget: self }
    }
}

/// What a caller authored on a [`Widget`], from [`Widget::authored`]: each setting as set, and
/// `None` or the setting's default where the caller stayed silent.
#[derive(Clone, Copy, Debug)]
pub struct Authored<'a> {
    node: &'a Node,
}

impl Authored<'_> {
    /// The size the caller authored, or `None` where they stayed silent.
    #[inline]
    pub const fn size(&self) -> Option<SizeSpec> {
        self.node.size
    }

    /// The lower size bound the caller authored, or `None`.
    #[inline]
    pub const fn min_size(&self) -> Option<Size> {
        self.node.min_size
    }

    /// The upper size bound the caller authored, or `None`.
    #[inline]
    pub const fn max_size(&self) -> Option<Size> {
        self.node.max_size
    }

    /// The padding the caller authored, or `None`.
    #[inline]
    pub const fn padding(&self) -> Option<Spacing> {
        self.node.padding
    }

    /// The margin the caller authored, or `None`.
    #[inline]
    pub const fn margin(&self) -> Option<Spacing> {
        self.node.margin
    }

    /// The paint transform the caller authored; identity if none.
    #[inline]
    pub const fn transform(&self) -> TranslateScale {
        self.node.transform
    }

    /// The `Canvas`-parent position the caller authored; `Vec2::ZERO` if none.
    #[inline]
    pub const fn pos(&self) -> Vec2 {
        self.node.pos
    }

    /// The grid slot the caller named; a default [`GridCell`] means none.
    #[inline]
    pub const fn grid_cell(&self) -> GridCell {
        self.node.grid
    }

    /// The sibling gap the caller authored, or `None`.
    #[inline]
    pub const fn gap(&self) -> Option<f32> {
        self.node.gaps.gap()
    }

    /// The line gap the caller authored, or `None`.
    #[inline]
    pub const fn line_gap(&self) -> Option<f32> {
        self.node.gaps.line_gap()
    }

    /// The main-axis distribution the caller authored, or `None`.
    #[inline]
    pub const fn justify(&self) -> Option<Justify> {
        self.node.justify
    }

    /// How the caller aligned this widget in its parent; `Auto` per axis if silent.
    #[inline]
    pub const fn align(&self) -> Align {
        self.node.align
    }

    /// The child alignment the caller authored; `Auto` per axis if silent.
    #[inline]
    pub const fn child_align(&self) -> Align {
        self.node.child_align
    }

    /// What the caller made this widget sense.
    #[inline]
    pub const fn sense(&self) -> Sense {
        self.node.flags.sense()
    }

    /// Whether the caller disabled this widget.
    #[inline]
    pub const fn disabled(&self) -> bool {
        self.node.flags.is_disabled()
    }

    /// Whether the caller made this widget focusable.
    #[inline]
    pub const fn focusable(&self) -> bool {
        self.node.flags.is_focusable()
    }

    /// Whether the caller kept this widget a Tab stop ([`Configure::tab_stop`]).
    #[inline]
    pub const fn tab_stop(&self) -> bool {
        self.node.flags.is_tab_stop()
    }

    /// The axis the caller made this widget an arrow group along, or `None`.
    #[inline]
    pub const fn arrow_focus(&self) -> Option<Axis> {
        self.node.flags.arrow_focus()
    }

    /// The caller's Tab order key. See [`Configure::tab_idx`].
    #[inline]
    pub const fn tab_idx(&self) -> i16 {
        self.node.tab_idx
    }

    /// The input scope the caller declared; empty if none.
    #[inline]
    pub const fn input_scope(&self) -> KeyFilter {
        self.node.flags.key_filter()
    }

    /// The visibility the caller set; [`Visibility::Visible`] if silent.
    #[inline]
    pub const fn visibility(&self) -> Visibility {
        self.node.visibility
    }

    /// The clip mode the caller authored, or `None`.
    #[inline]
    pub const fn clip(&self) -> Option<ClipMode> {
        self.node.clip
    }
}

/// The span [`PressInput::seed`] wraps in, seconds: the sparkle ripple reads it as a noise phase
/// in ms, which an f32 then holds to 1/128 ms.
const SEED_PERIOD_S: f64 = 64.0;

/// Draw each live press of the widget `id` opened, in the effect its [`PressList`] holds, over
/// `chrome` and under the content, inside the chrome's corners, each timed as [`PressInput`] says.
fn draw_presses(ui: &mut Ui, id: WidgetId, chrome: Option<&Chrome>) {
    let corners = chrome.map_or(Corners::ZERO, |chrome| chrome.corners);
    let presses = ui
        .remove_state::<PressList>(id)
        .expect("a widget that wore a live press holds its list");
    let press = presses.effect();
    for live in presses.iter() {
        let released = live
            .released_at
            .map_or(-1.0, |at| (at - live.pressed_at).as_secs_f32());
        let period = live
            .ends_at(&press)
            .map_or(press.enter, |end| end - live.pressed_at);
        let input = PressInput {
            color: press.color,
            highlight: press.highlight,
            point: live.point,
            released,
            enter: press.enter.as_secs_f32(),
            exit: press.exit.as_secs_f32(),
            seed: (live.pressed_at.as_secs_f64() % SEED_PERIOD_S) as f32,
            pad0: 0.0,
            pad1: 0.0,
        };
        ui.add_shape_animated(
            Shape::effect(press.effect, input).corners(corners),
            PaintAnimation::clock()
                .with_started_at(live.pressed_at)
                .with_period(period),
        );
    }
    ui.set_state(id, presses);
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::node::id_recipe::IdRecipe;
    use crate::widget_core::widget::Ident;

    impl Ident {
        /// The recipe a widget not yet resolved holds.
        pub(crate) fn recipe(self) -> IdRecipe {
            match self {
                Self::Recipe(recipe) => recipe,
                Self::Resolved(resolved) | Self::Pressed(resolved) => {
                    panic!("{:?} is resolved already", resolved.id())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
