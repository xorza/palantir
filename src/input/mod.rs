pub(crate) mod keyboard;
pub(crate) mod sense;
pub(crate) mod shortcut;

use crate::forest::widget_id::WidgetId;
use crate::input::keyboard::{
    Key, KeyPress, Modifiers, TextChunk, key_from_winit, modifiers_from_winit,
};
use crate::input::sense::{DRAG_THRESHOLD, Sense};
use crate::primitives::rect::Rect;
use crate::ui::cascade::Cascades;
use glam::Vec2;
use rustc_hash::FxHashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)] // Middle reserved for v2.
pub enum PointerButton {
    Left,
    Right,
    Middle,
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
    /// Translate a winit `WindowEvent` into a palantir input event.
    /// `scale_factor` divides physical pointer coordinates so that the produced
    /// `PointerMoved` is in logical pixels (matches the units layout works in).
    /// Returns `None` for events we don't currently consume.
    pub fn from_winit(event: &winit::event::WindowEvent, scale_factor: f32) -> Option<Self> {
        use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let s = scale_factor.max(f32::EPSILON);
                Some(InputEvent::PointerMoved(Vec2::new(
                    position.x as f32 / s,
                    position.y as f32 / s,
                )))
            }
            WindowEvent::CursorLeft { .. } => Some(InputEvent::PointerLeft),
            WindowEvent::MouseInput { state, button, .. } => {
                let pb = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return None,
                };
                Some(match state {
                    ElementState::Pressed => InputEvent::PointerPressed(pb),
                    ElementState::Released => InputEvent::PointerReleased(pb),
                })
            }
            // Convert to "positive delta = pan offset forward" so widgets can
            // do `offset += delta` directly. winit reports +y when the wheel
            // rotates *away* from the user (scroll up) and +x when it rotates
            // / swipes right (reveal content to the right means panning
            // *into* it, i.e. content shifts left); flip both so positive
            // means "advance the scroll offset."
            WindowEvent::PinchGesture { delta, .. } => Some(InputEvent::Zoom(1.0 + *delta as f32)),
            WindowEvent::MouseWheel { delta, .. } => Some(match *delta {
                MouseScrollDelta::LineDelta(x, y) => InputEvent::ScrollLines(Vec2::new(-x, -y)),
                MouseScrollDelta::PixelDelta(p) => {
                    let s = scale_factor.max(f32::EPSILON);
                    InputEvent::ScrollPixels(Vec2::new(-p.x as f32 / s, -p.y as f32 / s))
                }
            }),
            WindowEvent::KeyboardInput { event, .. } => match event.state {
                // Releases are dropped — no consumer needs them yet.
                ElementState::Pressed => Some(InputEvent::KeyDown {
                    key: key_from_winit(&event.logical_key),
                    repeat: event.repeat,
                }),
                ElementState::Released => None,
            },
            // IME commit: what the user *meant* to insert after composition
            // (dead keys, multi-keystroke CJK input). Strings longer than
            // the inline buffer are dropped — IME commits over 15 bytes
            // are rare enough that we'd rather see them surface as a bug
            // than silently truncate at a non-grapheme boundary.
            WindowEvent::Ime(winit::event::Ime::Commit(s)) => {
                TextChunk::new(s).map(InputEvent::Text)
            }
            WindowEvent::ModifiersChanged(m) => Some(InputEvent::ModifiersChanged(
                modifiers_from_winit(&m.state()),
            )),
            _ => None,
        }
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
    pub(crate) active: Option<WidgetId>,
    hovered: Option<WidgetId>,
    /// Topmost `Sense::SCROLL` widget under the pointer, recomputed
    /// whenever the pointer moves and at `post_record`. The scroll widget
    /// matching this id consumes [`Self::frame_scroll_pixels`].
    scroll_target: Option<WidgetId>,
    /// Pointer position captured at the moment of the press that set
    /// `active`. Subtracted from the current pointer position to give
    /// drag widgets a rect-independent delta — the pointer can leave
    /// the originating widget mid-drag and the delta keeps tracking.
    /// Cleared on release / capture eviction.
    press_pos: Option<Vec2>,
    /// Set once the pointer has travelled at least [`DRAG_THRESHOLD`]
    /// from `press_pos` while `active.is_some()`. Held for the press
    /// lifetime — sticky even if the pointer drifts back inside the
    /// threshold. Cleared on release / active-eviction. Doubles as the
    /// "suppress click on release" bit (PointerReleased reads it).
    pub(crate) drag_latched: bool,
    /// One-frame edge: set to the active widget on the move event that
    /// flips `drag_latched` from `false` to `true`; cleared by
    /// `drain_per_frame_queues`, by release, and by active-eviction.
    /// Read by `Ui::drag_started` to expose a single-frame "drag began"
    /// signal to widgets without forcing them to compare last/this-frame
    /// state.
    pub(crate) frame_drag_started: Option<WidgetId>,
    frame_clicks: FxHashSet<WidgetId>,
    /// Right-button capture, parallel to `active`. Press latches; a
    /// release on the same id (no drag latch — secondary doesn't drive
    /// drags today) inserts into `frame_secondary_clicks`. Independent
    /// from `active` so a left-drag in progress doesn't block a
    /// right-click and vice-versa.
    active_secondary: Option<WidgetId>,
    press_pos_secondary: Option<Vec2>,
    frame_secondary_clicks: FxHashSet<WidgetId>,
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
    pub focus_policy: FocusPolicy,
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
            active: None,
            hovered: None,
            scroll_target: None,
            press_pos: None,
            drag_latched: false,
            frame_drag_started: None,
            frame_clicks: FxHashSet::default(),
            active_secondary: None,
            press_pos_secondary: None,
            frame_secondary_clicks: FxHashSet::default(),
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

    /// Feed a palantir-native input event. Hit-tests against the
    /// frozen `Cascades` from this frame's most recent run. Returns an
    /// [`InputDelta`] hosts use to decide whether to request a redraw —
    /// a `PointerMoved` over a non-hover-reactive surface (no active
    /// capture, no hover/scroll target change) leaves
    /// `requests_repaint` false so the frame can be skipped entirely.
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
                if !self.drag_latched
                    && self.active.is_some()
                    && let Some(press) = self.press_pos
                    && (p - press).length() >= DRAG_THRESHOLD
                {
                    self.drag_latched = true;
                    self.frame_drag_started = self.active;
                    self.frame_had_action = true;
                }
                self.recompute_hover(cascades);
                self.recompute_scroll_target(cascades);
                self.hovered != prev_hover
                    || self.scroll_target != prev_scroll
                    || self.active.is_some()
            }
            InputEvent::PointerLeft => {
                let observable =
                    self.hovered.is_some() || self.scroll_target.is_some() || self.active.is_some();
                self.pointer_pos = None;
                self.hovered = None;
                self.scroll_target = None;
                observable
            }
            InputEvent::PointerPressed(PointerButton::Left) => {
                // Press hits the topmost *clickable* widget — hover-only widgets
                // are transparent to presses even though they show as hovered.
                self.active = self
                    .pointer_pos
                    .and_then(|p| cascades.hit_test(p, Sense::clicks));
                self.press_pos = self.active.and(self.pointer_pos);
                // Focus updates on a separate hit-test: focusability is
                // orthogonal to clickability (clicking a Button shouldn't
                // steal focus from a TextEdit). Press on empty surface or
                // on a non-focusable widget defers to `focus_policy`.
                let focus_hit = self
                    .pointer_pos
                    .and_then(|p| cascades.hit_test_focusable(p));
                match (focus_hit, self.focus_policy) {
                    (Some(id), _) => self.focused = Some(id),
                    (None, FocusPolicy::ClearOnMiss) => self.focused = None,
                    (None, FocusPolicy::PreserveOnMiss) => {} // hold focus
                }
                true
            }
            InputEvent::PointerReleased(PointerButton::Left) => {
                if let Some(a) = self.active.take() {
                    let hit = self
                        .pointer_pos
                        .and_then(|p| cascades.hit_test(p, Sense::clicks));
                    if hit == Some(a) && !self.drag_latched {
                        self.frame_clicks.insert(a);
                    }
                }
                self.clear_capture();
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
            InputEvent::PointerPressed(PointerButton::Right) => {
                self.active_secondary = self
                    .pointer_pos
                    .and_then(|p| cascades.hit_test(p, Sense::clicks));
                self.press_pos_secondary = self.active_secondary.and(self.pointer_pos);
                true
            }
            InputEvent::PointerReleased(PointerButton::Right) => {
                if let Some(a) = self.active_secondary.take() {
                    let hit = self
                        .pointer_pos
                        .and_then(|p| cascades.hit_test(p, Sense::clicks));
                    if hit == Some(a) {
                        self.frame_secondary_clicks.insert(a);
                    }
                }
                self.press_pos_secondary = None;
                true
            }
            // Middle: not yet wired through to widgets. Silently drop.
            InputEvent::PointerPressed(_) | InputEvent::PointerReleased(_) => false,
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
        self.frame_clicks.clear();
        self.frame_secondary_clicks.clear();
        self.frame_drag_started = None;
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
        if let Some(active) = self.active
            && !cascades.by_id.contains_key(&active)
        {
            self.active = None;
            self.clear_capture();
        }
        if let Some(a) = self.active_secondary
            && !cascades.by_id.contains_key(&a)
        {
            self.active_secondary = None;
            self.press_pos_secondary = None;
        }
        // Focus eviction: same model as the active-capture eviction
        // above. A focused widget that vanished from the tree (was not
        // recorded this frame) drops focus to None; otherwise next
        // frame's keystrokes would route to a ghost.
        if let Some(focused) = self.focused
            && !cascades.by_id.contains_key(&focused)
        {
            self.focused = None;
        }
        self.recompute_hover(cascades);
        self.recompute_scroll_target(cascades);
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
        if self.active != Some(id) {
            return None;
        }
        let press = self.press_pos?;
        let now = self.pointer_pos?;
        Some(now - press)
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
        let me_under_pointer = self.hovered == Some(id);
        let me_captured = self.active == Some(id);
        let nothing_captured = self.active.is_none();

        let pressed = me_captured && me_under_pointer;
        let hovered = me_under_pointer && (nothing_captured || me_captured);
        let clicked = self.frame_clicks.contains(&id);
        let secondary_clicked = self.frame_secondary_clicks.contains(&id);
        let focused = self.focused == Some(id);
        let drag_delta = if me_captured && self.drag_latched {
            self.drag_delta(id)
        } else {
            None
        };
        let drag_started = self.frame_drag_started == Some(id);

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

    /// Clear all drag/press-related state. `active` is the caller's
    /// responsibility (Released takes it; eviction clears it). Called
    /// both on left-release and on cascade-evict of the active widget.
    fn clear_capture(&mut self) {
        self.press_pos = None;
        self.drag_latched = false;
        self.frame_drag_started = None;
    }

    fn recompute_hover(&mut self, cascades: &Cascades) {
        self.hovered = self
            .pointer_pos
            .and_then(|p| cascades.hit_test(p, Sense::hovers));
    }

    fn recompute_scroll_target(&mut self, cascades: &Cascades) {
        self.scroll_target = self
            .pointer_pos
            .and_then(|p| cascades.hit_test(p, Sense::scrolls));
    }
}

#[cfg(test)]
mod tests;
