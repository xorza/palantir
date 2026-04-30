use super::buffer::RenderBuffer;
use super::quad::QuadPipeline;
use crate::primitives::Color;

/// Per-submit GPU handles + clear color. The backend gets these fresh each
/// frame; everything else (quads, scissor groups, viewport) comes from the
/// `RenderBuffer` produced by `compose`.
pub struct RenderFrame<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub view: &'a wgpu::TextureView,
    pub clear: Color,
}

/// wgpu backend: owns the quad pipeline, uploads the buffer's quads, and
/// submits scissor-grouped draws. No layout, no encode, no compose — those
/// happen elsewhere and arrive here as a `RenderBuffer`.
pub struct WgpuBackend {
    quad: QuadPipeline,
}

impl WgpuBackend {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            quad: QuadPipeline::new(device, format),
        }
    }

    pub fn submit(&mut self, frame: RenderFrame, buffer: &RenderBuffer) {
        tracing::trace!(
            quads = buffer.quads.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            "wgpu_backend.submit"
        );

        self.quad.upload(
            frame.device,
            frame.queue,
            buffer.viewport_phys_f,
            &buffer.quads,
        );

        let mut encoder = frame
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: frame.clear.r as f64,
                            g: frame.clear.g as f64,
                            b: frame.clear.b as f64,
                            a: frame.clear.a as f64,
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
                self.quad.draw_range(&mut pass, g.start..g.end);
            }
        }
        frame.queue.submit(std::iter::once(encoder.finish()));
    }
}
