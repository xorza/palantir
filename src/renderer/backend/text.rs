//! Glyphon-backed text renderer with **per-batch prepare/render** so text
//! interleaves correctly with quads in paint order. The wgpu backend calls
//! [`TextRenderer::prepare_batch`] for each [`TextBatch`] (a coalesced run
//! of text spans that share one `glyphon::prepare_append`), then inside
//! the render pass calls [`TextRenderer::render_batch`] right after the
//! batch's last group's quads draw.
//!
//! **One [`GlyphonRenderer`] per [`StencilMode`].** Glyphon's vertex
//! buffer accumulates instances across batches via the vendored
//! `prepare_append` / `render_range` API — each batch gets a
//! `Range<u32>` of instances and `render_range` draws only that range,
//! so per-batch interleaving with quads still works while sharing one
//! buffer + one pipeline state across all batches. Stencil mode is a
//! separate renderer because glyphon caches its pipeline by
//! `(format, multisample, depth_stencil)`.
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
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonRenderer, Viewport,
};
use std::ops::Range;

/// Selects which renderer a `prepare_batch` / `render_batch` call
/// targets. Plain frames stay on the no-stencil renderer (existing
/// behavior). When the surrounding pass has a stencil attachment
/// (rounded-clip path), text must use a depth-stencil-aware glyphon
/// pipeline or wgpu validation errors — `Stencil` selects that one.
/// Both share the underlying `TextAtlas` so glyph caches hit across
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

/// One renderer + the per-batch ranges it produced this frame.
struct ModeState {
    renderer: GlyphonRenderer,
    /// `ranges[i]` = instance range returned by this frame's
    /// `prepare_append` for batch `i`, or `None` if batch `i` wasn't
    /// prepared in this mode this frame. Capacity retained across
    /// frames; reset to all-`None` (not deallocated) in
    /// [`TextRenderer::post_record`].
    ranges: Vec<Option<Range<u32>>>,
}

impl ModeState {
    fn new(renderer: GlyphonRenderer) -> Self {
        Self {
            renderer,
            ranges: Vec::new(),
        }
    }
}

/// Renderer-side encapsulation of the cosmic-text → glyphon path. Holds
/// glyphon device-bound state (atlas + viewport + swash cache) plus one
/// [`GlyphonRenderer`] per [`StencilMode`]. Renderers share the
/// atlas — glyph cache hits across batches and across modes are free.
pub(crate) struct TextRenderer {
    /// Shared shaper handle, installed at construction. Must be the
    /// *same* [`TextShaper`] the host installed on `Ui`, otherwise
    /// lookups in [`Self::prepare_batch`] miss against keys minted
    /// on a different cache.
    shaper: TextShaper,
    atlas: TextAtlas,
    viewport: Viewport,
    swash_cache: SwashCache,
    /// No-stencil renderer. Built eagerly.
    plain: ModeState,
    /// Stencil-aware renderer. Lazy-built on the first
    /// `prepare_batch(.., StencilMode::Stencil)` call. Apps that never
    /// use rounded clip never instantiate it. Shares the atlas with
    /// `plain`.
    stencil: Option<ModeState>,
    /// Last viewport size pushed to glyphon's viewport uniform. `ZERO`
    /// on construction; first non-zero `update_viewport` mismatches
    /// and uploads. Saves a per-frame `viewport.update` call in steady
    /// state.
    ///
    /// **Independent of [`super::viewport::ViewportUniform::last`].**
    /// Both fields track the same logical signal but gate writes to
    /// two *different* GPU buffers — glyphon owns its own uniform via
    /// [`Viewport::update`] and isn't reachable through the shared
    /// quad/mesh/image `ViewportUniform`.
    last_viewport: UVec2,
    /// True if at least one `prepare_batch` succeeded this frame.
    /// Drives `has_prepared` (the wgpu backend skips `post_record`
    /// entirely when false).
    prepared_anything: bool,
}

impl TextRenderer {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
    ) -> Self {
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let swash_cache = SwashCache::new();
        let plain_renderer =
            GlyphonRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Self {
            shaper,
            atlas,
            viewport,
            swash_cache,
            plain: ModeState::new(plain_renderer),
            stencil: None,
            last_viewport: UVec2::ZERO,
            prepared_anything: false,
        }
    }

    /// True if any batch has been prepared this frame and should render.
    pub(crate) fn has_prepared(&self) -> bool {
        self.prepared_anything
    }

    /// Update the viewport uniform. Called once per frame before the
    /// per-batch prepares so both renderers see the same viewport.
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
    /// shaper's buffer cache) and call `prepare_append` on the renderer
    /// for `mode`. Returns `false` and skips work if no runs resolve to
    /// a buffer. The per-batch `Range<u32>` returned by glyphon is
    /// stashed at `batch_idx` for [`Self::render_batch`].
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
        // Lazy-build the stencil-mode renderer on first use.
        if matches!(mode, StencilMode::Stencil) && self.stencil.is_none() {
            profiling::scope!("lazy_stencil_build");
            let renderer = GlyphonRenderer::new(
                &mut self.atlas,
                device,
                wgpu::MultisampleState::default(),
                Some(super::stencil::stencil_test_state()),
            );
            self.stencil = Some(ModeState::new(renderer));
        }

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

                // Inline match instead of a helper because we need
                // disjoint field borrows: `renderer.prepare_append`
                // also takes `&mut self.atlas`, `&self.viewport`,
                // `&mut self.swash_cache`.
                let state = match mode {
                    StencilMode::Plain => &mut self.plain,
                    StencilMode::Stencil => self.stencil.as_mut().unwrap(),
                };

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

                let result = state.renderer.prepare_append(
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
                        if state.ranges.len() <= batch_idx {
                            state.ranges.resize(batch_idx + 1, None);
                        }
                        let did_work = !range.is_empty();
                        state.ranges[batch_idx] = Some(range);
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

    /// Render the prepared text for `batch_idx` from the `mode`
    /// renderer. Silently no-ops if the batch wasn't prepared this
    /// frame in that mode (no text, no shaper, prepare failed, or
    /// wrong mode).
    pub(crate) fn render_batch(
        &self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'_>,
        mode: StencilMode,
    ) {
        let state = match mode {
            StencilMode::Plain => &self.plain,
            StencilMode::Stencil => {
                let Some(s) = self.stencil.as_ref() else {
                    return;
                };
                s
            }
        };
        let Some(range) = state.ranges.get(batch_idx).cloned().flatten() else {
            return;
        };
        if let Err(e) = state
            .renderer
            .render_range(range, &self.atlas, &self.viewport, pass)
        {
            tracing::warn!(?e, batch_idx, "glyphon render_range failed");
        }
    }

    /// Reclaim atlas slots for glyphs unused this frame and reset
    /// per-batch range tracking + the glyphon vertex accumulator. Call
    /// once after all `render_batch` calls have been submitted in the
    /// encoder pass.
    pub(crate) fn post_record(&mut self) {
        self.atlas.trim();
        self.plain.renderer.clear_frame();
        self.plain.ranges.fill(None);
        if let Some(s) = self.stencil.as_mut() {
            s.renderer.clear_frame();
            s.ranges.fill(None);
        }
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
    // Glyphon's default `ColorMode::Accurate` decodes the byte channels
    // sRGB→linear inside its shader before writing to the sRGB target —
    // `TextRun.color` is already sRGB-encoded at compose time.
    glyphon::Color::rgba(c.r, c.g, c.b, c.a)
}
