use super::buffer::RenderBuffer;
use super::quad::QuadPipeline;
use crate::primitives::Color;
use crate::text::SharedCosmic;

mod text;
use text::TextRenderer;

/// wgpu backend: owns the quad pipeline + text renderer and cloned
/// device/queue handles (cheap, Arc-backed). The text side holds a shared
/// handle to the same `CosmicMeasure` the Ui side measures against (set via
/// [`Self::set_cosmic`]) — without it, text rendering is silently skipped.
/// No layout, no encode, no compose — those happen elsewhere and arrive
/// here as a `RenderBuffer`.
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadPipeline,
    text: TextRenderer,
}

impl WgpuBackend {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let quad = QuadPipeline::new(&device, format);
        let text = TextRenderer::new(&device, &queue, format);
        Self {
            device,
            queue,
            quad,
            text,
        }
    }

    /// Install the shared shaper handle. Pass the same `SharedCosmic` to
    /// [`crate::Ui::set_cosmic`] so layout and rendering see one cache.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// Render one frame. Without a shared shaper installed (mono fallback)
    /// text rendering is silently skipped; the frame still draws quads.
    pub fn submit(&mut self, view: &wgpu::TextureView, clear: Color, buffer: &RenderBuffer) {
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            "wgpu_backend.submit"
        );

        self.quad.upload(
            &self.device,
            &self.queue,
            buffer.viewport_phys_f,
            &buffer.quads,
        );

        // Prepare text outside the encoder/pass borrow scope so glyphon can
        // upload to the atlas + instance buffer freely.
        let text_ready = self.text.prepare(
            &self.device,
            &self.queue,
            buffer.viewport_phys,
            buffer.scale,
            &buffer.texts,
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r as f64,
                            g: clear.g as f64,
                            b: clear.b as f64,
                            a: clear.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for g in &buffer.groups {
                if let Some(s) = g.scissor {
                    if s.w == 0 || s.h == 0 {
                        continue;
                    }
                    pass.set_scissor_rect(s.x, s.y, s.w, s.h);
                } else {
                    pass.set_scissor_rect(0, 0, buffer.viewport_phys[0], buffer.viewport_phys[1]);
                }
                self.quad.draw_range(&mut pass, g.quads.clone());
            }
            // Text on top, single full-viewport scissor; per-area `bounds`
            // does the actual clipping. See `text` module on the z-order
            // limitation.
            if text_ready {
                pass.set_scissor_rect(0, 0, buffer.viewport_phys[0], buffer.viewport_phys[1]);
                self.text.render(&mut pass);
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        if text_ready {
            self.text.end_frame();
        }
    }

    /// Re-create text atlas/renderer after a surface format change.
    pub fn surface_format_changed(&mut self, format: wgpu::TextureFormat) {
        self.text
            .rebuild_for_format(&self.device, &self.queue, format);
    }
}

#[cfg(test)]
mod tests;
