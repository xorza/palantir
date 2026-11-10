//! The WGSL every palantir shader shares, a pipeline's and an effect's alike, each name under the
//! `palantir_` prefix the effect contract reserves, so an app's effect text cannot collide with it.
//! One copy of each: a pipeline's prelude and an effect's module include the same text.

/// `palantir_edge_coverage`.
pub(crate) const ANTIALIAS: &str = include_str!("antialias.wgsl");
/// The sRGB transfers and `palantir_unpremultiply`.
pub(crate) const COLOR: &str = include_str!("color.wgsl");
/// The rounded rect's signed distance.
pub(crate) const SDF: &str = include_str!("sdf.wgsl");
/// A read of the gradient LUT atlas, whose bindings the includer declares.
pub(crate) const GRADIENT_LUT: &str = include_str!("gradient_lut.wgsl");
/// A gradient fill's color at a point, with the spread and kind constants it reads.
pub(crate) const GRADIENT_FILL: &str = include_str!("gradient_fill.wgsl");
/// The template a closed shape's vertex stage builds: per corner a ring, per edge a band, and an
/// interior fan, from a `PalantirShape` its includer describes from its own instance.
pub(crate) const SHAPE_TEMPLATE: &str = include_str!("shape_template.wgsl");

/// Segments of each corner's arc in [`SHAPE_TEMPLATE`], substituted as `ARC_SEGMENTS`. At 2, the
/// polygon around a quarter arc reaches `1 / cos(22.5°) − 1`, 8 %, past its reach, and the one
/// inside it falls 8 % short of its depth, both of a ring the fragment measures; at 3 both shrink
/// to 3.5 % for 12 more vertices a box.
pub(crate) const ARC_SEGMENTS: u32 = 2;
