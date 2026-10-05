//! Which shadow corners get a baked cutout table this frame, and where in
//! the atlas each table goes.
//!
//! A blurred shadow's coverage from `CUTOUT_MIN_SIGMA` up is its sharp box
//! less four corner cutouts (`fs_shadow` in `shader.wgsl`), and a cutout
//! depends only on the corner's radius `r` and the blur `σ`. So a distinct
//! `(r, σ)` among the frame's shadow corners can get one table, baked every
//! frame by `fs_cutout_bake`, which a corner reads in place of the cutout's
//! quadrature. Nothing survives the frame: no frame pays a sweep or a
//! repack that another did not.
//!
//! **A table only where it is cheaper.** Shading a corner costs
//! [`CutoutPlan::SHADED_NODES`] per pixel of its region the frame draws;
//! baking its table costs [`CutoutPlan::BAKED_NODES`] per texel, and a
//! table has `(10 / σ)²` texels per pixel. A key is baked when the drawn
//! area of every corner sharing it costs more to shade than its table to
//! bake, so a frame never spends more on cutouts than shading them all —
//! a partial repaint over a sliver of a large shadow shades the sliver.
//!
//! The keys follow the shader's own arithmetic on the instance's `f16`
//! lanes. A last-bit difference between this and the GPU's arithmetic only
//! reads a table for a radius one ulp away, which moves the cutout far below
//! the table's own error.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::quad::Quad;
use crate::shape::paint::lowered_shadow::ShadowGeom;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

/// One table to bake: where it goes ([`CutoutPlan::code`]) and its
/// `(r, σ)`. The instance layout of `fs_cutout_bake`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct BakeTable {
    pub(crate) table: u32,
    pub(crate) r: f32,
    pub(crate) sigma: f32,
}

/// A quad's four corner tables, `(tl, tr, br, bl)`, each a
/// [`CutoutPlan::code`] or [`CutoutPlan::NONE`]. The per-instance stream
/// `vs_shadow` reads beside the quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CornerTables(pub(crate) [u32; 4]);

impl CornerTables {
    const NONE: Self = Self([CutoutPlan::NONE; 4]);
}

/// One shadow corner as the plan sees it: the radius its cutout is cut
/// with, and the screen region where the cutout is not zero.
#[derive(Clone, Copy, Debug)]
struct CutoutCorner {
    r: f32,
    region: Rect,
}

/// The drawn area of the corners that share one key, summed once the keys
/// are sorted.
#[derive(Clone, Copy, Debug)]
struct KeyUse {
    key: CutoutKey,
    area: f32,
}

/// A table's key: the bits of `(r, σ)`, so equal keys are equal floats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CutoutKey {
    r: u32,
    sigma: u32,
}

impl CutoutKey {
    const fn new(r: f32, sigma: f32) -> Self {
        Self {
            r: r.to_bits(),
            sigma: sigma.to_bits(),
        }
    }

    const fn r(self) -> f32 {
        f32::from_bits(self.r)
    }

    const fn sigma(self) -> f32 {
        f32::from_bits(self.sigma)
    }
}

/// The frame's cutout tables and each quad's share of them. Every buffer is
/// scratch, cleared and refilled per frame with its capacity kept.
#[derive(Debug, Default)]
pub(crate) struct CutoutPlan {
    /// Every drawn corner's key, then sorted and merged per key.
    keys: Vec<KeyUse>,
    /// [`Self::code`] per entry of `keys`, or [`Self::NONE`].
    codes: Vec<u32>,
    /// Positions in `keys`, tallest table first, for the packer.
    order: Vec<u32>,
    tables: Vec<BakeTable>,
    corners: Vec<CornerTables>,
}

impl CutoutPlan {
    /// The σ from which the shader cuts corners out of the sharp box.
    pub(crate) const MIN_SIGMA: f32 = 0.25;
    /// A table's texels per σ on each axis.
    pub(crate) const TEXELS_PER_SIGMA: f32 = 10.0;
    /// The atlas's side in texels.
    pub(crate) const ATLAS_SIZE: u32 = 1024;
    /// The cell a table's origin is a multiple of, in texels.
    pub(crate) const CELL: u32 = 16;
    /// A corner with no table.
    pub(crate) const NONE: u32 = u32::MAX;
    /// Midpoint nodes of a cutout the shader shades (`CUTOUT_NODES`).
    pub(crate) const SHADED_NODES: u32 = 12;
    /// Midpoint nodes of a cutout baked into a table (`CUTOUT_BAKE_NODES`):
    /// twice [`Self::SHADED_NODES`], so a table errs a quarter of what the
    /// shaded form does.
    pub(crate) const BAKED_NODES: u32 = 24;
    /// The largest table side, in texels. A key past it — a small σ against
    /// a large radius — keeps the shaded cutout.
    const MAX_SIDE: u32 = 256;
    /// Cells on each axis of the atlas.
    const CELLS: u32 = Self::ATLAS_SIZE / Self::CELL;

