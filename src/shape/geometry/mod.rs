//! The geometry a shape record keeps, owner-relative, and what it places once the owner's size is
//! known: a box, a triangle, a Bézier, an arc, a point shape's placement and a stroke's trace.

pub(crate) mod arc_geometry;
pub(crate) mod bezier_geometry;
pub(crate) mod box_geometry;
pub(crate) mod curve_basis;
pub(crate) mod placed_stroke;
pub(crate) mod point_placement;
pub(crate) mod triangle_geometry;
