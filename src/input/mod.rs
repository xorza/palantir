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
use crate::input::response::{DragState, InputDelta, ResponseState};
use crate::input::sense::{DOUBLE_CLICK_RADIUS, DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD, Sense};
use crate::input::subscriptions::{KeyboardSense, PointerSense, Subscriptions};
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use glam::Vec2;
use std::time::Duration;
use strum::EnumCount as _;

/// Per-button press/drag/click capture. One slot per [`PointerButton`].
/// Cleared on release, on cascade-eviction of the captured widget, and
/// on [`InputState::post_record`] for the one-frame edges
/// (`frame_drag_started`, `frame_click`, `frame_double_click`).
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Capture {
    /// Widget the press latched onto, or `None` if the press missed.
    pub(crate) active: Option<WidgetId>,
    /// Pointer position at the moment of press. Subtracted from the
    /// current pointer position for rect-independent drag deltas.
    pub(crate) press_pos: Option<Vec2>,
    /// Pointer has travelled at least [`DRAG_THRESHOLD`] from
    /// `press_pos` since the press latched. Sticky for the press
    /// lifetime; doubles as "suppress click on release."
    pub(crate) drag_latched: bool,
    /// One-frame edge: the `active` id on the move that flipped
    /// `drag_latched` `false → true`. Cleared by `post_record` / release
    /// / eviction.
    pub(crate) frame_drag_started: Option<WidgetId>,
    /// One-frame edge: widget that this button's press+release latched
    /// onto when the release landed on the same id and no drag was
    /// latched. Cleared by `post_record`.
    pub(crate) frame_click: Option<WidgetId>,
    /// One-frame edge: widget on which two consecutive clicks landed
    /// within [`DOUBLE_CLICK_WINDOW`]. Cleared by `post_record`.
    pub(crate) frame_double_click: Option<WidgetId>,
    /// Frame time ([`crate::Ui`]'s clock) of the most recent click on
    /// this button, used to detect a follow-up click within
    /// [`DOUBLE_CLICK_WINDOW`]. Cleared once a double-click fires so a
    /// third click within the same window doesn't fire a second one.
    pub(crate) last_click_at: Option<Duration>,
    /// Widget id of the most recent click on this button. A double-
    /// click only fires when the follow-up click lands on the same id.
    pub(crate) last_click_id: Option<WidgetId>,
    /// Pointer position of the most recent click. A double-click only
    /// fires when the follow-up lands within [`DOUBLE_CLICK_RADIUS`].
    pub(crate) last_click_pos: Option<Vec2>,
}

impl Capture {
    fn clear_press(&mut self) {
        self.press_pos = None;
        self.drag_latched = false;
        self.frame_drag_started = None;
    }
}

/// What happens to the currently-focused widget when the user presses
/// the pointer somewhere that *isn't* a focusable widget. Set via
/// [`crate::Ui::set_focus_policy`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Pressing on a non-focusable widget or empty surface preserves
    /// the current focus. Friendlier for sketches and tooling UIs
    /// where every other widget is a Button — clicking a Button while
    /// editing a field keeps the cursor in the field. Default.
    PreserveOnMiss,
    /// Pressing anywhere that isn't a focusable widget clears focus.
    /// Native-app convention on most platforms (click-outside-to-blur).
    #[default]
    ClearOnMiss,
}

/// Palantir-native input event. Independent of any windowing toolkit.
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
    /// accumulate into [`InputState::frame_scroll_pixels`].
    ScrollPixels(Vec2),
    /// Notched scroll delta — classic wheel /
    /// `MouseScrollDelta::LineDelta`. Carries the raw line count
    /// (sign-flipped to match `ScrollPixels`); the consuming widget
    /// multiplies by its own font-derived line step at record time
    /// rather than this layer baking in a constant. Multiple events
    /// in one frame accumulate into [`InputState::frame_scroll_lines`].
    ScrollLines(Vec2),
    /// Multiplicative zoom factor from a touch / touchpad pinch gesture.
    /// `1.0` is identity; `1.05` zooms in 5%, `0.95` zooms out 5%.
    /// Multiple events in one frame multiply into
    /// [`InputState::frame_zoom_delta`]. Wheel-based zoom is *not*
    /// translated into `Zoom` — the active scroll widget decides at
    /// record time whether wheel ticks count as pan or zoom.
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

