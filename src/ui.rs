use crate::input::{PointerState, ResponseState};
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

    /// Topmost widget under the pointer this frame. Recomputed when pointer moves
    /// or when `last_rects` is rebuilt at `end_frame`.
    hovered: Option<WidgetId>,

    /// Last frame's `(WidgetId, Rect)` in pre-order paint order.
    /// Reverse iter = topmost-first for hit-testing. Linear scan for id lookup;
    /// fine at the widget counts we care about.
    last_rects: Vec<(WidgetId, Rect)>,

    /// Clicks emitted by the most recent press→release pair, consumed by widget reads.
    /// Cleared at `end_frame`.
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
            hovered: None,
            last_rects: Vec::new(),
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

    /// Rebuild last-frame rects + topmost cache from the just-arranged tree, and
    /// drop transient per-frame flags. Call after `layout::run`.
    pub fn end_frame(&mut self) {
        self.last_rects.clear();
        for node in &self.tree.nodes {
            self.last_rects.push((node.id, node.rect));
        }
        self.clicked_this_frame.clear();

        // Drop active if the widget no longer exists in the tree. Note: an unclicked
        // press whose widget then vanished is silently discarded, which is the
        // standard "fail-open on conditional rendering" rule.
        if let Some(active) = self.active
            && !self.contains_id(active)
        {
            self.active = None;
        }

        self.recompute_hover();
    }

    /// Forward a winit `WindowEvent` and update input state eagerly. Hit-tests run
    /// against last-frame's rects so visuals respond instantly.
    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.pos = Some(Vec2::new(position.x as f32, position.y as f32));
                self.recompute_hover();
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer.pos = None;
                self.hovered = None;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => {
                    self.active = self.hovered;
                }
                ElementState::Released => {
                    if let Some(a) = self.active.take()
                        && self.hovered == Some(a)
                    {
                        self.clicked_this_frame.insert(a);
                    }
                }
            },
            _ => {}
        }
    }

    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        let rect = self.rect_for(id);
        let me_under_pointer = self.hovered == Some(id);
        let me_captured = self.active == Some(id);
        let nothing_captured = self.active.is_none();

        // Pressed visual only while the cursor is over the captured widget — drag
        // off and the visual reverts; drag back and it returns. Standard button feel.
        let pressed = me_captured && me_under_pointer;
        // Hover suppressed when another widget owns the pointer.
        let hovered = me_under_pointer && (nothing_captured || me_captured);
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

    fn recompute_hover(&mut self) {
        self.hovered = match self.pointer.pos {
            Some(p) => self.hit_test(p),
            None => None,
        };
    }

    /// Reverse-iter `last_rects` → topmost-first under our pre-order paint walk
    /// (a widget appears after its parent, so the deepest match comes first).
    /// Bounding-rect only for v1; per-node `HitShape` lands later.
    fn hit_test(&self, pos: Vec2) -> Option<WidgetId> {
        for (id, rect) in self.last_rects.iter().rev() {
            if rect.contains(pos) {
                return Some(*id);
            }
        }
        None
    }

    fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.last_rects
            .iter()
            .find_map(|(i, r)| (*i == id).then_some(*r))
    }

    fn contains_id(&self, id: WidgetId) -> bool {
        self.last_rects.iter().any(|(i, _)| *i == id)
    }
}
