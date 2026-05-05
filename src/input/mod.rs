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
            // Convert to "positive y = scroll content down" so widgets can
            // do `offset += delta_y` directly. winit reports +y when the
            // wheel rotates *away* from the user (scroll up); flip it.
            WindowEvent::MouseWheel { delta, .. } => Some(match *delta {
                MouseScrollDelta::LineDelta(x, y) => {
                    InputEvent::Scroll(Vec2::new(x, -y) * SCROLL_LINE_PIXELS)
                }
                MouseScrollDelta::PixelDelta(p) => {
                    let s = scale_factor.max(f32::EPSILON);
                    InputEvent::Scroll(Vec2::new(p.x as f32 / s, -p.y as f32 / s))
                }
            }),
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
    clicked_this_frame: FxHashSet<WidgetId>,
    /// Wheel/touchpad delta accumulated this frame (logical px). Cleared
    /// in [`Self::end_frame`]. Read by scroll widgets at record time.
    pub(crate) frame_scroll_delta: Vec2,
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
            clicked_this_frame: FxHashSet::default(),
            frame_scroll_delta: Vec2::ZERO,
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
            }
            InputEvent::Scroll(d) => {
                self.frame_scroll_delta += d;
            }
            // Right/Middle: not yet wired through to widgets. Silently drop.
            InputEvent::PointerPressed(_) | InputEvent::PointerReleased(_) => {}
        }
    }

    /// Recompute hover, drop transient per-frame flags, evict captured
    /// widgets that disappeared from the tree. Call after
    /// `Cascades::run` (whose result `cascades` is passed here).
    pub(crate) fn end_frame(&mut self, cascades: &CascadeResult) {
        self.clicked_this_frame.clear();
        self.frame_scroll_delta = Vec2::ZERO;
        if let Some(active) = self.active
            && !cascades.by_id.contains_key(&active)
        {
            self.active = None;
        }
        self.recompute_hover(cascades);
        self.recompute_scroll_target(cascades);
    }

    /// Returns this frame's scroll delta if `id` is the current scroll
    /// hit-target; otherwise `Vec2::ZERO`. Scroll widgets call this at
    /// record time to claim wheel/touchpad input.
    #[allow(dead_code)] // wired to Scroll widget in step 3
    pub(crate) fn scroll_delta_for(&self, id: WidgetId) -> Vec2 {
        if self.scroll_target == Some(id) {
            self.frame_scroll_delta
        } else {
            Vec2::ZERO
        }
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
