use super::buffer::{DrawGroup, RenderBuffer, ScissorRect, TextRun};
use super::encoder::RenderCmd;
use super::quad::Quad;
use crate::primitives::{Rect, Stroke, TranslateScale};

/// Per-frame inputs to the compose pass. No GPU handles — compose only reads
/// commands and writes into a `RenderBuffer`.
pub struct ComposeParams {
    /// Surface size in logical (DIP) units.
    pub viewport_logical: [f32; 2],
    /// Logical→physical conversion factor (e.g. 2.0 on 2× retina).
    pub scale: f32,
    /// Snap rect edges to integer physical pixels (sharper, no half-px blur).
    pub pixel_snap: bool,
}

/// CPU-only compose engine: turns a `RenderCmd` stream into a `RenderBuffer`
/// (physical-px quads + text runs + scissor groups) supplied by the caller.
/// Owns just the compose-time scratch stacks so steady-state rendering is
/// alloc-free.
///
/// Composer doesn't know about `Tree`, `encode`, or where the output buffer
/// lives — it's pure algorithm + scratch. [`Pipeline`] orchestrates encode +
/// compose and owns the buffer.
#[derive(Default)]
pub struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    clip_stack: Vec<ScissorRect>,
    transform_stack: Vec<TranslateScale>,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume a logical-px command stream → physical-px `Quad`s + `TextRun`s
    /// + draw groups (scissor ranges) into `out`. Pure: no device, no queue.
    pub fn compose(&mut self, cmds: &[RenderCmd], params: &ComposeParams, out: &mut RenderBuffer) {
        let viewport_phys_f = [
            params.viewport_logical[0] * params.scale,
            params.viewport_logical[1] * params.scale,
        ];
        let viewport_phys = [
            viewport_phys_f[0].ceil() as u32,
            viewport_phys_f[1].ceil() as u32,
        ];

        out.quads.clear();
        out.texts.clear();
        out.groups.clear();
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;

        self.clip_stack.clear();
        self.transform_stack.clear();
        let mut current_transform = TranslateScale::IDENTITY;
        let mut current: Option<ScissorRect> = None;
        let mut quads_start: u32 = 0;
        let mut texts_start: u32 = 0;

        let scale = params.scale;
        let snap = params.pixel_snap;

        for cmd in cmds {
            match cmd {
                RenderCmd::PushClip(r) => {
                    let world = current_transform.apply_rect(*r);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let new = match self.clip_stack.last() {
                        Some(parent) => intersect_scissor(*parent, me),
                        None => me,
                    };
                    self.clip_stack.push(new);
                    switch_group(
                        Some(new),
                        &mut current,
                        &mut quads_start,
                        &mut texts_start,
                        out.quads.len() as u32,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                }
                RenderCmd::PopClip => {
                    self.clip_stack.pop();
                    switch_group(
                        self.clip_stack.last().copied(),
                        &mut current,
                        &mut quads_start,
                        &mut texts_start,
                        out.quads.len() as u32,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                }
                RenderCmd::PushTransform(t) => {
                    self.transform_stack.push(current_transform);
                    current_transform = current_transform.compose(*t);
                }
                RenderCmd::PopTransform => {
                    current_transform = self
                        .transform_stack
                        .pop()
                        .unwrap_or(TranslateScale::IDENTITY);
                }
                RenderCmd::DrawRect {
                    rect,
                    radius,
                    fill,
                    stroke,
                } => {
                    let world_rect = current_transform.apply_rect(*rect);
                    let world_radius = radius.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let phys_radius = world_radius.scaled_by(scale);
                    let phys_stroke = stroke.map(|s| Stroke {
                        width: s.width * current_transform.scale * scale,
                        color: s.color,
                    });
                    out.quads
                        .push(Quad::new(phys_rect, *fill, phys_radius, phys_stroke));
                }
                RenderCmd::DrawText { rect, color, key } => {
                    let world_rect = current_transform.apply_rect(*rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let bounds = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    out.texts.push(TextRun {
                        origin: [phys_rect.min.x, phys_rect.min.y],
                        bounds,
                        color: *color,
                        key: *key,
                    });
                }
            }
        }
        flush_group(
            current,
            quads_start,
            out.quads.len() as u32,
            texts_start,
            out.texts.len() as u32,
            &mut out.groups,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn switch_group(
    target: Option<ScissorRect>,
    current: &mut Option<ScissorRect>,
    quads_start: &mut u32,
    texts_start: &mut u32,
    quads_end: u32,
    texts_end: u32,
    groups: &mut Vec<DrawGroup>,
) {
    if target != *current {
        flush_group(
            *current,
            *quads_start,
            quads_end,
            *texts_start,
            texts_end,
            groups,
        );
        *current = target;
        *quads_start = quads_end;
        *texts_start = texts_end;
    }
}

fn flush_group(
    scissor: Option<ScissorRect>,
    quads_start: u32,
    quads_end: u32,
    texts_start: u32,
    texts_end: u32,
    groups: &mut Vec<DrawGroup>,
) {
    if quads_end > quads_start || texts_end > texts_start {
        groups.push(DrawGroup {
            scissor,
            quads: quads_start..quads_end,
            texts: texts_start..texts_end,
        });
    }
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
