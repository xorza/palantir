//! Real text shaping via [`cosmic_text`]. Caches one shaped `Buffer` per
//! [`TextShapeKey`], so steady-state measurement is a lookup with no reshape or
//! allocation; residency belongs to [`shaped_buffer_cache`]. Render code never sees
//! cosmic types: `TextShaper::glyphs` lends a `RefMut<CosmicMeasure>` whose
//! [`CosmicMeasure::extract_glyphs`] / [`CosmicMeasure::rasterize_glyph`] return
//! palantir-native placements and bitmaps.
//!
//! The key holds a 64-bit text hash, not the string; verifying on every hit would
//! cost more than the negligible collision risk.

use crate::primitives::math::num::F32Px;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::paint::raster_image::RasterImage;
use crate::text::cosmic::cache_entry::CachedExtent;
use crate::text::cosmic::cluster_glyph::ClusterGlyph;
use crate::text::cosmic::ellipsis_memo::EllipsisMemo;
use crate::text::cosmic::geometry::{
    SegmentScratch, first_line_right, intrinsic_min_width, shaped_geometry,
};
use crate::text::cosmic::glyph_ink::GlyphInk;
use crate::text::cosmic::shaped_buffer_cache::{ShapedBufferCache, ShapedRun};
use crate::text::error::FontLoadError;
use crate::text::extent::TextExtent;
use crate::text::font_family::FontFamily;
use crate::text::font_scope::FontScope;
use crate::text::font_slant::FontSlant;
use crate::text::font_source::FontSource;
use crate::text::font_weight::FontWeight;
use crate::text::key::{LineAlign, TextShapeKey};
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, WrapFloor};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKey, CacheKeyFlags, Family, Font, FontSystem,
    Metrics, Shaping, Style, Weight, fontdb,
};
use glam::{IVec2, UVec2};
use std::fmt;
use std::fs;
use std::sync::Arc;
use swash::scale::image::{Content, Image as SwashImage};
use swash::scale::{Render, ScaleContext, Scaler, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};
use tinyvec::ArrayVec;

pub(super) mod cache_entry;
pub(super) mod cluster_glyph;
pub(super) mod counters;
pub(super) mod ellipsis_memo;
pub(super) mod geometry;
pub(super) mod glyph_ink;
pub(super) mod shaped_buffer_cache;

const ELLIPSIS_MEMO_SLOTS: usize = 4;

/// The cosmic face one key shapes at; inverts [`TextShapeKey::unbounded`]'s fold in one place.
const fn metrics_of(key: TextShapeKey) -> Metrics {
    Metrics::new(key.font_size(), key.line_height())
}

/// The attributes a resolved family shapes under. Takes the resolved name so
/// startup warm-up and the per-shape path share it. Fake italic is cosmic's
/// (`override_fake_italic` sets `CacheKeyFlags::FAKE_ITALIC`, part of the glyph
/// cache key, so the atlas keeps slanted rasters apart).
const fn attrs_named(name: &'static str, weight: FontWeight, style: FontSlant) -> Attrs<'static> {
    // Skip TrueType hinting: skrifa's hint VM dominated zoom-frame CPU time and the difference is
    // imperceptible.
    let base = Attrs::new()
        .cache_key_flags(CacheKeyFlags::DISABLE_HINTING)
        .family(Family::Name(name))
        .weight(Weight(weight.get()));
    match style {
        FontSlant::Normal => base,
        FontSlant::Italic => base.style(Style::Italic),
    }
}

/// Whether any face in `db` answers to `name`; a scan, since `Database::query` allocates. The
/// caller memoizes.
fn family_present(db: &fontdb::Database, name: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(known, _)| known == name))
}

/// Build the match keys for `families` at the weights and styles themes ask for;
/// cosmic otherwise pays O(faces) on whichever frame first draws a face (see
/// [`FontScope::build`]). Takes families, not the interned table, which holds every
/// name on the machine.
pub(crate) fn warm_matches(font_system: &mut FontSystem, families: &[FontFamily]) {
    for &family in families {
        let present = family_present(font_system.db(), family.name());
        let name = shaping_name(family, present);
        for weight in [FontWeight::REGULAR, FontWeight::BOLD] {
            for style in [FontSlant::Normal, FontSlant::Italic] {
                font_system.get_font_matches(&attrs_named(name, weight, style));
            }
        }
    }
}

