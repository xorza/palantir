use crate::primitives::Rect;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum InputEvent {
    PointerMoved(Vec2),
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
}

#[derive(Default, Clone, Copy, Debug)]
pub struct PointerState {
    pub pos: Option<Vec2>,
}

#[derive(Default, Debug)]
pub(crate) struct InputState {
    pub pointer: PointerState,
    pub events: Vec<InputEvent>,
}

/// Snapshot of a single widget's interaction state for the most recently rendered frame.
/// Returned (wrapped in `Response`) by widget builders so the user can react to clicks etc.
#[derive(Default, Clone, Copy, Debug)]
pub struct ResponseState {
    pub rect: Option<Rect>,
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
}
