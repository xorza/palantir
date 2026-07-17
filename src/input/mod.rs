pub(crate) mod keyboard;
pub(crate) mod pointer;
pub(crate) mod policy;
pub(crate) mod response;
pub(crate) mod sense;
pub(crate) mod shortcut;
pub(crate) mod subscriptions;

use crate::input::keyboard::{
    Key, KeyPress, KeyboardEvent, Modifiers, TextChunk, key_from_winit, modifiers_from_winit,
    physical_key_from_winit,
};
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::FocusPolicy;
use crate::input::response::{
    ButtonPhase, ButtonState, Drag, InputDelta, ResponseState, ScrollDelta,
};
use crate::input::sense::{DOUBLE_CLICK_RADIUS, DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD, Sense};
use crate::input::subscriptions::{KeyboardSense, PointerSense, Subscriptions};
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use glam::Vec2;
use std::time::Duration;
use strum::EnumCount as _;

fn pointer_in_widget_space(pointer: Vec2, layout_origin: Vec2, transform: TranslateScale) -> Vec2 {
    let surface_origin = transform.apply_point(layout_origin);
    transform.inverse_vector(pointer - surface_origin)
}

/// Per-button capture. One slot per [`PointerButton`]; three
/// all-or-nothing pieces instead of twelve loose fields, so the old
/// by-convention invariants (a capture always has a press origin, a
/// drag latch always has a capture, click and drag-stop never
/// coexist, the run tracker never half-exists) are unrepresentable
/// rather than maintained.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Capture {
    /// The in-flight press, created on the press event and destroyed
    /// by release / cascade-eviction. `Some` == "this button's
    /// capture is latched".
    pub(crate) press: Option<Press>,
    /// One-frame edge: how a capture ended this frame. Cleared by
    /// `end_frame`.
    pub(crate) release: Option<Release>,
    /// Multi-press run tracker. Persists *across* presses (that's the
    /// chaining) — never cleared, only replaced by the next press.
    pub(crate) run: Option<PressRun>,
}

impl Capture {
    /// Latch a press on `target` at `pos`, chaining the multi-press
    /// run when it lands on the same target within
    /// [`DOUBLE_CLICK_WINDOW`] of the previous press and
    /// [`DOUBLE_CLICK_RADIUS`] of its position; any break restarts the
    /// run at 1. `seq` saturates so a caffeinated 255-click run can't
    /// wrap back to "single".
    fn begin_press(&mut self, target: WidgetId, pos: Vec2, now: Duration) {
        let seq = match &self.run {
            Some(run)
                if run.target == target
                    && now.saturating_sub(run.at) <= DOUBLE_CLICK_WINDOW
                    && pos.distance(run.pos) <= DOUBLE_CLICK_RADIUS =>
            {
                run.seq.saturating_add(1)
            }
            _ => 1,
        };
        self.run = Some(PressRun {
            at: now,
            target,
            pos,
            seq,
        });
        self.press = Some(Press {
            target,
            origin: pos,
            seq,
            fresh: true,
            drag: PressDrag::None,
        });
    }
}

/// One in-flight press: the capture target, the drag anchor, and this
/// press's run position, bundled so none can exist without the others.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Press {
    /// Widget the press latched onto.
    pub(crate) target: WidgetId,
    /// Pointer position at the press. Subtracted from the current
    /// pointer position for rect-independent drag deltas.
    pub(crate) origin: Vec2,
    /// This press's position in its multi-press run (1 = single,
    /// 2 = double-press, 3+ = triple…), stamped from [`PressRun::seq`]
    /// at press time so the release can carry the click count without
    /// depending on the run tracker's later state.
    pub(crate) seq: u8,
    /// One-frame edge: the press landed this frame (drives
    /// `ButtonPhase::Down`). Lowered by `drain_per_frame_queues`.
    pub(crate) fresh: bool,
    /// Drag latch. Sticky non-`None` for the press lifetime; doubles
    /// as "suppress click on release".
    pub(crate) drag: PressDrag,
}

/// Drag latch of an in-flight [`Press`]: `None` until the pointer has
/// travelled [`DRAG_THRESHOLD`] from `origin`, `Started` on exactly
/// the threshold-crossing frame (the drag-start edge),
/// `Active` after — `drain_per_frame_queues` lowers the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PressDrag {
    #[default]
    None,
    Started,
    Active,
}

/// One-frame edge: how this button's capture ended this frame. One
/// value instead of three parallel edge fields — a click and a
/// drag-stop are mutually exclusive by construction, and either can
/// only target the widget that was released.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Release {
    /// The widget whose capture ended.
    pub(crate) target: WidgetId,
    pub(crate) kind: ReleaseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReleaseKind {
    /// The release landed back on the captured widget with no drag
    /// latched — a click. `count` is the press run's number
    /// (2 = double-click, 3 = triple…), stamped from [`Press::seq`].
    Click { count: u8 },
    /// A latched drag ended — the commit edge for drag gestures.
    DragStopped,
    /// Released off the widget with no drag latched — the capture
    /// just dissolves (drives the click-less `ButtonPhase::Up`).
    Miss,
}

/// Multi-press run state: where/when/on-what the last press landed and
/// its position in the run. The next press chains (`seq + 1`) when it
/// lands on the same `target` within [`DOUBLE_CLICK_WINDOW`] of `at`
/// and [`DOUBLE_CLICK_RADIUS`] of `pos`; any break restarts at 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PressRun {
    pub(crate) at: Duration,
    pub(crate) target: WidgetId,
    pub(crate) pos: Vec2,
    pub(crate) seq: u8,
}

