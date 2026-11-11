//! The WGSL every palantir shader shares, a pipeline's and an effect's alike, each name under the
//! `palantir_` prefix the effect contract reserves, so an app's effect text cannot collide with it.
//! One copy of each: a pipeline's prelude and an effect's module include the same text.

/// The group of every color pipeline, an app effect's included, that the frame's shared resources
/// bind through, the gradient atlas: first in each layout, as wgpu binds again from the first slot
/// whose layout differs at a pipeline switch (`wgpu-core` 30.0.1, `command/bind.rs`,
/// `update_expectations`), so the main pass binds it once. It sits here, beside [`FRAME`], as
/// `effect` lies left of `renderer` and `gpu` in the module order and its contract reads it.
pub(crate) const FRAME_GROUP: u32 = 0;

/// The bits of a draw's clip chain (`ClipChain`) that hold its depth, below its start: what
/// [`FRAME`]'s `palantir_clip_coverage` unpacks.
pub(crate) const CLIP_DEPTH_BITS: u32 = 8;

/// The frame's coordinates: the immediate region, `PalantirImmediates`, and the maps between the
/// surface's physical px and a shader's positions. Apart from [`TURN`], as an app's effect text
/// validates against the contract without the immediates capability, which naga asks of every
/// immediate global, read or not (`naga` 30.0.1, `valid/interface.rs`, `validate_global_var`).
pub(crate) const VIEWPORT: &str = include_str!("viewport.wgsl");
/// `palantir_turned`, which an effect's text reads through the contract.
pub(crate) const TURN: &str = include_str!("turn.wgsl");
/// `palantir_edge_coverage` and the half-width it reads, with the marker `AA_HALF_WIDTH`.
pub(crate) const ANTIALIAS: &str = include_str!("antialias.wgsl");
/// A box's fill and border over its edge, `palantir_composite`, which a quad and a material shade
/// alike; it reads [`ANTIALIAS`].
pub(crate) const COMPOSITE: &str = include_str!("composite.wgsl");
/// The sRGB transfers, `palantir_unpremultiply` and the luma weights.
pub(crate) const COLOR: &str = include_str!("color.wgsl");
/// The rounded rect's signed distance.
pub(crate) const SDF: &str = include_str!("sdf.wgsl");
/// The frame group's bindings, the gradient LUT atlas and the rounded clips, and
/// `palantir_clip_coverage`, with the markers `FRAME_GROUP` and `CLIP_DEPTH_BITS`.
pub(crate) const FRAME: &str = include_str!("frame.wgsl");
/// A read of the gradient LUT atlas, whose bindings [`FRAME`] declares.
pub(crate) const GRADIENT_LUT: &str = include_str!("gradient_lut.wgsl");
/// A gradient fill's color at a point, with the spread and kind constants it reads.
pub(crate) const GRADIENT_FILL: &str = include_str!("gradient_fill.wgsl");
/// The template a closed shape's vertex stage builds: per corner a ring, per edge a band, and an
/// interior fan, from a `PalantirShape` its includer describes from its own instance, and a
/// fragment's coverage and box fill from its region; it reads [`ANTIALIAS`] and [`COMPOSITE`].
pub(crate) const SHAPE_TEMPLATE: &str = include_str!("shape_template.wgsl");

/// Segments of each corner's arc in [`SHAPE_TEMPLATE`], substituted as `ARC_SEGMENTS`. At 2, the
/// polygon around a quarter arc reaches `1 / cos(22.5°) − 1`, 8 %, past its reach, and the one
/// inside it falls 8 % short of its depth, both of a ring the fragment measures; at 3 both shrink
/// to 3.5 % for 12 more points a box, and the arc's directions need more than the halvings
/// `palantir_template_point` writes out.
pub(crate) const ARC_SEGMENTS: u32 = 2;

#[cfg(test)]
mod tests;
