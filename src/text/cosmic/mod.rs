//! Real text shaping via [`cosmic_text`]. Caches one shaped `Buffer`
//! per [`TextShapeKey`] — every input that affects shaping (text hash,
//! font size, wrap width, line height, family, weight, halign, fit) —
//! so steady-state measurement is `HashMap` lookup only: no reshape,
//! no allocation. The cache is bounded by **age, not capacity** —
//! [`CosmicMeasure::advance_to`] drops entries untouched for
//! [`PROBATION_KEEP_FRAMES`] / [`PROTECTED_KEEP_FRAMES`], see there for
//! why the two windows exist. Missing buffers are reconstructible from
//! the retained text source at the backend boundary, so a continuous
//! resize drag — every width unique, a fresh entry per run per frame —
//! stays bounded without explicit cache ownership. Evicted buffers feed a
//! bounded recycle pool so later misses retain Cosmic Text's internal
//! line, shaping, and layout allocations. The sweep itself is driven by
//! a deadline wheel, so it costs what expires rather than what is
//! resident — see [`CosmicMeasure::expiry`].
//!
//! The render side never sees cosmic types: `TextShaper::glyphs`
//! lends a `RefMut<CosmicMeasure>` whose
//! [`CosmicMeasure::extract_glyphs`] / [`CosmicMeasure::rasterize_glyph`]
//! translate shaped buffers into palantir-native placements and bitmaps;
//! [`crate::text`] documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::layout::types::align::HAlign;
use crate::text::cosmic::counters::CacheCounters;
use crate::text::error::FontLoadError;
use crate::text::font_family::FontFamily;
use crate::text::font_scope::FontScope;
use crate::text::font_source::FontSource;
use crate::text::font_style::FontStyle;
use crate::text::font_weight::FontWeight;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, WrapFloor};
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping,
    Style, SwashCache, Weight, fontdb,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tinyvec::ArrayVec;

use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::text::cosmic::cache_entry::{CacheEntry, CachedExtent};
use crate::text::cosmic::cluster_glyph::{ClusterGlyph, fitting_prefix};
use crate::text::cosmic::ellipsis_memo::EllipsisMemo;
use crate::text::cosmic::geometry::{
    ShapedGeometry, first_line_right, intrinsic_min_width, shaped_geometry,
};
use crate::text::render::{GlyphImage, GlyphPlacement, GlyphRasterKey, PlacedGlyph, RunPlacement};
use cosmic_text::SwashContent;
use std::collections::hash_map::Entry;

pub(super) mod cache_entry;
pub(super) mod cluster_glyph;
pub(super) mod counters;
pub(super) mod ellipsis_memo;
pub(super) mod geometry;

const RECYCLE_POOL_CAP: usize = 128;

/// Faces [`CosmicMeasure::ellipsis`] remembers the "…" advance for.
const ELLIPSIS_MEMO_SLOTS: usize = 4;

/// Frames a *probationary* entry survives before
/// [`CosmicMeasure::advance_to`] drops it: one inserted and never looked
/// up, or one [superseded](CosmicMeasure::supersede) after its reuse
/// slot moved to a different key.
///
/// Short on purpose. This population is scan traffic: a resize or zoom
/// drag quantizes to a new whole-pixel wrap width nearly every frame, so
/// each run mints a key that will never be asked for again. Holding those
/// for the protected window lets one drag accumulate
/// `runs × PROTECTED_KEEP_FRAMES` dead buffers.
///
/// **Supersession is what makes this window reach that population.**
/// Insertion alone does not: layout shapes a run and the encoder renders
/// it on the *same* frame, and that render is a lookup, so every drawn
/// buffer would otherwise be promoted the moment it was created and the
/// probation tier would be inert. Steady state cannot repair that by
/// re-touching it either — the measure cache and the encoded-run cache
/// both short-circuit before reaching here, so a resident buffer is
/// never looked up again on a later frame. `TextSystem` holds the only
/// signal that distinguishes "this run wants a different shape now"
/// (drag, typing, animation — dead) from "this run left the tree"
/// (scrolled away — may well return), and it reports the first through
/// [`CosmicMeasure::supersede`].
///
/// A demotion, not an eviction: four frames of grace means a label
/// oscillating between two keys, or a drag reversing back through a
/// width it just used, still hits.
///
/// # Why not reference-counted retention
///
/// Letting an entry die when no upper cache still holds it reads like
/// the obvious replacement for this whole scheme — no windows, no
/// demotion signal. It does not work, for a measured reason.
/// `EncodedKey` embeds [`TextShapeKey`], so a width drag mints a fresh
/// encoded entry every frame and those live
/// `ENCODED_CACHE_KEEP_FRAMES` — a shorter window than this one, but
/// still one with no probation tier under it. An encoded entry holding
/// its buffer *strongly* therefore pins `runs × (that window + 1)` of
/// them, an order of magnitude past what this window achieves, and the
/// exact growth it was added to stop. Holding it *weakly* keeps the drag
/// bounded but leaves buffers dying under live encoded entries, so the
/// whole restore path (`ShapedTextRef`, `InternedText`,
/// [`CosmicMeasure::ensure_buffer`]) has to stay — and deleting that was
/// the other half of the idea. The two wins are mutually exclusive.
pub(super) const PROBATION_KEEP_FRAMES: u64 = 4;

