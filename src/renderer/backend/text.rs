//! Glyphon-backed text renderer. Owns the device-bound state (`Cache`,
//! `TextAtlas`, `Viewport`, `glyphon::TextRenderer`, `SwashCache`) and a
//! shared handle to the [`CosmicMeasure`] populated by layout, so prepare
//! can look up each [`TextRun`]'s shaped buffer directly from the cache the
//! Ui side filled.
//!
//! v1 limitation: all text is prepared and rendered after all quads, so text
//! always paints on top of every quad in the frame. This matches the common
//! case (button label over button background) but means a parent's label
//! will visually float over a child's background. Fix when the first widget
//! needs the opposite z-order — likely via per-group prepare/render or
//! glyphon's depth metadata.
//!
//! [`CosmicMeasure`]: crate::text::CosmicMeasure
//! [`TextRun`]: super::super::buffer::TextRun

use super::super::buffer::{ScissorRect, TextRun};
use crate::text::SharedCosmic;
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonRenderer, Viewport,
};

/// Renderer-side encapsulation of the cosmic-text → glyphon path. Holds the
/// glyphon device-bound state plus a shared handle to the same
/// [`CosmicMeasure`] the Ui side measured against, so the buffer cache is
/// the single source of truth.
pub(crate) struct TextRenderer {
    cosmic: Option<SharedCosmic>,
    cache: Cache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: GlyphonRenderer,
    swash_cache: SwashCache,
    /// Reusable scratch for `TextArea`s built each frame from the
    /// `RenderBuffer.texts`. Cleared per submit, capacity retained.
    scratch: Vec<TextArea<'static>>,
}

impl TextRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let renderer =
            GlyphonRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let swash_cache = SwashCache::new();
        Self {
            cosmic: None,
            cache,
            atlas,
            viewport,
            renderer,
            swash_cache,
            scratch: Vec::new(),
        }
    }

    /// Install the shared shaper handle. Pass the same `SharedCosmic` to
    /// [`crate::Ui::set_cosmic`] so layout and rendering see one cache.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.cosmic = Some(cosmic);
    }

    /// Re-create on surface format change (e.g. after window recreation).
    pub fn rebuild_for_format(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        self.atlas = TextAtlas::new(device, queue, &self.cache, format);
        self.renderer = GlyphonRenderer::new(
            &mut self.atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
    }

    /// Build glyphon `TextArea`s from `runs` (looked up in the shared cosmic
    /// cache) and call `prepare`. Returns `false` and skips work if no
    /// shaper is installed or no runs resolve to a buffer.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_phys: [u32; 2],
        scale: f32,
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
                left: r.origin[0],
                top: r.origin[1],
                scale,
                bounds: text_bounds(r.bounds),
                default_color: glyphon_color(r.color),
                custom_glyphs: &[],
            });
        }
        if scratch.is_empty() {
            return false;
        }

        self.viewport.update(
            queue,
            Resolution {
                width: viewport_phys[0],
                height: viewport_phys[1],
            },
        );

        let result = self.renderer.prepare(
            device,
            queue,
            font_system,
            &mut self.atlas,
            &self.viewport,
            scratch.iter().cloned(),
            &mut self.swash_cache,
        );
        // Drop scratch borrows before returning so the `'static` placeholder
        // is sound for the next frame.
        scratch.clear();

        if let Err(e) = result {
            tracing::warn!(?e, "glyphon prepare failed");
            return false;
        }
        true
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Err(e) = self.renderer.render(&self.atlas, &self.viewport, pass) {
            tracing::warn!(?e, "glyphon render failed");
        }
    }

    /// Reclaim atlas slots that were allocated for glyphs unused this frame.
    /// Call once per frame after `render` returns.
    pub fn end_frame(&mut self) {
        self.atlas.trim();
    }
}

fn text_bounds(b: ScissorRect) -> TextBounds {
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
