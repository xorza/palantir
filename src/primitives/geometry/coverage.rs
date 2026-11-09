//! [`Coverage`]: which side of its boundary a shape covers.

/// Which side of its boundary a shape covers, as Skia's inverse fill types belong to the path. A
/// rect covers the inside by default; [`Self::Outside`] covers its rect outside the rounded
/// boundary and leaves the inside clear, a mask laid over content of the rect's extent, as a
/// rounded clip's corners are.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Coverage {
    /// Inside the boundary.
    #[default]
    Inside = 0,
    /// Outside the boundary, within the rect.
    Outside = 1,
}