/// Aperture-native input event. Independent of any windowing toolkit.
/// Convert from winit via [`InputEvent::from_winit`], then dispatch
/// through [`crate::Ui::on_input`].
///
/// All coordinates are in **logical pixels** (DIPs). Backends are responsible
/// for any physical→logical conversion before dispatching.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Pointer position in logical pixels, relative to the surface origin.
    PointerMoved(Vec2),
    /// Pointer left the surface; clears `hovered`.
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    /// Pixel-precise scroll delta — touchpad / precision wheel /
    /// `MouseScrollDelta::PixelDelta`. Logical pixels. Positive `y`
    /// means the user wants content to scroll *down* (a scroll widget
    /// should add to its vertical offset). Multiple events in one frame
    /// accumulate in the frame's pixel-scroll total.
    ScrollPixels(Vec2),
    /// Notched scroll delta — classic wheel /
    /// `MouseScrollDelta::LineDelta`. Carries the raw line count
    /// (sign-flipped to match `ScrollPixels`); the consuming widget
    /// multiplies by its own font-derived line step at record time
    /// rather than this layer baking in a constant. Multiple events
    /// in one frame accumulate in the frame's line-scroll total.
    ScrollLines(Vec2),
    /// Multiplicative zoom factor from a touch / touchpad pinch gesture.
    /// `1.0` is identity; `1.05` zooms in 5%, `0.95` zooms out 5%.
    /// Multiple events in one frame multiply into
    /// the frame's zoom total. Wheel-based zoom is *not*
    /// translated into `Zoom` — the active scroll widget decides at
    /// record time whether wheel ticks count as pan or zoom. Non-positive
    /// and non-finite factors are discarded at ingress.
    Zoom(f32),
    /// Logical key was pressed. `repeat` reflects OS-level key repeat
    /// (held keys re-emit). Modifier state isn't carried on the event;
    /// consumers read the latest [`Modifiers`] from `InputState`. We
    /// don't carry releases — no consumer needs them yet.
    KeyDown {
        key: Key,
        repeat: bool,
        /// Layout-independent physical key — see
        /// [`KeyPress::physical`](crate::input::keyboard::KeyPress::physical).
        physical: Key,
    },
    /// Committed text — a typed character or an IME composition that
    /// just finalized. Distinct from `KeyDown` because IME / dead-key
    /// composition produces text without a physical keypress, and
    /// because keys like `Enter` produce a logical key but no text we
    /// want to insert. Editors should consume `Text` for character
    /// input and `KeyDown` for navigation/control keys.
    Text(TextChunk),
    /// Modifier-key set changed. The carried snapshot is the new state
    /// (not a delta). Consumers track the latest snapshot to disambiguate
    /// e.g. ctrl+'a' (shortcut) from 'a' (text).
    ModifiersChanged(Modifiers),
}

const MIN_POSITIVE_ZOOM_FACTOR: f32 = f32::MIN_POSITIVE;

#[inline]
pub(crate) fn zoom_factor_is_valid(factor: f32) -> bool {
    factor.is_finite() && factor > 0.0
}

/// Multiply zoom factors without allowing valid sequences to underflow
/// to zero or overflow to infinity.
#[inline]
pub(crate) fn combine_zoom_factors(lhs: f32, rhs: f32) -> f32 {
    debug_assert!(zoom_factor_is_valid(lhs));
    debug_assert!(!rhs.is_nan() && rhs >= 0.0);
    let product = f64::from(lhs) * f64::from(rhs);
    if product <= f64::from(MIN_POSITIVE_ZOOM_FACTOR) {
        MIN_POSITIVE_ZOOM_FACTOR
    } else if product >= f64::from(f32::MAX) {
        f32::MAX
    } else {
        product as f32
    }
}

#[inline]
pub(crate) fn wheel_zoom_factor(step: f32, notches: f32) -> f32 {
    debug_assert!(zoom_factor_is_valid(step));
    debug_assert!(!notches.is_nan());
    combine_zoom_factors(1.0, step.powf(-notches))
}

impl InputEvent {
    /// Translate a winit `WindowEvent` into one or more aperture input
    /// events, invoking `emit` for each. Most events fan out 1:1; IME
    /// commits over [`TextChunk`]'s inline capacity split into multiple
    /// `Text` events at char boundaries so long CJK compositions don't
    /// silently drop. `scale_factor` divides physical pointer coordinates
    /// so that emitted `PointerMoved` is in logical pixels.
    pub fn from_winit(
        event: &winit::event::WindowEvent,
        scale_factor: f32,
        mut emit: impl FnMut(InputEvent),
    ) {
        use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
        let s = scale_factor.max(f32::EPSILON);
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                emit(InputEvent::PointerMoved(Vec2::new(
                    position.x as f32 / s,
                    position.y as f32 / s,
                )));
            }
            WindowEvent::CursorLeft { .. } => emit(InputEvent::PointerLeft),
            WindowEvent::MouseInput { state, button, .. } => {
                let pb = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return,
                };
                emit(match state {
                    ElementState::Pressed => InputEvent::PointerPressed(pb),
                    ElementState::Released => InputEvent::PointerReleased(pb),
                });
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let factor = 1.0 + *delta as f32;
                if zoom_factor_is_valid(factor) {
                    emit(InputEvent::Zoom(factor));
                }
            }
            // Convert to "positive delta = pan offset forward" so widgets can
            // do `offset += delta` directly. winit reports +y when the wheel
            // rotates *away* from the user (scroll up) and +x when it rotates
            // / swipes right (reveal content to the right means panning
            // *into* it, i.e. content shifts left); flip both so positive
            // means "advance the scroll offset."
            WindowEvent::MouseWheel { delta, .. } => emit(match *delta {
                MouseScrollDelta::LineDelta(x, y) => InputEvent::ScrollLines(Vec2::new(-x, -y)),
                MouseScrollDelta::PixelDelta(p) => {
                    InputEvent::ScrollPixels(Vec2::new(-p.x as f32 / s, -p.y as f32 / s))
                }
            }),
            // Releases are dropped — no consumer needs them yet.
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                emit(InputEvent::KeyDown {
                    key: key_from_winit(&event.logical_key),
                    repeat: event.repeat,
                    physical: physical_key_from_winit(&event.physical_key),
                });
            }
            // IME commit: what the user *meant* to insert after composition
            // (dead keys, multi-keystroke CJK input). Long commits (CJK
            // phrase input, emoji ZWJ sequences) routinely exceed the
            // 15-byte inline `TextChunk`; split at char boundaries so each
            // chunk fits without losing the typed text. Char (not grapheme)
            // boundaries are sufficient for byte safety; consumers
            // re-assemble at append time.
            WindowEvent::Ime(Ime::Commit(s)) => {
                emit_text_chunks(s, &mut emit);
            }
            WindowEvent::ModifiersChanged(m) => emit(InputEvent::ModifiersChanged(
                modifiers_from_winit(&m.state()),
            )),
            _ => {}
        }
    }
}

