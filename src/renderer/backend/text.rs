//! Glyphon-backed text renderer with **per-batch prepare/render** so text
//! interleaves correctly with quads in paint order. The wgpu backend calls
//! [`TextRenderer::prepare_batch`] for each [`TextBatch`] (a coalesced run
//! of text spans that share one `glyphon::prepare_append`), then inside
//! the render pass calls [`TextRenderer::render_batch`] right after the
//! batch's last group's quads draw.
//!
//! **One [`GlyphonRenderer`] holding two pipelines.** The vertex
//! buffer is shared across all batches regardless of [`StencilMode`];
//! each batch gets a `Range<u32>` of instances from `prepare_append`,
//! and `render_range(.., pipeline_idx, ..)` picks the matching
//! pipeline at draw time (plain vs depth-stencil-aware). Stencil
//! pipelines are needed because glyphon's pipeline state must match
//! the surrounding render pass's attachments — using the no-stencil
//! pipeline inside a stencil pass triggers wgpu validation errors.
//!
//! [`TextBatch`]: crate::renderer::render_buffer::TextBatch
//!
//! [`CosmicMeasure`]: crate::text::cosmic::CosmicMeasure
//! [`TextRun`]: crate::renderer::render_buffer::TextRun

use crate::primitives::color::ColorU8;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::TextRun;
use crate::text::TextShaper;
use crate::text::cosmic::RenderSplit;
use glam::UVec2;
use glyphon::{
    Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer,
    Viewport,
};
use std::ops::Range;

/// Selects which pipeline a `prepare_batch` / `render_batch` call
/// targets. Plain frames use the no-stencil pipeline; rounded-clip
/// frames need the stencil-aware pipeline so the per-pass stencil
/// reference applies. Both share one vertex buffer + one atlas — only
/// the [`RenderPipeline`] object differs.
///
/// [`RenderPipeline`]: wgpu::RenderPipeline
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilMode {
    Plain,
    Stencil,
}

impl StencilMode {
    /// Index into the `depth_stencil_states` slice passed to
    /// [`GlyphonRenderer::new`].
    fn pipeline_idx(self) -> usize {
        match self {
            Self::Plain => 0,
            Self::Stencil => 1,
        }
    }
}

/// Renderer-side encapsulation of the cosmic-text → glyphon path. Holds
/// glyphon device-bound state (atlas + viewport + swash cache) plus
/// one [`GlyphonRenderer`] carrying both pipelines (`Plain`,
/// `Stencil`) and one shared vertex buffer.
pub(crate) struct TextRenderer {
    /// Shared shaper handle, installed at construction. Must be the
    /// *same* [`TextShaper`] the host installed on `Ui`, otherwise
    /// lookups in [`Self::prepare_batch`] miss against keys minted
    /// on a different cache.
    shaper: TextShaper,
    atlas: TextAtlas,
    viewport: Viewport,
    swash_cache: SwashCache,
    renderer: GlyphonRenderer,
    /// `ranges[i]` = batch `i`'s instance range, or `None` if batch
    /// `i` wasn't prepared this frame. The pipeline (plain vs
    /// stencil) is passed at render time. Capacity retained across
    /// frames; reset to all-`None` (not deallocated) in
    /// [`Self::post_record`].
    ranges: Vec<Option<Range<u32>>>,
    /// True if at least one `prepare_batch` succeeded this frame.
    /// Drives `has_prepared` (the wgpu backend skips `post_record`
    /// entirely when false).
    prepared_anything: bool,
}

