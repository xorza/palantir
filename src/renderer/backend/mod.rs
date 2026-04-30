use super::encoder::{RenderCmd, encode};
use super::quad::{Quad, QuadPipeline};
use crate::primitives::{Color, Rect, Stroke};
use crate::tree::Tree;

/// Per-frame inputs the backend needs to actually draw. Bundled so the
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

/// wgpu backend: encodes a tree into commands, processes commands into
/// physical-px quad instances + scissor draw groups, and submits them.
///
/// All buffers are reused across frames to avoid per-frame allocs. Only
/// the trailing wgpu submission step needs a device/queue.
pub struct Renderer {
    quad: QuadPipeline,
    cmds: Vec<RenderCmd>,
    quads: Vec<Quad>,
    groups: Vec<DrawGroup>,
    /// Scratch clip stack for `process`; reused across frames so steady-state
    /// rendering allocates nothing here.
    clip_stack: Vec<ScissorRect>,
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
            clip_stack: Vec::new(),
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
            &mut self.clip_stack,
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
    clip_stack: &mut Vec<ScissorRect>,
) {
    quads.clear();
    groups.clear();
    clip_stack.clear();
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

#[cfg(test)]
mod tests;
