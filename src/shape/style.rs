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

impl LineCap {
    pub(crate) const fn from_u8(value: u8) -> Self {
        match value {
            0 => LineCap::Butt,
            1 => LineCap::Square,
            2 => LineCap::Round,
            _ => panic!("invalid LineCap discriminant in cmd buffer"),
        }
    }
}

/// Interior-join style for [`crate::shape::Shape::Polyline`]. Miter joins
/// downgrade to bevel when their extension exceeds the shared miter limit.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineJoin {
    #[default]
    Miter = 0,
    Bevel = 1,
    Round = 2,
}

impl LineJoin {
    pub(crate) const fn from_u8(value: u8) -> Self {
        match value {
            0 => LineJoin::Miter,
            1 => LineJoin::Bevel,
            2 => LineJoin::Round,
            _ => panic!("invalid LineJoin discriminant in cmd buffer"),
        }
    }
}
