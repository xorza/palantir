//! Which shadow corners get a baked cutout table this frame, and where in the atlas each goes.
//!
//! From `CUTOUT_MIN_SIGMA` up, a blurred shadow is its sharp box less four corner cutouts (`cutout_box_coverage` in `shadow.wgsl`), which depend only on radius `r` and blur `σ`. Each distinct `(r, σ)` can get a table baked by `fs_cutout_bake` into the pool's mask pages, where [`CutoutPlacements`](crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements) keeps it for its key across frames and windows with the texels baked, so a table a frame needs again is not baked again (`CutoutTables`).
//!
//! A key gets a table only when its corners' shown area costs more to shade ([`CutoutPlan::SHADED_NODES`] per pixel) than to bake ([`CutoutPlan::BAKED_NODES`] per texel, `(10 / σ)²` texels per pixel). A frame bakes only the texels its repainted pixels read ([`BakeTable`]).
//!
//! A partial repaint must pick the same forms as a full one (a table and the shaded cutout differ by an 8-bit level), but the encoder culls draws outside the damage. So the plan keeps a census of the last frame's shadows; if a key's decision moves under a shadow not redrawn whole, [`Census::Stale`] makes the frame repaint in full.
//!
//! A shadow whose shown corners all read tables draws through the cheaper `fs_shadow_tables` ([`ShadowEntry`]), else `fs_shadow`.

use crate::gpu::atlas::LIST_CONFIGS;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::Placement;
use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacing;
use crate::primitives::geometry::corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::render_buffer::quad_draw_row::{QuadDrawRow, QuadKey};
use crate::renderer::render_buffer::shadow_instance::{ShadowInstance, ShadowKind};
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use bytemuck::{Pod, Zeroable};
use glam::{U16Vec2, UVec2, Vec2};
use soa_rs::Soa;
use std::cmp::Reverse;
use std::mem;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct BakeTable {
    pub(crate) table: u32,
    pub(crate) r: f32,
    pub(crate) sigma: f32,
    pub(crate) lo: U16Vec2,
    pub(crate) hi: U16Vec2,
}

impl BakeTable {
    /// The key whose table this is.
    pub(crate) const fn key(self) -> CutoutKey {
        CutoutKey::new(self.r, self.sigma)
    }

    /// Whether `other` is the same table at the same place: its code, radius and σ alike.
    pub(crate) const fn is_at(self, other: Self) -> bool {
        self.table == other.table
            && self.r.to_bits() == other.r.to_bits()
            && self.sigma.to_bits() == other.sigma.to_bits()
    }

    /// Whether this span holds `other`'s, a table [at](Self::is_at) this one's place.
    pub(crate) fn holds(self, other: Self) -> bool {
        self.lo.cmple(other.lo).all() && other.hi.cmple(self.hi).all()
    }

    /// This table with the least span that holds its own and `other`'s.
    pub(crate) fn joined(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
            ..self
        }
    }
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

/// One shadow corner: cutout radius, the region where the cutout is nonzero, and the arc center with reflection signs.
#[derive(Clone, Copy, Debug)]
struct CutoutCorner {
    r: f32,
    region: Rect,
    center: Vec2,
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

    /// Texels of this corner's `side`-texel table at `σ` that pixels centered in `seen` read: `cutout_lookup` maps `p` to `t = (sign · (p − center) + reach) · TEXELS_PER_SIGMA / σ` and reads the texels either side, plus one per side for GPU last-bit differences.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a side is at most `MAX_SIDE`, exact in f32"
    )]
    fn texels(&self, seen: Rect, sigma: f32, side: u32) -> TexelSpan {
        let reach = ShadowGeometry::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        let scale = CutoutPlan::TEXELS_PER_SIGMA / sigma;
        let a = (seen.min - self.center) * self.sign;
        let b = (seen.max() - self.center) * self.sign;
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

/// A cutout table's radius and σ, by their bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CutoutKey {
    r: u32,
    sigma: u32,
}

impl CutoutKey {
    pub(crate) const fn new(r: f32, sigma: f32) -> Self {
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

/// How shadow corners get their cutout: every build bakes; the shaded reference exists only in
/// the builds that test against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CutoutMode {
    /// From tables baked where they pay, in the pool's mask pages.
    Bake,
    /// Shaded per pixel: the reference the visual suite holds the tables to.
    #[cfg(feature = "internals")]
    Shade,
}

/// The frame a [`CutoutPlan`] is built for: its quads, repainted inside `repaint` of a
/// physical-pixel `viewport`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CutoutFrame<'a> {
    pub(crate) quads: &'a Soa<QuadDrawRow>,
    pub(crate) repaint: &'a RepaintScissors,
    pub(crate) viewport: UVec2,
}

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Census {
    Cur,
    /// A shadow not redrawn whole gained or lost a table: the frame repaints in full.
    Stale,
}

