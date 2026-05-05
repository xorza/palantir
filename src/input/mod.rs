pub(crate) mod keyboard;

use crate::input::keyboard::{
    Key, KeyPress, Modifiers, TextChunk, key_from_winit, modifiers_from_winit,
};
use crate::layout::types::sense::Sense;
use crate::primitives::rect::Rect;
use crate::tree::widget_id::WidgetId;
use crate::ui::cascade::CascadeResult;
use glam::Vec2;
use rustc_hash::FxHashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)] // Right/Middle reserved for v2.
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
/// Convert from winit via [`InputEvent::from_winit`] (typical apps use
/// `Ui::handle_event` which does the conversion + dispatch in one call).
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
    /// Scroll-wheel / touchpad delta in logical pixels. Positive `y`
    /// means the user wants content to scroll *down* (a scroll widget
    /// should add to its vertical offset). Multiple events in one frame
    /// accumulate into [`InputState::frame_scroll_delta`].
    Scroll(Vec2),
    /// Logical key was pressed. `repeat` reflects OS-level key repeat
    /// (held keys re-emit). Modifier state isn't carried on the event;
    /// consumers read the latest [`Modifiers`] from `InputState` (wired
    /// in step 2 of the TextEdit plan).
    KeyDown {
        key: Key,
        repeat: bool,
    },
    /// Logical key was released.
    KeyUp {
        key: Key,
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

/// Logical pixels per `MouseScrollDelta::LineDelta` line. Matches the
/// winit / egui convention; text-aware step is a future polish.
const SCROLL_LINE_PIXELS: f32 = 40.0;

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
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => Some(match state {
                ElementState::Pressed => InputEvent::PointerPressed(PointerButton::Left),
                ElementState::Released => InputEvent::PointerReleased(PointerButton::Left),
            }),
            // Convert to "positive delta = pan offset forward" so widgets can
            // do `offset += delta` directly. winit reports +y when the wheel
            // rotates *away* from the user (scroll up) and +x when it rotates
            // / swipes right (reveal content to the right means panning
            // *into* it, i.e. content shifts left); flip both so positive
            // means "advance the scroll offset."
            WindowEvent::MouseWheel { delta, .. } => Some(match *delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    InputEvent::Scroll(Vec2::new(-x, -y) * SCROLL_LINE_PIXELS)
                }
                MouseScrollDelta::PixelDelta(p) => {
                    let s = scale_factor.max(f32::EPSILON);
                    InputEvent::Scroll(Vec2::new(-p.x as f32 / s, -p.y as f32 / s))
                }
            }),
            WindowEvent::KeyboardInput { event, .. } => {
                let key = key_from_winit(&event.logical_key);
                Some(match event.state {
                    ElementState::Pressed => InputEvent::KeyDown {
                        key,
                        repeat: event.repeat,
                    },
                    ElementState::Released => InputEvent::KeyUp { key },
                })
            }
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

#[derive(Default, Clone, Copy, Debug)]
pub struct PointerState {
    pub pos: Option<Vec2>,
}

/// Snapshot of one widget's interaction state for the current frame.
/// `rect` is the widget's last-frame logical-pixel rect (`None` on first frame).
#[derive(Default, Clone, Copy, Debug)]
pub struct ResponseState {
    pub rect: Option<Rect>,
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
}