/// Split `s` into `Text` events at char boundaries such that each
/// chunk fits the [`TextChunk`] inline buffer. Char boundaries (not
/// grapheme cluster boundaries) are safe for UTF-8; splitting inside a
/// grapheme cluster is visually ugly but doesn't corrupt the buffer —
/// downstream text consumers re-assemble at append time.
fn emit_text_chunks(s: &str, emit: &mut impl FnMut(InputEvent)) {
    let mut rest = s;
    while !rest.is_empty() {
        // Greedy max prefix ≤ INLINE_CAP, backed off to a char
        // boundary — at most 3 steps, and never to 0 since
        // INLINE_CAP ≥ 4 (the longest UTF-8 char).
        let mut end = rest.len().min(TextChunk::INLINE_CAP);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = rest.split_at(end);
        emit(InputEvent::Text(
            TextChunk::new(head).expect("chunk fits by construction"),
        ));
        rest = tail;
    }
}

/// Live input state machine: the things that survive across input events
/// independently of whether the tree was rebuilt. Per-frame rebuilt data
/// (last-frame rects, cascade scratch) lives in [`crate::ui::cascade::Cascade`].
pub(crate) struct InputState {
    /// Pointer position in logical pixels, `None` when off-surface.
    pub(crate) pointer_pos: Option<Vec2>,
    pub(crate) hovered: Option<WidgetId>,
    /// Topmost `Sense::SCROLL` widget under the pointer, recomputed
    /// whenever the pointer moves and at `end_frame`. The scroll widget
    /// matching this id consumes [`Self::frame_scroll_pixels`].
    pub(crate) scroll_target: Option<WidgetId>,
    /// Topmost `Sense::PINCH` widget under the pointer, recomputed
    /// alongside `scroll_target`. Pinch zoom factors route to this id
    /// instead of `scroll_target` so a widget can opt into pan-via-
    /// scroll *without* committing to pinch zoom (and vice versa).
    pub(crate) pinch_target: Option<WidgetId>,
    /// Per-button press capture (active widget, press pos, drag latch,
    /// frame edges for `drag_started` and `clicked`). Indexed by
    /// [`PointerButton`] via [`PointerButton::idx`]. Independent per
    /// button — a left-drag in progress doesn't block a right-click.
    pub(crate) captures: [Capture; PointerButton::COUNT],
    /// Pixel-precise wheel / touchpad delta accumulated this frame
    /// (logical px from `ScrollPixels`). Cleared in
    /// [`Self::end_frame`]. Read by scroll widgets at record time
    /// alongside [`Self::frame_scroll_lines`] — the widget combines
    /// the two via its font-derived line step.
    pub(crate) frame_scroll_pixels: Vec2,
    /// Notched wheel delta accumulated this frame (line count from
    /// `ScrollLines`). Cleared in [`Self::end_frame`]. The scroll
    /// widget multiplies by its own line-px step at consumption time
    /// instead of baking a constant in here, so wheel feel tracks the
    /// active font size. Also read directly by zoom routing (each line
    /// = one notch, no roundtrip through a pixel constant).
    pub(crate) frame_scroll_lines: Vec2,
    /// Multiplicative pinch-zoom delta accumulated this frame; `1.0` =
    /// no zoom. Cleared in [`Self::end_frame`]. Read by scroll widgets
    /// configured with a `ZoomConfig`. Wheel-based zoom is computed
    /// at the widget from [`Self::frame_scroll_pixels`] under the
    /// `ZoomConfig::modifier` gate, not accumulated here.
    pub(crate) frame_zoom_delta: f32,
    /// Frame-snapshot of "no widget can hold any non-default interaction
    /// state this frame" — no pointer on the surface, no routed
    /// scroll/pinch target, no live button capture or click/double-click
    /// edge. Filled once per record pass via
    /// [`Self::snapshot_frame_quiescent`];
    /// read in [`Self::response_for`] to default the whole interaction
    /// half out for every widget instead of re-deriving it per call.
    /// `focused` is excluded on purpose (see `snapshot_frame_quiescent`),
    /// so the fast path still reads it live.
    pub(crate) frame_quiescent: bool,
    /// Unified keyboard event stream this frame:
    /// [`KeyboardEvent::Down`] from `KeyDown` events and
    /// [`KeyboardEvent::Text`] from `Text` events, in arrival order.
    /// Capacity-retained; cleared in [`Self::drain_per_frame_queues`].
    /// Read by the focused widget (drains all events) and by global
    /// keyboard subscribers ([`KeyboardSense`]) — both reading the
    /// same buffer.
    pub(crate) frame_keyboard_events: Vec<KeyboardEvent>,
    /// Latest modifier-key snapshot. Persists across `end_frame` —
    /// modifier *state* is not a per-frame thing the way keystrokes
    /// are. Updated only on `ModifiersChanged` events.
    pub(crate) modifiers: Modifiers,
    /// Currently focused widget, or `None`. Set on `PointerPressed(Left)`
    /// when the press lands on a focusable widget. Evicted in
    /// [`Self::end_frame`] when the focused widget vanishes from the
    /// tree (matches the per-id state map's eviction model). Read by
    /// keyboard consumers to decide whether to drain
    /// `frame_keyboard_events`.
    pub(crate) focused: Option<WidgetId>,
    /// Press-on-non-focusable-widget behavior. See [`FocusPolicy`].
    pub(crate) focus_policy: FocusPolicy,
    /// Set in `on_input` when a routed event could drive a state mutation
    /// (pointer press/release, `KeyDown`, `Text`). Read by `Ui::frame`
    /// to decide whether to re-record the frame after pass 1's `end_frame`
    /// drains the input queues. Hover-only events (`PointerMoved`,
    /// `PointerLeft`) and modifier changes don't flip it. Unrouted actions
    /// leave it clear because no widget or subscriber can observe them.
    pub(crate) frame_had_action: bool,
    /// Sticky bit: set by every `on_input` call (any event, including
    /// pointer moves and mod changes), cleared by `Ui::frame`
    /// at the top of each frame after the paint-anim-only
    /// short-circuit gate has read it. Distinct from `frame_had_action`
    /// — that flag answers "did this *frame's* recording see a
    /// state-mutating event"; this one answers "did the host push
    /// *anything* between the previous `frame()` return and this
    /// one." The short-circuit fails on any input arrival, since the
    /// closure might react to even a pointer move.
    pub(crate) had_input_since_last_frame: bool,
    /// Sticky bit: set in [`Self::on_input`] whenever the returned
    /// [`InputDelta::requests_repaint`] is `true` — i.e. an event that
    /// could plausibly mutate visible state arrived (hover/scroll
    /// target change, capture-active move, click, key, IME, modifier
    /// change). Cleared alongside `had_input_since_last_frame`. Read
    /// by `Ui::classify_frame` under [`InputPolicy::OnDelta`](policy::InputPolicy::OnDelta).
    pub(crate) repaint_requested_since_last_frame: bool,
    /// Wake-gate subscriptions ([`PointerSense`] / [`KeyboardSense`]
    /// flag masks + specific-chord list). Cleared pre-record (in
    /// `Ui::record_pass`); widgets re-assert each active frame. The
    /// masks **persist across silent frames** — that's the wake
    /// signal a dormant popup needs to be paged in by the next click.
    /// `on_input` short-circuits on the masks before touching event
    /// buffers, so idle frames pay nothing.
    pub(crate) subs: Subscriptions,
    /// Unified pointer event stream this frame: moves, presses,
    /// releases, scrolls, zooms, leave. Pushes are gated per-category
    /// on [`Subscriptions::pointer_mask`] (`MOVE` for `Move`,
    /// `BUTTONS` for `Down`/`Up`, `SCROLL` for `Scroll`/`Zoom`, any
    /// pointer flag for `Leave`) — idle frames pay nothing. Cleared
    /// in [`Self::drain_per_frame_queues`].
    pub(crate) frame_pointer_events: Vec<PointerEvent>,
    /// Frame-runtime clock as of the last `Ui::frame`, refreshed
    /// once per frame so input handlers running *between* frames stamp
    /// events on the same deterministic clock the rest of the crate uses
    /// (vs wall-clock `Instant`). Drives double-click timing.
    pub(crate) frame_time: Duration,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            pointer_pos: None,
            hovered: None,
            scroll_target: None,
            pinch_target: None,
            captures: [Capture::default(); PointerButton::COUNT],
            frame_scroll_pixels: Vec2::ZERO,
            frame_scroll_lines: Vec2::ZERO,
            frame_zoom_delta: 1.0,
            // Recomputed each record pass before any `response_for`
            // call; `false` is the safe pre-frame default (forces the
            // full path).
            frame_quiescent: false,
            frame_keyboard_events: Vec::new(),
            modifiers: Modifiers::NONE,
            focused: None,
            focus_policy: FocusPolicy::default(),
            frame_had_action: false,
            had_input_since_last_frame: false,
            repaint_requested_since_last_frame: false,
            subs: Subscriptions::default(),
            frame_pointer_events: Vec::new(),
            frame_time: Duration::ZERO,
        }
    }
}

