pub(crate) mod keyboard;
pub(crate) mod sense;
pub(crate) mod shortcut;

use crate::input::keyboard::{
    Key, KeyPress, Modifiers, TextChunk, key_from_winit, modifiers_from_winit,
};
use crate::input::sense::{DRAG_THRESHOLD, Sense};
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PointerButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

impl PointerButton {
    pub(crate) const COUNT: usize = 3;

    #[inline]
    fn idx(self) -> usize {
        self as usize
    }
}

/// Per-button press/drag/click capture. One slot per [`PointerButton`].
/// Cleared on release, on cascade-eviction of the captured widget, and
/// on [`InputState::post_record`] for the one-frame edges
/// (`frame_drag_started`, `frame_click`).
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDelta {
    pub requests_repaint: bool,
}

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

/// Snapshot of one widget's interaction state for the current frame.
/// `rect` is the widget's last-frame logical-pixel rect (`None` on first frame).
///
/// `disabled` is the **cascaded** disabled flag (the widget OR any ancestor),
/// read from the previous frame's cascade — one-frame stale, like
/// hover/press. Widgets that need lag-free self-disabled visuals also
/// merge their own `Element::disabled` (`state.disabled |= element.disabled`)
/// before reading the field.
///
/// `focused` is `true` when this widget currently holds keyboard focus
/// (`Ui::focused_id() == Some(id)`). Updated synchronously with focus
/// changes, so unlike `hovered`/`pressed` it isn't one-frame stale —
/// a widget that just called `ui.request_focus(id)` reads `true` on
/// the same frame.
#[derive(Default, Clone, Copy, Debug)]
pub struct ResponseState {
    pub rect: Option<Rect>,
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
    /// One-frame edge: right-button click landed and released on this
    /// widget without a drag. Independent of `clicked` (left-button).
    pub secondary_clicked: bool,
    pub disabled: bool,
    pub focused: bool,
    /// Cumulative pointer travel since press while `id` holds the
    /// active, threshold-crossed drag. `None` outside drag and for
    /// sub-threshold wiggle — never `Some(Vec2::ZERO)`. Callers
    /// compose `pos = anchor + delta`, capturing `anchor` on the
    /// `drag_started` frame.
    pub drag_delta: Option<Vec2>,
    /// One-frame edge: `true` on the frame the drag latches (the
    /// threshold-crossing pointer move). `false` everywhere else.
    /// Snapshot the position here so subsequent `drag_delta` reads
    /// compose against a stable anchor.
    pub drag_started: bool,
}

