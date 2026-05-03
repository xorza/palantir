//! Glyphon-backed text renderer with **per-group prepare/render** so text
//! interleaves correctly with quads in paint order. The wgpu backend calls
//! [`TextRenderer::prepare_group`] for each `DrawGroup` whose `texts` range
//! is non-empty, then inside the render pass calls
//! [`TextRenderer::render_group`] right after that group's quads draw.
//! Glyph data is shared via a single [`TextAtlas`] across all renderers in
//! the pool, so the cache is hit for free across groups.
//!
//! [`CosmicMeasure`]: crate::text::CosmicMeasure
//! [`TextRun`]: super::super::buffer::TextRun

use super::super::buffer::TextRun;
use crate::primitives::URect;
use crate::text::SharedCosmic;
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonRenderer, Viewport,
};

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
    cosmic: Option<SharedCosmic>,
    cache: Cache,
    atlas: TextAtlas,
    viewport: Viewport,
    swash_cache: SwashCache,
    /// Pool of glyphon renderers, one slot per group that ever held text
    /// in this app's lifetime. Grows on demand to the historical high
    /// water; capacity retained across frames so steady state is alloc-
    /// free.
    renderers: Vec<GlyphonRenderer>,
    /// Same length as `renderers`. `ready[i]` says whether
    /// `renderers[i].prepare(...)` was called this frame and should be
    /// rendered. Reset to all-false in [`Self::end_frame`].
    ready: Vec<bool>,
    /// Highest `group_idx + 1` prepared this frame. Used by
    /// [`Self::end_frame`] to truncate the pool down to the slots that
    /// were actually used, so a frame burst (e.g. an open modal with
    /// many labels) doesn't leave its renderer slots — and their wgpu
    /// vertex buffers — alive forever after the modal closes.
    high_water: usize,
    /// Reusable scratch for `TextArea`s built each `prepare_group` call.
    /// Capacity retained.
    scratch: Vec<TextArea<'static>>,
}

impl TextRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(device);
        let atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let swash_cache = SwashCache::new();
        Self {
            cosmic: None,
            cache,
            atlas,
            viewport,
            swash_cache,
            renderers: Vec::new(),
            ready: Vec::new(),
            high_water: 0,
            scratch: Vec::new(),
        }
    }

    /// Install the shared shaper handle. Pass the same `SharedCosmic` to
    /// [`crate::Ui::set_cosmic`] so layout and rendering see one cache.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.cosmic = Some(cosmic);
    }

    /// Re-create on surface format change (e.g. after window recreation).
    /// Replaces the atlas + drops the renderer pool (each renderer holds
    /// pipeline state tied to the old format).
    pub fn rebuild_for_format(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        self.atlas = TextAtlas::new(device, queue, &self.cache, format);
        self.renderers.clear();
        self.ready.clear();
        self.high_water = 0;
    }

    /// True if any group has been prepared this frame and should render.
    pub fn has_prepared(&self) -> bool {
        self.ready.iter().any(|&r| r)
    }

    /// Update the viewport uniform. Called once per frame before the
    /// per-group prepares so all renderers see the same viewport.
    pub fn update_viewport(&mut self, queue: &wgpu::Queue, viewport_phys: [u32; 2]) {
        self.viewport.update(
            queue,
            Resolution {
                width: viewport_phys[0],
                height: viewport_phys[1],
            },
        );
    }

    /// Build glyphon `TextArea`s from `runs` (looked up in the shared
    /// cosmic cache) and call `prepare` on the renderer pool slot at
    /// `group_idx`. Returns `false` and skips work if no shaper is
    /// installed or no runs resolve to a buffer. The pool grows on
    /// demand if `group_idx` exceeds the current pool length.
    pub fn prepare_group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
        group_idx: usize,
        runs: &[TextRun],
    ) -> bool {
        let Some(cosmic) = self.cosmic.as_ref() else {
            return false;
        };
        let mut cosmic = cosmic.borrow_mut();

        // Erase the `'static` lifetime of `self.scratch` to a frame-local
        // borrow tied to `cosmic`. Sound: scratch is cleared at the bottom
        // of this method, no `TextArea` reference escapes the function.
        let scratch: &mut Vec<TextArea<'_>> = unsafe { std::mem::transmute(&mut self.scratch) };
        scratch.clear();

        let (font_system, lookup) = cosmic.split_for_render();
        for r in runs {
            let Some(buffer) = lookup.get(r.key) else {
                continue;
            };
            scratch.push(TextArea {
                buffer,
                left: r.origin.x,
                top: r.origin.y,
                scale,
                bounds: text_bounds(r.bounds),
                default_color: glyphon_color(r.color),
                custom_glyphs: &[],
            });
        }
        if scratch.is_empty() {
            return false;
        }

        // Grow pool to accommodate `group_idx`. Each renderer is small
        // (one wgpu vertex buffer + a Vec<GlyphToRender>); pipeline is
        // reused via `atlas.get_or_create_pipeline`.
        while self.renderers.len() <= group_idx {
            let renderer = GlyphonRenderer::new(
                &mut self.atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            );
            self.renderers.push(renderer);
            self.ready.push(false);
        }

        let result = self.renderers[group_idx].prepare(
            device,
            queue,
            font_system,
            &mut self.atlas,
            &self.viewport,
            scratch.iter().cloned(),
            &mut self.swash_cache,
        );
        // Drop scratch borrows before returning so the `'static`
        // placeholder is sound for the next call.
        scratch.clear();

        if let Err(e) = result {
            tracing::warn!(?e, group_idx, "glyphon prepare failed");
            self.ready[group_idx] = false;
            return false;
        }
        self.ready[group_idx] = true;
        if group_idx + 1 > self.high_water {
            self.high_water = group_idx + 1;
        }
        true
    }

    /// Render the prepared text for `group_idx`. Silently no-ops if the
    /// group wasn't prepared this frame (no text, no shaper, or prepare
    /// failed).
    pub fn render_group(&self, group_idx: usize, pass: &mut wgpu::RenderPass<'_>) {
        if !matches!(self.ready.get(group_idx), Some(true)) {
            return;
        }
        if let Err(e) = self.renderers[group_idx].render(&self.atlas, &self.viewport, pass) {
            tracing::warn!(?e, group_idx, "glyphon render failed");
        }
    }

    /// Reclaim atlas slots for glyphs unused this frame, shrink the
    /// renderer pool if it's grossly over-allocated, and reset
    /// per-renderer ready flags. Call once after all `render_group`
    /// calls have been submitted in the encoder pass.
    pub fn end_frame(&mut self) {
        self.atlas.trim();
        // Shrink only when pool is more than 2× high_water — see
        // [`POOL_SHRINK_RATIO`]. Skips truncate work entirely in
        // steady state.
        if self.renderers.len() > self.high_water.saturating_mul(POOL_SHRINK_RATIO) {
            self.renderers.truncate(self.high_water);
            self.ready.truncate(self.high_water);
        }
        for r in &mut self.ready {
            *r = false;
        }
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

fn glyphon_color(c: crate::primitives::Color) -> glyphon::Color {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    glyphon::Color::rgba(r, g, b, a)
}
