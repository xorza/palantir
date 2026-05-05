use crate::layout::types::sense::Sense;
use crate::primitives::transform::TranslateScale;
use crate::tree::NodeId;
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use glam::Vec2;

/// One scroll widget recorded this frame: the stable `WidgetId` keying
/// its [`ScrollState`] row plus the per-frame `NodeId` for reading
/// arranged rect / measured content. Pushed during recording, drained
/// in `Ui::end_frame` after arrange.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollNode {
    pub(crate) id: WidgetId,
    pub(crate) node: NodeId,
}

/// Cross-frame state row for one [`Scroll`] widget. Persisted via
/// `Ui::state_mut` keyed by the widget's `WidgetId` and refreshed in
/// `Ui::end_frame` after arrange — `viewport_h`/`content_h` reflect the
/// just-finished frame, while `offset` is the *next* frame's starting
/// pan position. Clamping uses the previous frame's viewport/content,
/// so a single frame after a resize may render with a stale clamp; the
/// next frame settles.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: f32,
    pub(crate) viewport_h: f32,
    pub(crate) content_h: f32,
}

/// Vertical scroll viewport. Records like a `VStack` — children added
/// inside the closure flow top-to-bottom — but measures them with the
/// main axis unbounded so they report full natural height. Wheel /
/// touchpad input over the viewport pans children via a `transform`
/// applied at record time using the previous frame's clamp.
pub struct Scroll {
    element: Element,
}

impl Scroll {
    #[track_caller]
    pub fn vertical() -> Self {
        let mut element = Element::new_auto(LayoutMode::ScrollV);
        element.clip = true;
        element.sense = Sense::Scroll;
        Self { element }
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;

        // Record-time clamp: uses *last* frame's `viewport_h`/`content_h`
        // because this frame's measure hasn't run yet. The matching
        // re-clamp in `Ui::end_frame` corrects with fresh numbers so
        // next frame's record starts in-bounds.
        let delta_y = ui.input.scroll_delta_for(id).y;
        let row = ui.state_mut::<ScrollState>(id);
        let max_offset = (row.content_h - row.viewport_h).max(0.0);
        let offset = (row.offset + delta_y).clamp(0.0, max_offset);
        row.offset = offset;

        let mut element = self.element;
        if offset > 0.0 {
            element.transform = Some(TranslateScale::from_translation(Vec2::new(0.0, -offset)));
        }

        let node = ui.node(element, body);
        ui.scroll_nodes.push(ScrollNode { id, node });

        let resp_state = ui.response_for(id);
        Response {
            node,
            state: resp_state,
        }
    }
}

impl Configure for Scroll {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
