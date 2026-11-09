//! A quad-tier draw reduced to physical space: its rect, corners and lanes, past which the
//! composer is shape-blind.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::num::F32Px;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::packed::half_simd::F16x4;
use crate::renderer::frontend::composer::lowering::{Lowering, ScaledRect};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, QuadGeometry};
use crate::renderer::frontend::payload::placed_outline::PlacedOutline;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::quad::Quad;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::Vec2;

/// A quad-tier draw reduced to physical space: its covering rect, and the closed shape inside it
/// as sharp points, a radius at each and how deep its interior reaches (`Quad::points`).
#[derive(Debug)]
pub(super) struct PackedQuad {
    /// The quad's own rect, unturned, and the whole pixels it reaches turned.
    pub(super) rect: ScaledRect,
    /// A radius at each point, `(tl, tr, br, bl)` for a box.
    pub(super) corners: Corners,
    pub(super) fill_axis: FillAxis,
    pub(super) border_width: f32,
    /// A paint animation's turn about the rect's center, in radians.
    pub(super) rotation: f32,
    /// The shape's sharp points from the rect's top-left, clockwise on screen.
    pub(super) points: [Vec2; 4],
    /// How far inside every edge the shape's medial axis stops its interior.
    pub(super) deep: f32,
}

impl PackedQuad {
    /// Unturned with neither a border nor a rounded corner: the edges are the rect's.
    pub(super) const fn is_sharp(&self) -> bool {
        self.rotation == 0.0 && is_invisible(self.border_width) && self.corners.is_approx_zero()
    }

    /// [`Self::is_sharp`] plus physical edges on whole pixels. Exactness makes the
    /// fragment fast path bitwise-identical to the SDF.
    pub(super) fn is_pixel_aligned(&self) -> bool {
        let physical = self.rect.physical;
        let max = physical.max();
        self.is_sharp()
            && physical.min.x.is_integral()
            && physical.min.y.is_integral()
            && max.x.is_integral()
            && max.y.is_integral()
    }

    /// Reduce a quad-tier draw to physical space; everything past this is shape-blind.
    // Always inlined into `quad` and `backdrop`: as a call, `repeated_chrome` ran 4 % slower.
    #[inline(always)]
    pub(super) fn pack(p: &DrawQuadPayload, lowering: &Lowering<'_>) -> Self {
        let scale_physical = lowering.transform.physical_scale();
        match p.geometry {
            QuadGeometry::Rect {
                rect,
                corners,
                spin,
            } => {
                // A turned rect snaps to no pixel: its edges are not on the pixel grid.
                let edges = match spin {
                    None => lowering.display.edge_rounding(),
                    Some(_) => EdgeRounding::Exact,
                };
                let source = lowering.scaled_rect_with(rect, edges);
                // Shadow scalars are logical px, so scale them; a gradient axis is unit-space,
                // and a backdrop's lanes are ratios.
                let fill_axis = if p.fill.kind.is_shadow() {
                    p.fill_axis.scaled_saturating(scale_physical)
                } else {
                    p.fill_axis
                };
                let corners = corners.fit_to(source.physical.size, scale_physical);
                // A drop shadow's quad is its snapped source moved and grown, not re-snapped, so
                // the source inside it lies on the same pixels as the fill.
                let rect = if p.fill.kind == FillKind::SHADOW_DROP {
                    let geometry = ShadowGeometry::from_lanes(fill_axis.lanes());
                    let physical = Rect {
                        min: source.physical.min + geometry.offset,
                        size: source.physical.size,
                    }
                    .inflated(geometry.halo());
                    ScaledRect::from_physical(physical, lowering.display.physical)
                } else {
                    source
                };
                let (rect, rotation) = match spin {
                    None => (rect, 0.0),
                    Some(spin) => (lowering.turned_rect(rect.physical, spin), spin.angle),
                };
                let size = rect.physical.size;
                Self {
                    rect,
                    corners,
                    fill_axis,
                    border_width: p.border.width * scale_physical,
                    rotation,
                    points: Quad::box_points(size),
                    deep: Quad::box_deep(size),
                }
            }
            QuadGeometry::Triangle {
                a,
                b,
                c,
                radius,
                spin,
            } => {
                let triangle = TemplateShape::triangle([a, b, c], radius, lowering);
                // Turned as a rect is, so its gradient turns with it.
                let (rect, rotation) = match spin {
                    None => (
                        ScaledRect::from_physical(triangle.rect, lowering.display.physical),
                        0.0,
                    ),
                    Some(spin) => (lowering.turned_rect(triangle.rect, spin), spin.angle),
                };
                Self {
                    rect,
                    corners: triangle.corners,
                    // A gradient axis is unit-space.
                    fill_axis: p.fill_axis,
                    border_width: (p.border.width * scale_physical).max(0.0),
                    rotation,
                    points: triangle.points,
                    deep: triangle.deep,
                }
            }
        }
    }
}

