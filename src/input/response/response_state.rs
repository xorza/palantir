//! Everything a widget asks about itself for one frame: where it arranged,
//! and what the pointer and the keyboard did to it.

use crate::input::pointer::PointerButton;
use crate::input::response::button_phase::ButtonPhase;
use crate::input::response::button_state::ButtonState;
use crate::input::response::scroll_delta::ScrollDelta;
use crate::primitives::num::F32Ext;
use crate::primitives::rect::Rect;
use crate::primitives::translate_scale::TranslateScale;
use glam::Vec2;

/// Snapshot of one widget's interaction state for the current frame.
/// `rect` is the widget's last-frame visible surface-space rect (`None`
/// on first frame), after ancestor transforms and clipping.
///
/// `disabled` is the **cascaded** disabled flag (the widget OR any
/// ancestor), read from the previous frame's cascade — one-frame stale,
/// like hover/press. The widget's own `Node::disabled` is folded on top
/// by `Widget::response`, through the same fold `Ui::response_for` runs,
/// so a widget disabled *this* frame reads and paints as disabled
/// without waiting for the cascade.
///
/// `focused` is `true` when this widget currently holds keyboard focus
/// (`Ui::focused_id() == Some(id)`). Updated synchronously with focus
/// changes, so unlike `hovered`/`left.held` it isn't one-frame stale —
/// a widget that just called `ui.request_focus(id)` reads `true` on
/// the same frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResponseState {
    /// Last frame's *visible* rect in surface space — after ancestor
    /// transforms and clipping, so it is what the pointer actually hit.
    /// `None` on the widget's first frame, before it has been arranged.
    /// For the untrimmed geometry use [`Self::layout_rect`].
    pub rect: Option<Rect>,
    /// Pre-transform, unclipped layout rect in world coords — the
    /// widget's arranged position before any ancestor `transform`
    /// (scroll pan/zoom) or `clip` is applied. Use when you need a
    /// widget's true position regardless of how its parent scrolls
    /// or clips it; subtract two such rects to get one widget's
    /// owner-local offset under another.
    pub layout_rect: Option<Rect>,
    /// Cumulative ancestor transform mapping this widget's `layout_rect`
    /// into unclipped surface space. The visible [`Self::rect`] may be
    /// smaller when an ancestor clips the widget.
    /// [`TranslateScale::IDENTITY`] when the widget sits under no transform.
    pub transform: TranslateScale,
    /// Cursor position in pre-transform widget-local logical coordinates,
    /// relative to [`Self::layout_rect`]'s origin. `None` when the pointer
    /// is off-surface or the widget didn't arrange. This remains relative
    /// to the full widget when ancestor clipping trims [`Self::rect`].
    pub pointer_local: Option<Vec2>,
    /// Pointer is over this widget's visible rect, and nothing above it
    /// took the pointer first. Read from the previous frame's cascade,
    /// so it lags input by one frame.
    ///
    /// **The observation, where [`Self::hovered`] is the reaction.** A
    /// disabled widget still covers what is behind it and the pointer
    /// still rests on it, which is what a tooltip explaining *why* it is
    /// disabled needs to know — so this survives the disabled fold and
    /// [`Self::hovered`] does not. For an enabled widget the two answer
    /// alike.
    pub pointer_over: bool,
    /// Disabled — this widget *or* any ancestor. The cascaded half is
    /// one frame stale; the widget's own flag is folded in on top by the
    /// time it reads this.
    ///
    /// **`true` empties the interaction half.** [`Self::left`],
    /// [`Self::right`], [`Self::middle`] and [`Self::scroll`] are all at
    /// their default, so `clicked()`, `held()`, `pressed()` and every
    /// drag read `false` on their own. Guarding a click with
    /// `!state.disabled &&` is therefore dead code, not safety.
    ///
    /// [`Self::pointer_over`], [`Self::rect`] and [`Self::pointer_local`]
    /// survive the fold — they are geometry, not something the widget was
    /// allowed to do, and a tooltip explaining *why* a control is
    /// disabled needs them.
    pub disabled: bool,
    /// This widget holds keyboard focus. Unlike the other flags this is
    /// current, not one frame stale.
    pub focused: bool,
    /// Primary-button state. The classic single-pointer surface
    /// (`clicked`, `held`, press runs, drags) lives here.
    pub left: ButtonState,
    /// Secondary-button state — `right.clicked` is the context-menu
    /// trigger.
    pub right: ButtonState,
    /// Middle / wheel-button state, with the same surface as
    /// [`Self::left`] — press runs and drags included.
    pub middle: ButtonState,
    /// Wheel / touchpad / pinch deltas routed to this widget.
    pub scroll: ScrollDelta,
}

