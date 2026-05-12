//! Glyphon-backed text renderer with **per-group prepare/render** so text
//! interleaves correctly with quads in paint order. The wgpu backend calls
//! [`TextRenderer::prepare_group`] for each `DrawGroup` whose `texts` range
//! is non-empty, then inside the render pass calls
//! [`TextRenderer::render_group`] right after that group's quads draw.
//! Glyph data is shared via a single [`TextAtlas`] across all renderers in
//! the pool, so the cache is hit for free across groups.
//!
//! [`CosmicMeasure`]: crate::text::cosmic::CosmicMeasure
//! [`TextRun`]: crate::renderer::render_buffer::TextRun

use crate::primitives::color::Srgb8;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use fixedbitset::FixedBitSet;
use glam::UVec2;
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonRenderer, Viewport,
};

/// Selects which renderer pool a `prepare_group` / `render_group` call
/// targets. Plain frames stay on the no-stencil pool (existing
/// behavior). When the surrounding pass has a stencil attachment
/// (rounded-clip path), text must use a depth-stencil-aware glyphon
/// pipeline or wgpu validation errors — `Stencil` selects that pool.
/// Pools share the underlying `TextAtlas` so glyph caches hit across
/// modes for free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilMode {
    Plain,
    /// Glyphon pipeline built with `depth_stencil = Some(...)` matching
    /// the quad pipeline's `stencil_test` state. The render pass sets
    /// `stencil_reference` per draw — text under a rounded clip
    /// inherits the active reference and gets stencil-rounded.
    Stencil,
}

/// Pool shrinks only when grossly over-allocated: pool length must
/// exceed `2 × high_water` to trigger a truncate down to high_water.
/// That hysteresis absorbs frame-to-frame fluctuation (tooltip in/out,
/// modal toggling) without forcing buffer recreation, while still
/// reclaiming memory after a one-off burst (e.g. an app briefly
/// rendering 1000 text groups, then settling at 50). Steady-state UIs
/// never trigger the shrink branch — pool just hovers at peak usage.
const POOL_SHRINK_RATIO: usize = 2;

/// Upper bound on consecutive frames that may hash-skip
/// `glyphon::prepare`. When this many "skip-something" frames pass
/// without a clean trim opportunity, the next frame ignores hash hits
/// and re-prepares every group so `atlas.trim` can run safely.
/// Bounds atlas memory growth in zoom/scroll-dominated workloads
/// where adjacent frames mint similar-but-different glyph cache keys.
const FORCE_PREPARE_INTERVAL: u32 = 30;

