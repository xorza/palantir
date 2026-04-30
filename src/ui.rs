use crate::input::{InputEvent, InputState, PointerButton, PointerState, ResponseState};
use crate::primitives::{Rect, Style, WidgetId};
use crate::shape::Shape;
use crate::tree::{LayoutKind, NodeId, Tree};
use glam::Vec2;
use std::collections::HashMap;
use winit::event::{ElementState, MouseButton, WindowEvent};

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// but keeps input state and last-frame response data.
pub struct Ui {
    pub tree: Tree,
    parents: Vec<NodeId>,
    root: Option<NodeId>,

    #[cfg(debug_assertions)]
    seen_ids: HashMap<WidgetId, NodeId>,

    input: InputState,

    /// Per-id response data produced by the most recent `end_frame`. Widgets read this
    /// during the *next* frame's `build_ui` to populate their `Response`.
    prev_responses: HashMap<WidgetId, ResponseState>,

    /// Widget capturing the pointer (currently held). Survives across frames until release.
    active: Option<WidgetId>,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            tree: Tree::new(),
            parents: Vec::new(),
            root: None,
            #[cfg(debug_assertions)]
            seen_ids: HashMap::new(),
            input: InputState::default(),
            prev_responses: HashMap::new(),
            active: None,
        }
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
        #[cfg(debug_assertions)]
        self.seen_ids.clear();
    }

    /// Process this frame's input events against the just-arranged tree, producing
    /// `prev_responses` for the next frame's `build_ui`. Call after `layout::run`.
    pub fn end_frame(&mut self) {
        let events = std::mem::take(&mut self.input.events);
        let mut next: HashMap<WidgetId, ResponseState> = HashMap::new();

        for ev in events {
            match ev {
                InputEvent::PointerMoved(p) => self.input.pointer.pos = Some(p),
                InputEvent::PointerLeft => self.input.pointer.pos = None,
                InputEvent::PointerPressed(PointerButton::Left) => {
                    if let Some(p) = self.input.pointer.pos {
                        let hit = self.hit_test(p);
                        self.active = hit;
                        if let Some(id) = hit {
                            next.entry(id).or_default().pressed = true;
                        }
                    }
                }
                InputEvent::PointerReleased(PointerButton::Left) => {
                    let hit = self.input.pointer.pos.and_then(|p| self.hit_test(p));
                    if let Some(active) = self.active.take()
                        && self.exists(active)
                        && hit == Some(active)
                    {
                        next.entry(active).or_default().clicked = true;
                    }
                }
                _ => {}
            }
        }

        // Active widget remains pressed across frames until release.
        if let Some(active) = self.active {
            if self.exists(active) {
                next.entry(active).or_default().pressed = true;
            } else {
                self.active = None;
            }
        }

        // Hover: active overrides (mouse capture); else topmost under pointer.
        let hovered = if let Some(active) = self.active {
            Some(active)
        } else {
            self.input.pointer.pos.and_then(|p| self.hit_test(p))
        };
        if let Some(id) = hovered {
            next.entry(id).or_default().hovered = true;
        }

        // Attach this-frame rects.
        for (id, state) in next.iter_mut() {
            state.rect = self.rect_for(*id);
        }

        self.prev_responses = next;
    }

    /// Forward a winit `WindowEvent` into the input queue. Convenience for typical apps;
    /// non-winit backends can call the lower-level setters directly (not yet exposed).
    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.input.events.push(InputEvent::PointerMoved(Vec2::new(
                    position.x as f32,
                    position.y as f32,
                )));
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.events.push(InputEvent::PointerLeft);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return,
                };
                let ev = match state {
                    ElementState::Pressed => InputEvent::PointerPressed(b),
                    ElementState::Released => InputEvent::PointerReleased(b),
                };
                self.input.events.push(ev);
            }
            _ => {}
        }
    }

    pub fn pointer(&self) -> PointerState {
        self.input.pointer
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.prev_responses.get(&id).copied().unwrap_or_default()
    }

    pub fn root(&self) -> NodeId {
        self.root
            .expect("no root pushed yet — open a node before any other ops")
    }

    pub(crate) fn node(
        &mut self,
        id: WidgetId,
        style: Style,
        layout: LayoutKind,
        f: impl FnOnce(&mut Ui),
    ) -> NodeId {
        let parent = self.parents.last().copied();
        let node = self.tree.push_node(id, style, layout, parent);
        #[cfg(debug_assertions)]
        if let Some(prev) = self.seen_ids.insert(id, node) {
            tracing::warn!(
                ?id, ?node, first_seen = ?prev,
                "WidgetId collision — use `with_id(...)` to disambiguate"
            );
        }
        if self.root.is_none() {
            self.root = Some(node);
        }
        self.parents.push(node);
        f(self);
        self.parents.pop();
        node
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        let node = *self
            .parents
            .last()
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }

    fn hit_test(&self, pos: Vec2) -> Option<WidgetId> {
        // Reverse declaration order ≈ topmost-first under our pre-order paint walk.
        // Bounding-rect only for v1; per-node `HitShape` lands later.
        for node in self.tree.nodes.iter().rev() {
            if node.rect.contains(pos) {
                return Some(node.id);
            }
        }
        None
    }

    fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.tree.nodes.iter().find(|n| n.id == id).map(|n| n.rect)
    }

    fn exists(&self, id: WidgetId) -> bool {
        self.tree.nodes.iter().any(|n| n.id == id)
    }
}