/// Whether a face answers to a family. Three states, not `Option<bool>`: the third is "not asked
/// yet".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FamilyResolution {
    #[default]
    Unasked,
    Present,
    Missing,
}

/// The family a request shapes in: itself if a face answers, else
/// [`FontFamily::SANS`]. Never cosmic's platform fallback, which would make a
/// missing family look like whatever the machine has installed.
fn shaping_name(family: FontFamily, present: bool) -> &'static str {
    if present {
        family.name()
    } else {
        FontFamily::SANS.name()
    }
}

/// Map a stored align to cosmic's per-line align. `Auto` maps to `None`; `Justified` and `End` are
/// not surfaced.
const fn cosmic_align(align: LineAlign) -> Option<CosmicAlign> {
    match align {
        LineAlign::Auto => None,
        LineAlign::Left => Some(CosmicAlign::Left),
        LineAlign::Center => Some(CosmicAlign::Center),
        LineAlign::Right => Some(CosmicAlign::Right),
    }
}

/// Glyph sources [`CosmicMeasure::rasterize_glyph`] tries, in cosmic-text's order. An emoji face
/// must hit one of the first two or it renders as a coverage mask.
const GLYPH_SOURCES: [Source; 3] = [
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

/// The OpenType weight axis, for variable faces.
const WGHT_AXIS: swash::Tag = u32::from_be_bytes(*b"wght");

/// The angle a synthetic italic leans by, rasterized and measured alike; cosmic's own.
const FAKE_ITALIC_SKEW_DEGREES: f32 = 14.0;

/// The scaler a glyph of `font` is read through, instanced on [`WGHT_AXIS`] for
/// variable faces. Uses `normalized_coords`, not `variations`, which leaves stale
/// entries in the retained context so a bold glyph bleeds into the next regular one.
fn glyph_scaler<'a>(
    context: &'a mut ScaleContext,
    font: &'a Font,
    size: f32,
    hint: bool,
    weight: Weight,
) -> Scaler<'a> {
    let face = font.as_swash();
    let mut builder = context.builder(face).size(size).hint(hint);
    if let Some(axis) = face.variations().find_by_tag(WGHT_AXIS) {
        builder = builder.normalized_coords(face.variations().normalized_coords([(
            WGHT_AXIS,
            f32::from(weight.0).clamp(axis.min_value(), axis.max_value()),
        )]));
    }
    builder.build()
}

/// The fractional pen offset `key` was binned at. A pixel font rounds its bins away: sub-pixel
/// offsets would resample pixel-grid artwork.
const fn subpixel_offset(key: CacheKey) -> Vector {
    let (x, y) = (key.x_bin.as_float(), key.y_bin.as_float());
    if key.flags.contains(CacheKeyFlags::PIXEL_FONT) {
        Vector::new(x.round(), y.round())
    } else {
        Vector::new(x, y)
    }
}

/// Real-shaping text measurer: owns a [`FontSystem`] per [`FontScope`] and the shaped-buffer cache.
pub(super) struct CosmicMeasure {
    font_system: FontSystem,
    /// Whether a face answers to each [`FontFamily`] index, filled on demand (see
    /// [`Self::has_font`]). The shaping name derives from it via [`shaping_name`] so they cannot
    /// disagree.
    resolved: Vec<FamilyResolution>,
    /// Swash's scaler caches, retained across glyphs.
    scale_context: ScaleContext,
    /// The image every glyph rasterizes into, reused to avoid per-glyph allocation.
    glyph_image: SwashImage,
    /// Which buffers are resident and for how long; also owns the shared frame clock.
    cache: ShapedBufferCache,
    /// Trailing advance of "…" for the last few faces asked about. Fixed slots, not a
    /// map: one slot misses on every truncation when record order interleaves two faces
    /// (`text_shape/ellipsis_width_churn`). Newest first: a miss pushes to the front and
    /// drops the back.
    ellipsis: ArrayVec<[EllipsisMemo; ELLIPSIS_MEMO_SLOTS]>,
    /// Scratch for the truncated string built on a miss; misses are hot during a width drag.
    truncate_scratch: String,
    break_scratch: SegmentScratch,
    glyph_ink: GlyphInk,
    /// Snapshot of the truncation probe's first layout run in visual order, copied
    /// once per miss so back-off rounds hold no cache borrow.
    cut_glyphs: Vec<ClusterGlyph>,
}