/// Live input state machine: the things that survive across input events
/// independently of whether the tree was rebuilt. Per-frame rebuilt data
/// (last-frame rects, cascade scratch) lives in [`HitIndex`].
pub struct InputState {
    /// Pointer position in logical pixels, `None` when off-surface.
    pub(crate) pointer_pos: Option<Vec2>,
    hovered: Option<WidgetId>,
    /// Topmost `Sense::SCROLL` widget under the pointer, recomputed
    /// whenever the pointer moves and at `post_record`. The scroll widget
    /// matching this id consumes [`Self::frame_scroll_pixels`].
    scroll_target: Option<WidgetId>,
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
    /// Keystrokes accumulated this frame, awaiting drain by the focused
    /// widget at record time. Capacity-retained across frames; cleared
    /// in [`Self::post_record`] regardless of whether anything consumed
    /// them. Step-3 focus dispatch reads this; today nothing does.
    pub(crate) frame_keys: Vec<KeyPress>,
    /// Committed text accumulated this frame from `Text(chunk)` events.
    /// One `String` rather than `Vec<TextChunk>` so consumers can splice
    /// directly into their buffer without re-concatenating; chunks are
    /// already grapheme-aligned at the translation boundary so byte
    /// concatenation is safe. Capacity-retained, cleared in `post_record`.
    pub(crate) frame_text: String,
    /// Latest modifier-key snapshot. Persists across `post_record` —
    /// modifier *state* is not a per-frame thing the way keystrokes
    /// are. Updated only on `ModifiersChanged` events.
    pub(crate) modifiers: Modifiers,
    /// Currently focused widget, or `None`. Set on `PointerPressed(Left)`
    /// when the press lands on a focusable widget. Evicted in
    /// [`Self::post_record`] when the focused widget vanishes from the
    /// tree (matches the per-id state map's eviction model). Read by
    /// keyboard consumers to decide whether to drain `frame_keys` /
    /// `frame_text` (step 5 of the TextEdit plan).
    pub(crate) focused: Option<WidgetId>,
    /// Press-on-non-focusable-widget behavior. See [`FocusPolicy`].
    pub(crate) focus_policy: FocusPolicy,
    /// Set in `on_input` when an event arrives that could plausibly
    /// drive a state mutation (clicks, keys, text, scroll). Read by
    /// `Ui::run_frame` to decide whether to re-record the frame after
    /// pass 1's `post_record` drains the input queues. Hover-only events
    /// (`PointerMoved`, `PointerLeft`) and modifier changes don't flip
    /// it — they fire too often and don't typically mutate user state.
    /// Reset by `Ui::run_frame` after the decision is made.
    pub(crate) frame_had_action: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub fn new() -> Self {
        Self {
            pointer_pos: None,
            hovered: None,
            scroll_target: None,
            captures: [Capture::default(); PointerButton::COUNT],
            frame_scroll_pixels: Vec2::ZERO,
            frame_scroll_lines: Vec2::ZERO,
            frame_zoom_delta: 1.0,
            frame_keys: Vec::new(),
            frame_text: String::new(),
            modifiers: Modifiers::NONE,
            focused: None,
            focus_policy: FocusPolicy::default(),
            frame_had_action: false,
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

    /// Feed a palantir-native input event. Hit-tests against the
    /// frozen `Cascades` from this frame's most recent run. Returns an
    /// [`InputDelta`] hosts use to decide whether to request a redraw —
    /// a `PointerMoved` over a non-hover-reactive surface (no active
    /// capture, no hover/scroll target change) leaves
    /// `requests_repaint` false so the frame can be skipped entirely.
    #[profiling::function]
    pub(crate) fn on_input(&mut self, event: InputEvent, cascades: &Cascades) -> InputDelta {
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
                // Drag-latch check, left button only today. When middle/
                // right drag widgets land, gate by `Capture::drags_when_latched`
                // or similar; until then, only left flips `drag_latched`.
                let lc = self.capture_mut(PointerButton::Left);
                if !lc.drag_latched
                    && lc.active.is_some()
                    && let Some(press) = lc.press_pos
                    && (p - press).length() >= DRAG_THRESHOLD
                {
                    lc.drag_latched = true;
                    lc.frame_drag_started = lc.active;
                    self.frame_had_action = true;
                }
                let hits = cascades.hit_test_pair(p, Sense::hovers, Sense::scrolls);
                self.hovered = hits.hover;
                self.scroll_target = hits.scroll;
                self.hovered != prev_hover
                    || self.scroll_target != prev_scroll
                    || self.captures.iter().any(|c| c.active.is_some())
            }
            InputEvent::PointerLeft => {
                let observable = self.hovered.is_some()
                    || self.scroll_target.is_some()
                    || self.captures.iter().any(|c| c.active.is_some());
                self.pointer_pos = None;
                self.hovered = None;
                self.scroll_target = None;
                observable
            }
            InputEvent::PointerPressed(btn) => {
                // Hit-test for the press target (the topmost *clickable*
                // widget under the pointer). Hover-only widgets are
                // transparent to presses even though they show as hovered.
                let pointer_pos = self.pointer_pos;
                let hit = pointer_pos.and_then(|p| cascades.hit_test(p, Sense::clicks));
                let cap = self.capture_mut(btn);
                cap.active = hit;
                cap.press_pos = hit.and(pointer_pos);
                // Focus updates on a separate hit-test on the *left*
                // button only — right/middle clicks shouldn't steal
                // focus from a TextEdit. Focusability is orthogonal to
                // clickability (clicking a Button shouldn't steal focus
                // from a TextEdit either, hence the separate test).
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
                true
            }
            InputEvent::PointerReleased(btn) => {
                let pointer_pos = self.pointer_pos;
                let cap = self.capture_mut(btn);
                let drag_suppressed_click = cap.drag_latched;
                let captured = cap.active.take();
                cap.clear_press();
                if let Some(a) = captured {
                    let hit = pointer_pos.and_then(|p| cascades.hit_test(p, Sense::clicks));
                    if hit == Some(a) && !drag_suppressed_click {
                        self.capture_mut(btn).frame_click = Some(a);
                    }
                }
                true
            }
            InputEvent::ScrollPixels(d) => {
                self.frame_scroll_pixels += d;
                self.scroll_target.is_some()
            }
            InputEvent::ScrollLines(d) => {
                self.frame_scroll_lines += d;
                self.scroll_target.is_some()
            }
            InputEvent::Zoom(f) => {
                self.frame_zoom_delta *= f;
                self.scroll_target.is_some()
            }
            InputEvent::KeyDown { key, repeat } => {
                self.frame_keys.push(KeyPress {
                    key,
                    mods: self.modifiers,
                    repeat,
                });
                true
            }
            InputEvent::Text(chunk) => {
                self.frame_text.push_str(chunk.as_str());
                true
            }
            InputEvent::ModifiersChanged(m) => {
                self.modifiers = m;
                true
            }
        };
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
            cap.frame_drag_started = None;
        }
        self.frame_scroll_pixels = Vec2::ZERO;
        self.frame_scroll_lines = Vec2::ZERO;
        self.frame_zoom_delta = 1.0;
        self.frame_keys.clear();
        self.frame_text.clear();
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
                && !cascades.by_id.contains_key(&a)
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
            && !cascades.by_id.contains_key(&focused)
        {
            self.focused = None;
        }
        if let Some(p) = self.pointer_pos {
            let hits = cascades.hit_test_pair(p, Sense::hovers, Sense::scrolls);
            self.hovered = hits.hover;
            self.scroll_target = hits.scroll;
        } else {
            self.hovered = None;
            self.scroll_target = None;
        }
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
    /// scroll hit-target; otherwise `1.0`. Pinch ingest is unconditional
    /// (touch already disambiguates intent), so widgets get the raw
    /// multiplicative factor regardless of `ZoomConfig::modifier`.
    pub(crate) fn zoom_delta_for(&self, id: WidgetId) -> f32 {
        if self.scroll_target == Some(id) {
            self.frame_zoom_delta
        } else {
            1.0
        }
    }

