//! Which shadow corners get a baked cutout table this frame, and where in
//! the atlas each table goes.
//!
//! A blurred shadow's coverage from `CUTOUT_MIN_SIGMA` up is its sharp box
//! less four corner cutouts (`cutout_box_coverage` in `shader.wgsl`), and a
//! cutout depends only on the corner's radius `r` and the blur `σ`. So a
//! distinct `(r, σ)` among the frame's shadow corners can get one table,
//! baked every frame by `fs_cutout_bake`, which a corner reads in place of
//! the cutout's quadrature. No table survives the frame: no frame pays a
//! sweep or a repack that another did not.
//!
//! **A table only where it is cheaper.** Shading a corner costs
//! [`CutoutPlan::SHADED_NODES`] per pixel of its region the viewport shows;
//! baking its table costs [`CutoutPlan::BAKED_NODES`] per texel, and a
//! table has `(10 / σ)²` texels per pixel. A key gets a table when the
//! shown area of every corner sharing it costs more to shade than its table
//! to bake. The shown area is the corner's region inside its quad and the
//! viewport, so it still counts the pixels a clip or the source's own
//! interior spares: it can tip a marginal key toward a table, never away.
//!
//! **Only the texels a repainted pixel reads.** A texel's value hangs on
//! its `(r, σ)` and its place in the table alone, so a frame bakes, of each
//! table, the span its repainted pixels read ([`BakeTable`]): a partial
//! repaint over a sliver of a large shadow bakes the sliver's texels, and
//! what it draws is what a full bake would draw.
//!
//! **One answer for a partial and a full repaint.** A table and the shaded
//! cutout round apart by an 8-bit level, so a pixel's form must not depend
//! on how much of the frame repaints. The plan decides from the whole
//! frame's shadows, never from the damage, and a partial frame does not have
//! them: its encoder culls every draw outside the damage. So the plan keeps
//! a census of the last frame's shadows. A partial frame takes the census's
//! shadows it does not repaint, which the damage guarantees are unchanged,
//! and its own that it does, and so holds the census a full repaint of the
//! frame would build. Keys are summed in one order and packed in another
//! that hang on the set alone, never on the order of the quads. When the
//! decision for a key moves under a shadow the frame does not redraw whole,
//! the pixels it keeps would show the old form: the plan reports
//! [`Census::Stale`], and the frame repaints in full.
//!
//! **A shadow all of whose shown corners read tables draws through
//! `fs_shadow_tables`** ([`ShadowEntry`]), which holds no shaded cutout and
//! no outline integral and so runs far cheaper on a tiler. Any other shadow
//! draws through `fs_shadow`. The plan decides both from the one shown area
//! (`CutoutCorner::shown_area`), so a table it skips is never one the
//! cheaper entry needs, and the cheaper entry cuts nothing at a corner no
//! pixel of the viewport needs cut.
//!
//! The keys follow the shader's own arithmetic on the instance's `f16`
//! lanes. A last-bit difference between this and the GPU's arithmetic only
//! reads a table for a radius one ulp away, which moves the cutout far below
//! the table's own error.

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

/// One table to bake: where it goes ([`CutoutPlan::code`]), its `(r, σ)`,
/// and the texels of it to bake, from `lo` up to `hi` on each axis. The
/// instance layout of `vs_cutout_bake`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct BakeTable {
    pub(crate) table: u32,
    pub(crate) r: f32,
    pub(crate) sigma: f32,
    pub(crate) lo: [u16; 2],
    pub(crate) hi: [u16; 2],
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

/// The fragment entry that draws a shadow quad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowEntry {
    /// `fs_shadow_tables`: blurred from [`CutoutPlan::MIN_SIGMA`] to
    /// [`CutoutPlan::SERIES_MIN_SIGMA`], and every corner with a radius
    /// whose region the viewport shows reads a table.
    Tables,
    /// [`Self::Tables`] blurred from [`CutoutPlan::SERIES_MIN_SIGMA`] up,
    /// whose edges take `filter_cdf`'s series form.
    TablesWide,
    /// `fs_shadow`, which draws any shadow: one blurred below the cutout
    /// form, one with a corner that shades its cutout, and the reference
    /// every other form is compared with.
    General,
}