/// Live input state machine: the things that survive across input events
/// independently of whether the tree was rebuilt. Per-frame rebuilt data
/// (last-frame rects, cascade scratch) lives in [`HitIndex`].
pub struct InputState {
    pointer: PointerState,
    active: Option<WidgetId>,
    hovered: Option<WidgetId>,
    /// Topmost `Sense::Scroll` widget under the pointer, recomputed
    /// whenever the pointer moves and at `end_frame`. The scroll widget
    /// matching this id consumes [`Self::frame_scroll_delta`].
    scroll_target: Option<WidgetId>,
    /// Pointer position captured at the moment of the press that set
    /// `active`. Subtracted from the current pointer position to give
    /// drag widgets a rect-independent delta — the pointer can leave
    /// the originating widget mid-drag and the delta keeps tracking.
    /// Cleared on release / capture eviction.
    press_pos: Option<Vec2>,
    clicked_this_frame: FxHashSet<WidgetId>,
    /// Wheel/touchpad delta accumulated this frame (logical px). Cleared
    /// in [`Self::end_frame`]. Read by scroll widgets at record time.
    pub(crate) frame_scroll_delta: Vec2,
    /// Keystrokes accumulated this frame, awaiting drain by the focused
    /// widget at record time. Capacity-retained across frames; cleared
    /// in [`Self::end_frame`] regardless of whether anything consumed
    /// them. Step-3 focus dispatch reads this; today nothing does.
    pub(crate) frame_keys: Vec<KeyPress>,
    /// Committed text accumulated this frame from `Text(chunk)` events.
    /// One `String` rather than `Vec<TextChunk>` so consumers can splice
    /// directly into their buffer without re-concatenating; chunks are
    /// already grapheme-aligned at the translation boundary so byte
    /// concatenation is safe. Capacity-retained, cleared in `end_frame`.
    pub(crate) frame_text: String,
    /// Latest modifier-key snapshot. Persists across `end_frame` —
    /// modifier *state* is not a per-frame thing the way keystrokes
    /// are. Updated only on `ModifiersChanged` events.
    pub(crate) modifiers: Modifiers,
    /// Currently focused widget, or `None`. Set on `PointerPressed(Left)`
    /// when the press lands on a focusable widget. Evicted in
    /// [`Self::end_frame`] when the focused widget vanishes from the
    /// tree (matches the per-id state map's eviction model). Read by
    /// keyboard consumers to decide whether to drain `frame_keys` /
    /// `frame_text` (step 5 of the TextEdit plan).
    pub(crate) focused: Option<WidgetId>,
    /// Press-on-non-focusable-widget behavior. See [`FocusPolicy`].
    pub focus_policy: FocusPolicy,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

impl InputState {
    pub fn new() -> Self {
        Self {
            pointer: PointerState::default(),
            active: None,
            hovered: None,
            scroll_target: None,
            press_pos: None,
            clicked_this_frame: FxHashSet::default(),
            frame_scroll_delta: Vec2::ZERO,
            frame_keys: Vec::new(),
            frame_text: String::new(),
            modifiers: Modifiers::NONE,
            focused: None,
            focus_policy: FocusPolicy::PreserveOnMiss,
        }
    }

    /// Feed a palantir-native input event. Hit-tests against the
    /// frozen `CascadeResult` from this frame's most recent run.
    pub(crate) fn on_input(&mut self, event: InputEvent, cascades: &CascadeResult) {
        match event {
            InputEvent::PointerMoved(p) => {
                self.pointer.pos = Some(p);
                self.recompute_hover(cascades);
                self.recompute_scroll_target(cascades);
            }
            InputEvent::PointerLeft => {
                self.pointer.pos = None;
                self.hovered = None;
                self.scroll_target = None;
            }
            InputEvent::PointerPressed(PointerButton::Left) => {
                // Press hits the topmost *clickable* widget — hover-only widgets
                // are transparent to presses even though they show as hovered.
                self.active = self
                    .pointer
                    .pos
                    .and_then(|p| cascades.hit_test(p, Sense::click));
                self.press_pos = self.active.and(self.pointer.pos);
                // Focus updates on a separate hit-test: focusability is
                // orthogonal to clickability (clicking a Button shouldn't
                // steal focus from a TextEdit). Press on empty surface or
                // on a non-focusable widget defers to `focus_policy`.
                let focus_hit = self
                    .pointer
                    .pos
                    .and_then(|p| cascades.hit_test_focusable(p));
                match (focus_hit, self.focus_policy) {
                    (Some(id), _) => self.focused = Some(id),
                    (None, FocusPolicy::ClearOnMiss) => self.focused = None,
                    (None, FocusPolicy::PreserveOnMiss) => {} // hold focus
                }
            }
            InputEvent::PointerReleased(PointerButton::Left) => {
                if let Some(a) = self.active.take() {
                    let hit = self
                        .pointer
                        .pos
                        .and_then(|p| cascades.hit_test(p, Sense::click));
                    if hit == Some(a) {
                        self.clicked_this_frame.insert(a);
                    }
                }
                self.press_pos = None;
            }
            InputEvent::Scroll(d) => {
                self.frame_scroll_delta += d;
            }
            // Right/Middle: not yet wired through to widgets. Silently drop.
            InputEvent::PointerPressed(_) | InputEvent::PointerReleased(_) => {}
            InputEvent::KeyDown { key, repeat } => {
                self.frame_keys.push(KeyPress {
                    key,
                    mods: self.modifiers,
                    repeat,
                });
            }
            InputEvent::Text(chunk) => {
                self.frame_text.push_str(chunk.as_str());
            }
            InputEvent::ModifiersChanged(m) => {
                self.modifiers = m;
            }
            // Releases aren't queued — editors consume presses, and a
            // release queue without a reader would invent state we
            // don't yet need.
            InputEvent::KeyUp { .. } => {}
        }
    }

