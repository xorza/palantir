//! Whether a node clips its descendants, and at what cost: no clip, a
//! scissor, or a rounded mask that needs the stencil.

/// How a node clips its descendants' paint.
///
/// `None` = no clip. `Rect` = axis-aligned scissor (the cheap, GPU-native
/// path). `Rounded` = clip to the node's `Background.radius`; requires a
/// stencil pass on the backend, so apps that never use it pay nothing.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[repr(u8)]
pub enum ClipMode {
    #[default]
    None = 0,
    Rect = 1,
    Rounded = 2,
}

impl ClipMode {
    pub const fn is_clip(self) -> bool {
        !matches!(self, ClipMode::None)
    }
}