/// One shadow corner as the plan sees it: the radius its cutout is cut
/// with, the screen region where the cutout is not zero, and the centre of
/// its arc with the signs that reflect a pixel into the table's quadrant,
/// as `corner_points` does.
#[derive(Clone, Copy, Debug)]
struct CutoutCorner {
    r: f32,
    region: Rect,
    centre: Vec2,
    sign: Vec2,
}

impl CutoutCorner {
    /// The area of this corner's region inside its `shaded` quad and the
    /// `viewport`: zero for a corner with no radius. The quad's fragments
    /// are the only ones that shade the cutout: an inset hole moved by its
    /// offset, or a corner's reach past the quad, adds nothing. What a table
    /// pays for, and so what `fs_shadow_tables` must find a table for.
    fn shown_area(&self, shaded: Rect, viewport: Rect) -> f32 {
        if self.r <= 0.0 {
            return 0.0;
        }
        self.region
            .intersect(shaded)
            .and_then(|region| region.intersect(viewport))
            .map_or(0.0, Rect::area)
    }

    /// The texels of this corner's `side`-texel table at `σ` that the pixels
    /// centred in `seen` read: `cutout_lookup` takes a pixel at `p` to
    /// `t = (sign · (p − centre) + reach) · TEXELS_PER_SIGMA / σ`, and reads
    /// the texel below `t` and the one past it on each axis. One texel more
    /// on each side covers a last-bit difference from the GPU's arithmetic.
    /// Clamped to the table, so empty where `seen` reads none of it.
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

/// The texels of one table a frame's repainted pixels read, from `lo` up to
/// `hi` on each axis.
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

/// A corner's key and the area of its region the viewport shows: one
/// corner's in the census, the sum of every corner sharing the key once the
/// keys are merged.
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

/// One shadow quad as the census keeps it: the rect its fragments cover,
/// and each corner's key and shown area.
#[derive(Clone, Copy, Debug)]
struct CensusShadow {
    shaded: Rect,
    uses: [KeyUse; 4],
}

/// Whether the pixels a partial repaint leaves alone still show the forms
/// [`CutoutPlan::build`] picked.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Census {
    /// They do, and the plan stands.
    Current,
    /// A key a shadow the frame does not redraw whole reads gained or lost
    /// its table. The plan's outputs are void: the frame must repaint in
    /// full and be planned again.
    Stale,
}

/// The frame's cutout tables and each quad's share of them, and the census
/// of the frame's shadows the next partial repaint builds on. Every buffer
/// is scratch, cleared and refilled per frame with its capacity kept.
#[derive(Debug)]
pub(crate) struct CutoutPlan {
    /// Whether tables are baked at all. Off where the backend cannot render
    /// to its atlas: every corner keeps the shaded cutout.
    bake: bool,
    /// The last planned frame's shadows, each with a corner the viewport
    /// shows.
    census: Vec<CensusShadow>,
    /// This frame's census while it is built.
    next: Vec<CensusShadow>,
    /// Every shown corner's key, then sorted and merged per key.
    keys: Vec<KeyUse>,
    /// [`Self::code`] per entry of `keys`, or [`Self::NONE`].
    codes: Vec<u32>,
    /// Positions in `keys`, tallest table first, for the packer.
    order: Vec<u32>,
    /// Per entry of `keys`, the texels of its table this frame's repainted
    /// pixels read, which it bakes.
    spans: Vec<TexelSpan>,
    /// The rects this frame repaints: the viewport, or the damage's
    /// scissors.
    repaint: Vec<Rect>,
    /// The keys with a table, sorted: this frame's, and the last planned
    /// frame's.
    tabled: Vec<CutoutKey>,
    last_tabled: Vec<CutoutKey>,
    tables: Vec<BakeTable>,
    corners: Vec<CornerTables>,
    /// [`ShadowEntry`] per quad, parallel to `corners`.
    entries: Vec<ShadowEntry>,
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
    /// The σ from which `filter_cdf` takes its series form, more exact in
    /// `f32` than the difference of the box's two ends from here up, and
    /// cheaper. A shadow from here up draws through
    /// [`ShadowEntry::TablesWide`].
    pub(crate) const SERIES_MIN_SIGMA: f32 = 4.0;
    /// The largest table side, in texels. A key past it — a small σ against
    /// a large radius — keeps the shaded cutout.
    const MAX_SIDE: u32 = 256;
    /// Cells on each axis of the atlas.
    const CELLS: u32 = Self::ATLAS_SIZE / Self::CELL;