impl InputState {
    #[inline]
    fn capture(&self, b: PointerButton) -> &Capture {
        &self.captures[b.idx()]
    }

    #[inline]
    fn capture_mut(&mut self, b: PointerButton) -> &mut Capture {
        &mut self.captures[b.idx()]
    }

    /// Push a pointer event to [`Self::frame_pointer_events`] and
    /// answer "should this event wake the next frame?" Wake fires
    /// when any subscriber holds `sense` — single bitwise AND on the
    /// cached `pointer_mask`. Returns `true` even when `pos` is `None`
    /// so an off-surface press still wakes; the `PointerEvent` itself
    /// is only pushed if there's a position (no consumer can do
    /// anything useful without one).
    fn push_pointer_event(
        &mut self,
        sense: PointerSense,
        pos: Option<Vec2>,
        make: impl FnOnce(Vec2) -> PointerEvent,
    ) -> bool {
        if !self.subs.pointer_mask.contains(sense) {
            return false;
        }
        if let Some(pos) = pos {
            self.frame_pointer_events.push(make(pos));
        }
        true
    }

    /// SCROLL-class push ([`PointerEvent::Scroll`] / [`PointerEvent::Zoom`])
    /// whose wake additionally requires a pointer position — scroll
    /// with no pointer routes nowhere, so waking would be pointless.
    fn push_scroll_class(&mut self, make: impl FnOnce(Vec2) -> PointerEvent) -> bool {
        self.pointer_pos.is_some()
            && self.push_pointer_event(PointerSense::SCROLL, self.pointer_pos, make)
    }

