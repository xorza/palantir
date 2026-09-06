//! Cap and join styling, shared by every stroked shape.

/// Endpoint cap style for stroked shapes (Line / Polyline / béziers / Arc).
///
/// - `Butt` ends exactly at the endpoint.
/// - `Square` extends by half the width along the tangent.
/// - `Round` adds a half-disc past the endpoint.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineCap {
    /// Ends exactly at the endpoint. The default.
    #[default]
    Butt = 0,
    /// Extends by half the stroke width along the tangent.
    Square = 1,
    /// Adds a half-disc past the endpoint.
    Round = 2,
}

/// Interior-join style for [`Shape::polyline`](crate::widget::Shape::polyline). Miter joins
/// downgrade to bevel when their extension exceeds the shared miter limit.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineJoin {
    /// Extends both edges to their crossing point. The default, and it
    /// downgrades to [`Self::Bevel`] past the miter limit.
    #[default]
    Miter = 0,
    /// Cuts the corner off square.
    Bevel = 1,
    /// Rounds the corner by the stroke radius.
    Round = 2,
}
