//! Widget-facing input results: [`ResponseState`](crate::ResponseState)
//! (one widget's interaction snapshot for the frame),
//! [`ButtonState`](crate::ButtonState) (its per-button slice),
//! [`ButtonPhase`](crate::ButtonPhase) / [`Drag`](crate::Drag) (its press
//! and drag lifecycles), [`ScrollDelta`](crate::ScrollDelta) (routed
//! wheel/touchpad/pinch deltas), [`PointerAction`](crate::PointerAction) /
//! [`PointerEdge`](crate::PointerEdge) (the same frame collated the other
//! way about — what the pointer did, widget by widget, rather than what
//! one widget saw), and [`InputDelta`](crate::InputDelta) (the repaint
//! hint `Ui::on_input` returns).
//!
//! These are pure outputs — they never reference the
//! [`InputState`](crate::input::input_state::InputState) machine that
//! produces them.

pub(crate) mod button_phase;
pub(crate) mod button_state;
pub(crate) mod drag;
pub(crate) mod input_delta;
pub(crate) mod pointer_action;
pub(crate) mod pointer_edge;
pub(crate) mod response_state;
pub(crate) mod scroll_delta;