/// A closed shape in physical px as the shape template takes it: its covering rect, its sharp
/// points relative to that rect, a radius at each, and its depth. A quad's triangle and an image's
/// shape both pack through it.
#[derive(Debug)]
pub(super) struct TemplateShape {
    pub(super) rect: Rect,
    pub(super) points: [Vec2; 4],
    pub(super) corners: Corners,
    pub(super) deep: f32,
}

impl TemplateShape {
    /// `outline` in physical px, turned by `spin` or not: its rect on the pixels [`Self::edges`]
    /// picks, its corners fit to it; or its triangle.
    pub(super) fn placed(
        outline: PlacedOutline,
        spin: Option<Spin>,
        lowering: &Lowering<'_>,
    ) -> Self {
        match outline {
            PlacedOutline::Rect { rect, corners } => {
                let rect = lowering
                    .scaled_rect_with(rect, Self::edges(outline, spin, lowering))
                    .physical;
                Self::rounded_rect(
                    rect,
                    corners.fit_to(rect.size, lowering.transform.physical_scale()),
                )
            }
            PlacedOutline::Triangle { points, radius } => Self::triangle(points, radius, lowering),
        }
    }

    /// How `outline`'s edges round: a still rect's to the display's pixels; a turned rect's
    /// exactly, as its edges are not on the pixel grid, and a triangle's, which places itself to
    /// the sub-pixel.
    pub(super) const fn edges(
        outline: PlacedOutline,
        spin: Option<Spin>,
        lowering: &Lowering<'_>,
    ) -> EdgeRounding {
        match (outline, spin) {
            (PlacedOutline::Rect { .. }, None) => lowering.display.edge_rounding(),
            _ => EdgeRounding::Exact,
        }
    }

    /// The physical `rect` rounded by `corners`, already fit to it.
    pub(super) const fn rounded_rect(rect: Rect, corners: Corners) -> Self {
        Self {
            rect,
            points: Quad::box_points(rect.size),
            corners,
            deep: Quad::box_deep(rect.size),
        }
    }

    /// The triangle of logical corner centers `points` dilated by `radius`, its rect unsnapped,
    /// as the shape places itself to the sub-pixel, its fourth point repeating the third.
    pub(super) fn triangle(points: [Vec2; 3], radius: f32, lowering: &Lowering<'_>) -> Self {
        let to_physical = lowering.transform.physical_map(Vec2::ZERO);
        let [a, b, c] = points.map(|q| to_physical.point(q));
        // Saturated as the f16 lane holds it, so the quad and the shader take one radius.
        let radius_physical =
            F16x4::saturate_lane((radius * lowering.transform.physical_scale()).max(0.0));
        // The rounded shape's bounds, which its gradient and its image span; a border sits on the
        // inner edge, so it adds no outward reach.
        let rect = Rect::from_min_max(a.min(b).min(c), a.max(b).max(c)).inflated(radius_physical);
        let grown = GrownTriangle::of([a, b, c], radius_physical);
        let [p0, p1, p2] = grown.points.map(|q| q - rect.min);
        Self {
            rect,
            points: [p0, p1, p2, p2],
            corners: Corners::saturating([radius_physical, radius_physical, radius_physical, 0.0]),
            deep: grown.deep,
        }
    }
}

/// A triangle grown by a radius, as `sdf_triangle - radius` draws it: its corners are arcs of the
/// radius about its points, so its sharp polygon's corners lie that radius past them along each
/// corner's bisector, and the template rounds them back about the points.
#[derive(Debug)]
struct GrownTriangle {
    /// The grown polygon's corners, clockwise on screen.
    points: [Vec2; 3],
    /// The triangle's inradius past its growth: where its medial axis stops its interior.
    deep: f32,
}

impl GrownTriangle {
    fn of([a, b, c]: [Vec2; 3], radius: f32) -> Self {
        // Clockwise on screen, y down, is a positive cross product, as a box's `(tl, tr, br)`.
        let cross = (b - a).perp_dot(c - a);
        let [a, b, c] = if cross > 0.0 { [a, b, c] } else { [a, c, b] };
        let sharp = [a, b, c];
        // The outward normal of the edge from `p` to `q`, as the template takes it.
        let normal = |p: Vec2, q: Vec2| {
            let d = q - p;
            Vec2::new(d.y, -d.x) / d.length()
        };
        let points = std::array::from_fn(|i| {
            let p = sharp[i];
            let na = normal(sharp[(i + 2) % 3], p);
            let nb = normal(p, sharp[(i + 1) % 3]);
            p + radius * (na + nb) / (1.0 + na.dot(nb))
        });
        let perimeter = (b - a).length() + (c - b).length() + (a - c).length();
        Self {
            points,
            deep: cross.abs() / perimeter + radius,
        }
    }
}