/// Renderer-side encapsulation of the cosmic-text → glyphon path. Holds
/// glyphon device-bound state (atlas + viewport + swash cache) plus a
/// pool of [`GlyphonRenderer`]s, one per draw group with text. The
/// renderers share the atlas — glyph cache hits across groups are free.
///
/// **Why a pool, not a single renderer.** `GlyphonRenderer::prepare`
/// clears its `glyph_vertices` and overwrites its `vertex_buffer` —
/// calling `prepare` twice between two `render` draws would let the
/// second prepare overwrite the buffer the first draw still needs at
/// GPU execution time. Glyphon's `bind_group` and `get_or_create_pipeline`
/// are `pub(crate)`, so we can't bypass `render()` and slice into one
/// renderer's buffer ourselves with our own draw offsets.
///
/// Cost is small: pipeline is `Arc`-cloned across renderers (free), and
/// each renderer is just a ~4 KB vertex buffer + a `Vec<GlyphToRender>`
/// scratch. Capacity retains across frames; pool grows to historical
/// high water.
pub(crate) struct TextRenderer {
    /// Shared shaper handle, installed by
    /// [`super::WgpuBackend::new`]. Must be the *same* [`TextShaper`]
    /// the host installed on `Ui`, otherwise lookups in
    /// [`Self::prepare_group`] miss against keys minted on a different
    /// cache. [`Self::prepare_group`] silently skips when the shaper
    /// has no cosmic ([`TextShaper::with_render_split`] returns `None`).
    shaper: TextShaper,
    atlas: TextAtlas,
    viewport: Viewport,
    swash_cache: SwashCache,
    /// Pool of glyphon renderers, one slot per group that ever held text
    /// in this app's lifetime. Grows on demand to the historical high
    /// water; capacity retained across frames so steady state is alloc-
    /// free.
    renderers: Vec<GlyphonRenderer>,
    /// Stencil-aware mirror of `renderers`, lazy-built on the first
    /// `prepare_group(.., StencilMode::Stencil)` call. Apps that never
    /// use rounded clip never allocate this pool. Shares the same
    /// `atlas` (glyphon caches pipelines by `(format, multisample,
    /// depth_stencil)` — no fork needed).
    stencil_renderers: Vec<GlyphonRenderer>,
    /// Bit `i` says whether `renderers[i].prepare(...)` was called this
    /// frame and should be rendered. Length grows with the pool; bits
    /// past `renderers.len()` are unused. Reset to all-false in
    /// [`Self::post_record`].
    ready: FixedBitSet,
    /// Same shape as `ready`, for `stencil_renderers`.
    stencil_ready: FixedBitSet,
    /// Per-slot fingerprint of the runs handed to the most recent
    /// successful `prepare_group` call against `renderers[i]`. On a
    /// fresh call where `hash_runs(runs) == last_hashes[i]`, we skip
    /// the inner `glyphon::prepare` entirely — the renderer's vertex
    /// buffer + the atlas glyphs from the prior frame are still
    /// valid as long as [`Self::post_record`] suppresses
    /// `atlas.trim` on skip frames. `None` = no prior prepare, or
    /// the prior prepare's glyphs were invalidated by a trim.
    last_hashes: Vec<Option<u64>>,
    /// Mirror of `last_hashes` for `stencil_renderers`.
    last_hashes_stencil: Vec<Option<u64>>,
    /// Set if any `prepare_group` call this frame hash-skipped the
    /// inner `glyphon::prepare`. When set, [`Self::post_record`]
    /// must *not* call `atlas.trim` — trimming would evict glyphs
    /// the skipped renderers still depend on. Cleared at end of
    /// `post_record`.
    skipped_any_this_frame: bool,
    /// Counts consecutive frames where `skipped_any_this_frame` was
    /// true. When it reaches [`FORCE_PREPARE_INTERVAL`], the next
    /// `prepare_group` calls ignore hash hits and re-run
    /// `glyphon::prepare` so a subsequent `atlas.trim` can clean up
    /// long-tail accumulated cache_keys (e.g. unique scales minted
    /// during a long zoom gesture).
    frames_since_full_prepare: u32,
    /// Highest `group_idx + 1` prepared this frame across **either**
    /// pool. Used by [`Self::post_record`] to shrink whichever pool grew
    /// past `2 × high_water`. Shared because a given frame is either
    /// all-`Plain` or all-`Stencil` (the surrounding render pass picks
    /// one), so `high_water` reflects the active mode's group count
    /// and the inactive pool — if it overshot in a prior frame —
    /// trims down without losing live state (its `ready` bits were
    /// already cleared, slots are unused).
    high_water: usize,
    /// Last viewport size pushed to glyphon's viewport uniform. `ZERO`
    /// on construction; first non-zero `update_viewport` mismatches and
    /// uploads. Saves a per-frame `viewport.update` call in steady state.
    last_viewport: UVec2,
}

