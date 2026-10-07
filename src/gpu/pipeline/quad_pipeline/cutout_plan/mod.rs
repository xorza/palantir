//! Which shadow corners get a baked cutout table this frame, and where in the atlas each goes.
//!
//! From `CUTOUT_MIN_SIGMA` up, a blurred shadow is its sharp box less four corner cutouts (`cutout_box_coverage` in `shader.wgsl`), which depend only on radius `r` and blur `σ`. Each distinct `(r, σ)` can get a table baked by `fs_cutout_bake`; none survives the frame.
//!
//! A key gets a table only when its corners' shown area costs more to shade ([`CutoutPlan::SHADED_NODES`] per pixel) than to bake ([`CutoutPlan::BAKED_NODES`] per texel, `(10 / σ)²` texels per pixel). A frame bakes only the texels its repainted pixels read ([`BakeTable`]).
//!
//! A partial repaint must pick the same forms as a full one (a table and the shaded cutout differ by an 8-bit level), but the encoder culls draws outside the damage. So the plan keeps a census of the last frame's shadows; if a key's decision moves under a shadow not redrawn whole, [`Census::Stale`] makes the frame repaint in full.
//!
//! A shadow whose shown corners all read tables draws through the cheaper `fs_shadow_tables` ([`ShadowEntry`]), else `fs_shadow`.

