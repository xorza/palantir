mod hit_index;

use crate::cascade::Cascades;
use crate::layout::LayoutResult;
use crate::primitives::{Rect, Sense, WidgetId};
use crate::tree::Tree;
use glam::Vec2;
use hit_index::HitIndex;
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
}

impl InputEvent {
    /// Translate a winit `WindowEvent` into a palantir input event.
    /// `scale_factor` divides physical pointer coordinates so that the produced
    /// `PointerMoved` is in logical pixels (matches the units layout works in).
    /// Returns `None` for events we don't currently consume.
    pub fn from_winit(event: &winit::event::WindowEvent, scale_factor: f32) -> Option<Self> {
        use winit::event::{ElementState, MouseButton, WindowEvent};
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
    clicked_this_frame: FxHashSet<WidgetId>,
    /// Pre-order rect/sense snapshot of the last arranged tree. Rebuilt every
    /// `end_frame`; queried by `on_input` and `response_for`.
    hit_index: HitIndex,
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
            clicked_this_frame: FxHashSet::default(),
            hit_index: HitIndex::new(),
        }
    }

    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    /// Feed a palantir-native input event.
    pub fn on_input(&mut self, event: InputEvent) {
        match event {
            InputEvent::PointerMoved(p) => {
                self.pointer.pos = Some(p);
                self.recompute_hover();
            }
            InputEvent::PointerLeft => {
                self.pointer.pos = None;
                self.hovered = None;
            }
            InputEvent::PointerPressed(PointerButton::Left) => {
                // Press hits the topmost *clickable* widget — hover-only widgets
                // are transparent to presses even though they show as hovered.
                self.active = self
                    .pointer
                    .pos
                    .and_then(|p| self.hit_index.hit_test(p, Sense::click));
            }
            InputEvent::PointerReleased(PointerButton::Left) => {
                if let Some(a) = self.active.take() {
                    let hit = self
                        .pointer
                        .pos
                        .and_then(|p| self.hit_index.hit_test(p, Sense::click));
                    if hit == Some(a) {
                        self.clicked_this_frame.insert(a);
                    }
                }
            }
            // Right/Middle: not yet wired through to widgets. Silently drop.
            InputEvent::PointerPressed(_) | InputEvent::PointerReleased(_) => {}
        }
    }

    /// Rebuild last-frame rects from the just-arranged tree, recompute hover,
    /// drop transient per-frame flags. Call after layout. The cascade
    /// resolution itself lives in [`Cascades`]; `HitIndex::rebuild` just
    /// flattens its output to the per-id form hit-testing wants.
    pub(crate) fn end_frame(&mut self, tree: &Tree, layout: &LayoutResult, cascades: &Cascades) {
        self.hit_index.rebuild(tree, layout, cascades);
        self.clicked_this_frame.clear();
        if let Some(active) = self.active
            && !self.hit_index.contains_id(active)
        {
            self.active = None;
        }
        self.recompute_hover();
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let rect = self.hit_index.rect_for(id);
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

    fn recompute_hover(&mut self) {
        self.hovered = self
            .pointer
            .pos
            .and_then(|p| self.hit_index.hit_test(p, Sense::hover));
    }
}

#[cfg(test)]
mod tests;
