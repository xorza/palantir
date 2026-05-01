//! Glyphon-backed text pipeline. Owns the device-bound state
//! ([`Cache`], [`TextAtlas`], [`Viewport`], [`TextRenderer`], [`SwashCache`])
//! that the wgpu backend needs to rasterize shaped runs.
//!
//! Authoring-time shaping state ([`FontSystem`] + the per-key shaped
//! `cosmic_text::Buffer` cache) is held by [`crate::text::CosmicMeasure`] on
//! the `Ui` side. At submit time the backend takes
//! `Option<&mut CosmicMeasure>` directly, looks up each [`TextRun`]'s
//! buffer, builds a `glyphon::TextArea`, and hands the lot to glyphon.
//!
//! v1 limitation: all text is prepared and rendered after all quads, so text
//! always paints on top of every quad in the frame. This matches the common
//! case (button label over button background) but means a parent's label
//! will visually float over a child's background. Fix when the first widget
//! needs the opposite z-order — likely via per-group prepare/render or
//! glyphon's depth metadata.
//!
//! [`FontSystem`]: cosmic_text::FontSystem
//! [`TextRun`]: super::super::buffer::TextRun

use super::super::buffer::{ScissorRect, TextRun};
use crate::text::CosmicMeasure;
use glyphon::{
    Cache, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};

pub(crate) struct TextPipeline {
    cache: Cache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: TextRenderer,
    swash_cache: SwashCache,
    /// Reusable scratch for `TextArea`s built each frame from the
    /// `RenderBuffer.texts`. Cleared per submit, capacity retained.
    scratch: Vec<TextArea<'static>>,
}

impl TextPipeline {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let swash_cache = SwashCache::new();
        Self {
            cache,
            atlas,
            viewport,
            renderer,
            swash_cache,
            scratch: Vec::new(),
        }
    }

    /// Re-create on surface format change (e.g. after window recreation).
    pub fn rebuild_for_format(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        self.atlas = TextAtlas::new(device, queue, &self.cache, format);
        self.renderer = TextRenderer::new(
            &mut self.atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
    }

    /// Build glyphon `TextArea`s from `runs` (looking up shaped buffers in
    /// `cosmic`) and call `prepare`. Returns `false` and skips work if no
    /// runs resolve to a buffer.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport_phys: [u32; 2],
        scale: f32,
        runs: &[TextRun],
        cosmic: &mut CosmicMeasure,
    ) -> bool {
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