/// Frames a *protected* entry — one that has been looked up at least once
/// since insertion — survives untouched. The floor of the window;
/// [`PROTECTED_SPREAD_MASK`] is what an entry may add to it.
///
/// The name cosmic's two-tier policy reads
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`] under, beside
/// [`PROBATION_KEEP_FRAMES`]. Why that number is a ceiling the encoded
/// cache stays under rather than one it shares is stated there.
pub(super) const PROTECTED_KEEP_FRAMES: u64 = RENDERED_RUN_KEEP_FRAMES;

/// Extra frames a protected entry keeps, masked out of its own key — so
/// the window is `PROTECTED_KEEP_FRAMES ..= + this` rather than a single
/// frame.
///
/// The name cosmic's policy reads
/// [`crate::text::RENDERED_RUN_KEEP_SPREAD_MASK`] under, beside
/// [`PROTECTED_KEEP_FRAMES`]. Why the window is a range at all is stated
/// there.
pub(super) const PROTECTED_SPREAD_MASK: u64 = RENDERED_RUN_KEEP_SPREAD_MASK;

fn recycle_buffer(pool: &mut Vec<Buffer>, buffer: Buffer) {
    if pool.len() < RECYCLE_POOL_CAP {
        pool.push(buffer);
    }
}

/// The cosmic face one key shapes at, in the two values cosmic asks
/// for.
///
/// The inverse of [`TextShapeKey::unbounded`]'s fold: the key packed a
/// `GlyphFont` in, and every shaping path unpacks it here rather than
/// through four accessors of its own, so no two of them can shape one
/// key against different faces.
fn metrics_of(key: TextShapeKey) -> Metrics {
    Metrics::new(key.font_size_px(), key.line_height_px())
}

/// The attributes a resolved family shapes under.
///
/// Takes the **resolved** name rather than a [`FontFamily`], because the
/// resolution needs the database and this does not — which is what lets
/// the startup warm-up and the per-shape path share one spelling of the
/// attributes without sharing the memo.
///
/// Fake italic is cosmic's to add: `override_fake_italic` sets
/// `CacheKeyFlags::FAKE_ITALIC` when the matched face is upright and the
/// request was not, and the flag is part of the glyph cache key, so the
/// atlas keeps the slanted raster apart from the upright one.
fn attrs_named(name: &'static str, weight: FontWeight, style: FontStyle) -> Attrs<'static> {
    // Skip TrueType bytecode hinting: skrifa's hint VM dominated zoom-frame
    // CPU time, and at HiDPI / during animated zoom the visual difference
    // is imperceptible.
    let base = Attrs::new()
        .cache_key_flags(CacheKeyFlags::DISABLE_HINTING)
        .family(Family::Name(name))
        // fontdb instantiates the `wght` axis at this value on a variable
        // face, and picks the nearest static face otherwise.
        .weight(Weight(weight.value()));
    match style {
        FontStyle::Normal => base,
        FontStyle::Italic => base.style(Style::Italic),
    }
}

/// Whether any face in `db` answers to `name`.
///
/// A scan rather than `Database::query`, which allocates a candidate
/// `Vec` to answer a question that only needs the name — and the caller
/// memoizes the result, so this runs once per family.
fn family_present(db: &fontdb::Database, name: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(known, _)| known == name))
}

/// Build the match keys for `families`, at the weights and styles a theme
/// actually asks for.
///
/// cosmic builds a `FontMatchKey` per face the first time a
/// family/weight/style triple is shaped — O(faces), and otherwise paid on
/// whichever frame first draws that face. See [`FontScope::build`] for
/// why startup is where that belongs.
///
/// **Takes the families rather than walking the interned table**, which
/// is not the same set: `CosmicMeasure::font_families` interns every name
/// on the machine, and warming several hundred of them at O(faces) each
/// would cost more than the misses it saves. The callers pass what will
/// actually be shaped — the bundled pair at startup, and what the app has
/// already drawn after a load.
pub(crate) fn warm_matches(font_system: &mut FontSystem, families: &[FontFamily]) {
    for &family in families {
        let name = resolve_against(font_system.db(), family).name;
        for weight in [FontWeight::REGULAR, FontWeight::BOLD] {
            for style in [FontStyle::Normal, FontStyle::Italic] {
                font_system.get_font_matches(&attrs_named(name, weight, style));
            }
        }
    }
}

/// What one family resolved to.
///
/// Two answers off one walk of the database, because they are not the
/// same question: [`FontFamily::SANS`] shapes under its own name whether
/// or not a face answers to it, since it *is* what everything else falls
/// back to.
#[derive(Clone, Copy, Debug)]
struct FamilyResolution {
    /// The name to shape under.
    name: &'static str,
    /// Whether a face answers to the family that was asked for.
    available: bool,
}

/// The family a request for `family` actually shapes in: itself when a
/// face answers to it, and [`FontFamily::SANS`] when none does.
///
/// **Never cosmic's platform fallback**, which is what an unresolved
/// `Family::Name` reaches on its own: a missing family would then look
/// like whatever the machine happens to have installed, and the same app
/// would read differently on two machines. Falling back to the bundled
/// default is a look the app can predict, and
/// [`CosmicMeasure::font_available`] is how it asks in advance.
fn resolve_against(db: &fontdb::Database, family: FontFamily) -> FamilyResolution {
    let name = family.name();
    let available = family_present(db, name);
    FamilyResolution {
        name: if available {
            name
        } else {
            FontFamily::SANS.name()
        },
        available,
    }
}

/// Map an Palantir [`HAlign`] to cosmic-text's per-line align.
/// `Auto`/`Stretch` map to `None` — cosmic falls back to its
/// left-or-rtl-aware default, which is what "no per-line align" means.
/// `Left`/`Center`/`Right` translate directly. Cosmic's `Justified` and
/// `End` aren't surfaced.
fn cosmic_align(halign: HAlign) -> Option<CosmicAlign> {
    match halign {
        HAlign::Left => Some(CosmicAlign::Left),
        HAlign::Center => Some(CosmicAlign::Center),
        HAlign::Right => Some(CosmicAlign::Right),
        // `Auto` is the documented "no per-line align" default;
        // `Stretch` doesn't make sense per-line for text and falls
        // through to the same path.
        HAlign::Auto | HAlign::Stretch => None,
    }
}

/// A resident shaped buffer paired with the x its glyph block starts at,
/// so every reader normalizes the same way off one lookup.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapedRun<'a> {
    pub(super) buffer: &'a Buffer,
    /// See [`CacheEntry::left`].
    pub(super) left: f32,
}

/// Real-shaping text measurer. Owns a [`FontSystem`] populated per
/// [`FontScope`] and a cache of shaped `Buffer`s keyed on the inputs that
/// affect shaping. Per-call face selection comes from [`FontFamily`],
/// [`FontWeight`] and [`FontStyle`] on each measurement, resolved through
/// [`Self::resolve_family`].
pub(super) struct CosmicMeasure {
    font_system: FontSystem,
    /// One [`FamilyResolution`] per [`FontFamily`] index, grown on demand
    /// — see [`resolve_against`] for the rule and [`Self::resolve_family`]
    /// for why it is memoized.
    ///
    /// A `Vec` indexed by the family's own index, not a map: the indices
    /// are dense and start at zero, so a shape pays one bounds check
    /// rather than a hash.
    resolved: Vec<Option<FamilyResolution>>,
    /// Swash rasterization context for [`Self::rasterize_glyph`]. Used
    /// uncached — the renderer's glyph atlas is the real bitmap cache.
    swash_cache: SwashCache,
    cache: FxHashMap<TextShapeKey, CacheEntry>,
    /// Latest value of the shaper's shared frame clock, mirrored here by
    /// [`Self::advance_to`]. Stamped onto every entry this touches, and
    /// the reference point both retention windows measure back from.
    ///
    /// Mirrored rather than counted: the renderer's encoded-run cache
    /// and glyph atlas age against the same clock, and only one owner
    /// can keep them in step — see
    /// [`ShaperInner::frame`](crate::text::shaper::ShaperInner).
    frame: u64,
    /// Which keys come due on which frame, so [`Self::advance_to`] costs
    /// what expires rather than what is resident.
    ///
    /// A wheel rather than a single earliest-`keep_until` gate, which is
    /// O(1) only while nothing churns: one key that changes every frame
    /// — a clock, an FPS counter, a scrubbing value — re-pins that
    /// minimum a probation window out on every insert, the gate stops
    /// firing, and every frame walks the whole map to reclaim one entry.
    /// The churn that would motivate such a gate is precisely the churn
    /// that defeats it.
    expiry: ExpiryWheel<TextShapeKey>,
    /// LIFO pool fed by LRU eviction. `Buffer::set_text` reclaims its
    /// line, shaping, and layout allocations when the buffer is reset.
    recycle_pool: Vec<Buffer>,
    /// Trailing advance of "…" for the last few faces asked about.
    ///
    /// A fixed set of slots, not a map: a frame draws its ellipsized
    /// labels in a handful of text styles, and a miss is a single glyph
    /// through a recycled buffer. Nothing to bound, evict, or clear —
    /// the round-robin victim is simply overwritten.
    ///
    /// One slot was not enough. It held only the *last* face, so any
    /// record order that interleaves two — header and detail rows, a
    /// tree sized per depth, regular beside bold in one row — missed on
    /// every single truncation. Measured on
    /// `text_shape/ellipsis_width_churn`: the `two_faces` arm runs
    /// 3.85 µs on one slot against 2.77 µs on four, a 28% cut, while
    /// `one_face` is unchanged inside noise — so the extra slots cost
    /// nothing in the easy case. Four covers the interleavings a frame
    /// actually produces, and a lookup is four compares against a
    /// `Copy` struct.
    ///
    /// Newest first: a miss pushes to the front and drops the back, so
    /// the entry evicted is the one shaped longest ago and the linear
    /// scan meets the most recently shaped face first. With four
    /// entries the shift is three 12-byte copies, cheaper than the
    /// cursor an in-place ring would need.
    ellipsis: ArrayVec<[EllipsisMemo; ELLIPSIS_MEMO_SLOTS]>,
    /// Retained scratch for the truncated string
    /// [`Self::shape_truncated`] builds on a miss (cut prefix +
    /// optional `…`). Misses are the hot case — a continuous width drag
    /// mints a fresh quantized target per label per frame — so building
    /// into a retained buffer keeps that path free of `String` allocs,
    /// while the unbounded probe itself comes from `cache`.
    truncate_scratch: String,
    /// Retained scratch for `collect_break_offsets`, so the unbounded
    /// shape's segment scan allocates nothing per miss.
    break_scratch: Vec<u32>,
    /// Retained scratch holding the truncation probe's glyph indices in
    /// logical order — visual order is what the shaped run gives us, and
    /// truncation needs the logical prefix.
    logical_order: Vec<u32>,
    /// Shape / hit / supersede / expire tallies. Zero-sized outside
    /// tests.
    pub(super) counters: CacheCounters,
}

impl CosmicMeasure {
    /// Build the database `scope` names — see [`FontScope`] for what each
    /// one costs and what it makes resolvable.
    pub(super) fn new(scope: FontScope) -> Self {
        Self::over(scope.build())
    }

    /// The measurer around a database somebody else built — the seam
    /// [`FontScan`](crate::text::font_scan::FontScan) needs, since the
    /// point of that type is that [`FontScope::build`] ran on another
    /// thread.
    pub(super) fn over(font_system: FontSystem) -> Self {
        Self {
            font_system,
            resolved: Vec::new(),
            swash_cache: SwashCache::new(),
            cache: FxHashMap::default(),
            frame: 0,
            expiry: ExpiryWheel::with_keep(PROTECTED_KEEP_FRAMES + PROTECTED_SPREAD_MASK),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            ellipsis: ArrayVec::new(),
            truncate_scratch: String::new(),
            break_scratch: Vec::new(),
            logical_order: Vec::new(),
            counters: CacheCounters::default(),
        }
    }

    /// Register every face in `source`, and hand back the family of the
    /// first one.
    ///
    /// A collection loads all of its faces, and each of their families
    /// interns, so `FontFamily::named` reaches them too. There is no
    /// unload: the atlas keys on cosmic's `font_id`, and fontdb never
    /// reuses one.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`] when the file cannot be read or mapped, and
    /// [`FontLoadError::NoFaces`] when the bytes hold no face fontdb can
    /// parse.
    pub(super) fn load_font(&mut self, source: FontSource) -> Result<FontFamily, FontLoadError> {
        let source = match source {
            // The `Cow` goes into the `Arc` whole: it is `AsRef<[u8]>`,
            // `Send` and `Sync`, so one arm covers borrowed and owned
            // bytes alike and neither is copied.
            FontSource::Bytes(bytes) => fontdb::Source::Binary(Arc::new(bytes)),
            FontSource::File(path) => {
                // Opened first only to answer *why* a bad path failed:
                // `load_font_source` reports an unreadable file as zero
                // faces, which reads as "not a font" rather than "not
                // there". fontdb maps the file to parse its face table and
                // then keeps the path, re-mapping on the rare later read
                // of the face data.
                std::fs::File::open(&path).map_err(|source| FontLoadError::Io {
                    path: path.clone(),
                    source,
                })?;
                fontdb::Source::File(path)
            }
        };
        // `db_mut` clears cosmic's own match cache, so the faces below are
        // reachable to the next shape without any further prompting.
        let ids = self.font_system.db_mut().load_font_source(source);
        // Names are collected before any of them is interned:
        // `FontFamily::named` takes the name table's write lock, and the
        // borrow of the database would otherwise still be live across it.
        // fontdb rejects a face with no family name, so the first name
        // here is the first face's, and an empty list means nothing
        // parsed.
        let mut names: Vec<String> = Vec::new();
        for id in ids {
            let Some(face) = self.font_system.db().face(id) else {
                continue;
            };
            names.extend(face.families.iter().map(|(name, _)| name.clone()));
        }
        let mut loaded = None;
        for name in &names {
            loaded.get_or_insert(FontFamily::named(name));
        }
        let loaded = loaded.ok_or(FontLoadError::NoFaces)?;

        // Everything downstream of the database is now stale: a family
        // that resolved to SANS may answer for itself, and every shaped
        // buffer was laid out against the old resolution.
        //
        // The families already resolved are the ones on screen, so they
        // are what the re-warm covers — `db_mut` dropped cosmic's match
        // cache along with them, and the frame after a load re-shapes
        // everything it draws.
        let mut warm: Vec<FontFamily> = self
            .resolved
            .iter()
            .enumerate()
            .filter(|(_, name)| name.is_some())
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

    /// Whether a face answers to `family`, so an app can pick a family it
    /// knows will be used rather than one that quietly resolves to
    /// [`FontFamily::SANS`].
    ///
    /// Answered off the same memo the shaping path fills, not a second
    /// scan: an immediate-mode app asks this inside a record pass, and a
    /// walk of every face per frame is a walk of every face per frame.
    /// Sharing the memo is also what stops the answer and the resolution
    /// from ever disagreeing.
    pub(super) fn font_available(&mut self, family: FontFamily) -> bool {
        self.resolve_family(family).available
    }

    /// Every family the database knows, system fonts included, interned
    /// so the caller gets names it can hand straight back.
    ///
    /// A `Vec` rather than an iterator: the database sits behind the
    /// shaper's `RefCell`, so a lending iterator would hold that borrow
    /// across the caller's whole walk. Cold — a preferences picker asks
    /// once.
    pub(super) fn font_families(&self) -> Vec<FontFamily> {
        let mut names: Vec<&str> = self
            .font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names.into_iter().map(FontFamily::named).collect()
    }

    /// Drop every shaped buffer now, recycling each one, without waiting
    /// out a retention window.
    ///
    /// [`Self::load_font`] owes this: the buffers were laid out against a
    /// database that has since changed. Tests that exercise the *restore*
    /// path use it to set up a guaranteed-cold cache in one call, instead
    /// of encoding this cache's retention policy into tests that aren't
    /// about it.
    pub(super) fn drop_all_buffers(&mut self) {
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        for (_, entry) in cache.drain() {
            recycle_buffer(recycle_pool, entry.buffer);
        }
        self.expiry.clear();
    }

    /// The attributes `key` shapes under, resolved family and all.
    fn attrs_of(&mut self, key: TextShapeKey) -> Attrs<'static> {
        let name = self.resolve_family(key.family()).name;
        attrs_named(name, key.weight(), key.style())
    }

    /// What `family` resolved to, memoized.
    ///
    /// The rule is [`resolve_against`]'s; what belongs here is that asking
    /// it walks every face, and neither a shape nor a per-frame
    /// availability check may pay that. The memo is cleared by
    /// [`Self::load_font`], which is the only thing that can change an
    /// answer.
    fn resolve_family(&mut self, family: FontFamily) -> FamilyResolution {
        let index = usize::from(family.raw());
        if let Some(Some(resolution)) = self.resolved.get(index) {
            return *resolution;
        }
        let resolution = resolve_against(self.font_system.db(), family);
        if !resolution.available {
            // Worded for both callers: `font_available` asks the same
            // question without going on to shape anything.
            tracing::warn!(
                family = family.name(),
                "no face answers to this font family; it resolves to {}",
                resolution.name,
            );
        }
        if self.resolved.len() <= index {
            self.resolved.resize(index + 1, None);
        }
        self.resolved[index] = Some(resolution);
        resolution
    }

    /// Look up the shaped run for `key`, or `None` when no buffer is
    /// resident under it — never measured on this `CosmicMeasure`, or
    /// aged out since.
    ///
    /// Unlike [`Self::ensure_buffer`] this is a lookup, so absence is an
    /// answer rather than a wiring bug: the probe path takes it for a run
    /// that was never shaped, and a residency check is the question
    /// itself.
    ///
    /// [`TextShapeKey::INVALID`] is not among the keys it answers for.
    /// The sentinel means "this run has no shaped buffer at all", and
    /// nothing ever inserts one — a [`TextShapeRequest`] cannot hold
    /// either run the sentinel stands for. Asking the cache about it is a
    /// category error rather than a miss, so it is asserted instead of
    /// answered, and each caller that can hold one filters it first: the
    /// encoder drops those runs, `TextProbe::shaped` answers `None`.
    pub(super) fn shaped_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        debug_assert!(
            !key.is_invalid(),
            "the invalid sentinel names no cache entry — filter it before the lookup",
        );
        self.cache.get(&key).map(|e| ShapedRun {
            buffer: &e.buffer,
            left: e.left,
        })
    }

    /// The run's **unbounded** shape: the root every wrap policy reasons
    /// from, shaped or served from the cache.
    ///
    /// `floor` opts into the segment scan behind
    /// [`TextRoot::intrinsic_min`]. It takes no width because the floor is
    /// a property of the unbounded root and of nothing else — which is why
    /// [`Self::resolve`], the bounded half, has no such parameter to get
    /// wrong.
    pub(super) fn root(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        let key = request.key;
        debug_assert!(
            key.max_width_px().is_none(),
            "a committed width has no unbounded root to answer with",
        );
        // One probe for the whole hit path, entry held across the
        // backfill: a resident entry shaped by a policy that didn't want
        // the floor still owes it to one that does, and reading the
        // extent out first meant hashing the same key again to write it.
        // This is the resize-drag path.
        let breaks = &mut self.break_scratch;
        if let Some(entry) = Self::hit_entry(&mut self.cache, &mut self.counters, self.frame, key) {
            let root = entry.extent.root_mut();
            if floor == WrapFloor::Scan && root.intrinsic_min.is_none() {
                root.intrinsic_min = Some(intrinsic_min_width(&entry.buffer, breaks));
            }
            return entry.extent.root();
        }
        self.shape_wrapped(request, floor).root()
    }

    /// The extent this run resolves to at the width its key commits,
    /// routed to the wrapping or truncating path by the key's fit.
    ///
    /// An extent and nothing else, because that is all a bounded shape
    /// has: it never scanned for a wrapping floor, and its line count
    /// describes the resolve rather than the run.
    pub(super) fn resolve(&mut self, request: TextShapeRequest<'_>) -> Size {
        let key = request.key;
        debug_assert!(
            key.max_width_px().is_some(),
            "an unbounded request commits no width to resolve against",
        );
        if let Some(hit) = self.cache_hit(key) {
            return hit.size();
        }
        match key.fit() {
            LineFit::Clip | LineFit::Ellipsis => self.shape_truncated(request),
            _ => self.shape_wrapped(request, WrapFloor::Skip).size,
        }
    }

    /// Shape `request` into a fresh buffer, file it under its key, and
    /// hand back what the buffer laid out to. The one wrapping shape
    /// path; [`Self::root`] and [`Self::resolve`] each check the cache
    /// first and lift the result into their own kind.
    fn shape_wrapped(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> ShapedGeometry {
        let key = request.key;
        let mut buffer = self.acquire_buffer(metrics_of(key), key.max_width_px());
        // Per-line alignment travels through cosmic's `set_text`
        // `alignment` slot — that's the canonical entry point and
        // applies the align to every parsed buffer line in one
        // shot. Iterating `buffer.lines.iter_mut().set_align` after
        // `set_text` is the older API surface and tends to no-op on
        // freshly populated lines in 0.18+. Per-line align is only
        // meaningful with a finite wrap target (cosmic uses it as the
        // line width); without one we pass `None` so single-line
        // editors keep their widget-side `dx` placement.
        let alignment = key.max_width_px().and_then(|_| cosmic_align(key.halign()));
        let attrs = self.attrs_of(key);
        buffer.set_text(request.text, &attrs, Shaping::Advanced, alignment);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let geometry = shaped_geometry(&buffer, floor, &mut self.break_scratch);
        // Which kind the entry is follows from the key: a committed width
        // means a bounded resolve, and nothing else can name that entry.
        let extent = match key.max_width_px() {
            None => CachedExtent::Root(geometry.root()),
            Some(_) => CachedExtent::Bounded(geometry.size),
        };
        self.insert(key, buffer, extent, geometry.left);
        geometry
    }

    /// Restore a missing shaped buffer from the retained source text and
    /// the canonical parameters encoded by `key`, and hand it back.
    /// Truncated runs restore their unbounded probe first; callers never
    /// manage that dependency. Any valid key is shaped on the spot, so
    /// the final lookup doubles as the check that the restore landed
    /// under its own key.
    ///
    /// A run with no shaped buffer never gets this far: the encoder drops
    /// [`TextShapeKey::INVALID`] runs before paint and the backend keeps
    /// its own backstop, so arriving with one is a wiring bug rather than
    /// a case to answer. Handing back an `Option` for it only moved the
    /// panic to the caller.
    pub(super) fn ensure_buffer(&mut self, request: TextShapeRequest<'_>) -> ShapedRun<'_> {
        debug_assert!(
            !request.key.is_invalid(),
            "restoring a buffer for a run the encoder should have dropped",
        );
        // Residency is the whole point here, so the measurement is dropped
        // either way — but the two paths shape differently, and the key is
        // what says which one this run went through. Both open with the
        // cache lookup, so a resident buffer costs one and no reshape.
        match request.key.max_width_px() {
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
        let mut buffer = match self.recycle_pool.pop() {
            Some(buffer) => buffer,
            None => Buffer::new(&mut self.font_system, metrics),
        };
        buffer.set_metrics_and_size(metrics, width, None);
        buffer
    }

    /// Store a freshly shaped buffer. Entries start probationary; only a
    /// later lookup promotes them (see [`PROBATION_KEEP_FRAMES`]).
    fn insert(&mut self, key: TextShapeKey, buffer: Buffer, extent: CachedExtent, left: f32) {
        // Counted here rather than per `shape_until_scroll` so one
        // cached run is one tally: `measure_truncated`'s back-off can
        // reshape a prefix several times to land inside the committed
        // width, and a workload test cares that the run was shaped, not
        // how many attempts the cut took. The memoized ellipsis probe
        // shapes without inserting and is deliberately not counted.
        self.counters.shapes.bump();
        let dies_at = self.probation_dies_at();
        let ticket_seq = self.expiry.schedule(key, dies_at);
        let displaced = self.cache.insert(
            key,
            CacheEntry {
                buffer,
                extent,
                left,
                dies_at,
                ticket_seq,
            },
        );
        // Unreachable today — every caller checks `cache_hit` first, so a
        // key is inserted once — but the pool must not silently leak a
        // buffer the moment a second insert path appears.
        if let Some(old) = displaced {
            recycle_buffer(&mut self.recycle_pool, old.buffer);
        }
    }

    /// What the entry under `key` measured to, or `None` on a miss.
    fn cache_hit(&mut self, key: TextShapeKey) -> Option<CachedExtent> {
        Self::hit_entry(&mut self.cache, &mut self.counters, self.frame, key)
            .map(|entry| entry.extent)
    }

    /// The resident entry under `key`, its deadline pushed out to the
    /// protected window and the hit counted.
    ///
    /// Being asked for at all is the evidence that separates reuse from
    /// scan traffic, so no separate promotion step is needed.
    ///
    /// Takes the three fields rather than `&mut self`, like
    /// [`CacheEntry::probe`]: [`Self::root`] holds `break_scratch` across
    /// the call, and only a field-level borrow leaves that field free.
    fn hit_entry<'a>(
        cache: &'a mut FxHashMap<TextShapeKey, CacheEntry>,
        counters: &mut CacheCounters,
        frame: u64,
        key: TextShapeKey,
    ) -> Option<&'a mut CacheEntry> {
        let entry = cache.get_mut(&key)?;
        entry.dies_at = frame + PROTECTED_KEEP_FRAMES + key.keep_spread() + 1;
        counters.hits.bump();
        Some(entry)
    }

    /// The frame an entry filed into the probation window is first dead.
    /// Read by [`Self::insert`] and by [`Self::supersede`], the two sites
    /// that file one.
    fn probation_dies_at(&self) -> u64 {
        self.frame + PROBATION_KEEP_FRAMES + 1
    }

    /// Demote `key` to the probation window: the reuse slot that owned
    /// it now answers a different key, so nothing can ask for it through
    /// that slot again. See [`PROBATION_KEEP_FRAMES`] for why this is
    /// the signal the two-tier policy runs on.
    ///
    /// Only ever shortens a deadline — a supersede must not extend the
    /// life of an entry already closer to expiry — and files a second
    /// ticket for the earlier frame, since the outstanding one sits at
    /// the deadline this just retracted.
    ///
    /// Silent on a key that isn't resident: the buffer may already have
    /// aged out, and superseding what is gone is a no-op, not an error.
    pub(super) fn supersede(&mut self, key: TextShapeKey) {
        debug_assert!(
            !key.is_invalid(),
            "the invalid sentinel names no cache entry — filter it before the demotion",
        );
        let dies_at = self.probation_dies_at();
        let Some(entry) = self.cache.get_mut(&key) else {
            return;
        };
        self.counters.supersedes.bump();
        // Never *extends* a life: an entry already closer to expiry —
        // one that was inserted and never looked up — keeps its own
        // deadline.
        if entry.dies_at > dies_at {
            entry.dies_at = dies_at;
            // The new ticket is earlier than the outstanding one, so it
            // is the one that decides this entry's fate: stamping it
            // here retires the supplanted ticket when it fires.
            entry.ticket_seq = self.expiry.schedule(key, dies_at);
        }
    }

    /// Take the shaper's `frame` clock and drop every buffer whose
    /// deadline has passed.
    ///
    /// `frame` is only ever read, never derived from a local counter, so
    /// a clock that jumps several frames at once is handled by the same
    /// comparison as one that advances by one.
    ///
    /// Age, not capacity. A count budget cannot express what this cache
    /// needs: set below the live working set it thrashes — UI redraw is a
    /// cyclic access pattern, LRU's worst case, so the overflow misses
    /// every frame forever — and set above it, a resize drag fills it with
    /// widths that can never be hit again. Ageing bounds both without a
    /// number to guess: an app keeps exactly what it keeps touching, and
    /// scan traffic falls out on its own.
    ///
    /// Cost tracks what expires, not what is resident: [`Self::expiry`]
    /// hands back only the keys whose ticket came due, so a frame holding
    /// a scrolled document's whole working set pays the same as an empty
    /// one unless something actually lapsed.
    ///
    /// A ticket is a hint, never authority to drop. Deadlines move after
    /// it is filed — [`Self::cache_hit`] pushes one out and deliberately
    /// files nothing, which is what keeps a re-read entry from filing a
    /// ticket per frame — so the real `dies_at` is re-read here and a
    /// still-live entry is simply re-filed.
    pub(super) fn advance_to(&mut self, frame: u64) {
        debug_assert!(frame >= self.frame, "the shared frame clock ran backwards");
        self.frame = frame;
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        let probe = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already — a demote leaves two tickets outstanding
            // and both can come due in one drain, so whichever settled
            // first may have evicted the entry this one is holding.
            let Entry::Occupied(slot) = cache.entry(key) else {
                return None;
            };
            // Supplanted by a later `supersede`: the entry's live ticket
            // is still outstanding and will settle it, so this one is
            // surplus and dies here. Re-filing it instead is what let the
            // per-entry ticket count — and with it the per-frame drain —
            // grow for as long as the entry stayed resident.
            if seq != slot.get().ticket_seq {
                return None;
            }
            if slot.get().dies_at > frame {
                // Re-filed under the same serial, so the entry's stamp
                // still names it and nothing has to be written back.
                return Some(slot.get().dies_at);
            }
            probe.expiries.bump();
            recycle_buffer(recycle_pool, slot.remove().buffer);
            None
        });
    }

    /// Resolve `request` to palantir-native glyph placements for the
    /// renderer. Restores the shaped buffer if evicted (truncated runs
    /// restore their unbounded probe internally), walks its layout runs,
    /// y-culls whole lines against `placement.bounds`, and rewrites
    /// `out` with one [`PlacedGlyph`] per surviving glyph. Returns
    /// whether any line was culled — such partial extractions must not
    /// become renderer cache templates (its encoded key carries no
    /// bounds).
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
        // `origin` positions the *measured block*, whose left edge is
        // `left` in buffer space — so pull the origin back by it and the
        // per-glyph offsets land where the measurement said they would.
        // Folding it into the origin rather than into each `physical.x`
        // keeps the subpixel binning consistent with the shift.
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
            for glyph in run.glyphs.iter() {
                // The renderer caches encoded runs on one uniform area
                // colour — correct only while cosmic never produces a
                // per-glyph override ([`attrs_named`] sets no per-span
                // colour). If this fires, per-span colour was added
                // without folding a colour fingerprint into the
                // renderer's `EncodedKey`.
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

    /// Rasterize one glyph via swash, uncached on the cosmic side — the
    /// renderer's atlas is the real cache. `None` when swash cannot
    /// produce an image for the key (e.g. a glyph the face lacks).
    pub(super) fn rasterize_glyph(&mut self, key: GlyphRasterKey) -> Option<GlyphImage> {
        let image = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, key.0)?;
        let kind = match image.content {
            SwashContent::Color => ContentType::Color,
            SwashContent::Mask | SwashContent::SubpixelMask => ContentType::Mask,
        };
        Some(GlyphImage {
            kind,
            placement: GlyphPlacement {
                left: image.placement.left,
                top: image.placement.top,
                width: image.placement.width,
                height: image.placement.height,
            },
            data: image.data,
        })
    }

    /// Shape `text` as a single line truncated to fit `w`. Truncation is
    /// cluster-precise: the cached unbounded shape gives per-glyph advances,
    /// [`fitting_prefix`] cuts after the last fully paid-for cluster, then we
    /// shape the (possibly truncated) prefix on one **natural** line — no
    /// per-line align. The committed width only decides the cut; the encoder
    /// positions/aligns the single line, so the measured extent is the glyph
    /// width, not `w` (binding to `w` + center align would inflate a
    /// fits-anyway label to ~half the box). `LineFit::Ellipsis` reserves room
    /// for and appends a trailing `…`; `LineFit::Clip` cuts flush to `w`
    /// with no marker. The buffer caches under a fit-discriminated key (so it
    /// can't collide with the wrapped buffer — or the other truncation mode —
    /// at the same width). `intrinsic_min` is 0 — a truncated run can shrink
    /// to nothing.
    ///
    /// The shaped prefix is verified against `w` and retires a further
    /// cluster until it fits, so the measured extent never exceeds the
    /// committed width — the cut alone cannot guarantee that, since
    /// reshaping the prefix changes its shaping context.
    ///
    /// # Why not `Buffer::set_ellipsize`
    ///
    /// Cosmic 0.19 can do this itself, and delegating to it deletes
    /// roughly 290 lines: this function, [`fitting_prefix`],
    /// [`ClusterGlyph`], the ellipsis memo, and three retained scratch
    /// fields. That was written, measured, and reverted — **4.9x slower**
    /// on `text_shape/resize_drag_frame` (1.42 µs -> 7.13 µs per run per
    /// frame).
    ///
    /// The reason is not the wrap mode (`Wrap::Glyph`, `Wrap::None` and a
    /// one-line height cap all measure within 10% of each other) but how
    /// much text is reshaped per committed width. Cosmic has to see the
    /// whole string to decide where to cut, so every new width reshapes
    /// all of it; shaping a 108-character label costs ~9x what its
    /// seven-character cut prefix costs.
    ///
    /// **So the dependency on the cached unbounded probe is the point,
    /// not a wart.** It is what a drag reuses: the full-string shape is
    /// paid once, [`fitting_prefix`] finds the cut by scanning glyphs
    /// that are already there, and only the short prefix is reshaped per
    /// frame. Anything that revisits this has to keep the full-string
    /// shape cached across widths — cosmic allows that (`set_size`
    /// re-lays-out without re-shaping), but one buffer holds one layout
    /// and the cache is keyed per width, so it needs a different
    /// buffer/key model rather than a different call.
    fn shape_truncated(&mut self, request: TextShapeRequest<'_>) -> Size {
        let key = request.key;
        let fit = key.fit();
        let width = key
            .max_width_px()
            .expect("a truncating fit resolves against a committed width");
        let unbounded = request.unbounded_version();
        // Residency *and* the measure, from one lookup. `ensure_buffer`
        // answers the same question with a second lookup for a
        // `ShapedRun` this drops, and the root it hands back below is
        // exactly what the fit test wants — this is the resize-drag path,
        // where the key was hashed three times over.
        let root = self.root(unbounded, WrapFloor::Skip);
        let attrs = self.attrs_of(key);
        // Reserve the ellipsis width only when we'll append one; a plain
        // clip cuts flush to the full available width. Resolved before
        // borrowing the probe, since shaping "…" needs `&mut self`.
        let mut append_ellipsis = false;
        let avail = if matches!(fit, LineFit::Ellipsis) {
            let ellipsis_w = self.ellipsis_advance(key);
            append_ellipsis = ellipsis_w <= width;
            (width - ellipsis_w).max(0.0)
        } else {
            width
        };
        let probe_key = unbounded.key;
        // Same question `TextSystem::measure` asks before it ever gets
        // here, against the same root — so it is asked the same way. The
        // shape above already measured it, which is why this re-walks
        // neither the glyphs nor the cache.
        let fits_whole = fit.resolves_to_unbounded(&root, width);

        // Shape unbounded on one line: the cut already fit it to `w`, and the
        // encoder owns single-line placement. Binding to `Some(w)` + align
        // would measure the aligned glyph position, inflating a fits-anyway
        // label toward the box width.
        let mut buffer = self.acquire_buffer(metrics_of(key), None);
        let size = if fits_whole {
            // Re-shaping the identical text reproduces the probe, so this
            // branch cannot overrun `width`.
            buffer.set_text(request.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch).size
        } else {
            // The cut spends advances measured in the *whole* run's shaping,
            // but the prefix reshapes in its own context: a joining script's
            // last letter is exposed at a new word end and takes a final form
            // wider than the medial one the budget paid for. So verify the
            // shaped result, and while it overruns, retire one more cluster.
            // `max_end` makes every retry strictly shorter, so the sequence
            // bottoms out at the empty prefix.
            let mut max_end = usize::MAX;
            loop {
                let cut = match CacheEntry::probe(&self.cache, probe_key)
                    .buffer
                    .layout_runs()
                    .next()
                {
                    Some(run) => fitting_prefix(
                        run.glyphs.len(),
                        |i| ClusterGlyph {
                            start: run.glyphs[i].start,
                            end: run.glyphs[i].end,
                            advance: run.glyphs[i].w,
                        },
                        &mut self.logical_order,
                        avail,
                        max_end,
                    ),
                    None => 0,
                };
                self.truncate_scratch.clear();
                self.truncate_scratch
                    .push_str(request.text[..cut].trim_end());
                if append_ellipsis {
                    self.truncate_scratch.push('…');
                }
                // `set_text` resets the buffer in place, so a retry reuses
                // the line, shaping, and layout allocations it just filled.
                buffer.set_text(
                    self.truncate_scratch.as_str(),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
                let size = shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch).size;
                if size.w <= width || cut == 0 {
                    break size;
                }
                max_end = cut;
            }
        };

        // The prefix reshapes on an unbounded buffer with no per-line
        // align, so its block already starts at 0.
        self.insert(key, buffer, CachedExtent::Bounded(size), 0.0);
        size
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized for
    /// the last face asked about.
    ///
    /// Only the *opening* budget: [`Self::shape_truncated`] verifies the
    /// shaped result against the committed width either way, so a stale or
    /// imprecise reservation costs retries, never correctness. What it buys
    /// is measured on `text_shape/ellipsis_width_churn`, whose arms hold
    /// the width churning so every frame is a truncation miss and the
    /// reservation is asked for again. See [`CosmicMeasure::ellipsis`]
    /// for what the slot count buys there.
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
        recycle_buffer(&mut self.recycle_pool, buffer);
        self.counters.ellipsis_misses.bump();
        // `insert` panics at capacity, so retire the oldest first.
        if self.ellipsis.len() == ELLIPSIS_MEMO_SLOTS {
            self.ellipsis.pop();
        }
        self.ellipsis
            .insert(0, EllipsisMemo::wanted(face).measured(advance));
        advance
    }
}