use crate::gpu::surface::viewport::RepaintScissors;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::quad::Quad;
use crate::shape::paint::lowered_shadow::ShadowGeom;
use bytemuck::{Pod, Zeroable};
use glam::{UVec2, Vec2};
use std::cmp::Reverse;
use std::mem;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct BakeTable {
    pub(crate) table: u32,
    pub(crate) r: f32,
    pub(crate) sigma: f32,
    pub(crate) lo: [u16; 2],
    pub(crate) hi: [u16; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct CornerTables(pub(crate) [u32; 4]);

impl CornerTables {
    const NONE: Self = Self([CutoutPlan::NONE; 4]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowEntry {
    Tables,
    TablesWide,
    General,
}

/// One shadow corner: cutout radius, the region where the cutout is nonzero, and the arc centre with reflection signs.
#[derive(Clone, Copy, Debug)]
struct CutoutCorner {
    r: f32,
    region: Rect,
    centre: Vec2,
    sign: Vec2,
}

impl CutoutCorner {
    /// Area of the region inside its `shaded` quad and the `viewport`; zero with no radius. What a table pays for.
    fn shown_area(&self, shaded: Rect, viewport: Rect) -> f32 {
        if self.r <= 0.0 {
            return 0.0;
        }
        self.region
            .intersect(shaded)
            .and_then(|region| region.intersect(viewport))
            .map_or(0.0, Rect::area)
    }

    /// Texels of this corner's `side`-texel table at `σ` that pixels centred in `seen` read: `cutout_lookup` maps `p` to `t = (sign · (p − centre) + reach) · TEXELS_PER_SIGMA / σ` and reads the texels either side, plus one per side for GPU last-bit differences.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a side is at most `MAX_SIDE`, exact in f32"
    )]
    fn texels(&self, seen: Rect, sigma: f32, side: u32) -> TexelSpan {
        let reach = ShadowGeom::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        let scale = CutoutPlan::TEXELS_PER_SIGMA / sigma;
        let a = (seen.min - self.centre) * self.sign;
        let b = (seen.max() - self.centre) * self.sign;
        let side = Vec2::splat(side as f32);
        let lo = ((a.min(b) + reach) * scale).floor() - 1.0;
        let hi = ((a.max(b) + reach) * scale).floor() + 3.0;
        TexelSpan {
            lo: lo.clamp(Vec2::ZERO, side).as_uvec2(),
            hi: hi.clamp(Vec2::ZERO, side).as_uvec2(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TexelSpan {
    lo: UVec2,
    hi: UVec2,
}

impl TexelSpan {
    const EMPTY: Self = Self {
        lo: UVec2::MAX,
        hi: UVec2::ZERO,
    };

    const fn is_empty(self) -> bool {
        self.lo.x >= self.hi.x || self.lo.y >= self.hi.y
    }

    fn union(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct KeyUse {
    key: CutoutKey,
    area: f32,
}

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

#[derive(Clone, Copy, Debug)]
struct CensusShadow {
    shaded: Rect,
    uses: [KeyUse; 4],
}

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Census {
    Current,
    /// A shadow not redrawn whole gained or lost a table: the frame repaints in full.
    Stale,
}

#[derive(Debug)]
pub(crate) struct CutoutPlan {
    /// Whether tables are baked; off where the backend can't render to its atlas.
    bake: bool,
    census: Vec<CensusShadow>,
    next: Vec<CensusShadow>,
    keys: Vec<KeyUse>,
    codes: Vec<u32>,
    order: Vec<u32>,
    spans: Vec<TexelSpan>,
    repaint: Vec<Rect>,
    tabled: Vec<CutoutKey>,
    last_tabled: Vec<CutoutKey>,
    tables: Vec<BakeTable>,
    corners: Vec<CornerTables>,
    entries: Vec<ShadowEntry>,
}

impl CutoutPlan {
    pub(crate) const MIN_SIGMA: f32 = 0.25;
    pub(crate) const TEXELS_PER_SIGMA: f32 = 10.0;
    pub(crate) const ATLAS_SIZE: u32 = 1024;
    pub(crate) const CELL: u32 = 16;
    pub(crate) const NONE: u32 = u32::MAX;
    /// Midpoint nodes of the shaded cutout (`CUTOUT_NODES`).
    pub(crate) const SHADED_NODES: u32 = 12;
    pub(crate) const BAKED_NODES: u32 = 24;
    pub(crate) const SERIES_MIN_SIGMA: f32 = 4.0;
    const MAX_SIDE: u32 = 256;
    const CELLS: u32 = Self::ATLAS_SIZE / Self::CELL;

    pub(crate) const fn new(bake: bool) -> Self {
        Self {
            bake,
            census: Vec::new(),
            next: Vec::new(),
            keys: Vec::new(),
            codes: Vec::new(),
            order: Vec::new(),
            spans: Vec::new(),
            repaint: Vec::new(),
            tabled: Vec::new(),
            last_tabled: Vec::new(),
            tables: Vec::new(),
            corners: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Plans the tables for `quads` repainting inside `repaint` of a physical-pixel `viewport`; the census supplies shadows a partial frame doesn't redraw. On [`Census::Stale`] the outputs are void.
    pub(crate) fn build(
        &mut self,
        quads: &[Quad],
        repaint: &RepaintScissors,
        viewport: UVec2,
    ) -> Census {
        self.tables.clear();
        self.corners.clear();
        self.entries.clear();
        if !self.bake {
            if quads.iter().any(|quad| quad.fill_kind.is_shadow()) {
                self.corners.resize(quads.len(), CornerTables::NONE);
                self.entries.resize(quads.len(), ShadowEntry::General);
            }
            return Census::Current;
        }
        let viewport = Rect::from_min_max(Vec2::ZERO, viewport.as_vec2());
        let partial = match repaint {
            RepaintScissors::Full => None,
            RepaintScissors::Partial(rects) => Some(rects),
        };
        // Only a shadow one scissor covers whole is redrawn; two scissors covering it together count as kept, which at worst repaints in full needlessly.
        let reached = |shaded: Rect| match partial {
            None => true,
            Some(rects) => rects.iter().any(|rect| shaded.intersects(Rect::from(rect))),
        };
        let covered = |shaded: Rect| match partial {
            None => true,
            Some(rects) => shaded.intersect(viewport).is_none_or(|shown| {
                rects
                    .iter()
                    .any(|rect| Rect::from(rect).contains_rect(shown))
            }),
        };
        self.next.clear();
        self.next
            .extend(self.census.iter().filter(|shadow| !reached(shadow.shaded)));
        for quad in quads {
            if let Some(shadow) = Self::census_shadow(quad, viewport)
                && reached(shadow.shaded)
            {
                self.next.push(shadow);
            }
        }
        self.decide();
        if partial.is_some() {
            let last = &self.last_tabled;
            let moved = self
                .census
                .iter()
                .filter(|shadow| !covered(shadow.shaded))
                .flat_map(|shadow| shadow.uses)
                .filter(|used| used.area > 0.0)
                .any(|used| {
                    self.tabled.binary_search(&used.key).is_ok()
                        != last.binary_search(&used.key).is_ok()
                });
            if moved {
                return Census::Stale;
            }
        }
        mem::swap(&mut self.census, &mut self.next);
        mem::swap(&mut self.tabled, &mut self.last_tabled);
        if !quads.iter().any(|quad| quad.fill_kind.is_shadow()) {
            return Census::Current;
        }
        self.repaint.clear();
        match partial {
            None => self.repaint.push(viewport),
            Some(rects) => self.repaint.extend(rects.iter().map(Rect::from)),
        }
        self.spans.clear();
        self.spans.resize(self.keys.len(), TexelSpan::EMPTY);
        self.corners.reserve_exact(quads.len());
        self.entries.reserve_exact(quads.len());
        for quad in quads {
            let Some(corners) = Self::cutout_corners(quad) else {
                self.corners.push(CornerTables::NONE);
                self.entries.push(ShadowEntry::General);
                continue;
            };
            let sigma = quad.fill_axis.lanes()[2];
            let shaded = quad.shaded_rect();
            let mut codes = CornerTables::NONE.0;
            let mut tabled = true;
            for (code, corner) in codes.iter_mut().zip(&corners) {
                let key = CutoutKey::new(corner.r, sigma);
                match self.keys.binary_search_by_key(&key, |used| used.key) {
                    Ok(at) if self.codes[at] != Self::NONE => {
                        *code = self.codes[at];
                        let side = Self::side(key);
                        for &rect in &self.repaint {
                            let Some(seen) = shaded.intersect(rect) else {
                                continue;
                            };
                            let texels = corner.texels(seen, sigma, side);
                            if !texels.is_empty() {
                                self.spans[at] = self.spans[at].union(texels);
                            }
                        }
                    }
                    _ => tabled &= corner.shown_area(shaded, viewport) <= 0.0,
                }
            }
            self.corners.push(CornerTables(codes));
            self.entries.push(if !tabled {
                ShadowEntry::General
            } else if sigma >= Self::SERIES_MIN_SIGMA {
                ShadowEntry::TablesWide
            } else {
                ShadowEntry::Tables
            });
        }
        for (used, (&table, span)) in self.keys.iter().zip(self.codes.iter().zip(&self.spans)) {
            if !span.is_empty() {
                self.tables.push(BakeTable {
                    table,
                    r: used.key.r(),
                    sigma: used.key.sigma(),
                    lo: span.lo.to_array().map(texel),
                    hi: span.hi.to_array().map(texel),
                });
            }
        }
        Census::Current
    }

    fn decide(&mut self) {
        self.keys.clear();
        self.keys.extend(
            self.next
                .iter()
                .flat_map(|shadow| shadow.uses)
                .filter(|used| used.area > 0.0),
        );
        self.keys
            .sort_unstable_by(|a, b| a.key.cmp(&b.key).then(a.area.total_cmp(&b.area)));
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
        self.tabled.clear();
        self.tabled.extend(
            self.keys
                .iter()
                .zip(&self.codes)
                .filter(|&(_, &code)| code != Self::NONE)
                .map(|(used, _)| used.key),
        );
    }

    pub(crate) fn tables(&self) -> &[BakeTable] {
        &self.tables
    }

    pub(crate) fn corners(&self) -> &[CornerTables] {
        &self.corners
    }

    pub(crate) fn entries(&self) -> &[ShadowEntry] {
        &self.entries
    }

    pub(crate) const fn code(cell: [u32; 2], side: u32) -> u32 {
        cell[0] | cell[1] << 6 | side << 12
    }

    /// A table's side for `(r, σ)`: a texel per `σ / TEXELS_PER_SIGMA` across the cutout square widened by `reach`, plus one for bilinear.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a key's radius and σ are positive, so the span is too"
    )]
    fn side(key: CutoutKey) -> u32 {
        let sigma = key.sigma();
        let reach = ShadowGeom::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        ((key.r() + 2.0 * reach) * (Self::TEXELS_PER_SIGMA / sigma)) as u32 + 2
    }

    fn pays(used: KeyUse) -> bool {
        let side = Self::side(used.key);
        side <= Self::MAX_SIDE
            && used.area * Self::SHADED_NODES as f32 > (side * side * Self::BAKED_NODES) as f32
    }

    /// Shelf-packs the paying tables into the atlas, tallest first; a key that doesn't pay or fit keeps [`Self::NONE`].
    fn pack(&mut self) {
        self.codes.clear();
        self.codes.resize(self.keys.len(), Self::NONE);
        self.order.clear();
        let keys = &self.keys;
        self.order
            .extend((0..keys.len() as u32).filter(|&at| Self::pays(keys[at as usize])));
        self.order.sort_unstable_by_key(|&at| {
            let key = keys[at as usize].key;
            (Reverse(Self::side(key)), key)
        });
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
            self.codes[at as usize] = Self::code([x, y], side);
            x += cells;
            shelf = shelf.max(cells);
        }
    }

    fn census_shadow(quad: &Quad, viewport: Rect) -> Option<CensusShadow> {
        let corners = Self::cutout_corners(quad)?;
        let sigma = quad.fill_axis.lanes()[2];
        let shaded = quad.shaded_rect();
        let uses = corners.map(|corner| KeyUse {
            key: CutoutKey::new(corner.r, sigma),
            area: corner.shown_area(shaded, viewport),
        });
        uses.iter()
            .any(|used| used.area > 0.0)
            .then_some(CensusShadow { shaded, uses })
    }

    /// A shadow quad's corners `(tl, tr, br, bl)` as the shader cuts them, or `None` if not a shadow or blurred below [`Self::MIN_SIGMA`].
    fn cutout_corners(quad: &Quad) -> Option<[CutoutCorner; 4]> {
        let [x, y, sigma, spread] = quad.fill_axis.lanes();
        if !quad.fill_kind.is_shadow() || sigma.is_nan() || sigma < Self::MIN_SIGMA {
            return None;
        }
        let half = Vec2::new(quad.rect.size.w, quad.rect.size.h) * 0.5;
        let radius = quad.corners.as_array();
        // A drop shadow's box is centred on its quad; an inset hole is `offset` from its source's centre.
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
        // Each square runs `r + reach` inward and `reach` outward.
        let span = |edge: f32, r: f32, inward: f32| {
            let far = edge + inward * (r + reach);
            let near = edge - inward * reach;
            (near.min(far), near.max(far))
        };
        // The arc centre is `r` inward; pixels reflect against the inward direction.
        let corner = |at: usize, x_edge: f32, x_in: f32, y_edge: f32, y_in: f32| {
            let r = radii[at];
            let (x0, x1) = span(x_edge, r, x_in);
            let (y0, y1) = span(y_edge, r, y_in);
            CutoutCorner {
                r,
                region: Rect::from_min_max(Vec2::new(x0, y0), Vec2::new(x1, y1)),
                centre: Vec2::new(x_edge + x_in * r, y_edge + y_in * r),
                sign: Vec2::new(-x_in, -y_in),
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

const _: () = {
    assert!(CutoutPlan::CELLS <= 1 << 6);
    assert!(CutoutPlan::MAX_SIDE < 1 << 20);
    assert!(CutoutPlan::MAX_SIDE <= u16::MAX as u32);
    assert!(CutoutPlan::code([63, 63], CutoutPlan::MAX_SIDE) != CutoutPlan::NONE);
};

#[expect(
    clippy::cast_possible_truncation,
    reason = "a table's side is at most `MAX_SIDE`"
)]
const fn texel(at: u32) -> u16 {
    at as u16
}

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