    /// Returns the cumulative drag delta (pointer pos minus press pos)
    /// when `id` is the actively-captured widget and both positions are
    /// known. Rect-independent — the pointer can leave the widget's
    /// rect mid-drag and the delta keeps tracking. `None` when `id`
    /// isn't active or the pointer has left the surface.
    pub(crate) fn drag_delta(&self, id: WidgetId) -> Option<Vec2> {
        let cap = self.capture(PointerButton::Left);
        if cap.active != Some(id) {
            return None;
        }
        Some(self.pointer_pos? - cap.press_pos?)
    }

    pub(crate) fn response_for(&self, id: WidgetId, cascades: &Cascades) -> ResponseState {
        let entry = cascades
            .by_id
            .get(&id)
            .map(|&i| &cascades.entries[i as usize]);
        let rect = entry.map(|e| e.rect);
        // Cascade flattens parent-disabled into each entry, so this is
        // the **effective** ancestor-or-self disabled — one frame stale.
        // Widgets that need lag-free self-toggle response merge their
        // own `element.disabled` on top after calling.
        let disabled = entry.is_some_and(|e| e.disabled);
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
        let drag_delta = if me_left_captured && left.drag_latched {
            self.drag_delta(id)
        } else {
            None
        };
        let drag_started = left.frame_drag_started == Some(id);

        ResponseState {
            rect,
            hovered,
            pressed,
            clicked,
            secondary_clicked,
            disabled,
            focused,
            drag_delta,
            drag_started,
        }
    }
}

#[cfg(test)]
mod tests;