#[derive(Debug)]
pub(crate) struct CutoutPlan {
    mode: CutoutMode,
    census: Vec<CensusShadow>,
    next: Vec<CensusShadow>,
    keys: Vec<KeyUse>,
    codes: Vec<u32>,
    order: Vec<u32>,
    /// The keys this plan holds a placement for, sorted: the tables of its last committed frame,
    /// and any a stale frame placed since.
    held: Vec<CutoutKey>,
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
    /// A corner with no table; never a table's [`code`](Self::code).
    pub(crate) const NONE: u32 = u32::MAX;
    /// Bits of each of a code's origin coordinates, which reach a page edge of `1 << 11`.
    pub(crate) const COORD_BITS: u32 = 11;
    /// Where a code's `side − 1` sits.
    pub(crate) const SIDE_SHIFT: u32 = 2 * Self::COORD_BITS;
    /// Where a code's page sits, past `side − 1`'s 8 bits.
    pub(crate) const PAGE_SHIFT: u32 = Self::SIDE_SHIFT + 8;
    /// Midpoint nodes of the shaded cutout (`CUTOUT_NODES`).
    pub(crate) const SHADED_NODES: u32 = 12;
    pub(crate) const BAKED_NODES: u32 = 24;
    pub(crate) const SERIES_MIN_SIGMA: f32 = 4.0;
    const MAX_SIDE: u32 = 1 << (Self::PAGE_SHIFT - Self::SIDE_SHIFT);

    /// A plan that bakes its tables where they pay.
    pub(crate) const fn new() -> Self {
        Self::with_mode(CutoutMode::Bake)
    }

    const fn with_mode(mode: CutoutMode) -> Self {
        Self {
            mode,
            census: Vec::new(),
            next: Vec::new(),
            keys: Vec::new(),
            codes: Vec::new(),
            order: Vec::new(),
            held: Vec::new(),
            spans: Vec::new(),
            repaint: Vec::new(),
            tabled: Vec::new(),
            last_tabled: Vec::new(),
            tables: Vec::new(),
            corners: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Plans the tables of `frame`, placed through `placing`; the census supplies shadows a partial frame doesn't redraw. On [`Census::Stale`] the outputs are void.
    pub(crate) fn build(
        &mut self,
        frame: CutoutFrame<'_>,
        placing: &mut CutoutPlacing<'_>,
    ) -> Census {
        let CutoutFrame {
            quads,
            repaint,
            viewport,
        } = frame;
        self.tables.clear();
        self.corners.clear();
        self.entries.clear();
        #[cfg(feature = "internals")]
        if matches!(self.mode, CutoutMode::Shade) {
            if quads.key().contains(&QuadKey::Shadow) {
                self.corners.resize(quads.len(), CornerTables::NONE);
                self.entries.resize(quads.len(), ShadowEntry::General);
            }
            return Census::Cur;
        }
        let viewport = Rect::from_min_max(Vec2::ZERO, viewport.as_vec2());
        let partial = match repaint {
            RepaintScissors::Full => None,
            RepaintScissors::Partial(rects) => Some(rects),
        };
        // A shadow the scissors cover whole is redrawn. They are disjoint, so the pixels it touches lie under them all exactly when their parts inside it add up to it.
        let reached = |shaded: Rect| match partial {
            None => true,
            Some(rects) => rects.iter().any(|rect| shaded.intersects(Rect::from(rect))),
        };
        let covered = |shaded: Rect| match partial {
            None => true,
            Some(rects) => shaded.intersect(viewport).is_none_or(|shown| {
                let touched = URect::covering(shown);
                rects
                    .iter()
                    .filter_map(|rect| rect.intersect(touched))
                    .map(URect::area)
                    .sum::<u64>()
                    == touched.area()
            }),
        };
        self.next.clear();
        self.next
            .extend(self.census.iter().filter(|shadow| !reached(shadow.shaded)));
        for shadow in shadows(quads).flatten() {
            if let Some(shadow) = Self::census_shadow(&shadow, viewport)
                && reached(shadow.shaded)
            {
                self.next.push(shadow);
            }
        }
        self.decide(placing);
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
        self.release_untabled(placing);
        if !quads.key().contains(&QuadKey::Shadow) {
            return Census::Cur;
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
        for shadow in shadows(quads) {
            let Some((shadow, corners)) = shadow
                .and_then(|shadow| Self::cutout_corners(&shadow).map(|corners| (shadow, corners)))
            else {
                self.corners.push(CornerTables::NONE);
                self.entries.push(ShadowEntry::General);
                continue;
            };
            let sigma = shadow.sigma();
            let shaded = shadow.header.shaded_rect();
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
                    lo: U16Vec2::from_array(span.lo.to_array().map(texel)),
                    hi: U16Vec2::from_array(span.hi.to_array().map(texel)),
                });
            }
        }
        Census::Cur
    }

