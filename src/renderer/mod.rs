mod quad;

use crate::primitives::{Color, Corners, Rect, Size, Stroke};
use crate::shape::{Shape, ShapeRect};
use crate::tree::{NodeId, Tree};
use glam::Vec2;
pub use quad::{Quad, QuadPipeline};

pub struct Renderer {
    quad: QuadPipeline,
    quads: Vec<Quad>,
    groups: Vec<DrawGroup>,
}

/// One range of quads that share a scissor rect (in physical pixels).
/// `None` scissor = unclipped (use viewport).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DrawGroup {
    scissor: Option<ScissorRect>,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScissorRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self {
            quad: QuadPipeline::new(device, format),
            quads: Vec::new(),
            groups: Vec::new(),
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
        let viewport_physical = [viewport_logical[0] * scale, viewport_logical[1] * scale];
        let viewport_u = [viewport_physical[0] as u32, viewport_physical[1] as u32];

        self.quads.clear();
        self.groups.clear();
        if let Some(root) = tree.nodes.first().map(|_| NodeId(0)) {
            let mut state = CollectState {
                quads: &mut self.quads,
                groups: &mut self.groups,
                current: None,
                current_start: 0,
                scale,
                snap: pixel_snap,
                viewport: viewport_u,
            };
            walk(tree, root, None, &mut state);
            state.flush();
        }

        tracing::trace!(
            quads = self.quads.len(),
            groups = self.groups.len(),
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
            for g in &self.groups {
                if let Some(s) = g.scissor {
                    if s.w == 0 || s.h == 0 {
                        continue;
                    }
                    pass.set_scissor_rect(s.x, s.y, s.w, s.h);
                } else {
                    pass.set_scissor_rect(0, 0, viewport_u[0], viewport_u[1]);
                }
                self.quad.draw_range(&mut pass, g.start..g.end);
            }
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

struct CollectState<'a> {
    quads: &'a mut Vec<Quad>,
    groups: &'a mut Vec<DrawGroup>,
    current: Option<ScissorRect>,
    current_start: u32,
    scale: f32,
    snap: bool,
    viewport: [u32; 2],
}

impl CollectState<'_> {
    /// Switch the active scissor. Closes the current group if any quads have
    /// been emitted under it; the next quad starts a new group.
    fn set_scissor(&mut self, target: Option<ScissorRect>) {
        if target == self.current {
            return;
        }
        let end = self.quads.len() as u32;
        if end > self.current_start {
            self.groups.push(DrawGroup {
                scissor: self.current,
                start: self.current_start,
                end,
            });
        }
        self.current = target;
        self.current_start = end;
    }

    /// Close the trailing group (if any quads were emitted under the active
    /// scissor since the last flush). Call once at end of walk.
    fn flush(&mut self) {
        let end = self.quads.len() as u32;
        if end > self.current_start {
            self.groups.push(DrawGroup {
                scissor: self.current,
                start: self.current_start,
                end,
            });
        }
    }
}

/// Pre-order walk: emit own quads, recurse into children, and apply
/// `element.clip` by intersecting a scissor against the parent's. Sibling
/// scissors are restored to `parent_scissor` after recursion.
fn walk(
    tree: &Tree,
    node_id: NodeId,
    parent_scissor: Option<ScissorRect>,
    state: &mut CollectState,
) {
    let node = tree.node(node_id);
    let my_scissor = if node.element.clip {
        let phys = scale_rect(node.rect, state.scale, state.snap);
        let me = scissor_from_phys(phys, state.viewport);
        Some(intersect_scissor(parent_scissor, me))
    } else {
        parent_scissor
    };

    state.set_scissor(my_scissor);
    emit_own_quads(tree, node_id, state);

    let mut c = node.first_child;
    while let Some(child) = c {
        walk(tree, child, my_scissor, state);
        c = tree.node(child).next_sibling;
    }

    state.set_scissor(parent_scissor);
}

fn emit_own_quads(tree: &Tree, node_id: NodeId, state: &mut CollectState) {
    let node = tree.node(node_id);
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
            let phys_rect = scale_rect(logical_rect, state.scale, state.snap);
            let phys_radius = scale_corners(*radius, state.scale);
            let phys_stroke = stroke.map(|s| Stroke {
                width: s.width * state.scale,
                color: s.color,
            });
            state
                .quads
                .push(Quad::new(phys_rect, *fill, phys_radius, phys_stroke));
        }
    }
}

fn scissor_from_phys(r: Rect, viewport: [u32; 2]) -> ScissorRect {
    let x = r.min.x.max(0.0) as u32;
    let y = r.min.y.max(0.0) as u32;
    let right = ((r.min.x + r.size.w).max(0.0) as u32).min(viewport[0]);
    let bottom = ((r.min.y + r.size.h).max(0.0) as u32).min(viewport[1]);
    ScissorRect {
        x: x.min(viewport[0]),
        y: y.min(viewport[1]),
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

fn intersect_scissor(parent: Option<ScissorRect>, me: ScissorRect) -> ScissorRect {
    match parent {
        None => me,
        Some(p) => {
            let x = p.x.max(me.x);
            let y = p.y.max(me.y);
            let r = (p.x + p.w).min(me.x + me.w);
            let b = (p.y + p.h).min(me.y + me.h);
            ScissorRect {
                x,
                y,
                w: r.saturating_sub(x),
                h: b.saturating_sub(y),
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
