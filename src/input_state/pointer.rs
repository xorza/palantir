//! [`Pointer`]: where the pointer is, what it routes to, and each button's capture.

use crate::cascade::Cascade;
use crate::input::interaction::scroll_delta::ScrollDelta;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::scroll_targets::ScrollTargets;
use crate::input_state::capture::{Capture, DRAG_THRESHOLD, PressDrag};
use crate::input_state::target_scroll_delta::TargetScrollDelta;
use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;

/// What the pointer leaves behind between events, and this frame's deltas and events.
#[derive(Debug, Default)]
pub(super) struct Pointer {
    pub(super) pos: Option<Vec2>,
    pub(super) hovered: Option<WidgetId>,
    /// Topmost scroll-sensing widgets under the pointer.
    pub(super) scroll_targets: ScrollTargets,
    /// Topmost `Sense::PINCH` widget under the pointer; separate from scroll so a widget can pan
    /// without pinch-zooming.
    pub(super) pinch_target: Option<WidgetId>,
    /// This frame's scroll and zoom, by the widget it reached.
    pub(super) target_deltas: Vec<TargetScrollDelta>,
    /// Per-button press capture, indexed by [`PointerButton::idx`].
    pub(super) captures: [Capture; PointerButton::COUNT],
    /// This frame's pointer events for the watchers that asked.
    pub(super) events: Vec<PointerEvent>,
}

impl Pointer {
    /// Move to `p`, carrying every held press's travel; a press whose travel crosses
    /// [`DRAG_THRESHOLD`] latches a drag, a right-drag's latch suppressing its click too, and
    /// ends its multi-click run. Returns whether one latched.
    pub(super) fn move_to(&mut self, p: Vec2) -> bool {
        self.pos = Some(p);
        let mut latched = false;
        for cap in &mut self.captures {
            if let Some(press) = &mut cap.press {
                press.travel = p - press.origin;
                if press.drag == PressDrag::None
                    && press.travel.length_squared() >= DRAG_THRESHOLD * DRAG_THRESHOLD
                {
                    press.drag = PressDrag::Started;
                    latched = true;
                    cap.run = None;
                }
            }
        }
        latched
    }

    pub(super) const fn capture(&self, b: PointerButton) -> &Capture {
        &self.captures[b.idx()]
    }

    pub(super) const fn capture_mut(&mut self, b: PointerButton) -> &mut Capture {
        &mut self.captures[b.idx()]
    }

    /// Whether any button holds a live capture; a captured widget tracks the pointer wherever it
    /// is.
    pub(super) fn any_press(&self) -> bool {
        self.captures.iter().any(|c| c.press.is_some())
    }

    /// The scroll and zoom `target` took this frame.
    pub(super) fn delta(&self, target: WidgetId) -> Option<&ScrollDelta> {
        self.target_deltas
            .iter()
            .find(|deltas| deltas.target == target)
            .map(|deltas| &deltas.delta)
    }

    /// The scroll and zoom `target` takes this frame, opened empty on its first.
    pub(super) fn delta_mut(&mut self, target: WidgetId) -> &mut ScrollDelta {
        if let Some(idx) = self
            .target_deltas
            .iter()
            .position(|deltas| deltas.target == target)
        {
            return &mut self.target_deltas[idx].delta;
        }
        self.target_deltas.push(TargetScrollDelta::new(target));
        &mut self.target_deltas.last_mut().unwrap().delta
    }

    /// Re-resolve `hovered` / `scroll_targets` / `pinch_target` from `pos`; the single owner of
    /// that assignment.
    pub(super) fn retarget(&mut self, cascade: &Cascade) {
        if let Some(p) = self.pos {
            let hits = cascade.hit_test_targets(p);
            self.hovered = hits.hover;
            self.scroll_targets = hits.scroll;
            self.pinch_target = hits.pinch;
        } else {
            self.hovered = None;
            self.scroll_targets = ScrollTargets::default();
            self.pinch_target = None;
        }
    }

    /// Forget this frame's edges: each capture's release and fresh press, a drag that started,
    /// the deltas and the events.
    pub(super) fn end_frame(&mut self) {
        for cap in &mut self.captures {
            cap.release = None;
            if let Some(press) = &mut cap.press {
                press.fresh = false;
                if press.drag == PressDrag::Started {
                    press.drag = PressDrag::Active;
                }
            }
        }
        self.events.clear();
        self.target_deltas.clear();
    }
}
