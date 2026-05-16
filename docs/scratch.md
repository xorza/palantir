image

checkbox

skip frame if window is not visible

refactor internals - move them to corresponding modules witf cfg mod

remove
pub(crate) struct DamageEngine { #[cfg(any(test, feature = "internals"))]
pub(crate) dirty: Vec<NodeId>,
