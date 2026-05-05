use crate::layout::types::sense::Sense;
use crate::primitives::size::Size;
use crate::primitives::transform::TranslateScale;
use crate::tree::NodeId;
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use glam::{BVec2, Vec2};

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
/// `Ui::end_frame` after arrange — `viewport`/`content` reflect the
/// just-finished frame, while `offset` is the *next* frame's starting
/// pan position. Clamping uses the previous frame's numbers, so a
/// single frame after a resize may render with a stale clamp; the next
/// frame settles. Single-axis scrolls leave the un-panned axis at 0.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(crate) viewport: Size,
    pub(crate) content: Size,
}

/// Scroll viewport. Three flavors via constructor:
/// - [`Scroll::vertical`]: pans on Y, lays children out as a `VStack`.
/// - [`Scroll::horizontal`]: pans on X, lays children out as an
///   `HStack`.
/// - [`Scroll::both`]: pans on both axes, lays children out as a
///   `ZStack` measured with both axes unbounded.
///
/// All three measure the panned axes as `INF` so children report their
/// full natural extent; the viewport itself takes whatever its parent
/// gave it. Wheel / touchpad input over the viewport pans children via
/// a `transform` applied at record time using the previous frame's
/// clamp.
pub struct Scroll {
    element: Element,
    /// Mask of axes that consume scroll deltas. Single-axis scrolls
    /// keep the off-axis offset at 0 even if a delta arrives on it.
    pan: BVec2,
}

impl Scroll {
    #[track_caller]
    pub fn vertical() -> Self {
        Self::with_mode(LayoutMode::ScrollV, BVec2::new(false, true))
    }

    #[track_caller]
    pub fn horizontal() -> Self {
        Self::with_mode(LayoutMode::ScrollH, BVec2::new(true, false))
    }

    #[track_caller]
    pub fn both() -> Self {
        Self::with_mode(LayoutMode::ScrollXY, BVec2::TRUE)
    }

    #[track_caller]
    fn with_mode(mode: LayoutMode, pan: BVec2) -> Self {
        let mut element = Element::new_auto(mode);
        element.clip = true;
        element.sense = Sense::Scroll;
        Self { element, pan }
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;

        // Record-time clamp: uses *last* frame's `viewport`/`content`
        // because this frame's measure hasn't run yet. The matching
        // re-clamp in `Ui::end_frame` corrects with fresh numbers so
        // next frame's record starts in-bounds. Off-axis offsets stay
        // at 0 for single-axis scrolls.
        let delta = ui.input.scroll_delta_for(id);
        let row = ui.state_mut::<ScrollState>(id);
        let max_x = (row.content.w - row.viewport.w).max(0.0);
        let max_y = (row.content.h - row.viewport.h).max(0.0);
        let mut offset = row.offset;
        if self.pan.x {
            offset.x = (offset.x + delta.x).clamp(0.0, max_x);
        }
        if self.pan.y {
            offset.y = (offset.y + delta.y).clamp(0.0, max_y);
        }
        row.offset = offset;

        let mut element = self.element;
        if offset != Vec2::ZERO {
            element.transform = Some(TranslateScale::from_translation(-offset));
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
