mod quad;

use crate::primitives::{Color, Corners, Rect, Stroke};
use crate::shape::{Shape, ShapeRect};
use crate::tree::{NodeId, Tree};
use glam::Vec2;
pub use quad::{Quad, QuadPipeline};

/// One typed paint instruction in logical (DIP) coordinates. Produced by
/// `encode` from the tree, consumed by `process` which scales/snaps to
/// physical pixels and groups by scissor.
///
/// Decoupling the encode and process steps means: (a) the encoder is pure
/// data and tree-shaped knowledge; (b) the GPU backend never sees `Tree`;
/// (c) future shape kinds (Text, Line, Path) just add variants without
/// touching pipeline code.
#[derive(Clone, Debug)]
pub enum RenderCmd {
    /// Push a logical-px clip rect; intersected with the parent at process
    /// time. Pairs with `PopClip`.
    PushClip(Rect),
    PopClip,
    DrawRect {
        rect: Rect,
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    },
    // Future: DrawText { … }, DrawLine { … }, DrawPath { … }.
}

/// Per-frame inputs the renderer needs to actually draw. Bundled so the
/// `render(...)` signature is stable as more frame-state arrives.
pub struct RenderFrame<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub view: &'a wgpu::TextureView,
    /// Surface size in logical (DIP) units.
    pub viewport_logical: [f32; 2],
    /// Logical→physical conversion factor (e.g. 2.0 on 2× retina).
    pub scale: f32,
    /// Snap rect edges to integer physical pixels (sharper, no half-px blur).
    pub pixel_snap: bool,
    pub clear: Color,
}

pub struct Renderer {
    quad: QuadPipeline,
    cmds: Vec<RenderCmd>,
    quads: Vec<Quad>,
    groups: Vec<DrawGroup>,
}

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
            cmds: Vec::new(),
            quads: Vec::new(),
            groups: Vec::new(),
        }
    }

    pub fn render(&mut self, frame: RenderFrame, tree: &Tree) {
        let viewport_phys = [
            frame.viewport_logical[0] * frame.scale,
            frame.viewport_logical[1] * frame.scale,
        ];
        let viewport_u = [
            viewport_phys[0].ceil() as u32,
            viewport_phys[1].ceil() as u32,
        ];

        encode(tree, &mut self.cmds);
        process(
            &self.cmds,
            frame.scale,
            frame.pixel_snap,
            viewport_u,
            &mut self.quads,
            &mut self.groups,
        );

        tracing::trace!(
            cmds = self.cmds.len(),
            quads = self.quads.len(),
            groups = self.groups.len(),
            viewport = ?viewport_phys,
            scale = frame.scale,
            pixel_snap = frame.pixel_snap,
            "renderer.render"
        );
        self.quad
            .upload(frame.device, frame.queue, viewport_phys, &self.quads);

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
        frame.queue.submit(std::iter::once(encoder.finish()));
    }
}

/// Walk the tree pre-order and emit logical-px paint commands. No GPU work,
/// no scale/snap math — that lives in `process`. Public so backends and
/// tests can consume the command stream directly.
pub fn encode(tree: &Tree, out: &mut Vec<RenderCmd>) {
    out.clear();
    if let Some(root) = tree.root() {
        encode_node(tree, root, out);
    }
}

fn encode_node(tree: &Tree, id: NodeId, out: &mut Vec<RenderCmd>) {
    let node = tree.node(id);
    if node.element.clip {
        out.push(RenderCmd::PushClip(node.rect));
    }

    let owner = node.rect;
    for shape in tree.shapes_of(id) {
        match shape {
            Shape::RoundedRect {
                bounds,
                radius,
                fill,
                stroke,
            } => {
                let rect = match bounds {
                    ShapeRect::Full => owner,
                    ShapeRect::Offset(r) => Rect {
                        min: owner.min + Vec2::new(r.min.x, r.min.y),
                        size: r.size,
                    },
                };
                out.push(RenderCmd::DrawRect {
                    rect,
                    radius: *radius,
                    fill: *fill,
                    stroke: *stroke,
                });
            }
            // No pipeline for these yet — drop with a trace so they're not silently invisible.
            Shape::Line { .. } | Shape::Text { .. } => {
                tracing::trace!(?shape, "renderer: dropping unsupported shape");
            }
        }
    }

    let mut c = node.first_child;
    while let Some(child) = c {
        encode_node(tree, child, out);
        c = tree.node(child).next_sibling;
    }

    if node.element.clip {
        out.push(RenderCmd::PopClip);
    }
}

/// Consume a logical-px command stream → physical-px `Quad` instances + draw
/// groups (scissor ranges). Maintains a clip stack so nested `PushClip`s
/// intersect correctly.
fn process(
    cmds: &[RenderCmd],
    scale: f32,
    snap: bool,
    viewport: [u32; 2],
    quads: &mut Vec<Quad>,
    groups: &mut Vec<DrawGroup>,
) {
    quads.clear();
    groups.clear();

    let mut clip_stack: Vec<ScissorRect> = Vec::new();
    let mut current: Option<ScissorRect> = None;
    let mut current_start: u32 = 0;

    let flush =
        |scissor: Option<ScissorRect>, start: u32, end: u32, groups: &mut Vec<DrawGroup>| {
            if end > start {
                groups.push(DrawGroup {
                    scissor,
                    start,
                    end,
                });
            }
        };

    for cmd in cmds {
        match cmd {
            RenderCmd::PushClip(r) => {
                let me = scissor_from_logical(*r, scale, snap, viewport);
                let new = match clip_stack.last() {
                    Some(parent) => intersect_scissor(*parent, me),
                    None => me,
                };
                clip_stack.push(new);
                let target = Some(new);
                if target != current {
                    flush(current, current_start, quads.len() as u32, groups);
                    current = target;
                    current_start = quads.len() as u32;
                }
            }
            RenderCmd::PopClip => {
                clip_stack.pop();
                let target = clip_stack.last().copied();
                if target != current {
                    flush(current, current_start, quads.len() as u32, groups);
                    current = target;
                    current_start = quads.len() as u32;
                }
            }
            RenderCmd::DrawRect {
                rect,
                radius,
                fill,
                stroke,
            } => {
                let phys_rect = rect.scaled_by(scale, snap);
                let phys_radius = radius.scaled_by(scale);
                let phys_stroke = stroke.map(|s| Stroke {
                    width: s.width * scale,
                    color: s.color,
                });
                quads.push(Quad::new(phys_rect, *fill, phys_radius, phys_stroke));
            }
        }
    }
    flush(current, current_start, quads.len() as u32, groups);
}

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: [u32; 2]) -> ScissorRect {
    let phys = r.scaled_by(scale, snap);
    let x = (phys.min.x.max(0.0) as u32).min(viewport[0]);
    let y = (phys.min.y.max(0.0) as u32).min(viewport[1]);
    let right = ((phys.min.x + phys.size.w).max(0.0) as u32).min(viewport[0]);
    let bottom = ((phys.min.y + phys.size.h).max(0.0) as u32).min(viewport[1]);
    ScissorRect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

fn intersect_scissor(parent: ScissorRect, me: ScissorRect) -> ScissorRect {
    let x = parent.x.max(me.x);
    let y = parent.y.max(me.y);
    let r = (parent.x + parent.w).min(me.x + me.w);
    let b = (parent.y + parent.h).min(me.y + me.h);
    ScissorRect {
        x,
        y,
        w: r.saturating_sub(x),
        h: b.saturating_sub(y),
    }
}
