//! [`FontStyle`] — the upright/italic axis, independent of weight.

/// Whether a run shapes against an upright or an italic face.
///
/// A separate axis from [`FontWeight`](crate::FontWeight), the way CSS
/// `font-style` is separate from `font-weight`: bold italic is both, not
/// a third weight. When a family registers no italic face, cosmic
/// synthesizes one — see `attrs_named`.
///
/// `#[repr(u8)]` with explicit discriminants pins the tag the shape key
/// packs and the `ShapeRecord::Text` hash carries.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FontStyle {
    #[default]
    Normal = 0,
    Italic = 1,
}
