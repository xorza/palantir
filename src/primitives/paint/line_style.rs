//! How a stroke ends and turns: [`LineCap`] and [`LineJoin`].

/// How a [`Stroke`](crate::Stroke) ends: a line, a Bézier, an arc or a polyline.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum LineCap {
    /// Ends exactly at the endpoint. The default.
    #[default]
    Butt = 0,
    /// Extends by half the stroke width along the tangent.
    Square = 1,
    /// Adds a half-disc past the endpoint.
    Round = 2,
}

/// How a [`Stroke`](crate::Stroke) turns at a polyline's interior points.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub enum LineJoin {
    /// Extends both edges to their crossing point. The default; becomes [`Self::Bevel`] past the miter limit.
    #[default]
    Miter = 0,
    /// Cuts the corner off square.
    Bevel = 1,
    /// Rounds the corner by the stroke radius.
    Round = 2,
}