impl CosmicMeasure {
    pub(super) fn new(scope: FontScope) -> Self {
        Self::over(scope.build())
    }

    /// The measurer around a database someone else built: the seam
    /// [`FontScan`](crate::text::font_scan::FontScan) needs, since [`FontScope::build`] ran on
    /// another thread.
    pub(super) fn over(font_system: FontSystem) -> Self {
        Self {
            font_system,
            resolved: Vec::new(),
            scale_context: ScaleContext::new(),
            glyph_image: SwashImage::new(),
            cache: ShapedBufferCache::default(),
            ellipsis: ArrayVec::new(),
            truncate_scratch: String::new(),
            break_scratch: SegmentScratch::default(),
            glyph_ink: GlyphInk::default(),
            cut_glyphs: Vec::new(),
        }
    }

    /// Register every face in `source` and return the family of the first. No unload:
    /// the atlas keys on cosmic's `font_id`, which fontdb never reuses.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`] when the file cannot be read or mapped,
    /// [`FontLoadError::NoFaces`] when the bytes hold no parsable face,
    /// [`FontLoadError::FamilyTableFull`] when no named family fits the table.
    pub(super) fn load_font(&mut self, source: FontSource) -> Result<FontFamily, FontLoadError> {
        let source = match source {
            FontSource::Bytes(bytes) => fontdb::Source::Binary(Arc::new(bytes)),
            FontSource::File(path) => {
                // Opened first only to learn why a bad path failed: `load_font_source` reports an
                // unreadable file as zero faces.
                fs::File::open(&path).map_err(|source| FontLoadError::Io {
                    path: path.clone(),
                    source,
                })?;
                fontdb::Source::File(path)
            }
        };
        let ids = self.font_system.db_mut().load_font_source(source);
        // Collected before interning: `FontFamily::named` takes the name table's write lock while
        // the database borrow is live.
        let mut names: Vec<String> = Vec::new();
        for id in ids {
            let Some(face) = self.font_system.db().face(id) else {
                continue;
            };
            names.extend(face.families.iter().map(|(name, _)| name.clone()));
        }
        let loaded = first_family(&names, FontFamily::named)?;

        // Everything downstream of the database is stale: a family that resolved to SANS
        // may answer now. The families already resolved are the ones on screen, so the
        // re-warm covers them.
        let mut warm: Vec<FontFamily> = self
            .resolved
            .iter()
            .enumerate()
            .filter(|(_, answer)| **answer != FamilyResolution::Unasked)
            .map(|(index, _)| FontFamily::from_raw(index as u16))
            .collect();
        if !warm.contains(&loaded) {
            warm.push(loaded);
        }
        self.resolved.clear();
        self.drop_all_buffers();
        self.ellipsis.clear();
        warm_matches(&mut self.font_system, &warm);
        Ok(loaded)
    }

    /// Whether a face answers to `family`. Memoized, and the shaping path reads the
    /// same memo so the answer and the shaped face cannot disagree.
    pub(super) fn has_font(&mut self, family: FontFamily) -> bool {
        let index = usize::from(family.raw());
        match self.resolved.get(index).copied().unwrap_or_default() {
            FamilyResolution::Present => return true,
            FamilyResolution::Missing => return false,
            FamilyResolution::Unasked => {}
        }
        let present = family_present(self.font_system.db(), family.name());
        if !present {
            tracing::warn!(
                family = family.name(),
                "no face answers to this font family; it resolves to {}",
                FontFamily::SANS.name(),
            );
        }
        if self.resolved.len() <= index {
            self.resolved.resize(index + 1, FamilyResolution::Unasked);
        }
        self.resolved[index] = if present {
            FamilyResolution::Present
        } else {
            FamilyResolution::Missing
        };
        present
    }

    /// Every family the database knows, interned. A `Vec` because the database sits behind the
    /// shaper's `RefCell`. Cold.
    pub(super) fn font_families(&self) -> Vec<FontFamily> {
        let mut names: Vec<&str> = self
            .font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names.into_iter().filter_map(FontFamily::named).collect()
    }

    pub(super) fn drop_all_buffers(&mut self) {
        self.cache.drop_all();
    }

