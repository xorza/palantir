//! Whether a node clips its descendants: no clip, a scissor, or a rounded stencil mask.

/// How a node clips its descendants' paint.
///
/// `Rounded` clips to the node's `Background.radius` and needs a backend stencil pass, so apps that never use it pay nothing.
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
    /// No clip. The default.
    #[default]
    None = 0,
    /// Axis-aligned scissor — the cheap, GPU-native path.
    Rect = 1,
    /// Clip to the node's corner radii. Costs a stencil pass.
    Rounded = 2,
}

impl ClipMode {
    /// Whether this mode clips at all.
    pub const fn is_clip(self) -> bool {
        !matches!(self, ClipMode::None)
    }
}
