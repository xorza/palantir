//! Widget-facing input results: [`Response`](crate::Response) and [`PointerButtonState`](crate::PointerButtonState), the [`PointerButtonPhase`](crate::PointerButtonPhase)/[`Drag`](crate::Drag) lifecycles, [`ScrollDelta`](crate::ScrollDelta), [`PointerAction`](crate::PointerAction)/[`PointerEdge`](crate::PointerEdge) (what the pointer did, widget by widget), and `InputDelta` (the repaint hint `Ui::on_input` returns).
//!
//! Pure outputs; they never reference the [`Input`](crate::input::Input) that produces them.

pub(crate) mod drag;
pub(crate) mod input_delta;
pub(crate) mod pointer_action;
pub(crate) mod pointer_button_phase;
pub(crate) mod pointer_button_state;
pub(crate) mod pointer_edge;
pub(crate) mod response;
pub(crate) mod scroll_delta;
pub(crate) mod wheel_delta;