    fn attrs_of(&mut self, key: TextShapeKey) -> Attrs<'static> {
        let family = key.family();
        let name = shaping_name(family, self.has_font(family));
        attrs_named(name, key.weight(), key.slant())
    }

    pub(super) fn shaped_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        self.cache.shaped_run(key)
    }

    /// The run's unbounded shape, the root every wrap policy reasons from. `floor`
    /// opts into the segment scan behind [`TextRoot::intrinsic_min`]; it takes no width
    /// because the floor belongs to the unbounded root.
    pub(super) fn root(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        let key = request.key;
        debug_assert!(
            key.max_width().is_none(),
            "a committed width has no unbounded root to answer with",
        );
        // One lookup for the whole hit path: a resident entry shaped without the floor still owes
        // it to a policy that wants it. The resize-drag path.
        let breaks = &mut self.break_scratch;
        if let Some(entry) = self.cache.hit(key) {
            let root = entry.extent.root_mut();
            if floor == WrapFloor::Scan && root.intrinsic_min.is_none() {
                root.intrinsic_min = Some(intrinsic_min_width(&entry.buffer, breaks));
            }
            return entry.extent.root();
        }
        self.shape_wrapped(request, floor).root()
    }

    /// The extent this run resolves to at its key's committed width, routed by the key's fit.
    /// Extent and ink only.
    pub(super) fn resolve(&mut self, request: TextShapeRequest<'_>) -> TextExtent {
        let key = request.key;
        debug_assert!(
            key.max_width().is_some(),
            "an unbounded request commits no width to resolve against",
        );
        if let Some(entry) = self.cache.hit(key) {
            return entry.extent.extent();
        }
        match key.fit() {
            LineFit::Clip | LineFit::Ellipsis => self.shape_truncated(request),
            LineFit::Wrap => self.shape_wrapped(request, WrapFloor::Skip).extent(),
        }
    }

    /// Shape `request` into a fresh buffer and file it under its key. The one wrapping shape path.
    fn shape_wrapped(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> CachedExtent {
        let key = request.key;
        let mut buffer = self.acquire_buffer(metrics_of(key), key.max_width());
        // Per-line alignment goes through `set_text`'s `alignment` slot (setting it afterwards
        // tends to no-op). It needs a finite wrap target, else we pass `None`.
        let alignment = key.max_width().and_then(|_| cosmic_align(key.line_align()));
        let attrs = self.attrs_of(key);
        buffer.set_text(request.text, &attrs, Shaping::Advanced, alignment);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let geometry = shaped_geometry(&buffer, floor, &mut self.break_scratch);
        let extent = self
            .glyph_ink
            .extent(&buffer, &mut self.font_system, &geometry);
        let extent = match key.max_width() {
            None => CachedExtent::Root(geometry.root(extent)),
            Some(_) => CachedExtent::Bounded(extent),
        };
        self.cache.insert(key, buffer, extent, geometry.left);
        extent
    }

    /// Restore a missing shaped buffer from the retained source text and `key`'s
    /// parameters. Truncated runs restore their unbounded probe first. A
    /// [`TextShapeRequest`] cannot hold a run with no shaped buffer.
    pub(super) fn ensure_buffer(&mut self, request: TextShapeRequest<'_>) -> ShapedRun<'_> {
        match request.key.max_width() {
            Some(_) => {
                self.resolve(request);
            }
            None => {
                self.root(request, WrapFloor::Skip);
            }
        }
        self.shaped_run(request.key)
            .expect("restored text buffer did not land under its own TextShapeKey")
    }

    fn acquire_buffer(&mut self, metrics: Metrics, width: Option<f32>) -> Buffer {
        let mut buffer = match self.cache.take_recycled() {
            Some(buffer) => buffer,
            None => Buffer::new(&mut self.font_system, metrics),
        };
        buffer.set_metrics_and_size(metrics, width, None);
        buffer
    }

    pub(super) fn supersede(&mut self, key: TextShapeKey) {
        self.cache.supersede(key);
    }

    pub(super) const fn frame(&self) -> u64 {
        self.cache.frame()
    }

    pub(super) fn tick_frame(&mut self) {
        self.cache.tick_frame();
    }

    /// Resolve `request` to palantir-native glyph placements, restoring an evicted
    /// buffer and y-culling whole lines against `placement.bounds`. Returns whether any
    /// line was culled; such partial extractions must not become renderer cache
    /// templates, since the encoded key carries no bounds.
    pub(super) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        let ShapedRun { buffer, left } = self.ensure_buffer(request);

        out.clear();
        let RunPlacement {
            origin,
            scale,
            bounds,
        } = placement;
        // `origin` positions the measured block whose left edge is `left`; folding the pull-back
        // into the origin keeps subpixel binning consistent.
        let origin_x = origin.x - left * scale;
        let cull = bounds.map(|b| (b.min.y as f32, b.max().y as f32));
        let mut culled = false;
        for run in buffer.layout_runs() {
            if let Some((bounds_top, bounds_bot)) = cull {
                if (run.line_top + run.line_height) * scale + origin.y < bounds_top {
                    culled = true;
                    continue;
                }
                if run.line_top * scale + origin.y > bounds_bot {
                    culled = true;
                    break;
                }
            }
            let line_y_px = (run.line_y * scale).fast_round() as i32;
            for glyph in run.glyphs {
                // The renderer caches encoded runs on one uniform area colour, valid only while
                // cosmic produces no per-glyph override ([`attrs_named`] sets no per-span colour).
                debug_assert!(
                    glyph.color_opt.is_none(),
                    "per-glyph colour override requires folding colour into EncodedKey",
                );
                let physical = glyph.physical((origin_x, origin.y), scale);
                out.push(PlacedGlyph {
                    raster_key: GlyphRasterKey(physical.cache_key),
                    x: physical.x,
                    y: line_y_px + physical.y,
                });
            }
        }
        culled
    }

    /// Rasterize one glyph via swash, uncached here; the renderer's atlas is the
    /// cache. `None` when swash cannot produce an image. The pixels stay in
    /// [`Self::glyph_image`]; copy before the next call.
    pub(super) fn rasterize_glyph(&mut self, key: GlyphRasterKey) -> Option<RasterImage<'_>> {
        let cache_key = key.0;
        let font = self
            .font_system
            .get_font(cache_key.font_id, cache_key.font_weight)?;
        let mut scaler = glyph_scaler(
            &mut self.scale_context,
            &font,
            f32::from_bits(cache_key.font_size_bits),
            !cache_key.flags.contains(CacheKeyFlags::DISABLE_HINTING),
            cache_key.font_weight,
        );
        // Cleared, not overwritten: `render_into` zero-fills only what a resize added, so a smaller
        // glyph would read the last one's coverage.
        self.glyph_image.clear();
        let rendered = Render::new(&GLYPH_SOURCES)
            .format(Format::Alpha)
            .offset(subpixel_offset(cache_key))
            .transform(
                cache_key
                    .flags
                    .contains(CacheKeyFlags::FAKE_ITALIC)
                    .then(|| {
                        Transform::skew(
                            Angle::from_degrees(FAKE_ITALIC_SKEW_DEGREES),
                            Angle::from_degrees(0.0),
                        )
                    }),
            )
            .render_into(&mut scaler, cache_key.glyph_id, &mut self.glyph_image);
        if !rendered {
            return None;
        }
        let placement = self.glyph_image.placement;
        Some(RasterImage {
            content: match self.glyph_image.content {
                Content::Color => ContentType::Color,
                Content::Mask | Content::SubpixelMask => ContentType::Mask,
            },
            size: UVec2::new(placement.width, placement.height),
            bearing: IVec2::new(placement.left, placement.top),
            data: &self.glyph_image.data,
        })
    }

    /// Shape `text` as a single line truncated to fit `w`. The cached unbounded shape
    /// gives per-glyph advances, [`ClusterGlyph::fitting_prefix`] cuts after the last
    /// fully paid-for cluster, and the prefix is shaped on one natural line with no
    /// per-line align (the encoder positions it, so the extent is the glyph width, not
    /// `w`). `LineFit::Ellipsis` reserves room for a trailing `…`; `LineFit::Clip` cuts
    /// flush. `intrinsic_min` is 0.
    ///
    /// The shaped prefix is verified against `w` and retires a further cluster until it
    /// fits: reshaping changes the shaping context, so the cut alone cannot guarantee it.
    ///
    /// # Why not `Buffer::set_ellipsize`
    ///
    /// Cosmic can truncate itself, but that was measured and reverted: about 4.9x slower
    /// on `text_shape/resize_drag_frame`. Cosmic must see the whole string to cut, so
    /// every new width reshapes all of it. The dependency on the cached unbounded probe
    /// is the point: a drag pays the full-string shape once and reshapes only the short
    /// prefix. Revisiting needs the full shape cached across widths, i.e. a different
    /// buffer/key model.
    fn shape_truncated(&mut self, request: TextShapeRequest<'_>) -> TextExtent {
        let key = request.key;
        let fit = key.fit();
        let width = key
            .max_width()
            .expect("a truncating fit resolves against a committed width");
        let unbounded = request.unbounded_version();
        // Residency and the measure from one lookup; this is the resize-drag path.
        let root = self.root(unbounded, WrapFloor::Skip);
        let attrs = self.attrs_of(key);
        // Reserve the ellipsis width only when one will be appended; resolved before borrowing the
        // probe, since shaping "…" needs `&mut self`.
        let mut append_ellipsis = false;
        let avail = if matches!(fit, LineFit::Ellipsis) {
            let ellipsis_w = self.ellipsis_advance(key);
            append_ellipsis = ellipsis_w <= width;
            (width - ellipsis_w).max(0.0)
        } else {
            width
        };
        let probe_key = unbounded.key;
        let fits_whole = fit.resolves_to_unbounded(&root, width);

        // Unbounded on one line: binding to `Some(w)` plus align would measure the aligned
        // position, inflating a fits-anyway label.
        let mut buffer = self.acquire_buffer(metrics_of(key), None);
        let geometry = if fits_whole {
            buffer.set_text(request.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch)
        } else {
            // The cut spends advances from the whole run's shaping, but the prefix reshapes
            // in its own context (a joining script's last letter takes a wider final form). So
            // verify and retire one more cluster while it overruns; `max_end` guarantees
            // termination.
            self.cut_glyphs.clear();
            if let Some(run) = self.cache.probe(probe_key).buffer.layout_runs().next() {
                self.cut_glyphs.reserve_exact(run.glyphs.len());
                self.cut_glyphs
                    .extend(run.glyphs.iter().map(|g| ClusterGlyph {
                        start: g.start,
                        end: g.end,
                        advance: g.w,
                    }));
            }
            let mut max_end = usize::MAX;
            loop {
                let cut = ClusterGlyph::fitting_prefix(&mut self.cut_glyphs, avail, max_end);
                self.truncate_scratch.clear();
                self.truncate_scratch
                    .push_str(request.text[..cut].trim_end());
                if append_ellipsis {
                    self.truncate_scratch.push('…');
                }
                buffer.set_text(
                    self.truncate_scratch.as_str(),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
                let geometry = shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch);
                if geometry.size.w <= width || cut == 0 {
                    break geometry;
                }
                max_end = cut;
            }
        };

        let extent = self
            .glyph_ink
            .extent(&buffer, &mut self.font_system, &geometry);
        self.cache
            .insert(key, buffer, CachedExtent::Bounded(extent), geometry.left);
        extent
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized. Only the
    /// opening budget: [`Self::shape_truncated`] verifies the result, so an imprecise
    /// value costs retries, never correctness.
    fn ellipsis_advance(&mut self, key: TextShapeKey) -> f32 {
        let face = key.face();
        if let Some(advance) = self.ellipsis.iter().find_map(|memo| memo.advance_for(face)) {
            return advance;
        }
        let attrs = self.attrs_of(key);
        let mut buffer = self.acquire_buffer(metrics_of(key), None);
        buffer.set_text("…", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let advance = first_line_right(&buffer);
        self.cache.recycle(buffer);
        self.cache.counters.ellipsis_misses.bump();
        if self.ellipsis.len() == ELLIPSIS_MEMO_SLOTS {
            self.ellipsis.pop();
        }
        self.ellipsis.insert(0, EllipsisMemo { face, advance });
        advance
    }
}

// Manual: swash's `ScaleContext` and `Image` aren't `Debug`.
impl fmt::Debug for CosmicMeasure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CosmicMeasure")
            .field("cache", &self.cache.len())
            .field("frame", &self.cache.frame())
            .finish_non_exhaustive()
    }
}

