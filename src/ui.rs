use crate::input::{PointerButton, PointerState, ResponseState};
use crate::primitives::{Rect, Style, WidgetId};
use crate::shape::Shape;
use crate::tree::{LayoutKind, NodeId, Tree};
use glam::Vec2;
use std::collections::{HashMap, HashSet};
use winit::event::{ElementState, MouseButton, WindowEvent};

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// but keeps input state and last-frame hit-test data.
pub struct Ui {
    pub tree: Tree,
    parents: Vec<NodeId>,
    root: Option<NodeId>,

    #[cfg(debug_assertions)]
    seen_ids: HashMap<WidgetId, NodeId>,

    pointer: PointerState,

    /// Widget capturing the pointer (mouse pressed inside it). Cleared on release.
    active: Option<WidgetId>,

    /// Last-frame rects keyed by `WidgetId`, used by `handle_event` to hit-test
    /// presses/releases against what the user was looking at when they clicked.
    last_rects: HashMap<WidgetId, Rect>,

    /// Reverse paint order over last frame (topmost-first). Each entry is
    /// `(WidgetId, Rect)`. Used by `hit_test` to pick the deepest hit.
    last_order: Vec<(WidgetId, Rect)>,

    /// Clicks emitted by the most recent press→release pair, consumed by widget reads.
    /// Cleared at `end_frame` so they're only visible during one `build_ui`.
    clicked_this_frame: HashSet<WidgetId>,
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
            pointer: PointerState::default(),
            active: None,
            last_rects: HashMap::new(),
            last_order: Vec::new(),
            clicked_this_frame: HashSet::new(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
        #[cfg(debug_assertions)]
        self.seen_ids.clear();
    }

    /// Rebuild last-frame state from the just-arranged tree, and drop transient
    /// per-frame flags (`clicked_this_frame`). Call after `layout::run`.
    pub fn end_frame(&mut self) {
        self.last_rects.clear();
        self.last_order.clear();
        for node in &self.tree.nodes {
            self.last_rects.insert(node.id, node.rect);
            self.last_order.push((node.id, node.rect));
        }
        self.clicked_this_frame.clear();

        // Drop active if the widget no longer exists in the tree.
        if let Some(active) = self.active
            && !self.last_rects.contains_key(&active)
        {
            self.active = None;
        }
    }

    /// Forward a winit `WindowEvent` and update input state eagerly. Hit-tests run
    /// against last-frame's rects so visuals respond instantly.
    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.pos = Some(Vec2::new(position.x as f32, position.y as f32));
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer.pos = None;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return,
                };
                if b != PointerButton::Left {
                    return;
                }
                match state {
                    ElementState::Pressed => {
                        self.active = self.pointer.pos.and_then(|p| self.hit_test(p));
                    }
                    ElementState::Released => {
                        let hit = self.pointer.pos.and_then(|p| self.hit_test(p));
                        if let Some(a) = self.active.take()
                            && hit == Some(a)
                        {
                            self.clicked_this_frame.insert(a);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let rect = self.last_rects.get(&id).copied();
        let pressed = self.active == Some(id);
        let hovered = if pressed {
            true
        } else if self.active.is_none() {
            match (self.pointer.pos, rect) {
                (Some(p), Some(r)) => r.contains(p),
                _ => false,
            }
        } else {
            false
        };
        let clicked = self.clicked_this_frame.contains(&id);
        ResponseState {
            rect,
            hovered,
            pressed,
            clicked,
        }
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

    /// Reverse declaration order ≈ topmost-first under our pre-order paint walk.
    /// Bounding-rect only for v1; per-node `HitShape` lands later.
    fn hit_test(&self, pos: Vec2) -> Option<WidgetId> {
        for (id, rect) in self.last_order.iter().rev() {
            if rect.contains(pos) {
                return Some(*id);
            }
        }
        None
    }
}