    /// Plan the frame's tables for `quads`, given the area of a screen
    /// region the frame draws: all of it on a full repaint, its share of
    /// the damage on a partial one. With `bake` off, every corner keeps the
    /// shaded cutout.
    pub(crate) fn build(&mut self, quads: &[Quad], bake: bool, visible: impl Fn(Rect) -> f32) {
        self.keys.clear();
        self.codes.clear();
        self.tables.clear();
        self.corners.clear();
        if !quads.iter().any(|quad| quad.fill_kind.is_shadow()) {
            return;
        }
        if !bake {
            self.corners.resize(quads.len(), CornerTables::NONE);
            return;
        }
        for quad in quads {
            let Some(corners) = Self::cutout_corners(quad) else {
                continue;
            };
            let sigma = quad.fill_axis.lanes()[2];
            for corner in corners.iter().filter(|corner| corner.r > 0.0) {
                let area = visible(corner.region);
                if area > 0.0 {
                    self.keys.push(KeyUse {
                        key: CutoutKey::new(corner.r, sigma),
                        area,
                    });
                }
            }
        }
        self.keys.sort_unstable_by_key(|used| used.key);
        let mut merged = 0;
        for at in 0..self.keys.len() {
            let used = self.keys[at];
            if merged > 0 && self.keys[merged - 1].key == used.key {
                self.keys[merged - 1].area += used.area;
            } else {
                self.keys[merged] = used;
                merged += 1;
            }
        }
        self.keys.truncate(merged);
        self.pack();
        self.corners.reserve_exact(quads.len());
        for quad in quads {
            let Some(corners) = Self::cutout_corners(quad) else {
                self.corners.push(CornerTables::NONE);
                continue;
            };
            let sigma = quad.fill_axis.lanes()[2];
            // A corner the frame does not draw adds no area to its key, and
            // reads a table only when a drawn corner pays for one.
            self.corners.push(CornerTables(corners.map(|corner| {
                self.keys
                    .binary_search_by_key(&CutoutKey::new(corner.r, sigma), |used| used.key)
                    .map_or(Self::NONE, |at| self.codes[at])
            })));
        }
    }

    /// The tables to bake.
    pub(crate) fn tables(&self) -> &[BakeTable] {
        &self.tables
    }

    /// Each quad's corner tables, parallel to the quads, or empty when no
    /// quad is a shadow.
    pub(crate) fn corners(&self) -> &[CornerTables] {
        &self.corners
    }

    /// A table's origin cell and side, as `cutout_lookup` unpacks them.
    pub(crate) const fn code(cell: [u32; 2], side: u32) -> u32 {
        cell[0] | cell[1] << 6 | side << 12
    }