/// The first of a loaded file's family `names` that `intern` admits; a name that
/// does not fit is skipped. No names means [`FontLoadError::NoFaces`]; names of which
/// none fits is [`FontLoadError::FamilyTableFull`].
pub(super) fn first_family(
    names: &[String],
    intern: impl Fn(&str) -> Option<FontFamily>,
) -> Result<FontFamily, FontLoadError> {
    if names.is_empty() {
        return Err(FontLoadError::NoFaces);
    }
    let mut loaded = None;
    for name in names {
        if let Some(family) = intern(name) {
            loaded.get_or_insert(family);
        }
    }
    loaded.ok_or(FontLoadError::FamilyTableFull)
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::text::cosmic::counters::CacheCounts;
    #[cfg(test)]
    use crate::text::cosmic::shaped_buffer_cache::internals::RecyclePoolStats;
    #[cfg(test)]
    use crate::text::glyph_font::GlyphFont;
    #[cfg(test)]
    use crate::text::request::internals::TestShape;
    #[cfg(test)]
    use crate::text::root::internals::TestMeasure;

    impl Default for CosmicMeasure {
        fn default() -> Self {
            Self::new(FontScope::Bundled)
        }
    }

    impl CosmicMeasure {
        #[cfg(test)]
        pub(crate) fn measure(&mut self, text: &str, shape: TestShape) -> TestMeasure {
            self.measure_with_fit_key(shape.request(text, LineFit::Wrap))
        }

        #[cfg(test)]
        fn measure_with_fit_key(&mut self, request: TextShapeRequest<'_>) -> TestMeasure {
            let key = request.key;
            match key.max_width() {
                Some(_) => TestMeasure {
                    size: self.resolve(request).size,
                    key: Some(key),
                    intrinsic_min: None,
                },
                None => TestMeasure::new(self.root(request, WrapFloor::Scan), key),
            }
        }

        #[cfg(test)]
        pub(crate) fn measure_with_fit(
            &mut self,
            text: &str,
            shape: TestShape,
            fit: LineFit,
            unbounded_key: TextShapeKey,
        ) -> TestMeasure {
            let request = shape.request(text, fit);
            debug_assert_eq!(request.key.unbounded_version(), unbounded_key);
            self.measure_with_fit_key(request)
        }

        pub(crate) fn cache_len(&self) -> usize {
            self.cache.len()
        }

        #[cfg(test)]
        pub(crate) fn pending_tickets(&self) -> usize {
            self.cache.pending_tickets()
        }

        #[cfg(test)]
        pub(crate) fn cached_extent(&self, key: TextShapeKey) -> Option<TextExtent> {
            self.cache.extent(key)
        }

        /// A measurer over an empty database, so a case can watch a family go from unresolvable to
        /// resolved.
        #[cfg(test)]
        pub(crate) fn with_no_fonts() -> Self {
            Self::over(FontSystem::new_with_locale_and_db(
                "en-US".to_owned(),
                fontdb::Database::new(),
            ))
        }

        #[cfg(test)]
        pub(crate) fn recycle_pool_stats(&self) -> RecyclePoolStats {
            self.cache.recycle_pool_stats()
        }

        #[cfg(test)]
        pub(crate) fn cache_counts(&self) -> CacheCounts {
            self.cache.counts()
        }

        /// The face cosmic-text shaped `text` with, as its database id; widths can't prove a font
        /// mapping since two faces can share an advance.
        #[cfg(test)]
        fn shaped_face(&mut self, text: &str, face: GlyphFont) -> Option<fontdb::ID> {
            let key = TextShapeKey::for_text(text, face).expect("a fixture face is usable");
            let attrs = self.attrs_of(key);
            let mut buf = Buffer::new(&mut self.font_system, Metrics::new(16.0, 19.2));
            buf.set_text(text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(&mut self.font_system, false);
            Some(buf.layout_runs().next()?.glyphs.first()?.font_id)
        }

        #[cfg(test)]
        pub(crate) fn resolved_family(&mut self, text: &str, face: GlyphFont) -> Option<String> {
            let id = self.shaped_face(text, face)?;
            self.font_system
                .db()
                .face(id)
                .map(|f| f.families[0].0.clone())
        }

        /// The PostScript name of [`Self::shaped_face`]: which file won.
        #[cfg(test)]
        pub(crate) fn resolved_post_script_name(
            &mut self,
            text: &str,
            face: GlyphFont,
        ) -> Option<String> {
            let id = self.shaped_face(text, face)?;
            self.font_system
                .db()
                .face(id)
                .map(|f| f.post_script_name.clone())
        }
    }
}
