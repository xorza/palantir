//! Widget-facing input results: [`ResponseState`](crate::ResponseState) and [`ButtonState`](crate::ButtonState), the [`ButtonPhase`](crate::ButtonPhase)/[`Drag`](crate::Drag) lifecycles, [`ScrollDelta`](crate::ScrollDelta), [`PointerAction`](crate::PointerAction)/[`PointerEdge`](crate::PointerEdge) (what the pointer did, widget by widget), and `InputDelta` (the repaint hint `Ui::on_input` returns).
//!
//! Pure outputs; they never reference the [`InputState`](crate::input::input_state::InputState) that produces them.

pub(crate) mod button_phase;
pub(crate) mod button_state;
pub(crate) mod drag;
pub(crate) mod input_delta;
pub(crate) mod pointer_action;
pub(crate) mod pointer_edge;
pub(crate) mod response_state;
pub(crate) mod scroll_delta;