    /// Feed an aperture-native input event. Hit-tests against the
    /// frozen `Cascades` from this frame's most recent run. Returns an
    /// [`InputDelta`] hosts use to decide whether to request a redraw —
    /// a `PointerMoved` over a non-hover-reactive surface (no active
    /// capture, no hover/scroll target change) leaves
    /// `requests_repaint` false so the frame can be skipped entirely.
    pub(crate) fn on_input(&mut self, event: InputEvent, cascades: &Cascades) -> InputDelta {
        if let InputEvent::Zoom(factor) = event
            && !zoom_factor_is_valid(factor)
        {
            return InputDelta::default();
        }
        // Any host-pushed event disqualifies the next frame from the
        // paint-anim-only short-circuit — the recording closure might
        // observe even a pointer move (hover styling) or modifier
        // change (shortcut hint). Cleared at the top of `frame`
        // after the gate has read it.
        self.had_input_since_last_frame = true;
        let requests_repaint = match event {
            InputEvent::PointerMoved(p) => {
                let prev_hover = self.hovered;
                let prev_scroll = self.scroll_target;
                let prev_pinch = self.pinch_target;
                self.pointer_pos = Some(p);
                // Drag-latch check per button. Every captured button
                // independently latches once travel crosses
                // `DRAG_THRESHOLD`. Right-drag latching just suppresses
                // the click (same as left), so a slow right-press that
                // wiggles no longer pops a context menu — consistent
                // with click-suppression semantics.
                let mut latched = false;
                for cap in &mut self.captures {
                    if let Some(press) = &mut cap.press
                        && press.drag == PressDrag::None
                        && p.distance_squared(press.origin) >= DRAG_THRESHOLD * DRAG_THRESHOLD
                    {
                        press.drag = PressDrag::Started;
                        latched = true;
                    }
                }
                self.frame_had_action |= latched;
                self.refresh_pointer_targets(cascades);
                let move_subbed =
                    self.push_pointer_event(PointerSense::MOVE, Some(p), PointerEvent::Move);
                self.hovered != prev_hover
                    || self.scroll_target != prev_scroll
                    || self.pinch_target != prev_pinch
                    || self.captures.iter().any(|c| c.press.is_some())
                    || move_subbed
            }
            InputEvent::PointerLeft => {
                let observable = self.hovered.is_some()
                    || self.scroll_target.is_some()
                    || self.pinch_target.is_some()
                    || self.captures.iter().any(|c| c.press.is_some());
                self.pointer_pos = None;
                self.refresh_pointer_targets(cascades);
                // `Leave` is rare; emit whenever any pointer-class
                // subscription is active so subscribers can clean up
                // (clear crosshair, dismiss hover preview).
                let pointer_subbed = !self.subs.pointer_mask.is_empty();
                if pointer_subbed {
                    self.frame_pointer_events.push(PointerEvent::Leave);
                }
                observable || pointer_subbed
            }
            InputEvent::PointerPressed(btn) => {
                // Hit-test for the press target (the topmost *clickable*
                // widget under the pointer). Hover-only widgets are
                // transparent to presses even though they show as hovered.
                let pointer_pos = self.pointer_pos;
                let hit = pointer_pos.and_then(|p| cascades.hit_test(p, Sense::clicks));
                let buttons_subbed =
                    self.push_pointer_event(PointerSense::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Down { pos, button: btn }
                    });
                // Frame clock for multi-press timing — read before the
                // `capture_mut` borrow.
                let now = self.frame_time;
                let cap = self.capture_mut(btn);
                match hit.zip(pointer_pos) {
                    Some((target, pos)) => cap.begin_press(target, pos, now),
                    // A missed press clears any stale capture and
                    // leaves the run alone.
                    None => cap.press = None,
                }
                // Focus updates on a separate hit-test on the *left*
                // button only — right/middle clicks shouldn't steal
                // focus from a TextEdit. Focusability is orthogonal to
                // clickability (clicking a Button shouldn't steal focus
                // from a TextEdit either, hence the separate test).
                let prev_focus = self.focused;
                if btn == PointerButton::Left {
                    let focus_hit = pointer_pos.and_then(|p| cascades.hit_test_focusable(p));
                    match (focus_hit, self.focus_policy) {
                        (Some(id), _) => self.focused = Some(id),
                        (None, FocusPolicy::ClearOnMiss) => self.focused = None,
                        (None, FocusPolicy::PreserveOnMiss) => {}
                    }
                }
                // Press on inert surface (no click target, no focus
                // change, no `BUTTONS` subscriber) is observably
                // a no-op — under `OnDelta` the frame stays on the
                // paint-anim path. Focus-clearing clicks (outside a
                // focused TextEdit) and any sense hit still record;
                // popup-dismiss subscribers wake themselves.
                let observable = hit.is_some() || self.focused != prev_focus || buttons_subbed;
                self.frame_had_action |= observable;
                observable
            }
            InputEvent::PointerReleased(btn) => {
                let pointer_pos = self.pointer_pos;
                let cap = self.capture_mut(btn);
                // A captureless release (the press missed every widget)
                // has no press to take and touches nothing — an earlier
                // same-batch gesture's release edge survives it.
                let released = cap.press.take();
                if let Some(press) = released {
                    // A latched drag ending is its own edge (the release
                    // just destroyed the drag, so widgets can't infer it);
                    // otherwise a release back on the widget is a click
                    // carrying its press's run number — double-click is
                    // simply "the click whose press was #2 in the run".
                    let kind = if press.drag != PressDrag::None {
                        ReleaseKind::DragStopped
                    } else {
                        let hit = pointer_pos.and_then(|p| cascades.hit_test(p, Sense::clicks));
                        if hit == Some(press.target) {
                            ReleaseKind::Click { count: press.seq }
                        } else {
                            ReleaseKind::Miss
                        }
                    };
                    cap.release = Some(Release {
                        target: press.target,
                        kind,
                    });
                }
                let buttons_subbed =
                    self.push_pointer_event(PointerSense::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Up { pos, button: btn }
                    });
                // Capture was live ⇒ owning widget needs a record;
                // otherwise only `BUTTONS` subscribers wake.
                let observable = released.is_some() || buttons_subbed;
                self.frame_had_action |= observable;
                observable
            }
            InputEvent::ScrollPixels(d) => {
                let routed = self.scroll_target.is_some();
                if routed {
                    self.frame_scroll_pixels += d;
                }
                let subbed = self.push_scroll_class(|pos| PointerEvent::Scroll {
                    pos,
                    pixels: d,
                    lines: Vec2::ZERO,
                });
                routed || subbed
            }
            InputEvent::ScrollLines(d) => {
                let routed = self.scroll_target.is_some();
                if routed {
                    self.frame_scroll_lines += d;
                }
                let subbed = self.push_scroll_class(|pos| PointerEvent::Scroll {
                    pos,
                    pixels: Vec2::ZERO,
                    lines: d,
                });
                routed || subbed
            }
            InputEvent::Zoom(f) => {
                let routed = self.pinch_target.is_some();
                if routed {
                    self.frame_zoom_delta = combine_zoom_factors(self.frame_zoom_delta, f);
                }
                let subbed = self.push_scroll_class(|pos| PointerEvent::Zoom { pos, factor: f });
                routed || subbed
            }
            InputEvent::KeyDown {
                key,
                repeat,
                physical,
            } => {
                let kp = KeyPress {
                    key,
                    mods: self.modifiers,
                    repeat,
                    physical,
                };
                // Wake when a focused widget would consume the key
                // OR a specific-chord subscriber asked for it
                // OR a `KeyboardSense::KEY` subscriber is recording
                // raw key events. Idle keys with none of those
                // (typing into empty surface) skip the frame. The
                // chord check takes the whole `KeyPress` so the
                // non-Latin layout fallback applies — an off-focus
                // Cmd+Z still wakes on a Russian layout.
                let observable = self.focused.is_some()
                    || self.subs.matches_press(kp)
                    || self.subs.keyboard_mask.contains(KeyboardSense::KEY);
                if observable {
                    self.frame_keyboard_events.push(KeyboardEvent::Down(kp));
                    self.frame_had_action = true;
                }
                observable
            }
            InputEvent::Text(chunk) => {
                // Text is rare (only fires on IME commit / dead-key
                // resolution on most platforms). Wake when a focused
                // widget would consume it OR a TEXT subscriber wants
                // it.
                let observable =
                    self.focused.is_some() || self.subs.keyboard_mask.contains(KeyboardSense::TEXT);
                if observable {
                    self.frame_keyboard_events.push(KeyboardEvent::Text(chunk));
                    self.frame_had_action = true;
                }
                observable
            }
            InputEvent::ModifiersChanged(m) => {
                self.modifiers = m;
                // Only wake if a subscriber asked. Accel-underline
                // UIs / modifier debug overlays must subscribe to
                // `MODIFIER`; nothing else cares.
                self.subs.keyboard_mask.contains(KeyboardSense::MODIFIER)
            }
        };
        if requests_repaint {
            self.repaint_requested_since_last_frame = true;
        }
        InputDelta { requests_repaint }
    }

    /// Read and reset [`Self::frame_had_action`]. Called by
    /// [`crate::Ui::frame`] to decide whether to run a discarded
    /// pre-pass for state-mutation settling.
    pub(crate) fn take_action_flag(&mut self) -> bool {
        std::mem::take(&mut self.frame_had_action)
    }

    /// Drain the per-frame input queues without touching cascade-
    /// dependent state (active/focused eviction, hover recompute).
    /// Used by [`crate::Ui::frame`] for the discarded pass — pass
    /// 2's recording must see empty queues so `Response::clicked()`
    /// returns `false` everywhere and clicks aren't double-fired.
    /// Capacity-retained on the backing buffers.
    pub(crate) fn drain_per_frame_queues(&mut self) {
        for cap in &mut self.captures {
            cap.release = None;
            if let Some(press) = &mut cap.press {
                press.fresh = false;
                if press.drag == PressDrag::Started {
                    press.drag = PressDrag::Active;
                }
            }
        }
        self.had_input_since_last_frame = false;
        self.repaint_requested_since_last_frame = false;
        self.frame_had_action = false;
        self.frame_pointer_events.clear();
        self.frame_scroll_pixels = Vec2::ZERO;
        self.frame_scroll_lines = Vec2::ZERO;
        self.frame_zoom_delta = 1.0;
        self.frame_keyboard_events.clear();
    }

    /// Re-resolve `hovered` / `scroll_target` / `pinch_target` against
    /// `cascades` using the current `pointer_pos` — the single owner of
    /// the target-triple assignment (the `PointerMoved` / `PointerLeft`
    /// arms, `end_frame`, and the cold-start warmup all route through
    /// it). The warmup case: pre-frame-1 input events arrived with an
    /// empty cascade so their hit-tests resolved to nothing; after the
    /// warmup record pass has built a real cascade, `Ui::frame` calls
    /// this to route the held pointer position onto the right widgets
    /// before the user-visible record pass runs — so hover styling on
    /// frame 1 reflects the actual content under the cursor.
    pub(crate) fn refresh_pointer_targets(&mut self, cascades: &Cascades) {
        if let Some(p) = self.pointer_pos {
            let hits = cascades.hit_test_targets(p, Sense::hovers, Sense::scrolls, Sense::pinches);
            self.hovered = hits.hover;
            self.scroll_target = hits.scroll;
            self.pinch_target = hits.pinch;
        } else {
            self.hovered = None;
            self.scroll_target = None;
            self.pinch_target = None;
        }
    }

    /// Once-per-frame close-out (from `Ui::finalize_frame`, after the
    /// final record pass): recompute hover, drop transient per-frame
    /// flags, evict captured widgets that disappeared from the tree.
    /// Call after `CascadesEngine::run` (whose result `cascades` is
    /// passed here).
    pub(crate) fn end_frame(&mut self, cascades: &Cascades) {
        self.drain_per_frame_queues();
        // `modifiers` deliberately persists: modifier state is a running
        // snapshot, not per-frame. Held shift across multiple frames must
        // stay `true`.
        for cap in &mut self.captures {
            if let Some(press) = &cap.press
                && !cascades.by_id.contains_key(&press.target)
            {
                cap.press = None;
            }
        }
        // Focus eviction: same model as the per-button capture eviction
        // above. A focused widget that vanished from the tree drops
        // focus to None; otherwise next frame's keystrokes route to a
        // ghost.
        if let Some(focused) = self.focused
            && !cascades.by_id.contains_key(&focused)
        {
            self.focused = None;
        }
        self.refresh_pointer_targets(cascades);
    }

    /// Returns this frame's combined scroll delta if `id` is the
    /// current scroll hit-target; otherwise `Vec2::ZERO`. Combines the
    /// pixel-precise accumulator with the line-discrete accumulator
    /// scaled by `line_px` — caller supplies the line step (typically
    /// `theme.text.line_height_for(font_size)`) so wheel feel tracks
    /// the active font size instead of a hard-coded constant.
    pub(crate) fn scroll_delta_for(&self, id: WidgetId, line_px: f32) -> Vec2 {
        if self.scroll_target == Some(id) {
            self.frame_scroll_pixels + self.frame_scroll_lines * line_px
        } else {
            Vec2::ZERO
        }
    }

    /// Returns this frame's notched scroll count if `id` is the
    /// current scroll hit-target; otherwise `Vec2::ZERO`. Combines
    /// real line deltas (classic wheel) with touchpad-pixel deltas
    /// converted via `line_px` — so a touchpad gesture under a
    /// zoom modifier produces fractional notches at the same rate
    /// the pan side would have moved pixels, matching the pre-split
    /// behavior. Used by zoom routing.
    pub(crate) fn scroll_notches_for(&self, id: WidgetId, line_px: f32) -> Vec2 {
        if self.scroll_target != Some(id) {
            return Vec2::ZERO;
        }
        let denom = line_px.max(f32::EPSILON);
        self.frame_scroll_lines + self.frame_scroll_pixels / denom
    }

    /// Returns this frame's pinch-zoom factor if `id` is the current
    /// pinch hit-target (separate from `scroll_target` since
    /// `Sense::PINCH` and `Sense::SCROLL` are independent bits);
    /// otherwise `1.0`. Pinch ingest is unconditional (touch already
    /// disambiguates intent), so widgets get the raw multiplicative
    /// factor regardless of `ZoomConfig::modifier`.
    pub(crate) fn zoom_delta_for(&self, id: WidgetId) -> f32 {
        if self.pinch_target == Some(id) {
            self.frame_zoom_delta
        } else {
            1.0
        }
    }

    /// Snapshot into [`Self::frame_quiescent`] whether any widget can
    /// hold non-default interaction state this frame: no pointer on the
    /// surface, no routed scroll/pinch target, and no live button
    /// capture or per-frame click/double-click edge. Taken once per
    /// record pass so [`Self::response_for`] can default the interaction
    /// half out for every widget at once.
    ///
    /// `focused` is deliberately *not* part of this: [`crate::Ui::request_focus`]
    /// can set it mid-record, after the snapshot is taken, so
    /// `response_for` always reads it live — even on the fast path.
    pub(crate) fn snapshot_frame_quiescent(&mut self) {
        self.frame_quiescent = self.pointer_pos.is_none()
            && self.hovered.is_none()
            && self.scroll_target.is_none()
            && self.pinch_target.is_none()
            && self
                .captures
                .iter()
                .all(|c| c.press.is_none() && c.release.is_none());
    }

    pub(crate) fn pointer_local_for(&self, id: WidgetId, cascades: &Cascades) -> Option<Vec2> {
        let pointer = self.pointer_pos?;
        let entry_idx = cascades.entry_idx_of(id)? as usize;
        let layout_rect = cascades.entries.layout_rect()[entry_idx];
        let transform = cascades.entries.transform()[entry_idx];
        Some(pointer_in_widget_space(pointer, layout_rect.min, transform))
    }

    pub(crate) fn response_for(&self, id: WidgetId, cascades: &Cascades) -> ResponseState {
        // Geometry half — needed every frame for theme picking and
        // layout-relative math. `entry_idx_of` is the lone hash probe.
        let entry_idx = cascades.entry_idx_of(id).map(|i| i as usize);
        let rect = entry_idx.map(|i| cascades.entries.rect()[i]);
        let layout_rect = entry_idx.map(|i| cascades.entries.layout_rect()[i]);
        let transform = entry_idx.map_or(TranslateScale::IDENTITY, |i| {
            cascades.entries.transform()[i]
        });
        // Cascade flattens parent-disabled into each entry, so this is
        // the **effective** ancestor-or-self disabled — one frame stale.
        // Widgets that need lag-free self-toggle response merge their
        // own `element.disabled` on top after calling.
        let disabled = entry_idx.is_some_and(|i| cascades.entries.disabled()[i]);

        // Interaction half — on a quiescent frame every field below is at
        // its default, so skip the per-button capture scan and the
        // scroll/zoom lookups that every idle widget would otherwise pay.
        // `focused` is the one interaction field still read live: it can
        // be set by `request_focus` after `frame_quiescent` was snapshotted.
        if self.frame_quiescent {
            return ResponseState {
                rect,
                layout_rect,
                transform,
                disabled,
                focused: self.focused == Some(id),
                ..ResponseState::default()
            };
        }

        let me_under_pointer = self.hovered == Some(id);
        let left_press = self.capture(PointerButton::Left).press;
        // Hover is left-capture-gated: while some *other* widget holds
        // the left press, nothing else reads hovered.
        let hovered = me_under_pointer && left_press.is_none_or(|p| p.target == id);
        let focused = self.focused == Some(id);

        // One uniform slice per button. Phase priority mirrors the
        // capture: a live press is `Down` (its `fresh` edge) or
        // `Held`; with no press, a release edge is `Up` — so a
        // same-batch press+release collapses to `Up{click}` (the
        // completed click outranks the lost press edge) and a
        // same-batch re-press collapses to `Down` (the live capture
        // outranks the stale release).
        let mut buttons = [ButtonState::default(); PointerButton::COUNT];
        // Drag exclusivity: only the priority-first latched button
        // owns the widget's drag, so at most one slot goes live.
        let mut drag_owned = false;
        for btn in PointerButton::all() {
            let cap = self.capture(btn);
            let phase = match &cap.press {
                Some(press) if press.target == id => {
                    if press.fresh {
                        ButtonPhase::Down { press: press.seq }
                    } else {
                        ButtonPhase::Held
                    }
                }
                _ => match &cap.release {
                    Some(release) if release.target == id => ButtonPhase::Up {
                        click: match release.kind {
                            ReleaseKind::Click { count } => Some(count),
                            ReleaseKind::DragStopped | ReleaseKind::Miss => None,
                        },
                    },
                    _ => ButtonPhase::Idle,
                },
            };
            let mut drag = match &cap.release {
                Some(release)
                    if release.target == id && release.kind == ReleaseKind::DragStopped =>
                {
                    Drag::Stopped
                }
                _ => Drag::None,
            };
            // A threshold-crossed press overrides the stale stop edge
            // (same-frame stop-and-relatch reports the fresh gesture).
            // Rect-independent: the pointer can leave `id`'s rect
            // mid-drag and the delta keeps tracking.
            if !drag_owned
                && let Some(pointer) = self.pointer_pos
                && let Some(press) = &cap.press
                && press.target == id
                && press.drag != PressDrag::None
            {
                let delta = transform.inverse_vector(pointer - press.origin);
                drag = if press.drag == PressDrag::Started {
                    Drag::Started { delta }
                } else {
                    Drag::Active { delta }
                };
                drag_owned = true;
            }
            buttons[btn.idx()] = ButtonState { phase, drag };
        }
        let [left, right, middle] = buttons;

        // Scroll routes on `Sense::SCROLL`, pinch on `Sense::PINCH`.
        // Both gates fire even when the routed delta is `Vec2::ZERO`
        // / `1.0` — the caller checks against the identity value to
        // distinguish "not routed" from "routed but quiet".
        let scrolled = self.scroll_target == Some(id);
        let scroll = ScrollDelta {
            pixels: if scrolled {
                self.frame_scroll_pixels
            } else {
                Vec2::ZERO
            },
            lines: if scrolled {
                self.frame_scroll_lines
            } else {
                Vec2::ZERO
            },
            zoom: self.zoom_delta_for(id),
        };
        let pointer_local = self
            .pointer_pos
            .zip(layout_rect)
            .map(|(pointer, layout)| pointer_in_widget_space(pointer, layout.min, transform));

        ResponseState {
            rect,
            layout_rect,
            transform,
            pointer_local,
            hovered,
            disabled,
            focused,
            left,
            right,
            middle,
            scroll,
        }
    }
}

#[cfg(test)]
mod tests;
