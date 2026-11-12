//! Whether a node clips its descendants: no clip, a scissor, or a rounded rect.

/// How a node clips its descendants' paint.
///
/// `Rounded` clips to the node's `Chrome.radius`, with an antialiased edge: each draw it cuts takes
/// the clip's coverage in its fragment stage.
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
#[serde(deny_unknown_fields)]
#[repr(u8)]
pub enum ClipMode {
    /// No clip. The default.
    #[default]
    None = 0,
    /// Axis-aligned scissor — the cheap, GPU-native path.
    Rect = 1,
    /// Clip to the node's corner radii. Costs a coverage term in each draw that crosses its edge.
    Rounded = 2,
}

impl ClipMode {
    /// Whether this mode clips at all.
    pub const fn is_clip(self) -> bool {
        !matches!(self, Self::None)
    }
}