/// What changed observably after an [`InputEvent`] was dispatched.
/// Hosts read [`Self::requests_repaint`] to decide whether to schedule
/// a redraw — pointer moves over inert surfaces leave it `false`, so
/// the frame can be skipped entirely. Animation/tooltip-delay wakes
/// still drive paints via `FrameReport::repaint_after`, independently.
impl InputEvent {
    /// Translate a winit `WindowEvent` into one or more palantir input
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
        use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
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
            // Convert to "positive delta = pan offset forward" so widgets can
            // do `offset += delta` directly. winit reports +y when the wheel
            // rotates *away* from the user (scroll up) and +x when it rotates
            // / swipes right (reveal content to the right means panning
            // *into* it, i.e. content shifts left); flip both so positive
            // means "advance the scroll offset."
            WindowEvent::PinchGesture { delta, .. } => emit(InputEvent::Zoom(1.0 + *delta as f32)),
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
            WindowEvent::Ime(winit::event::Ime::Commit(s)) => {
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
    let cap = TextChunk::INLINE_CAP;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        let next_end = i + c.len_utf8();
        if next_end - start > cap {
            if i > start {
                // SAFETY: `start` and `i` are both char-boundary offsets.
                if let Some(chunk) = TextChunk::new(&s[start..i]) {
                    emit(InputEvent::Text(chunk));
                }
            }
            start = i;
        }
    }
    if start < s.len()
        && let Some(chunk) = TextChunk::new(&s[start..])
    {
        emit(InputEvent::Text(chunk));
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
    /// whenever the pointer moves and at `post_record`. The scroll widget
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
    /// [`Self::post_record`]. Read by scroll widgets at record time
    /// alongside [`Self::frame_scroll_lines`] — the widget combines
    /// the two via its font-derived line step.
    pub(crate) frame_scroll_pixels: Vec2,
    /// Notched wheel delta accumulated this frame (line count from
    /// `ScrollLines`). Cleared in [`Self::post_record`]. The scroll
    /// widget multiplies by its own line-px step at consumption time
    /// instead of baking a constant in here, so wheel feel tracks the
    /// active font size. Also read directly by zoom routing (each line
    /// = one notch, no roundtrip through a pixel constant).
    pub(crate) frame_scroll_lines: Vec2,
    /// Multiplicative pinch-zoom delta accumulated this frame; `1.0` =
    /// no zoom. Cleared in [`Self::post_record`]. Read by scroll widgets
    /// configured with a `ZoomConfig`. Wheel-based zoom is computed
    /// at the widget from [`Self::frame_scroll_pixels`] under the
    /// `ZoomConfig::modifier` gate, not accumulated here.
    pub(crate) frame_zoom_delta: f32,
    /// Frame-snapshot of the theme's default font line height in
    /// logical px. Filled by [`crate::Ui::frame`] before any
    /// `response_for` calls; read here to convert
    /// `frame_scroll_lines` into pixels for `scroll_delta_for` without
    /// each call dereffing `theme.text` again. Cached on `InputState`
    /// (not on `Ui`) because the consumer (`scroll_delta_for`) already
    /// takes `&self.input`, so the snapshot lives in the same borrow.
    pub(crate) frame_line_px: f32,
    /// Frame-snapshot of "no widget can hold any non-default interaction
    /// state this frame" — no pointer on the surface, no routed
    /// scroll/pinch target, no live button capture or click/double-click
    /// edge. Filled once per record pass by [`crate::Ui::record_pass`]
    /// (alongside `frame_line_px`) from [`Self::compute_frame_quiescent`];
    /// read in [`Self::response_for`] to default the whole interaction
    /// half out for every widget instead of re-deriving it per call.
    /// `focused` is excluded on purpose (see `compute_frame_quiescent`),
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
    /// Latest modifier-key snapshot. Persists across `post_record` —
    /// modifier *state* is not a per-frame thing the way keystrokes
    /// are. Updated only on `ModifiersChanged` events.
    pub(crate) modifiers: Modifiers,
    /// Currently focused widget, or `None`. Set on `PointerPressed(Left)`
    /// when the press lands on a focusable widget. Evicted in
    /// [`Self::post_record`] when the focused widget vanishes from the
    /// tree (matches the per-id state map's eviction model). Read by
    /// keyboard consumers to decide whether to drain
    /// `frame_keyboard_events`.
    pub(crate) focused: Option<WidgetId>,
    /// Press-on-non-focusable-widget behavior. See [`FocusPolicy`].
    pub(crate) focus_policy: FocusPolicy,
    /// Set in `on_input` when an event arrives that could plausibly
    /// drive a state mutation (pointer press/release, `KeyDown`,
    /// `Text`). Read by `Ui::run_frame` to decide whether to
    /// re-record the frame after pass 1's `post_record` drains the
    /// input queues. Hover-only events (`PointerMoved`, `PointerLeft`)
    /// and modifier changes don't flip it — they fire too often and
    /// don't typically mutate user state. Note: this is the
    /// *event-type* gate; it doesn't filter for whether the event
    /// was actually consumed (an idle `KeyDown` with no focus + no
    /// chord sub still sets it). False positives waste a pass; bias
    /// is toward not missing real mutations.
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
    /// Frame clock (`Ui::time`) as of the last `Ui::frame`, refreshed
    /// once per frame so input handlers running *between* frames stamp
    /// events on the same deterministic clock the rest of the crate uses
    /// (vs wall-clock `Instant`). Drives double-click timing.
    pub(crate) frame_time: Duration,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub(crate) fn new() -> Self {
        Self {
            pointer_pos: None,
            hovered: None,
            scroll_target: None,
            pinch_target: None,
            captures: [Capture::default(); PointerButton::COUNT],
            frame_scroll_pixels: Vec2::ZERO,
            frame_scroll_lines: Vec2::ZERO,
            frame_zoom_delta: 1.0,
            // Populated by `Ui::frame` before record runs;
            // 16.0 is a safe pre-frame fallback (matches the default
            // theme's body line height) so the rare "response_for
            // before first frame" path doesn't divide by zero.
            frame_line_px: 16.0,
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

    #[inline]
    fn capture(&self, b: PointerButton) -> &Capture {
        &self.captures[b.idx()]
    }

    #[inline]
    fn capture_mut(&mut self, b: PointerButton) -> &mut Capture {
        &mut self.captures[b.idx()]
    }

    /// Push a [`PointerEvent::Scroll`] when at least one subscriber
    /// holds [`PointerSense::SCROLL`] AND we have a pointer position.
    /// Returns whether the wake fires.
    fn push_scroll_event(&mut self, pixels: Vec2, lines: Vec2) -> bool {
        if !self.subs.pointer_mask.contains(PointerSense::SCROLL) {
            return false;
        }
        let Some(pos) = self.pointer_pos else {
            return false;
        };
        self.frame_pointer_events
            .push(PointerEvent::Scroll { pos, pixels, lines });
        true
    }

    fn record_pointer_down(&mut self, pos: Option<glam::Vec2>, button: PointerButton) -> bool {
        self.record_pointer_button(pos, |pos| PointerEvent::Down { pos, button })
    }

    fn record_pointer_up(&mut self, pos: Option<glam::Vec2>, button: PointerButton) -> bool {
        self.record_pointer_button(pos, |pos| PointerEvent::Up { pos, button })
    }

    /// Push a button event to [`Self::frame_pointer_events`] and
    /// answer "should this event wake the next frame?" Wake fires
    /// when any subscriber holds [`PointerSense::BUTTONS`] — single
    /// bitwise AND on the cached `pointer_mask`. Returns `true` even
    /// when `pos` is `None` so an off-surface press still wakes; the
    /// `PointerEvent` itself is only pushed if there's a position
    /// (no consumer can do anything useful without one).
    fn record_pointer_button(
        &mut self,
        pos: Option<glam::Vec2>,
        make_event: impl FnOnce(glam::Vec2) -> PointerEvent,
    ) -> bool {
        if !self.subs.pointer_mask.contains(PointerSense::BUTTONS) {
            return false;
        }
        if let Some(pos) = pos {
            self.frame_pointer_events.push(make_event(pos));
        }
        true
    }

    /// Feed a palantir-native input event. Hit-tests against the
    /// frozen `Cascades` from this frame's most recent run. Returns an
    /// [`InputDelta`] hosts use to decide whether to request a redraw —
    /// a `PointerMoved` over a non-hover-reactive surface (no active
    /// capture, no hover/scroll target change) leaves
    /// `requests_repaint` false so the frame can be skipped entirely.
    pub(crate) fn on_input(&mut self, event: InputEvent, cascades: &Cascades) -> InputDelta {
        // Any host-pushed event disqualifies the next frame from the
        // paint-anim-only short-circuit — the recording closure might
        // observe even a pointer move (hover styling) or modifier
        // change (shortcut hint). Cleared at the top of `frame`
        // after the gate has read it.
        self.had_input_since_last_frame = true;
        if matches!(
            event,
            InputEvent::PointerPressed(_)
                | InputEvent::PointerReleased(_)
                | InputEvent::KeyDown { .. }
                | InputEvent::Text(_) // | InputEvent::Scroll{Pixels,Lines}(_)
                                      // | InputEvent::Zoom(_)
        ) {
            self.frame_had_action = true;
        }
        let requests_repaint = match event {
            InputEvent::PointerMoved(p) => {
                let prev_hover = self.hovered;
                let prev_scroll = self.scroll_target;
                self.pointer_pos = Some(p);
                // Drag-latch check per button. Every captured button
                // independently latches once travel crosses
                // `DRAG_THRESHOLD`. `ResponseState::drag_delta` reports
                // the left button; other buttons go through
                // [`crate::Response::drag_delta_by`] /
                // [`crate::Response::drag_started_by`]. Right-drag latching
                // just suppresses the click (same as left), so a slow
                // right-press that wiggles no longer pops a context
                // menu — consistent with click-suppression semantics.
                for btn in PointerButton::all() {
                    let cap = self.capture_mut(btn);
                    if !cap.drag_latched
                        && cap.active.is_some()
                        && let Some(press) = cap.press_pos
                        && (p - press).length() >= DRAG_THRESHOLD
                    {
                        cap.drag_latched = true;
                        cap.frame_drag_started = cap.active;
                        self.frame_had_action = true;
                    }
                }
                let prev_pinch = self.pinch_target;
                let hits =
                    cascades.hit_test_targets(p, Sense::hovers, Sense::scrolls, Sense::pinches);
                self.hovered = hits.hover;
                self.scroll_target = hits.scroll;
                self.pinch_target = hits.pinch;
                let move_subbed = self.subs.pointer_mask.contains(PointerSense::MOVE);
                if move_subbed {
                    self.frame_pointer_events.push(PointerEvent::Move(p));
                }
                self.hovered != prev_hover
                    || self.scroll_target != prev_scroll
                    || self.pinch_target != prev_pinch
                    || self.captures.iter().any(|c| c.active.is_some())
                    || move_subbed
            }
            InputEvent::PointerLeft => {
                let observable = self.hovered.is_some()
                    || self.scroll_target.is_some()
                    || self.pinch_target.is_some()
                    || self.captures.iter().any(|c| c.active.is_some());
                self.pointer_pos = None;
                self.hovered = None;
                self.scroll_target = None;
                self.pinch_target = None;
                // `Leave` is rare; emit whenever any pointer-class
                // subscription is active so subscribers can clean up
                // (clear crosshair, dismiss hover preview).
                let pointer_subbed = self
                    .subs
                    .pointer_mask
                    .intersects(PointerSense::MOVE | PointerSense::BUTTONS | PointerSense::SCROLL);
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
                let buttons_subbed = self.record_pointer_down(pointer_pos, btn);
                let cap = self.capture_mut(btn);
                cap.active = hit;
                cap.press_pos = hit.and(pointer_pos);
                // Focus updates on a separate hit-test on the *left*
                // button only — right/middle clicks shouldn't steal
                // focus from a TextEdit. Focusability is orthogonal to
                // clickability (clicking a Button shouldn't steal focus
                // from a TextEdit either, hence the separate test).
                let prev_focus = self.focused;
                if btn == PointerButton::Left {
                    let focus_hit = self
                        .pointer_pos
                        .and_then(|p| cascades.hit_test_focusable(p));
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
                hit.is_some() || self.focused != prev_focus || buttons_subbed
            }
            InputEvent::PointerReleased(btn) => {
                let pointer_pos = self.pointer_pos;
                // Frame clock for double-click timing — read before the
                // `capture_mut` borrow.
                let now = self.frame_time;
                let cap = self.capture_mut(btn);
                let drag_suppressed_click = cap.drag_latched;
                let captured = cap.active.take();
                cap.clear_press();
                if let Some(a) = captured {
                    let hit = pointer_pos.and_then(|p| cascades.hit_test(p, Sense::clicks));
                    if hit == Some(a) && !drag_suppressed_click {
                        let cap = self.capture_mut(btn);
                        cap.frame_click = Some(a);
                        // Double-click: same widget, within
                        // `DOUBLE_CLICK_WINDOW` on the frame clock *and*
                        // `DOUBLE_CLICK_RADIUS` of the first press — a slow
                        // drift between the two no longer false-fires.
                        // Frame-time (not wall-clock `Instant`) keeps this
                        // deterministic and testable. Resets the last-click
                        // slot on a fire so a third click doesn't pair with
                        // the second.
                        let near = match (pointer_pos, cap.last_click_pos) {
                            (Some(p), Some(q)) => p.distance(q) <= DOUBLE_CLICK_RADIUS,
                            _ => false,
                        };
                        let is_double = cap.last_click_id == Some(a)
                            && cap.last_click_at.is_some_and(|prev| {
                                now.saturating_sub(prev) <= DOUBLE_CLICK_WINDOW
                            })
                            && near;
                        if is_double {
                            cap.frame_double_click = Some(a);
                            cap.last_click_at = None;
                            cap.last_click_id = None;
                            cap.last_click_pos = None;
                        } else {
                            cap.last_click_at = Some(now);
                            cap.last_click_id = Some(a);
                            cap.last_click_pos = pointer_pos;
                        }
                    }
                }
                let buttons_subbed = self.record_pointer_up(pointer_pos, btn);
                // Capture was live ⇒ owning widget needs a record;
                // otherwise only `BUTTONS` subscribers wake.
                captured.is_some() || buttons_subbed
            }
            InputEvent::ScrollPixels(d) => {
                self.frame_scroll_pixels += d;
                let subbed = self.push_scroll_event(d, Vec2::ZERO);
                self.scroll_target.is_some() || subbed
            }
            InputEvent::ScrollLines(d) => {
                self.frame_scroll_lines += d;
                let subbed = self.push_scroll_event(Vec2::ZERO, d);
                self.scroll_target.is_some() || subbed
            }
            InputEvent::Zoom(f) => {
                self.frame_zoom_delta *= f;
                let subbed = if self.subs.pointer_mask.contains(PointerSense::SCROLL)
                    && let Some(pos) = self.pointer_pos
                {
                    self.frame_pointer_events
                        .push(PointerEvent::Zoom { pos, factor: f });
                    true
                } else {
                    false
                };
                self.pinch_target.is_some() || subbed
            }
            InputEvent::KeyDown {
                key,
                repeat,
                physical,
            } => {
                let mods = self.modifiers;
                let kp = KeyPress {
                    key,
                    mods,
                    repeat,
                    physical,
                };
                self.frame_keyboard_events.push(KeyboardEvent::Down(kp));
                // Wake when a focused widget would consume the key
                // OR a specific-chord subscriber asked for it
                // OR a `KeyboardSense::KEY` subscriber is recording
                // raw key events. Idle keys with none of those
                // (typing into empty surface) skip the frame. The
                // chord check takes the whole `KeyPress` so the
                // non-Latin layout fallback applies — an off-focus
                // Cmd+Z still wakes on a Russian layout.
                self.focused.is_some()
                    || self.subs.matches_press(kp)
                    || self.subs.keyboard_mask.contains(KeyboardSense::KEY)
            }
            InputEvent::Text(chunk) => {
                self.frame_keyboard_events.push(KeyboardEvent::Text(chunk));
                // Text is rare (only fires on IME commit / dead-key
                // resolution on most platforms). Wake when a focused
                // widget would consume it OR a TEXT subscriber wants
                // it.
                self.focused.is_some() || self.subs.keyboard_mask.contains(KeyboardSense::TEXT)
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
    /// [`crate::Ui::run_frame`] to decide whether to run a discarded
    /// pre-pass for state-mutation settling.
    pub(crate) fn take_action_flag(&mut self) -> bool {
        std::mem::take(&mut self.frame_had_action)
    }

    /// Drain the per-frame input queues without touching cascade-
    /// dependent state (active/focused eviction, hover recompute).
    /// Used by [`crate::Ui::run_frame`] for the discarded pass — pass
    /// 2's recording must see empty queues so `Response::clicked()`
    /// returns `false` everywhere and clicks aren't double-fired.
    /// Capacity-retained on the backing buffers.
    pub(crate) fn drain_per_frame_queues(&mut self) {
        for cap in &mut self.captures {
            cap.frame_click = None;
            cap.frame_double_click = None;
            cap.frame_drag_started = None;
        }
        self.had_input_since_last_frame = false;
        self.repaint_requested_since_last_frame = false;
        self.frame_pointer_events.clear();
        self.frame_scroll_pixels = Vec2::ZERO;
        self.frame_scroll_lines = Vec2::ZERO;
        self.frame_zoom_delta = 1.0;
        self.frame_keyboard_events.clear();
    }

    /// Re-resolve `hovered` / `scroll_target` / `pinch_target` against
    /// `cascades` using the current `pointer_pos`. Used by the
    /// cold-start warmup path in `Ui::frame`: pre-frame-1 input
    /// events arrived with an empty cascade so their hit-tests
    /// resolved to nothing. After the warmup record pass has built
    /// a real cascade, this routes the held pointer position onto the
    /// right widgets before the user-visible record pass runs — so
    /// hover styling on frame 1 reflects the actual content under
    /// the cursor instead of None.
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

    /// Recompute hover, drop transient per-frame flags, evict captured
    /// widgets that disappeared from the tree. Call after
    /// `CascadesEngine::run` (whose result `cascades` is passed here).
    pub(crate) fn post_record(&mut self, cascades: &Cascades) {
        self.drain_per_frame_queues();
        // `modifiers` deliberately persists: modifier state is a running
        // snapshot, not per-frame. Held shift across multiple frames must
        // stay `true`.
        for cap in &mut self.captures {
            if let Some(a) = cap.active
                && !cascades.contains_widget(a)
            {
                cap.active = None;
                cap.clear_press();
            }
        }
        // Focus eviction: same model as the per-button capture eviction
        // above. A focused widget that vanished from the tree drops
        // focus to None; otherwise next frame's keystrokes route to a
        // ghost.
        if let Some(focused) = self.focused
            && !cascades.contains_widget(focused)
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

    /// Pixel-precise scroll this frame for `id` if it's the current
    /// scroll target — the touchpad / precision-wheel source
    /// (`MouseScrollDelta::PixelDelta`). Sibling of [`Self::scroll_delta_for`]
    /// that exposes the touchpad slice on its own so consumers can
    /// distinguish "trackpad pan" from "wheel notch."
    pub(crate) fn scroll_pixels_for(&self, id: WidgetId) -> Vec2 {
        if self.scroll_target == Some(id) {
            self.frame_scroll_pixels
        } else {
            Vec2::ZERO
        }
    }

    /// Line-discrete scroll this frame for `id` if it's the current
    /// scroll target — the classic-wheel source
    /// (`MouseScrollDelta::LineDelta`), in **raw line units** (not
    /// multiplied by `line_px`). Sibling of [`Self::scroll_delta_for`]
    /// that exposes the wheel slice on its own.
    pub(crate) fn scroll_lines_for(&self, id: WidgetId) -> Vec2 {
        if self.scroll_target == Some(id) {
            self.frame_scroll_lines
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

    /// The active drag on `id`, or `None` outside a threshold-crossed
    /// drag. When multiple buttons are simultaneously latched on the
    /// same widget, the priority-first in [`PointerButton::all`]
    /// wins. Rect-independent: the pointer can leave `id`'s rect
    /// mid-drag and the delta keeps tracking.
    pub(crate) fn active_drag(&self, id: WidgetId) -> Option<DragState> {
        let pointer = self.pointer_pos?;
        for button in PointerButton::all() {
            let cap = self.capture(button);
            if cap.active == Some(id)
                && cap.drag_latched
                && let Some(press) = cap.press_pos
            {
                return Some(DragState {
                    button,
                    delta: pointer - press,
                    started: cap.frame_drag_started == Some(id),
                });
            }
        }
        None
    }

    /// `true` when no widget can hold any non-default interaction state
    /// this frame: no pointer on the surface, no routed scroll/pinch
    /// target, and no live button capture or per-frame click/double-click
    /// edge. Snapshotted once per record pass into
    /// [`Self::frame_quiescent`] so [`Self::response_for`] can default
    /// the interaction half out for every widget at once.
    ///
    /// `focused` is deliberately *not* part of this: [`crate::Ui::request_focus`]
    /// can set it mid-record, after the snapshot is taken, so
    /// `response_for` always reads it live — even on the fast path.
    pub(crate) fn compute_frame_quiescent(&self) -> bool {
        self.pointer_pos.is_none()
            && self.hovered.is_none()
            && self.scroll_target.is_none()
            && self.pinch_target.is_none()
            && self.captures.iter().all(|c| {
                c.active.is_none() && c.frame_click.is_none() && c.frame_double_click.is_none()
            })
    }

    pub(crate) fn response_for(&self, id: WidgetId, cascades: &Cascades) -> ResponseState {
        // Geometry half — needed every frame for theme picking and
        // layout-relative math. `entry_idx_of` is the lone hash probe.
        let entry_idx = cascades.entry_idx_of(id).map(|i| i as usize);
        let rect = entry_idx.map(|i| cascades.entries.rect()[i]);
        let layout_rect = entry_idx.map(|i| cascades.entries.layout_rect()[i]);
        // Cascade flattens parent-disabled into each entry, so this is
        // the **effective** ancestor-or-self disabled — one frame stale.
        // Widgets that need lag-free self-toggle response merge their
        // own `element.disabled` on top after calling.
        let disabled = entry_idx.is_some_and(|i| cascades.entries.disabled()[i]);

        // Interaction half — on a quiescent frame every field below is at
        // its default, so skip the per-button capture scans, the two
        // 3-iteration loops (`active_drag`, `double_click`), and the
        // scroll/zoom lookups that every idle widget would otherwise pay.
        // `focused` is the one interaction field still read live: it can
        // be set by `request_focus` after `frame_quiescent` was snapshotted.
        if self.frame_quiescent {
            return ResponseState {
                rect,
                layout_rect,
                disabled,
                focused: self.focused == Some(id),
                ..ResponseState::default()
            };
        }

        let left = self.capture(PointerButton::Left);
        let right = self.capture(PointerButton::Right);

        let me_under_pointer = self.hovered == Some(id);
        let me_left_captured = left.active == Some(id);
        let nothing_left_captured = left.active.is_none();

        let pressed = me_left_captured && me_under_pointer;
        let hovered = me_under_pointer && (nothing_left_captured || me_left_captured);
        let clicked = left.frame_click == Some(id);
        let secondary_clicked = right.frame_click == Some(id);
        let focused = self.focused == Some(id);
        let drag = self.active_drag(id);
        let double_click =
            PointerButton::all().find(|b| self.capture(*b).frame_double_click == Some(id));

        // Scroll routes on `Sense::SCROLL`, pinch on `Sense::PINCH`.
        // Both gates fire even when the routed delta is `Vec2::ZERO`
        // / `1.0` — the caller checks against the identity value to
        // distinguish "not routed" from "routed but quiet".
        let scroll_pixels = self.scroll_pixels_for(id);
        let scroll_lines = self.scroll_lines_for(id);
        let zoom_factor = self.zoom_delta_for(id);
        let pointer_local = self.pointer_pos.zip(rect).map(|(p, r)| p - r.min);

        ResponseState {
            rect,
            layout_rect,
            hovered,
            pressed,
            clicked,
            secondary_clicked,
            disabled,
            focused,
            drag,
            double_click,
            scroll_pixels,
            scroll_lines,
            zoom_factor,
            pointer_local,
        }
    }
}

#[cfg(test)]
mod tests;
