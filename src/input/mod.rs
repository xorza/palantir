use crate::primitives::{Rect, Sense, WidgetId};
use crate::tree::Tree;
use glam::Vec2;
use std::collections::HashSet;

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

/// All UI-input bookkeeping that lives across frames: pointer position,
/// active (captured) widget, the topmost widget under the pointer, last-frame's
/// rect cache, and clicks emitted this frame.
///
/// Owned by `Ui` but factored here so the input state machine is self-contained,
/// testable in isolation, and reusable by non-winit backends.
pub struct InputState {
    pointer: PointerState,
    active: Option<WidgetId>,
    hovered: Option<WidgetId>,
    /// `(WidgetId, Rect, Sense)` from last frame's tree, in pre-order paint order.
    /// Reverse iter = topmost-first for hit-testing. `Sense` filters out
    /// non-interactive widgets so clicks pass through containers.
    last_rects: Vec<(WidgetId, Rect, Sense)>,
    clicked_this_frame: HashSet<WidgetId>,
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
            last_rects: Vec::new(),
            clicked_this_frame: HashSet::new(),
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
                self.active = self.hovered;
            }
            InputEvent::PointerReleased(PointerButton::Left) => {
                if let Some(a) = self.active.take()
                    && self.hovered == Some(a)
                {
                    self.clicked_this_frame.insert(a);
                }
            }
            // Right/Middle: not yet wired through to widgets. Silently drop.
            InputEvent::PointerPressed(_) | InputEvent::PointerReleased(_) => {}
        }
    }

    /// Rebuild last-frame rects from the just-arranged tree, recompute hover,
    /// drop transient per-frame flags. Call after `layout::run`.
    pub(crate) fn end_frame(&mut self, tree: &Tree) {
        self.last_rects.clear();
        for node in &tree.nodes {
            self.last_rects.push((node.id, node.rect, node.sense));
        }
        self.clicked_this_frame.clear();

        if let Some(active) = self.active
            && !self.contains_id(active)
        {
            self.active = None;
        }
        self.recompute_hover();
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let rect = self.rect_for(id);
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
        self.hovered = self.pointer.pos.and_then(|p| self.hit_test(p));
    }

    /// Reverse-iter `last_rects` → topmost-first under our pre-order paint walk.
    /// Skips widgets with `Sense::NONE` so clicks/hover pass through containers
    /// and decorative overlays. Bounding-rect only for v1; per-node `HitShape`
    /// lands later.
    fn hit_test(&self, pos: Vec2) -> Option<WidgetId> {
        for (id, rect, sense) in self.last_rects.iter().rev() {
            if sense.is_interactive() && rect.contains(pos) {
                return Some(*id);
            }
        }
        None
    }

    fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.last_rects
            .iter()
            .find_map(|(i, r, _)| (*i == id).then_some(*r))
    }

    fn contains_id(&self, id: WidgetId) -> bool {
        self.last_rects.iter().any(|(i, _, _)| *i == id)
    }
}

#[cfg(test)]
mod tests;
