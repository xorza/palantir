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

use crate::primitives::color::Color;
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
    /// Shared shaper handle, installed via
    /// [`super::WgpuBackend::set_text_shaper`]. Must be the *same*
    /// [`TextShaper`] the host installed on `Ui`, otherwise lookups
    /// in [`Self::prepare_group`] miss against keys minted on a
    /// different cache. Default = [`TextShaper::default`] (mono);
    /// [`Self::prepare_group`] silently skips when the shaper has no
    /// cosmic ([`TextShaper::with_render_split`] returns `None`).
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
    /// [`Self::end_frame`].
    ready: FixedBitSet,
    /// Same shape as `ready`, for `stencil_renderers`.
    stencil_ready: FixedBitSet,
    /// Highest `group_idx + 1` prepared this frame across **either**
    /// pool. Used by [`Self::end_frame`] to shrink whichever pool grew
    /// past `2 × high_water`. Shared because a given frame is either
    /// all-`Plain` or all-`Stencil` (the surrounding render pass picks
    /// one), so `high_water` reflects the active mode's group count
    /// and the inactive pool — if it overshot in a prior frame —
    /// trims down without losing live state (its `ready` bits were
    /// already cleared, slots are unused).
    high_water: usize,
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
            high_water: 0,
        }
    }

    /// Install the shared shaper handle. Pass the same [`TextShaper`]
    /// to [`crate::Ui::set_text_shaper`] so layout and rendering see
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
    pub(crate) fn update_viewport(&mut self, queue: &wgpu::Queue, viewport_phys: UVec2) {
        self.viewport.update(
            queue,
            Resolution {
                width: viewport_phys.x,
                height: viewport_phys.y,
            },
        );
    }

    /// Build glyphon `TextArea`s from `runs` (looked up in the shared
    /// shaper's buffer cache) and call `prepare` on the pool slot at
    /// `group_idx`. `mode` selects the no-stencil or stencil-aware
    /// pool — both share `atlas`. Returns `false` and skips work if no
    /// shaper is installed or no runs resolve to a buffer. The pool
    /// grows on demand if `group_idx` exceeds its current length.
    pub(crate) fn prepare_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
        group_idx: usize,
        runs: &[TextRun],
        mode: StencilMode,
    ) -> bool {
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
                let (pool, ready) = match mode {
                    StencilMode::Plain => (&mut self.renderers, &mut self.ready),
                    StencilMode::Stencil => (&mut self.stencil_renderers, &mut self.stencil_ready),
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

                let text_areas = runs.iter().filter_map(|r| {
                    lookup.get(r.key).map(|buffer| TextArea {
                        buffer,
                        left: r.origin.x,
                        top: r.origin.y,
                        scale,
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
                    return false;
                }
                ready.insert(group_idx);
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
    pub(crate) fn end_frame(&mut self) {
        self.atlas.trim();
        // Shrink only when pool is more than 2× high_water — see
        // [`POOL_SHRINK_RATIO`]. Skips truncate work entirely in
        // steady state. Both pools follow the same rule.
        // Pools can shrink; `ready`/`stencil_ready` only grow (one bit
        // per renderer). Bits past `pool.len()` are never read after a
        // shrink, and `clear()` below zeros them anyway.
        if self.renderers.len() > self.high_water.saturating_mul(POOL_SHRINK_RATIO) {
            self.renderers.truncate(self.high_water);
        }
        if self.stencil_renderers.len() > self.high_water.saturating_mul(POOL_SHRINK_RATIO) {
            self.stencil_renderers.truncate(self.high_water);
        }
        self.ready.clear();
        self.stencil_ready.clear();
        self.high_water = 0;
    }
}

fn text_bounds(b: URect) -> TextBounds {
    TextBounds {
        left: b.x as i32,
        top: b.y as i32,
        right: (b.x + b.w) as i32,
        bottom: (b.y + b.h) as i32,
    }
}

fn glyphon_color(c: Color) -> glyphon::Color {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    glyphon::Color::rgba(r, g, b, a)
}