impl TextRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let cache = Cache::new(device);
        let atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let swash_cache = SwashCache::new();
        Self {
            shaper: TextShaper::default(),
            atlas,
            viewport,
            swash_cache,
            renderers: Vec::new(),
            stencil_renderers: Vec::new(),
            ready: FixedBitSet::new(),
            stencil_ready: FixedBitSet::new(),
            last_hashes: Vec::new(),
            last_hashes_stencil: Vec::new(),
            skipped_any_this_frame: false,
            frames_since_full_prepare: 0,
            high_water: 0,
            last_viewport: UVec2::ZERO,
        }
    }

    /// Install the shared shaper handle. Pass the same [`TextShaper`]
    /// to `Ui::text` so layout and rendering see
    /// one buffer cache.
    pub(crate) fn set_shaper(&mut self, shaper: TextShaper) {
        self.shaper = shaper;
    }

    /// True if any group has been prepared this frame and should render.
    pub(crate) fn has_prepared(&self) -> bool {
        self.ready.count_ones(..) > 0 || self.stencil_ready.count_ones(..) > 0
    }

    /// Update the viewport uniform. Called once per frame before the
    /// per-group prepares so all renderers see the same viewport.
    /// Skips the GPU upload when the viewport matches last frame's —
    /// glyphon's uniform contents are pure functions of the resolution.
    #[profiling::function]
    pub(crate) fn update_viewport(&mut self, queue: &wgpu::Queue, viewport_phys: UVec2) {
        if self.last_viewport == viewport_phys {
            return;
        }
        self.viewport.update(
            queue,
            Resolution {
                width: viewport_phys.x,
                height: viewport_phys.y,
            },
        );
        self.last_viewport = viewport_phys;
    }

    /// Build glyphon `TextArea`s from `runs` (looked up in the shared
    /// shaper's buffer cache) and call `prepare` on the pool slot at
    /// `group_idx`. `mode` selects the no-stencil or stencil-aware
    /// pool — both share `atlas`. Returns `false` and skips work if no
    /// shaper is installed or no runs resolve to a buffer. The pool
    /// grows on demand if `group_idx` exceeds its current length.
    ///
    /// **Hash-skip fast path.** Each successful prepare stashes a
    /// fingerprint of `runs` in [`Self::last_hashes`]. A later call
    /// with the same `(group_idx, runs)` skips the inner
    /// `glyphon::prepare` — the renderer's vertex buffer + atlas
    /// glyphs from the prior frame remain valid because
    /// [`Self::post_record`] suppresses `atlas.trim` on any frame
    /// where a skip occurred. After [`FORCE_PREPARE_INTERVAL`]
    /// consecutive skip frames, the next frame ignores hash hits to
    /// give `atlas.trim` a clean opportunity.
    #[profiling::function]
    pub(crate) fn prepare_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
        group_idx: usize,
        runs: &[TextRun],
        mode: StencilMode,
    ) -> bool {
        let want_force = self.frames_since_full_prepare >= FORCE_PREPARE_INTERVAL;
        let hash = hash_runs(runs);

        // Hash-skip check kept in a tight scope so we don't hold a
        // &mut on the per-mode pool/hash vec across the prepare path.
        if !want_force {
            let (pool_len, prior) = match mode {
                StencilMode::Plain => (
                    self.renderers.len(),
                    self.last_hashes.get(group_idx).copied().flatten(),
                ),
                StencilMode::Stencil => (
                    self.stencil_renderers.len(),
                    self.last_hashes_stencil.get(group_idx).copied().flatten(),
                ),
            };
            if group_idx < pool_len && prior == Some(hash) {
                match mode {
                    StencilMode::Plain => self.ready.insert(group_idx),
                    StencilMode::Stencil => self.stencil_ready.insert(group_idx),
                }
                if group_idx + 1 > self.high_water {
                    self.high_water = group_idx + 1;
                }
                self.skipped_any_this_frame = true;
                return true;
            }
        }

        // Clone to release the `&self.shaper` borrow for the closure
        // body — refcount bump, ~free. `with_render_split` returns
        // `None` when the shaper is mono (no cosmic to split), which
        // we surface as `false` (no work done this group).
        let shaper = self.shaper.clone();
        shaper
            .with_render_split(|split| {
                let RenderSplit {
                    font_system,
                    lookup,
                } = split;

                // Skip-empty without materializing a Vec<TextArea>. Two
                // passes over `runs` (count + filter_map into the iterator
                // handed to prepare). Both are O(runs.len()) on typical
                // handfuls of runs per group, and avoid the
                // lifetime-laundering scratch field.
                let resolvable = runs.iter().filter(|r| lookup.get(r.key).is_some()).count();
                if resolvable == 0 {
                    return false;
                }

                // Grow target pool to accommodate `group_idx`. Each
                // renderer is small (one wgpu vertex buffer + a
                // Vec<GlyphToRender>); pipelines are reused via
                // `atlas.get_or_create_pipeline`.
                let depth_stencil = match mode {
                    StencilMode::Plain => None,
                    StencilMode::Stencil => Some(super::stencil_test_state()),
                };
                let (pool, ready, last_hashes) = match mode {
                    StencilMode::Plain => {
                        (&mut self.renderers, &mut self.ready, &mut self.last_hashes)
                    }
                    StencilMode::Stencil => (
                        &mut self.stencil_renderers,
                        &mut self.stencil_ready,
                        &mut self.last_hashes_stencil,
                    ),
                };
                while pool.len() <= group_idx {
                    let renderer = GlyphonRenderer::new(
                        &mut self.atlas,
                        device,
                        wgpu::MultisampleState::default(),
                        depth_stencil.clone(),
                    );
                    pool.push(renderer);
                }
                ready.grow(pool.len());
                if last_hashes.len() < pool.len() {
                    last_hashes.resize(pool.len(), None);
                }

                let text_areas = runs.iter().filter_map(|r| {
                    lookup.get(r.key).map(|buffer| TextArea {
                        buffer,
                        left: r.origin.x,
                        top: r.origin.y,
                        // DPI scale × ancestor transform scale (composer
                        // picks up the cumulative `TranslateScale.scale`
                        // so a zoomed Scroll subtree paints proportionally
                        // larger glyphs).
                        scale: scale * r.scale,
                        bounds: text_bounds(r.bounds),
                        default_color: glyphon_color(r.color),
                        custom_glyphs: &[],
                    })
                });

                let result = pool[group_idx].prepare(
                    device,
                    queue,
                    font_system,
                    &mut self.atlas,
                    &self.viewport,
                    text_areas,
                    &mut self.swash_cache,
                );

                if let Err(e) = result {
                    tracing::error!(?e, group_idx, ?mode, "glyphon prepare failed");
                    ready.remove(group_idx);
                    last_hashes[group_idx] = None;
                    return false;
                }
                ready.insert(group_idx);
                last_hashes[group_idx] = Some(hash);
                if group_idx + 1 > self.high_water {
                    self.high_water = group_idx + 1;
                }
                true
            })
            .unwrap_or(false)
    }

    /// Render the prepared text for `group_idx` from the `mode` pool.
    /// Silently no-ops if the group wasn't prepared this frame in that
    /// mode (no text, no shaper, prepare failed, or wrong pool).
    pub(crate) fn render_group(
        &self,
        group_idx: usize,
        pass: &mut wgpu::RenderPass<'_>,
        mode: StencilMode,
    ) {
        let (pool, ready) = match mode {
            StencilMode::Plain => (&self.renderers, &self.ready),
            StencilMode::Stencil => (&self.stencil_renderers, &self.stencil_ready),
        };
        if !ready.contains(group_idx) {
            return;
        }
        if let Err(e) = pool[group_idx].render(&self.atlas, &self.viewport, pass) {
            tracing::warn!(?e, group_idx, "glyphon render failed");
        }
    }

    /// Reclaim atlas slots for glyphs unused this frame, shrink the
    /// renderer pool if it's grossly over-allocated, and reset
    /// per-renderer ready flags. Call once after all `render_group`
    /// calls have been submitted in the encoder pass.
    ///
    /// `atlas.trim` runs only on frames where every active group
    /// passed through the full `glyphon::prepare` path — those frames
    /// have `glyphs_in_use` fully populated, so trim is safe. On
    /// any frame where one or more groups hash-skipped, trim is
    /// suppressed (it would evict glyphs the skipped renderers still
    /// reference). The [`FORCE_PREPARE_INTERVAL`] threshold in
    /// `prepare_group` bounds how long this suppression can run
    /// before a forced full-prepare frame restores the trim
    /// opportunity.
    pub(crate) fn post_record(&mut self) {
        if self.skipped_any_this_frame {
            self.frames_since_full_prepare += 1;
        } else {
            self.atlas.trim();
            self.frames_since_full_prepare = 0;
            // After a real trim, atlas slots for groups that *weren't*
            // touched this frame may have been evicted. Their cached
            // hash would otherwise enable a future spurious skip
            // against an empty atlas — invalidate now.
            for i in 0..self.last_hashes.len() {
                if !self.ready.contains(i) {
                    self.last_hashes[i] = None;
                }
            }
            for i in 0..self.last_hashes_stencil.len() {
                if !self.stencil_ready.contains(i) {
                    self.last_hashes_stencil[i] = None;
                }
            }
        }
        self.skipped_any_this_frame = false;
        // Shrink only when pool is more than 2× high_water — see
        // [`POOL_SHRINK_RATIO`]. Skips truncate work entirely in
        // steady state. Both pools follow the same rule.
        // Pools can shrink; `ready`/`stencil_ready` only grow (one bit
        // per renderer). Bits past `pool.len()` are never read after a
        // shrink, and `clear()` below zeros them anyway. `last_hashes`
        // is truncated in lockstep so a future regrow doesn't reuse
        // a stale fingerprint from a different content era.
        if self.renderers.len() > self.high_water.saturating_mul(POOL_SHRINK_RATIO) {
            self.renderers.truncate(self.high_water);
            self.last_hashes.truncate(self.high_water);
        }
        if self.stencil_renderers.len() > self.high_water.saturating_mul(POOL_SHRINK_RATIO) {
            self.stencil_renderers.truncate(self.high_water);
            self.last_hashes_stencil.truncate(self.high_water);
        }
        self.ready.clear();
        self.stencil_ready.clear();
        self.high_water = 0;
    }
}

/// Fingerprint a slice of [`TextRun`]s for the `prepare_group`
/// hash-skip fast path. `TextRun` is `#[repr(C)]` with no internal
/// padding, so a single byte-slice write covers every byte glyphon
/// would consume — origin, bounds, color, key, scale.
fn hash_runs(runs: &[TextRun]) -> u64 {
    use std::hash::Hasher as _;
    let mut h = crate::common::hash::Hasher::new();
    h.write_usize(runs.len());
    h.write(bytemuck::cast_slice(runs));
    h.finish()
}

fn text_bounds(b: URect) -> TextBounds {
    TextBounds {
        left: b.x as i32,
        top: b.y as i32,
        right: (b.x + b.w) as i32,
        bottom: (b.y + b.h) as i32,
    }
}

fn glyphon_color(c: Srgb8) -> glyphon::Color {
    // Glyphon's default `ColorMode::Accurate` decodes the byte channels
    // sRGB→linear inside its shader before writing to the sRGB target —
    // `TextRun.color` is already sRGB-encoded at compose time.
    glyphon::Color::rgba(c.r, c.g, c.b, c.a)
}