    fn decide(&mut self, placing: &mut CutoutPlacing<'_>) {
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
        self.place(placing);
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

    /// The code of a table of `side` texels at `placement` in the mask pages, as `vs_shadow`
    /// carries it: the origin's `x` and `y` in [`Self::COORD_BITS`] each, then `side − 1`, then
    /// the page.
    pub(crate) fn code(placement: Placement, side: u32) -> u32 {
        let origin = placement.origin.as_uvec2();
        debug_assert!(
            origin.max_element() < 1 << Self::COORD_BITS,
            "a table at {origin} past the code's coordinates"
        );
        debug_assert!(
            (1..=Self::MAX_SIDE).contains(&side),
            "a table of side {side}"
        );
        debug_assert!(
            usize::from(placement.page) < MAX_PAGES,
            "a table on page {}",
            placement.page
        );
        origin.x
            | (origin.y << Self::COORD_BITS)
            | ((side - 1) << Self::SIDE_SHIFT)
            | (u32::from(placement.page) << Self::PAGE_SHIFT)
    }

    /// The page a table [`code`](Self::code) sits on.
    pub(crate) const fn table_page(table: u32) -> usize {
        (table >> Self::PAGE_SHIFT) as usize
    }

    /// A table's side for `(r, σ)`: a texel per `σ / TEXELS_PER_SIGMA` across the cutout square widened by `reach`, plus one for bilinear.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a key's radius and σ are positive, so the span is too"
    )]
    fn side(key: CutoutKey) -> u32 {
        let sigma = key.sigma();
        let reach = ShadowGeometry::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        ((key.r() + 2.0 * reach) * (Self::TEXELS_PER_SIGMA / sigma)) as u32 + 2
    }

    fn pays(used: KeyUse) -> bool {
        let side = Self::side(used.key);
        side <= Self::MAX_SIDE
            && used.area * Self::SHADED_NODES as f32 > (side * side * Self::BAKED_NODES) as f32
    }

    /// The paying tables' places in the mask pages, tallest first, as a shelf fills best: a key
    /// this plan holds keeps its placement, a new one takes a use of its key's. A key that does not
    /// pay or find room keeps [`Self::NONE`].
    fn place(&mut self, placing: &mut CutoutPlacing<'_>) {
        debug_assert_eq!(
            self.mode,
            CutoutMode::Bake,
            "a plan that shades places no table"
        );
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
        for &at in &self.order {
            let key = self.keys[at as usize].key;
            let code = match self.held.binary_search(&key) {
                Ok(_) => placing.code(key),
                Err(slot) => match placing.acquire(key, Self::side(key)) {
                    Some(code) => {
                        self.held.insert(slot, key);
                        code
                    }
                    None => continue,
                },
            };
            self.codes[at as usize] = code;
        }
    }

    /// Give back through `placing` every key this plan holds, as the window whose frames it
    /// planned closes on the device that holds them.
    pub(crate) fn release_all(&mut self, placing: &mut CutoutPlacing<'_>) {
        for key in self.held.drain(..) {
            placing.release(key);
        }
    }

    /// Give back through `placing` the keys this plan holds and its committed frame no longer
    /// tables.
    fn release_untabled(&mut self, placing: &mut CutoutPlacing<'_>) {
        let tabled = &self.last_tabled;
        self.held.retain(|&key| {
            let keep = tabled.binary_search(&key).is_ok();
            if !keep {
                placing.release(key);
            }
            keep
        });
    }

    fn census_shadow(shadow: &ShadowInstance, viewport: Rect) -> Option<CensusShadow> {
        let corners = Self::cutout_corners(shadow)?;
        let sigma = shadow.sigma();
        let shaded = shadow.header.shaded_rect();
        let uses = corners.map(|corner| KeyUse {
            key: CutoutKey::new(corner.r, sigma),
            area: corner.shown_area(shaded, viewport),
        });
        uses.iter()
            .any(|used| used.area > 0.0)
            .then_some(CensusShadow { shaded, uses })
    }

    /// A shadow's corners `(tl, tr, br, bl)` as the shader cuts them, or `None` if blurred below [`Self::MIN_SIGMA`].
    fn cutout_corners(quad: &ShadowInstance) -> Option<[CutoutCorner; 4]> {
        let sigma = quad.sigma();
        if sigma.is_nan() || sigma < Self::MIN_SIGMA {
            return None;
        }
        let shadow = ShadowBox::of(quad);
        let center = quad.header.rect.center() + shadow.center;
        let reach = ShadowGeometry::REACH_SIGMAS * sigma + AA_HALF_WIDTH;
        let (lo, hi) = (center - shadow.half, center + shadow.half);
        // Each square runs `r + reach` inward and `reach` outward.
        let span = |edge: f32, r: f32, inward: f32| {
            let far = edge + inward * (r + reach);
            let near = edge - inward * reach;
            (near.min(far), near.max(far))
        };
        // The arc center is `r` inward; pixels reflect against the inward direction.
        let corner = |at: usize, x_edge: f32, x_in: f32, y_edge: f32, y_in: f32| {
            let r = shadow.radius[at];
            let (x0, x1) = span(x_edge, r, x_in);
            let (y0, y1) = span(y_edge, r, y_in);
            CutoutCorner {
                r,
                region: Rect::from_min_max(Vec2::new(x0, y0), Vec2::new(x1, y1)),
                center: Vec2::new(x_edge + x_in * r, y_edge + y_in * r),
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

/// Each quad of `quads` as the shadow it holds, in stream order; `None` for a fill.
fn shadows(quads: &Soa<QuadDrawRow>) -> impl Iterator<Item = Option<ShadowInstance>> + '_ {
    quads
        .key()
        .iter()
        .zip(quads.instance())
        .map(|(&key, &quad)| (key == QuadKey::Shadow).then(|| ShadowInstance::from(quad)))
}

/// The box whose blur a shadow paints, relative to its quad's center, with the radii fitted to
/// it: the shader's `shadow_box`, which `shadow_box_follows_the_shader` runs against this.
#[derive(Clone, Copy, Debug)]
struct ShadowBox {
    center: Vec2,
    half: Vec2,
    radius: [f32; 4],
}

impl ShadowBox {
    /// A drop shadow's box is its source grown by the spread, centered on the quad; an inset
    /// shadow's is the hole, its source shrunk by the spread and moved by `offset`.
    fn of(quad: &ShadowInstance) -> Self {
        let [x, y, sigma, spread] = quad.geometry.lanes();
        let half = Vec2::new(quad.header.rect.size.w, quad.header.rect.size.h) * 0.5;
        let (center, half, spread) = if quad.kind == ShadowKind::DROP {
            let source = half - Vec2::splat(ShadowGeometry::REACH_SIGMAS * sigma + spread.max(0.0));
            (
                Vec2::ZERO,
                (source + Vec2::splat(spread)).max(Vec2::ZERO),
                spread,
            )
        } else {
            (
                Vec2::new(x, y),
                (half - Vec2::splat(spread)).max(Vec2::ZERO),
                -spread,
            )
        };
        Self {
            center,
            half,
            radius: corners::fit(
                spread_radius(quad.header.corners.to_array(), spread),
                Size::new(2.0 * half.x, 2.0 * half.y),
            ),
        }
    }
}

const _: () = {
    let edge = LIST_CONFIGS[AtlasFormat::R8 as usize].page_edge;
    assert!(
        edge <= 1 << CutoutPlan::COORD_BITS,
        "a mask page's origins fit a code"
    );
    assert!(
        CutoutPlan::MAX_SIDE <= edge,
        "a mask page holds the largest table"
    );
    assert!(
        MAX_PAGES == 1 << (u32::BITS - CutoutPlan::PAGE_SHIFT),
        "a code's page bits name every page"
    );
    // `NONE` reads as a table of the largest side at the last origin a coordinate holds, which
    // runs past every mask page's edge, so no placed table's code is all ones.
    assert!((1 << CutoutPlan::COORD_BITS) - 1 + CutoutPlan::MAX_SIDE > edge);
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, CutoutPlan};

    #[cfg(test)]
    use glam::UVec2;

    impl CutoutPlan {
        /// A plan that shades every corner: the reference the visual suite holds the tables to.
        pub(crate) const fn shading() -> Self {
            Self::with_mode(CutoutMode::Shade)
        }

        /// The first texel of a table [`code`](Self::code) on its page.
        #[cfg(test)]
        pub(crate) const fn table_origin(table: u32) -> UVec2 {
            let mask = (1 << Self::COORD_BITS) - 1;
            UVec2::new(table & mask, (table >> Self::COORD_BITS) & mask)
        }

        /// The side in texels of a table [`code`](Self::code).
        #[cfg(test)]
        pub(crate) const fn table_side(table: u32) -> u32 {
            ((table >> Self::SIDE_SHIFT) & (Self::MAX_SIDE - 1)) + 1
        }
    }
}

#[cfg(test)]
mod tests;
