//! Everything a widget asks about itself for one frame: where it arranged,
//! and what the pointer and the keyboard did to it.

use crate::input::pointer::PointerButton;
use crate::input::response::button_phase::ButtonPhase;
use crate::input::response::button_state::ButtonState;
use crate::input::response::scroll_delta::ScrollDelta;
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
#[derive(Clone, Copy, Debug)]
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
    /// Pointer is over this widget's visible rect. Read from the previous
    /// frame's cascade, so it lags input by one frame.
    pub hovered: bool,
    /// Disabled — this widget *or* any ancestor. The cascaded half is
    /// one frame stale; the widget's own flag is folded in on top by the
    /// time it reads this. `true` implies the whole interaction half is
    /// at its default.
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

impl Default for ResponseState {
    fn default() -> Self {
        Self {
            rect: None,
            layout_rect: None,
            transform: TranslateScale::IDENTITY,
            pointer_local: None,
            hovered: false,
            disabled: false,
            focused: false,
            left: ButtonState::default(),
            right: ButtonState::default(),
            middle: ButtonState::default(),
            scroll: ScrollDelta::default(),
        }
    }
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
    /// A disabled widget can never be under the pointer in steady state —
    /// the cascade gives disabled entries `Sense::NONE`, so they leave
    /// the hit index — and this makes the transition frame agree with
    /// the frames on either side of it.
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
    /// to do, so it stays with `rect` and `transform`.
    #[inline]
    fn clear_interaction(&mut self) {
        self.hovered = false;
        self.left = ButtonState::default();
        self.right = ButtonState::default();
        self.middle = ButtonState::default();
        self.scroll = ScrollDelta::default();
    }

    /// One-frame edge: a press+release landed on the widget on **any**
    /// button, without latching a drag.
    ///
    /// The dismissal question every overlay backdrop asks. Reading only
    /// `left` leaves a menu opened by a secondary button un-closable by
    /// that same button, and spelling it `left || right || middle` at
    /// each site leaves the next button silently unhandled.
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

    /// Left-button press with the pointer still over the widget — the
    /// "shows pressed visuals" predicate. Derived: `left.held &&
    /// hovered` (a held press whose pointer wandered off reports
    /// `left.held` but not `pressed`). The only cross-field
    /// derivation on this type — everything per-button reads its
    /// slot: `state.left.clicked()`, `state.left.drag.delta()`,
    /// `state.left.double_clicked()`.
    #[inline]
    pub fn pressed(&self) -> bool {
        self.left.held() && self.hovered
    }
}