    /// A table's side for `(r, σ)`: one texel per `σ / TEXELS_PER_SIGMA`
    /// across the cutout's square widened by `reach` on both sides, plus
    /// the texel bilinear filtering reads past the last one.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a key's radius and σ are positive, so the span is too"
    )]
    fn side(key: CutoutKey) -> u32 {
        let sigma = key.sigma();
        let reach = ShadowGeom::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        ((key.r() + 2.0 * reach) * (Self::TEXELS_PER_SIGMA / sigma)) as u32 + 2
    }

    /// Whether `used`'s table is cheaper to bake than its corners are to
    /// shade, and within the side budget.
    fn pays(used: KeyUse) -> bool {
        let side = Self::side(used.key);
        side <= Self::MAX_SIDE
            && used.area * Self::SHADED_NODES as f32 > (side * side * Self::BAKED_NODES) as f32
    }

    /// Shelf-pack the tables that pay into the atlas's cells, tallest
    /// first, and give each key its code. A key that does not pay, or does
    /// not fit, keeps [`Self::NONE`].
    fn pack(&mut self) {
        self.codes.clear();
        self.codes.resize(self.keys.len(), Self::NONE);
        self.order.clear();
        let keys = &self.keys;
        self.order
            .extend((0..keys.len() as u32).filter(|&at| Self::pays(keys[at as usize])));
        self.order
            .sort_unstable_by_key(|&at| std::cmp::Reverse(Self::side(keys[at as usize].key)));
        let (mut x, mut y, mut shelf) = (0, 0, 0);
        for &at in &self.order {
            let key = self.keys[at as usize].key;
            let side = Self::side(key);
            let cells = side.div_ceil(Self::CELL);
            if x + cells > Self::CELLS {
                (x, y, shelf) = (0, y + shelf, 0);
            }
            if y + cells > Self::CELLS {
                break;
            }
            let code = Self::code([x, y], side);
            self.codes[at as usize] = code;
            self.tables.push(BakeTable {
                table: code,
                r: key.r(),
                sigma: key.sigma(),
            });
            x += cells;
            shelf = shelf.max(cells);
        }
    }

    /// A shadow quad's corners `(tl, tr, br, bl)` as `fs_shadow` cuts them
    /// out, or `None` for a quad that cuts none out: not a shadow, or
    /// blurred below [`Self::MIN_SIGMA`]. The radii follow the shader's
    /// arithmetic; a corner's region is its `r`×`r` square at the corner of
    /// the shadow's box, widened by `reach`.
    fn cutout_corners(quad: &Quad) -> Option<[CutoutCorner; 4]> {
        let [x, y, sigma, spread] = quad.fill_axis.lanes();
        if !quad.fill_kind.is_shadow() || sigma.is_nan() || sigma < Self::MIN_SIGMA {
            return None;
        }
        let half = Vec2::new(quad.rect.size.w, quad.rect.size.h) * 0.5;
        let radius = quad.corners.as_array();
        // A drop shadow's box is centred on its quad; an inset shadow's
        // hole sits `offset` from its source's centre.
        let (centre, box_half, radii) = if quad.fill_kind.tag() == FillKind::TAG_SHADOW_DROP {
            let source = half - Vec2::splat(ShadowGeom::REACH_SIGMAS * sigma + spread.max(0.0));
            let shadow = (source + Vec2::splat(spread)).max(Vec2::ZERO);
            (
                quad.rect.center(),
                shadow,
                fit_radii(spread_radius(radius, spread), shadow),
            )
        } else {
            let hole = (half - Vec2::splat(spread)).max(Vec2::ZERO);
            (
                quad.rect.center() + Vec2::new(x, y),
                hole,
                fit_radii(spread_radius(radius, -spread), hole),
            )
        };
        let reach = ShadowGeom::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        let (lo, hi) = (centre - box_half, centre + box_half);
        // Each corner's square runs `r + reach` inward from its box corner
        // and `reach` outward.
        let span = |edge: f32, r: f32, inward: f32| {
            let far = edge + inward * (r + reach);
            let near = edge - inward * reach;
            (near.min(far), near.max(far))
        };
        let corner = |at: usize, x_edge: f32, x_in: f32, y_edge: f32, y_in: f32| {
            let r = radii[at];
            let (x0, x1) = span(x_edge, r, x_in);
            let (y0, y1) = span(y_edge, r, y_in);
            CutoutCorner {
                r,
                region: Rect::from_min_max(Vec2::new(x0, y0), Vec2::new(x1, y1)),
            }
        };
        Some([
            corner(0, lo.x, 1.0, lo.y, 1.0),
            corner(1, hi.x, -1.0, lo.y, 1.0),
            corner(2, hi.x, -1.0, hi.y, -1.0),
            corner(3, lo.x, 1.0, hi.y, -1.0),
        ])
    }
}

/// `spread_radius` in `shader.wgsl`: CSS's rule for a radius moved by a
/// spread `s`.
fn spread_radius(r: [f32; 4], s: f32) -> [f32; 4] {
    r.map(|r| {
        if s < 0.0 {
            return (r + s).max(0.0);
        }
        let t = r / s.max(1e-30) - 1.0;
        if r >= s {
            r + s
        } else {
            r + s * (1.0 + t * t * t)
        }
    })
}

/// `fit_radii` in `shader.wgsl`: CSS's overlapping-curves scale.
fn fit_radii(r: [f32; 4], half: Vec2) -> [f32; 4] {
    let size = 2.0 * half;
    let sums = [r[0] + r[1], r[3] + r[2], r[0] + r[3], r[1] + r[2]];
    let sides = [size.x, size.x, size.y, size.y];
    let f = sums
        .iter()
        .zip(sides)
        .map(|(&sum, side)| {
            if sum > 0.0 {
                side / sum.max(1e-30)
            } else {
                1.0
            }
        })
        .fold(1.0f32, f32::min);
    r.map(|r| r * f)
}

#[cfg(test)]
mod tests;