impl Default for CosmicMeasure {
    fn default() -> Self {
        Self::new(FontScope::Bundled)
    }
}

// Manual: cosmic's `SwashCache` isn't `Debug`.
impl std::fmt::Debug for CosmicMeasure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmicMeasure")
            .field("cache", &self.cache.len())
            .field("frame", &self.frame)
            .finish_non_exhaustive()
    }
}

// Wider than `cfg(test)`: `drop_all_buffers` is reached from the
// `internals`-gated GPU tests in `renderer::backend::text`, which build
// without `cfg(test)`.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use super::*;
    #[cfg(test)]
    use crate::text::glyph_font::GlyphFont;
    #[cfg(test)]
    use crate::text::request::test_support::TestShape;
    #[cfg(test)]
    use crate::text::root::test_support::TestMeasure;

    #[derive(Debug, PartialEq, Eq)]
    #[cfg(test)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    impl CosmicMeasure {
        #[cfg(test)]
        pub(crate) fn measure(&mut self, text: &str, shape: TestShape) -> TestMeasure {
            self.measure_with_fit_key(shape.request(text, LineFit::Wrap))
        }

        /// Shape `request` and pair the result with the key it shaped
        /// under.
        ///
        /// The wrap-floor tests measure through this helper, so it asks
        /// for the floor on every root it shapes — and a bounded request
        /// takes the resolve path, which has no floor to ask for, exactly
        /// as production does. The two answers differ in kind, so this
        /// flattens them into the `TestMeasure` a case asserts on.
        #[cfg(test)]
        fn measure_with_fit_key(&mut self, request: TextShapeRequest<'_>) -> TestMeasure {
            let key = request.key;
            match key.max_width_px() {
                Some(_) => TestMeasure {
                    size: self.resolve(request),
                    key,
                    intrinsic_min: None,
                    single_line: true,
                },
                None => TestMeasure::new(self.root(request, WrapFloor::Scan), key),
            }
        }

        /// Truncating-fit measure. Named apart from the production
        /// `shape_truncated` — inherent methods can't share a name.
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

        /// Number of shaped buffers currently cached. Reach-in for the
        /// in-tree eviction tests.
        pub(crate) fn cache_len(&self) -> usize {
            self.cache.len()
        }

        /// Outstanding expiry tickets. The number that says whether
        /// [`CosmicMeasure::supersede`] is holding up its end of the
        /// wheel's protocol: a demote files a ticket that supplants the
        /// outstanding one, and if the supplanted ticket re-files itself
        /// this grows by one per demote for as long as the entry lives.
        #[cfg(test)]
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        /// Advance the shared clock by one frame and sweep — what
        /// `TextShaper::tick_frame` does for a measurer reached through
        /// a `TextShaper`. The retention tests drive `CosmicMeasure`
        /// directly, with no shaper to hold the clock, so they own the
        /// tick; production never increments here.
        #[cfg(test)]
        pub(crate) fn tick_frame(&mut self) {
            self.advance_to(self.frame + 1);
        }

        /// A measurer over an empty database, so a case can watch a
        /// family go from unresolvable to resolved.
        ///
        /// Every [`FontScope`] loads the bundled faces, which is what a
        /// shipping app wants and what leaves no family for a load test
        /// to introduce. Nothing production reaches this state.
        #[cfg(test)]
        pub(crate) fn with_no_fonts() -> Self {
            Self::over(FontSystem::new_with_locale_and_db(
                "en-US".to_owned(),
                fontdb::Database::new(),
            ))
        }

        #[cfg(test)]
        pub(crate) fn recycle_pool_stats(&self) -> RecyclePoolStats {
            RecyclePoolStats {
                len: self.recycle_pool.len(),
                capacity: self.recycle_pool.capacity(),
                limit: RECYCLE_POOL_CAP,
            }
        }

        /// The face cosmic-text actually shaped `text` with, as its
        /// database id.
        ///
        /// Proves the resolution maps a [`GlyphFont`] to the intended
        /// physical face — a measured-width comparison can't, since two
        /// different faces can share an advance.
        #[cfg(test)]
        fn shaped_face(&mut self, text: &str, face: GlyphFont) -> Option<fontdb::ID> {
            let key = TextShapeKey::for_text(text, face);
            let attrs = self.attrs_of(key);
            let mut buf = Buffer::new(&mut self.font_system, Metrics::new(16.0, 19.2));
            buf.set_text(text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(&mut self.font_system, false);
            Some(buf.layout_runs().next()?.glyphs.first()?.font_id)
        }

        /// The family name of [`Self::shaped_face`] — which family won.
        #[cfg(test)]
        pub(crate) fn resolved_family(&mut self, text: &str, face: GlyphFont) -> Option<String> {
            let id = self.shaped_face(text, face)?;
            self.font_system
                .db()
                .face(id)
                .map(|f| f.families[0].0.clone())
        }

        /// The PostScript name of [`Self::shaped_face`] — which *file*
        /// won, which is the only thing that separates an italic face
        /// from the upright one of the same family.
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
