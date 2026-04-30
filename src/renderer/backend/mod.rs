use super::buffer::RenderBuffer;
use super::quad::QuadPipeline;
use crate::primitives::Color;

/// wgpu backend: owns the quad pipeline + cloned device/queue handles
/// (cheap, Arc-backed), uploads the buffer's quads, and submits scissor-
/// grouped draws. No layout, no encode, no compose — those happen elsewhere
/// and arrive here as a `RenderBuffer`.
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadPipeline,
}

impl WgpuBackend {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let quad = QuadPipeline::new(&device, format);
        Self {
            device,
            queue,
            quad,
        }
    }

    pub fn submit(&mut self, view: &wgpu::TextureView, clear: Color, buffer: &RenderBuffer) {
        tracing::trace!(
            quads = buffer.quads.len(),
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
                self.quad.draw_range(&mut pass, g.instances.clone());
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
