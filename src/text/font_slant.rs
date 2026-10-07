//! [`FontSlant`] — the upright/italic axis, independent of weight.

/// Whether a run shapes against an upright or an italic face.
///
/// A separate axis from [`FontWeight`](crate::FontWeight), as in CSS. When a family registers no italic face, cosmic synthesizes one (see `attrs_named`). `#[repr(u8)]` pins the tag the shape key and `ShapeRecord::Text` hash carry.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FontSlant {
    /// Upright. The default.
    #[default]
    Normal = 0,
    /// Italic: the family's italic face, or a synthesized slant.
    Italic = 1,
}