impl ResponseState {
    /// Fold one source of "disabled" in, and drop the interaction half
    /// once any of them says so.
    ///
    /// **Three sources reach a widget's state, at three different
    /// times**: the cascade's effective flag (ancestor-or-self, one frame
    /// stale), this frame's ancestor scratch, and the widget's own
    /// `Node::disabled` — which only `Widget::response` can see. Folding
    /// them by hand let a widget disabled *this* frame report `disabled`
    /// beside `hovered` and `left.clicked()`, because the reset ran
    /// between the second source and the third.
    ///
    /// Idempotent, so it holds after *every* fold rather than only the
    /// last: the interaction half is already gone when a later source
    /// arrives, and a later `false` cannot re-enable anything.
    ///
    /// **This fold is the whole of what disabling does.** The cascade
    /// keeps a disabled widget in the hit index with the sense it
    /// declared, so the pointer, the press and the wheel all still route
    /// to it — it covers what is behind it. Emptying what it reads is
    /// what turns that routing into nothing happening.
    ///
    /// [`Self::pointer_over`] is not part of the half that goes: it is
    /// the observation, and a tooltip explaining *why* the widget is
    /// disabled needs it. [`Self::hovered`] goes by reading
    /// [`Self::disabled`] rather than by being cleared here.
    #[inline]
    pub(crate) fn merge_disabled(&mut self, disabled: bool) {
        self.disabled |= disabled;
        if self.disabled {
            self.clear_interaction();
        }
    }

    /// Everything the pointer and the wheel contributed this frame, back
    /// to its default.
    ///
    /// Named rather than spelled as a struct literal listing what to
    /// *keep*: the literal quietly dropped `pointer_local` too, which
    /// [`Ui::peek_pointer_local`](crate::Ui::peek_pointer_local) went on
    /// answering — so the same question had two answers depending on
    /// which one a caller asked. `pointer_local` is geometry ("where is
    /// the cursor relative to me"), not something the widget was allowed
    /// to do, so it stays with `rect` and `transform` —
    /// [`Self::pointer_over`] with them, for the same reason.
    #[inline]
    fn clear_interaction(&mut self) {
        self.left = ButtonState::default();
        self.right = ButtonState::default();
        self.middle = ButtonState::default();
        self.scroll = ScrollDelta::default();
    }

    /// Pointer is over this widget and this widget can react to it.
    ///
    /// [`Self::pointer_over`] minus every widget that cannot act: a
    /// disabled widget is never hovered. Derived rather than stored, so
    /// the two can never disagree — a widget disabled part-way through
    /// the frame stops reading as hovered at the same moment, whichever
    /// of the three sources of `disabled` said so.
    #[inline]
    pub fn hovered(&self) -> bool {
        self.pointer_over && !self.disabled
    }

    /// One-frame edge: a primary-button press+release landed on the
    /// widget, without latching a drag.
    ///
    /// **The activation predicate** — the one an application branches a
    /// button, a menu row, or a tab on. Reads `left`, the same button
    /// [`Self::pressed`] and [`Self::press_fraction`] report, so the
    /// three name one gesture between them.
    ///
    /// No `disabled` guard here, and none needed at a call site: a
    /// disabled widget's button slices are already empty — see
    /// [`Self::disabled`].
    #[inline]
    pub fn clicked(&self) -> bool {
        self.left.clicked()
    }

    /// One-frame edge: this primary-button click completed a double.
    ///
    /// [`Self::clicked`] fires on the same frame — a double is a click
    /// whose press was the second in its run, not a separate event. Read
    /// [`ButtonState::click_count`] for triple and beyond.
    #[inline]
    pub fn double_clicked(&self) -> bool {
        self.left.double_clicked()
    }

