mod quad;

use crate::primitives::{Color, Corners, Rect, Size, Stroke};
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

    /// `viewport_logical` is the surface size in logical (DIP) units.
    /// The renderer multiplies by `scale` to address physical pixels and (if
    /// `pixel_snap`) snaps rect edges to integer physical pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        viewport_logical: [f32; 2],
        scale: f32,
        pixel_snap: bool,
        clear: Color,
        tree: &Tree,
    ) {
        self.quads.clear();
        collect_quads(tree, scale, pixel_snap, &mut self.quads);

        let viewport_physical = [viewport_logical[0] * scale, viewport_logical[1] * scale];
        tracing::trace!(
            quads = self.quads.len(),
            viewport = ?viewport_physical,
            scale,
            pixel_snap,
            "renderer.render"
        );
        self.quad
            .upload(device, queue, viewport_physical, &self.quads);

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

fn collect_quads(tree: &Tree, scale: f32, snap: bool, out: &mut Vec<Quad>) {
    for node in &tree.nodes {
        let owner = node.rect;
        for shape in &tree.shapes[node.shapes_start as usize..node.shapes_end as usize] {
            if let Shape::RoundedRect {
                bounds,
                radius,
                fill,
                stroke,
            } = shape
            {
                let logical_rect = match bounds {
                    ShapeRect::Full => owner,
                    ShapeRect::Offset(r) => Rect {
                        min: owner.min + Vec2::new(r.min.x, r.min.y),
                        size: r.size,
                    },
                };
                let phys_rect = scale_rect(logical_rect, scale, snap);
                let phys_radius = scale_corners(*radius, scale);
                let phys_stroke = stroke.map(|s| Stroke {
                    width: s.width * scale,
                    color: s.color,
                });
                out.push(Quad::new(phys_rect, *fill, phys_radius, phys_stroke));
            }
        }
    }
}

fn scale_rect(r: Rect, scale: f32, snap: bool) -> Rect {
    let mut left = r.min.x * scale;
    let mut top = r.min.y * scale;
    let mut right = (r.min.x + r.size.w) * scale;
    let mut bottom = (r.min.y + r.size.h) * scale;
    if snap {
        left = left.round();
        top = top.round();
        right = right.round();
        bottom = bottom.round();
    }
    Rect {
        min: Vec2::new(left, top),
        size: Size::new((right - left).max(0.0), (bottom - top).max(0.0)),
    }
}

fn scale_corners(c: Corners, scale: f32) -> Corners {
    Corners {
        tl: c.tl * scale,
        tr: c.tr * scale,
        br: c.br * scale,
        bl: c.bl * scale,
    }
}
