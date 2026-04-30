mod quad;

use crate::primitives::{Color, Rect};
use crate::shape::{Shape, ShapeRect};
use crate::tree::Tree;
use glam::Vec2;
pub use quad::{Quad, QuadPipeline};

pub struct Renderer {
    quad: QuadPipeline,
    quads: Vec<Quad>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            quad: QuadPipeline::new(device, format),
            quads: Vec::new(),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        viewport: [f32; 2],
        clear: Color,
        tree: &Tree,
    ) {
        self.quads.clear();
        collect_quads(tree, &mut self.quads);
        tracing::trace!(
            quads = self.quads.len(),
            viewport = ?viewport,
            "renderer.render"
        );
        self.quad.upload(device, queue, viewport, &self.quads);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
            self.quad.draw(&mut pass, self.quads.len() as u32);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

fn collect_quads(tree: &Tree, out: &mut Vec<Quad>) {
    for (i, node) in tree.nodes.iter().enumerate() {
        let owner = node.rect;
        for shape in &tree.shapes[node.shapes_start as usize..node.shapes_end as usize] {
            if let Shape::RoundedRect {
                bounds,
                radius,
                fill,
                ..
            } = shape
            {
                let rect = match bounds {
                    ShapeRect::Full => owner,
                    ShapeRect::Offset(r) => Rect {
                        min: owner.min + Vec2::new(r.min.x, r.min.y),
                        size: r.size,
                    },
                };
                out.push(Quad::from_rect(rect, *fill, *radius));
            }
        }
        let _ = i;
    }
}