    /// One-frame edge: a press+release landed on the widget on **any**
    /// button, without latching a drag.
    ///
    /// The dismissal question every overlay backdrop asks. [`Self::clicked`]
    /// leaves a menu opened by a secondary button un-closable by that same
    /// button, and spelling it `left || right || middle` at each site
    /// leaves the next button silently unhandled.
    #[inline]
    pub fn any_clicked(&self) -> bool {
        PointerButton::all().any(|button| self.button(button).clicked())
    }

    /// Report the widget as focused for the rest of this frame, after it
    /// called [`Ui::request_focus`](crate::Ui::request_focus) on itself.
    ///
    /// A probed state predates the request — focus resolves live, but
    /// the snapshot was taken on entry — so without this the widget's
    /// own response would deny the focus it just took.
    #[inline]
    pub(crate) fn mark_focused(&mut self) {
        self.focused = true;
    }

    /// Report a single click, for a widget activated by something the
    /// pointer pipeline never saw — a keyboard shortcut bound to a menu
    /// row. Callers read `.clicked()` and must not have to care which
    /// device produced it.
    ///
    /// This and [`Self::mark_focused`] are the only two things a widget
    /// legitimately knows that its probed snapshot cannot. Writing to a
    /// probed state any other way is inventing input.
    #[inline]
    pub(crate) fn mark_clicked(&mut self) {
        self.left.phase = ButtonPhase::Up { click: Some(1) };
    }

    /// The per-button slice for a **runtime** `button` value — the one
    /// thing the public fields can't express. For a compile-time-known
    /// button read the field directly (`state.left`, not
    /// `state.button(PointerButton::Left)`); reach for this only when
    /// the button is a variable (configurable gesture bindings, loops
    /// over every [`PointerButton`]).
    #[inline]
    pub fn button(&self, button: PointerButton) -> &ButtonState {
        match button {
            PointerButton::Left => &self.left,
            PointerButton::Right => &self.right,
            PointerButton::Middle => &self.middle,
        }
    }

    /// [`Self::button`], mutably — the one way the router writes a
    /// button's slot.
    ///
    /// Routing through a `[ButtonState; COUNT]` indexed by
    /// `PointerButton::idx()` and landed with
    /// `[left, right, middle] = buttons` would make the enum's
    /// declaration order a silent part of the wire: reorder two variants
    /// and every button routes to the wrong field, with nothing in the
    /// type system objecting. Going through this match means the two
    /// directions read the same mapping, so the order stops being a
    /// contract anyone has to remember.
    #[inline]
    pub(crate) fn button_mut(&mut self, button: PointerButton) -> &mut ButtonState {
        match button {
            PointerButton::Left => &mut self.left,
            PointerButton::Right => &mut self.right,
            PointerButton::Middle => &mut self.middle,
        }
    }

    /// Primary-button press with the pointer still over the widget — the
    /// "shows pressed visuals" predicate. Derived: `left.held &&
    /// hovered` (a held press whose pointer wandered off reports
    /// `left.held` but not `pressed`). The cross-field derivation that
    /// [`Self::clicked`] and [`Self::press_fraction`] read the same
    /// button as — anything else per-button reads its slot directly:
    /// `state.left.drag.delta()`, `state.right.clicked()`.
    #[inline]
    pub fn pressed(&self) -> bool {
        self.left.held() && self.hovered()
    }

    /// Where the primary button's gesture sits across the widget, as a
    /// `0..1` share of each axis: on the press, on every drag frame, and
    /// on the release. `None` on any other frame, while disabled, and
    /// before the widget has arranged.
    ///
    /// One answer for every widget a pointer drives along an axis — a
    /// slider, a colour field, a bar — so the frames a gesture writes on
    /// are named once. `band` is the width of a centred thing the pointer
    /// drags, a knob, and comes off each end before the division; pass
    /// zero when the pointer itself is the position — see
    /// [`F32Ext::band_fraction`](crate::widget::F32Ext::band_fraction). Clamped,
    /// so a pointer past an edge reports that edge, which is the only way
    /// a drag reaches an axis end.
    #[inline]
    pub fn press_fraction(&self, band: f32) -> Option<Vec2> {
        let in_gesture = self.pressed() || self.left.drag.dragging() || self.left.released();
        if self.disabled || !in_gesture {
            return None;
        }
        let local = self.pointer_local?;
        let rect = self.layout_rect?;
        Some(
            local
                .band_fraction(rect.size.into(), Vec2::splat(band))
                .unit_fraction_or(Vec2::ZERO),
        )
    }
}