    /// A plan that bakes tables, or with `bake` off keeps every corner's
    /// shaded cutout.
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

    /// Plan the tables for `quads`, the frame's draw list, which repaints
    /// inside `repaint` of a `viewport` in physical pixels. A partial
    /// frame's list holds only what its damage reaches; the census supplies
    /// the rest. On [`Census::Stale`] the outputs are void.
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
        // A shadow the repaint reaches is in the draw list, so it comes from
        // there rather than from the census. Only one it covers is redrawn
        // whole: one it reaches keeps its pixels outside the scissors. A
        // shadow two scissors cover only together counts as kept, which can
        // only repaint a frame in full that needed none.
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
            // `fs_shadow_tables` cuts nothing at a corner with no table,
            // which is its cutout only where the viewport shows no pixel.
            let mut tabled = true;
            for (code, corner) in codes.iter_mut().zip(&corners) {
                let key = CutoutKey::new(corner.r, sigma);
                match self.keys.binary_search_by_key(&key, |used| used.key) {
                    Ok(at) if self.codes[at] != Self::NONE => {
                        *code = self.codes[at];
                        // Every pixel the quad repaints, not only its corner's
                        // region: past the region's outer edge, the bilinear
                        // filter still reads the table's last two texels.
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

    /// Sum the census's shown area per key and pack the keys that pay. Both
    /// orders hang on the census as a set: the sum runs by key and then by
    /// area, and the packer by side and then by key, so a partial frame
    /// whose census is a full frame's in another order decides the same.
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

    /// The tables to bake.
    pub(crate) fn tables(&self) -> &[BakeTable] {
        &self.tables
    }

    /// Each quad's corner tables, parallel to the quads, or empty when no
    /// quad is a shadow.
    pub(crate) fn corners(&self) -> &[CornerTables] {
        &self.corners
    }

    /// Each quad's [`ShadowEntry`], parallel to [`Self::corners`]. A quad
    /// that is not a shadow reads [`ShadowEntry::General`].
    pub(crate) fn entries(&self) -> &[ShadowEntry] {
        &self.entries
    }

    /// A table's origin cell and side, as `cutout_origin` and
    /// `cutout_side` unpack them.
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

    /// `quad` as the census keeps it, or `None` for a quad that cuts no
    /// corner out or none the `viewport` shows.
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

    /// A shadow quad's corners `(tl, tr, br, bl)` as the shader cuts them
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
        // The arc's centre sits `r` inward from the box corner, and a pixel
        // reflects into the table's quadrant against the inward direction.
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

// A code's fields hold every cell and side the packer hands out, and no
// code reads as `NONE`.
const _: () = {
    assert!(CutoutPlan::CELLS <= 1 << 6);
    assert!(CutoutPlan::MAX_SIDE < 1 << 20);
    assert!(CutoutPlan::MAX_SIDE <= u16::MAX as u32);
    assert!(CutoutPlan::code([63, 63], CutoutPlan::MAX_SIDE) != CutoutPlan::NONE);
};

/// A texel index within a table, which `MAX_SIDE` keeps inside `u16`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a table's side is at most `MAX_SIDE`"
)]
const fn texel(at: u32) -> u16 {
    at as u16
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
