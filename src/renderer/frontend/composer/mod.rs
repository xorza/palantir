use super::cmd_buffer::{CmdKind, RenderCmdBuffer};
use crate::primitives::{Display, Rect, Stroke, TranslateScale, URect};
use crate::renderer::buffer::{DrawGroup, RenderBuffer, TextRun};
use crate::renderer::quad::Quad;

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
    clip_stack: Vec<URect>,
    transform_stack: Vec<TranslateScale>,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume a logical-px command stream → physical-px `Quad`s + `TextRun`s
    /// + draw groups (scissor ranges) into `out`. Pure: no device, no queue.
    pub fn compose(&mut self, cmds: &RenderCmdBuffer, display: &Display, out: &mut RenderBuffer) {
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys_f = [display.physical.x as f32, display.physical.y as f32];
        let viewport_phys = [display.physical.x, display.physical.y];

        out.quads.clear();
        out.texts.clear();
        out.groups.clear();
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        let mut current_transform = TranslateScale::IDENTITY;
        let mut current: Option<URect> = None;
        let mut quads_start: u32 = 0;
        let mut texts_start: u32 = 0;
        // Tracks whether the most recent draw in the active group was
        // text. A subsequent quad must start a new group so the prior
        // group's text renders BEFORE that quad — otherwise the quad
        // and the prior group's quads batch together and the text
        // floats on top. Reset on scissor switch (group already
        // flushed) and on flush.
        let mut last_was_text = false;

        for i in 0..cmds.len() {
            let start = cmds.starts[i];
            match cmds.kinds[i] {
                CmdKind::PushClip => {
                    let r = cmds.read_clip(start);
                    let world = current_transform.apply_rect(r);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let new = match self.clip_stack.last() {
                        Some(parent) => me.clamp_to(*parent),
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
                    last_was_text = false;
                }
                CmdKind::PopClip => {
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
                    last_was_text = false;
                }
                CmdKind::PushTransform => {
                    let t = cmds.read_transform(start);
                    self.transform_stack.push(current_transform);
                    current_transform = current_transform.compose(t);
                }
                CmdKind::PopTransform => {
                    current_transform = self
                        .transform_stack
                        .pop()
                        .unwrap_or(TranslateScale::IDENTITY);
                }
                kind @ (CmdKind::DrawRect | CmdKind::DrawRectStroked) => {
                    let p = if kind == CmdKind::DrawRect {
                        cmds.read_draw_rect(start)
                    } else {
                        cmds.read_draw_rect_stroked(start)
                    };
                    if last_was_text {
                        // Flush the current group so this quad renders
                        // *after* the prior group's text. Same scissor
                        // continues into the new group.
                        flush_group(
                            current,
                            quads_start,
                            out.quads.len() as u32,
                            texts_start,
                            out.texts.len() as u32,
                            &mut out.groups,
                        );
                        quads_start = out.quads.len() as u32;
                        texts_start = out.texts.len() as u32;
                        last_was_text = false;
                    }
                    let world_rect = current_transform.apply_rect(p.rect);
                    let world_radius = p.radius.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let phys_radius = world_radius.scaled_by(scale);
                    let phys_stroke = p.stroke.map(|s| Stroke {
                        width: s.width * current_transform.scale * scale,
                        color: s.color,
                    });
                    out.quads
                        .push(Quad::new(phys_rect, p.fill, phys_radius, phys_stroke));
                }
                CmdKind::DrawText => {
                    let t = cmds.read_draw_text(start);
                    let world_rect = current_transform.apply_rect(t.rect);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let bounds = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    out.texts.push(TextRun {
                        origin: [phys_rect.min.x, phys_rect.min.y],
                        bounds,
                        color: t.color,
                        key: t.key,
                    });
                    last_was_text = true;
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
    target: Option<URect>,
    current: &mut Option<URect>,
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
    scissor: Option<URect>,
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

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: [u32; 2]) -> URect {
    let phys = r.scaled_by(scale, snap);
    let x = (phys.min.x.max(0.0) as u32).min(viewport[0]);
    let y = (phys.min.y.max(0.0) as u32).min(viewport[1]);
    let right = ((phys.min.x + phys.size.w).max(0.0) as u32).min(viewport[0]);
    let bottom = ((phys.min.y + phys.size.h).max(0.0) as u32).min(viewport[1]);
    URect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

#[cfg(test)]
mod tests;
