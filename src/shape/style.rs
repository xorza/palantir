//! Cap and join styling, shared by every stroked shape.

/// Endpoint cap style for stroked shapes (Line / Polyline / béziers / Arc).
///
/// - `Butt` ends exactly at the endpoint.
/// - `Square` extends by half the width along the tangent.
/// - `Round` adds a half-disc past the endpoint.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineCap {
    #[default]
    Butt = 0,
    Square = 1,
    Round = 2,
}

/// Interior-join style for [`Shape::polyline`](crate::Shape::polyline). Miter joins
/// downgrade to bevel when their extension exceeds the shared miter limit.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineJoin {
    #[default]
    Miter = 0,
    Bevel = 1,
    Round = 2,
}
