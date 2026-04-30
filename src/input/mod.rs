use crate::primitives::{Rect, Sense, TranslateScale, Visibility, WidgetId};
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

/// One widget's hit-test entry from last frame: identity, rect, sense.
/// Stored as the unit cell of `InputState::last_rects`.
#[derive(Clone, Copy, Debug)]
struct HitEntry {
    id: WidgetId,
    rect: Rect,
    sense: Sense,
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
    /// Last-frame's hit-test entries, in pre-order paint order.
    /// Reverse iter = topmost-first; `Sense` filters out non-interactive widgets
    /// so clicks pass through containers.
    last_rects: Vec<HitEntry>,
    clicked_this_frame: HashSet<WidgetId>,

    /// Per-node disabled cascade scratch. Reused frame-to-frame; cleared in
    /// `end_frame`.
    effective_disabled: Vec<bool>,
    /// Per-node visibility cascade scratch. `true` if this node is `Hidden`/
    /// `Collapsed` itself or has any such ancestor — i.e. invisible to input.
    effective_invisible: Vec<bool>,
    /// Per-node clip-rect cascade scratch (clip inherited by descendants), in
    /// SCREEN space — clips are accumulated *after* applying transforms, so
    /// they're directly compared with screen-space hit-test rects.
    clip_for_descendants: Vec<Option<Rect>>,
    /// Per-node cumulative transform that applies to descendants. A node's
    /// own rect uses the *parent's* entry; the node's own transform contributes
    /// only to descendants. Reused frame-to-frame.
    transform_for_descendants: Vec<TranslateScale>,
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
            effective_disabled: Vec::new(),
            effective_invisible: Vec::new(),
            clip_for_descendants: Vec::new(),
            transform_for_descendants: Vec::new(),
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
                    .and_then(|p| self.hit_test(p, Sense::is_clickable));
            }
            InputEvent::PointerReleased(PointerButton::Left) => {
                if let Some(a) = self.active.take() {
                    let hit = self
                        .pointer
                        .pos
                        .and_then(|p| self.hit_test(p, Sense::is_clickable));
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
    /// drop transient per-frame flags. Call after `layout::run`.
    ///
    /// Three ancestor-cascading state machines run in this single pre-order pass:
    ///
    /// - **`disabled`**: any ancestor with `disabled = true` forces this
    ///   node's effective `Sense` to `NONE`, removing the subtree from
    ///   hit-testing.
    /// - **`visibility`**: any ancestor (or self) with `Hidden`/`Collapsed`
    ///   visibility forces this node's effective `Sense` to `NONE` for the
    ///   same reason. (Paint cascade is handled separately by the encoder.)
    /// - **`transform`**: each node's own rect is mapped to screen space via
    ///   its parent's cumulative transform (the panel's *own* transform applies
    ///   only to its descendants, matching the encoder's emit order).
    /// - **`clip`**: clipping ancestors bound the visible (and thus
    ///   hit-testable) area of descendants. Stored in screen space so it
    ///   composes with transformed rects directly.
    pub(crate) fn end_frame(&mut self, tree: &Tree) {
        self.last_rects.clear();
        self.effective_disabled.clear();
        self.effective_invisible.clear();
        self.clip_for_descendants.clear();
        self.transform_for_descendants.clear();
        let n = tree.nodes.len();
        self.last_rects.reserve(n);
        self.effective_disabled.reserve(n);
        self.effective_invisible.reserve(n);
        self.clip_for_descendants.reserve(n);
        self.transform_for_descendants.reserve(n);

        for node in &tree.nodes {
            // Disabled cascade.
            let parent_disabled = node
                .parent
                .map(|p| self.effective_disabled[p.0 as usize])
                .unwrap_or(false);
            let me_disabled = parent_disabled || node.element.disabled;
            self.effective_disabled.push(me_disabled);

            // Visibility cascade. `Hidden` and `Collapsed` both suppress input;
            // any non-`Visible` ancestor poisons the whole subtree.
            let parent_invisible = node
                .parent
                .map(|p| self.effective_invisible[p.0 as usize])
                .unwrap_or(false);
            let me_invisible = parent_invisible || node.element.visibility != Visibility::Visible;
            self.effective_invisible.push(me_invisible);

            // Transform cascade. Parent's cumulative transform places THIS
            // node's rect into screen space. Own transform contributes only
            // to descendants.
            let parent_t = node
                .parent
                .map(|p| self.transform_for_descendants[p.0 as usize])
                .unwrap_or(TranslateScale::IDENTITY);
            let descendant_t = match node.element.transform {
                Some(t) => parent_t.compose(t),
                None => parent_t,
            };
            self.transform_for_descendants.push(descendant_t);

            // Clip cascade. Both visible_rect and descendant_clip live in
            // screen space; intersect after applying parent's transform.
            let screen_rect = parent_t.apply_rect(node.rect);
            let parent_clip = node
                .parent
                .and_then(|p| self.clip_for_descendants[p.0 as usize]);
            let visible_rect = match parent_clip {
                Some(c) => screen_rect.intersect(c),
                None => screen_rect,
            };
            let descendant_clip = if node.element.clip {
                Some(match parent_clip {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                parent_clip
            };
            self.clip_for_descendants.push(descendant_clip);

            let sense = if me_disabled || me_invisible {
                Sense::NONE
            } else {
                node.element.sense
            };
            self.last_rects.push(HitEntry {
                id: node.element.id,
                rect: visible_rect,
                sense,
            });
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
        self.hovered = self
            .pointer
            .pos
            .and_then(|p| self.hit_test(p, Sense::is_hoverable));
    }

    /// Reverse-iter `last_rects` → topmost-first under our pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for hover
    /// id, clickable for press/release). Bounding-rect only for v1; per-node
    /// `HitShape` lands later.
    fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        for e in self.last_rects.iter().rev() {
            if filter(e.sense) && e.rect.contains(pos) {
                return Some(e.id);
            }
        }
        None
    }

    fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.last_rects
            .iter()
            .find_map(|e| (e.id == id).then_some(e.rect))
    }

    fn contains_id(&self, id: WidgetId) -> bool {
        self.last_rects.iter().any(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests;