    /// Recompute hover, drop transient per-frame flags, evict captured
    /// widgets that disappeared from the tree. Call after
    /// `Cascades::run` (whose result `cascades` is passed here).
    pub(crate) fn end_frame(&mut self, cascades: &CascadeResult) {
        self.clicked_this_frame.clear();
        self.frame_scroll_delta = Vec2::ZERO;
        // Capacity-retained — `Vec::clear` and `String::clear` keep the
        // backing buffer so steady-state typing stays alloc-free.
        self.frame_keys.clear();
        self.frame_text.clear();
        // `modifiers` deliberately persists: modifier state is a running
        // snapshot, not per-frame. Held shift across multiple frames must
        // stay `true`.
        if let Some(active) = self.active
            && !cascades.by_id.contains_key(&active)
        {
            self.active = None;
            self.press_pos = None;
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

    /// Returns this frame's scroll delta if `id` is the current scroll
    /// hit-target; otherwise `Vec2::ZERO`. Scroll widgets call this at
    /// record time to claim wheel/touchpad input.
    pub(crate) fn scroll_delta_for(&self, id: WidgetId) -> Vec2 {
        if self.scroll_target == Some(id) {
            self.frame_scroll_delta
        } else {
            Vec2::ZERO
        }
    }

    /// Returns the cumulative drag delta (pointer pos minus press pos)
    /// when `id` is the actively-captured widget and both positions are
    /// known. Rect-independent — the pointer can leave the widget's
    /// rect mid-drag and the delta keeps tracking. `None` when `id`
    /// isn't active or the pointer has left the surface.
    #[allow(dead_code)] // first consumer is the scrollbar widget (step 6)
    pub(crate) fn drag_delta(&self, id: WidgetId) -> Option<Vec2> {
        if self.active != Some(id) {
            return None;
        }
        let press = self.press_pos?;
        let now = self.pointer.pos?;
        Some(now - press)
    }

    pub(crate) fn response_for(&self, id: WidgetId, cascades: &CascadeResult) -> ResponseState {
        let rect = cascades
            .by_id
            .get(&id)
            .map(|&i| cascades.entries[i as usize].rect);
        let me_under_pointer = self.hovered == Some(id);
        let me_captured = self.active == Some(id);
        let nothing_captured = self.active.is_none();

        let pressed = me_captured && me_under_pointer;
        let hovered = me_under_pointer && (nothing_captured || me_captured);
        let clicked = self.clicked_this_frame.contains(&id);

        ResponseState {
            rect,
            hovered,
            pressed,
            clicked,
        }
    }

    fn recompute_hover(&mut self, cascades: &CascadeResult) {
        self.hovered = self
            .pointer
            .pos
            .and_then(|p| cascades.hit_test(p, Sense::hover));
    }

    fn recompute_scroll_target(&mut self, cascades: &CascadeResult) {
        self.scroll_target = self
            .pointer
            .pos
            .and_then(|p| cascades.hit_test(p, Sense::scroll));
    }
}

#[cfg(test)]
mod tests;