impl TextRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
    ) -> Self {
        let atlas = TextAtlas::new(device, format);
        let viewport = Viewport::new(device, &atlas);
        let swash_cache = SwashCache::new();
        let renderer = GlyphonRenderer::new(
            &atlas,
            device,
            wgpu::MultisampleState::default(),
            // Index 0 = Plain, index 1 = Stencil. See `StencilMode::pipeline_idx`.
            &[None, Some(super::stencil::stencil_test_state())],
        );
        Self {
            shaper,
            atlas,
            viewport,
            swash_cache,
            renderer,
            ranges: Vec::new(),
            prepared_anything: false,
        }
    }

    /// True if any batch has been prepared this frame and should render.
    pub(crate) fn has_prepared(&self) -> bool {
        self.prepared_anything
    }

    /// Push the viewport uniform with `viewport_phys`. Cheap when
    /// nothing changed — glyphon's [`Viewport::update`] short-circuits
    /// when `(resolution, atlas_sizes)` matches the previous call
    /// (`vendor/glyphon/src/viewport.rs:52`). Call once per frame
    /// before the per-batch prepares.
    #[profiling::function]
    pub(crate) fn update_viewport(&mut self, queue: &wgpu::Queue, viewport_phys: UVec2) {
        self.upload_viewport(
            queue,
            Resolution {
                width: viewport_phys.x,
                height: viewport_phys.y,
            },
        );
    }

    /// Re-push the viewport uniform after the frame's `prepare` calls
    /// in case the atlas grew (atlas sizes feed the uniform). Reads
    /// the resolution back from the viewport so it always matches the
    /// last `update_viewport`. Short-circuits inside glyphon when
    /// atlas + resolution are unchanged.
    #[profiling::function]
    pub(crate) fn sync_atlas_to_viewport(&mut self, queue: &wgpu::Queue) {
        let resolution = self.viewport.resolution();
        self.upload_viewport(queue, resolution);
    }

    fn upload_viewport(&mut self, queue: &wgpu::Queue, resolution: Resolution) {
        self.viewport.update(queue, resolution, &self.atlas);
    }

    /// Build glyphon `TextArea`s from `runs` (looked up in the shared
    /// shaper's buffer cache) and call `prepare_append` on the
    /// renderer. Returns `false` and skips work if no runs resolve to
    /// a buffer. The per-batch `Range<u32>` returned by glyphon is
    /// stashed at `batch_idx` along with `mode` for [`Self::render_batch`].
    #[profiling::function]
    pub(crate) fn prepare_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
        batch_idx: usize,
        runs: &[TextRun],
        mode: StencilMode,
    ) -> bool {
        // Clone to release the `&self.shaper` borrow for the closure
        // body — refcount bump, ~free. `with_render_split` returns
        // `None` when the shaper is mono (no cosmic to split), which
        // we surface as `false` (no work done this batch).
        let shaper = self.shaper.clone();
        shaper
            .with_render_split(|split| {
                let RenderSplit {
                    font_system,
                    lookup,
                } = split;

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
                    })
                });

                let result = self.renderer.prepare_append(
                    device,
                    queue,
                    font_system,
                    &mut self.atlas,
                    &self.viewport,
                    text_areas,
                    &mut self.swash_cache,
                );

                match result {
                    Ok(range) => {
                        if self.ranges.len() <= batch_idx {
                            self.ranges.resize(batch_idx + 1, None);
                        }
                        let did_work = !range.is_empty();
                        self.ranges[batch_idx] = Some(range);
                        did_work
                    }
                    Err(e) => {
                        tracing::error!(?e, batch_idx, ?mode, "glyphon prepare failed");
                        false
                    }
                }
            })
            .inspect(|&did_work| {
                if did_work {
                    self.prepared_anything = true;
                }
            })
            .unwrap_or(false)
    }

    /// Render the prepared text for `batch_idx`. Silently no-ops if
    /// the batch wasn't prepared this frame (no text, no shaper,
    /// prepare failed). `mode` selects which pipeline draws — must
    /// match the surrounding pass's stencil state.
    pub(crate) fn render_batch(
        &self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'_>,
        mode: StencilMode,
    ) {
        let Some(range) = self.ranges.get(batch_idx).cloned().flatten() else {
            return;
        };
        self.renderer.render_range(
            range,
            mode.pipeline_idx(),
            &self.atlas,
            &self.viewport,
            pass,
        );
    }

    /// Reclaim atlas slots for glyphs unused this frame and reset
    /// per-batch range tracking + the glyphon vertex accumulator. Call
    /// once after all `render_batch` calls have been submitted in the
    /// encoder pass.
    pub(crate) fn post_record(&mut self) {
        self.atlas.trim();
        self.renderer.clear_frame();
        self.ranges.fill(None);
        self.prepared_anything = false;
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

fn glyphon_color(c: ColorU8) -> glyphon::Color {
    // Glyphon's shader decodes the byte channels sRGB→linear before
    // writing to the sRGB target — `TextRun.color` is already sRGB-
    // encoded at compose time.
    glyphon::Color::rgba(c.r, c.g, c.b, c.a)
}
